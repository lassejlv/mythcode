use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::acp_client::AcpClient;
use crate::input::{self, FileIndex};
use crate::terminal_ui::{FrameBuffer, TerminalGuard, TerminalGuardOptions};
use crate::tui::Tui;
use crate::types::{
    AcpProvider, AppConfig, AppEvent, PermissionDecision, ShutdownSignal, SlashCommand,
    SlashCommandSource,
};

#[derive(Debug, Parser)]
#[command(name = "mythcode", version, about = "Minimal ACP client")]
struct Args {
    #[arg(value_name = "PROMPT", trailing_var_arg = true)]
    prompt: Vec<String>,
    #[arg(short = 'p', long = "project", value_name = "PATH")]
    project: Option<PathBuf>,
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,
    /// ACP provider: opencode or codex (interactive selection if omitted)
    #[arg(long, value_name = "PROVIDER")]
    provider: Option<String>,
    #[arg(long)]
    debug: bool,
}

pub async fn run() -> Result<()> {
    let args = Args::parse();
    let cwd = resolve_cwd(args.project)?;
    let prompt = if args.prompt.is_empty() {
        None
    } else {
        Some(args.prompt.join(" "))
    };

    let provider = match args.provider.as_deref() {
        Some("opencode") => AcpProvider::OpenCode,
        Some("codex") => AcpProvider::Codex,
        Some("claude") => AcpProvider::Claude,
        Some("pi") => AcpProvider::Pi,
        Some("gemini") => AcpProvider::Gemini,
        Some(other) => {
            anyhow::bail!(
                "unknown provider `{other}`. Use `opencode`, `codex`, `claude`, `pi`, or `gemini`."
            );
        }
        None if input::is_interactive_terminal() => match pick_provider()? {
            Some(provider) => provider,
            None => return Ok(()),
        },
        None => AcpProvider::OpenCode,
    };

    let config = AppConfig {
        cwd,
        debug: args.debug,
        model: args.model,
        prompt,
        provider,
    };

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let connected = if input::is_interactive_terminal() && config.prompt.is_none() {
                connect_with_loading(&config).await?
            } else {
                AcpClient::connect(&config).await?
            };
            let mut client = connected.client;
            let mut events = connected.events;
            let mut signals = SignalState::new()?;

            // Discover and spawn extension host (optional)
            let event_tx = connected.event_tx.clone();
            let mut ext_host = crate::extensions::ExtensionHost::discover_and_spawn(
                client.session_snapshot().cwd(),
                event_tx,
            )
            .await;

            if let Some(prompt) = &config.prompt {
                // One-shot mode: just print output directly
                run_one_shot(&client, &mut events, &mut signals, prompt).await
            } else if input::is_interactive_terminal() {
                // Interactive TUI mode
                let mut file_index = build_file_index(client.session_snapshot().cwd());
                let mut tui = Tui::new();
                let result = tui
                    .run(
                        &mut client,
                        &mut events,
                        &mut signals,
                        &mut file_index,
                        &mut ext_host,
                    )
                    .await;
                if let Some(ref mut host) = ext_host {
                    host.shutdown().await;
                }
                client.shutdown().await;
                result
            } else {
                // Non-interactive stdin mode
                run_non_interactive(&mut client, &mut events, &mut signals).await
            }
        })
        .await
}

async fn run_one_shot(
    client: &AcpClient,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    signals: &mut SignalState,
    prompt: &str,
) -> Result<()> {
    let mut stdout = io::stdout();
    let prompt_future = client.prompt(prompt);
    tokio::pin!(prompt_future);

    let mut cancel_sent = false;
    loop {
        tokio::select! {
            result = &mut prompt_future => {
                let _result = result?;
                // Drain remaining events
                while let Ok(event) = events.try_recv() {
                    handle_stream_event(event, &mut stdout)?;
                }
                writeln!(stdout)?;
                stdout.flush()?;
                return Ok(());
            }
            Some(event) = events.recv() => {
                handle_stream_event(event, &mut stdout)?;
            }
            signal = signals.recv() => {
                match signal {
                    ShutdownSignal::Sigint if !cancel_sent => {
                        client.cancel_current_turn().await?;
                        cancel_sent = true;
                    }
                    _ => return Ok(()),
                }
            }
        }
    }
}

async fn run_non_interactive(
    client: &mut AcpClient,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    signals: &mut SignalState,
) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = io::stdout();

    loop {
        let cwd = client.session_snapshot().cwd().to_path_buf();
        let label = cwd.file_name().and_then(|n| n.to_str()).unwrap_or(".");
        write!(stdout, "{label}> ")?;
        stdout.flush()?;

        let line = tokio::select! {
            maybe_line = lines.next_line() => {
                maybe_line.context("failed to read stdin")?
            },
            signal = signals.recv() => {
                match signal {
                    ShutdownSignal::Sigint | ShutdownSignal::Sigterm => {
                        println!();
                        break;
                    }
                }
            }
        };

        let Some(line) = line else {
            println!();
            break;
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Simple non-interactive prompt execution
        let prompt_future = client.prompt(line);
        tokio::pin!(prompt_future);
        let mut cancel_sent = false;

        loop {
            tokio::select! {
                result = &mut prompt_future => {
                    let _result = result?;
                    while let Ok(event) = events.try_recv() {
                        handle_stream_event(event, &mut stdout)?;
                    }
                    writeln!(stdout)?;
                    stdout.flush()?;
                    break;
                }
                Some(event) = events.recv() => {
                    handle_stream_event(event, &mut stdout)?;
                }
                signal = signals.recv() => {
                    match signal {
                        ShutdownSignal::Sigint if !cancel_sent => {
                            client.cancel_current_turn().await?;
                            cancel_sent = true;
                        }
                        _ => return Ok(()),
                    }
                }
            }
        }
    }

    client.shutdown().await;
    Ok(())
}

pub fn local_commands() -> Vec<SlashCommand> {
    vec![
        local_command("help", "show local commands", None),
        local_command("model", "change the active model", None),
        local_command("new", "start a fresh session", None),
        local_command("cwd", "print the current working directory", None),
        local_command("clear", "clear the terminal", None),
        local_command("resume", "resume a previous session", None),
        local_command("extensions", "show loaded extensions", None),
        local_command("exit", "exit mythcode", None),
    ]
}

fn local_command(name: &str, description: &str, hint: Option<&str>) -> SlashCommand {
    SlashCommand {
        name: name.to_string(),
        description: description.to_string(),
        hint: hint.map(ToOwned::to_owned),
        source: SlashCommandSource::Local,
    }
}

pub fn build_file_index(cwd: &Path) -> FileIndex {
    FileIndex::build(cwd).unwrap_or_default()
}

fn handle_stream_event(event: AppEvent, stdout: &mut impl Write) -> Result<()> {
    match event {
        AppEvent::AssistantText(text) => {
            write!(stdout, "{text}")?;
            stdout.flush()?;
        }
        AppEvent::PermissionRequest(req) => {
            let decision = auto_permission_decision(&req.options);
            let _ = req.responder.send(decision);
        }
        _ => {}
    }
    Ok(())
}

fn auto_permission_decision(options: &[crate::types::PermissionOptionView]) -> PermissionDecision {
    options
        .iter()
        .find(|option| option.kind.is_accept())
        .map(|option| PermissionDecision::Selected(option.option_id.clone()))
        .unwrap_or(PermissionDecision::Cancelled)
}

async fn connect_with_loading(config: &AppConfig) -> Result<crate::acp_client::ConnectedClient> {
    use crossterm::terminal;
    use std::time::Instant;

    const RESET: &str = "\x1b[0m";
    const DARK: &str = "\x1b[38;5;240m";
    const SPINNER_COLOR: &str = "\x1b[38;5;75m";

    let provider_label = match &config.provider {
        AcpProvider::OpenCode => "OpenCode",
        AcpProvider::Codex => "Codex",
        AcpProvider::Claude => "Claude Code",
        AcpProvider::Pi => "Pi",
        AcpProvider::Gemini => "Gemini",
    };

    let connect_messages: &[&str] = &["Connecting…", "Starting…", "Initializing…"];

    let _guard = TerminalGuard::enter(TerminalGuardOptions {
        alternate_screen: false,
        mouse_capture: false,
        enhanced_keys: false,
    })?;
    let mut stdout = io::stdout();

    let start = Instant::now();
    let mut tick: usize = 0;

    // Spawn the actual connection
    let connect_future = AcpClient::connect(config);
    tokio::pin!(connect_future);

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(
        crate::spinner::INTERVAL_MS,
    ));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let result = loop {
        tokio::select! {
            result = &mut connect_future => {
                break result;
            }
            _ = interval.tick() => {
                tick += 1;
                let elapsed = start.elapsed().as_secs();
                let msg_idx = (elapsed / 5) as usize % connect_messages.len();
                let status = connect_messages[msg_idx];
                let shimmer = crate::spinner::shimmer(tick, status);
                let timer = crate::spinner::format_elapsed(elapsed);
                let (_, height) = terminal::size().unwrap_or((80, 24));
                let center_y = height / 2;
                let mut screen = FrameBuffer::new(height);
                screen.set_line(
                    center_y,
                    format!(
                        "  {SPINNER_COLOR}{shimmer}{RESET}  {DARK}{provider_label} · {timer}{RESET}"
                    ),
                );
                screen.render(&mut stdout)?;
            }
        }
    };

    result
}

fn pick_provider() -> Result<Option<AcpProvider>> {
    use crossterm::event::{self, Event, KeyCode};
    use crossterm::terminal;

    // Colors
    const RESET: &str = "\x1b[0m";
    const DIM: &str = "\x1b[38;5;240m";
    const CYAN: &str = "\x1b[38;5;75m";
    const BOLD_CYAN: &str = "\x1b[1;38;5;75m";
    const GREEN: &str = "\x1b[38;5;114m";
    const MAGENTA: &str = "\x1b[38;5;176m";
    const ORANGE: &str = "\x1b[38;5;209m";

    struct ProviderEntry {
        provider: AcpProvider,
        name: &'static str,
        color: &'static str,
        icon: &'static str,
    }

    let providers = [
        ProviderEntry {
            provider: AcpProvider::Claude,
            name: "Claude Code",
            color: ORANGE,
            icon: "◆",
        },
        ProviderEntry {
            provider: AcpProvider::OpenCode,
            name: "OpenCode",
            color: GREEN,
            icon: "◇",
        },
        ProviderEntry {
            provider: AcpProvider::Codex,
            name: "Codex",
            color: CYAN,
            icon: "◈",
        },
        ProviderEntry {
            provider: AcpProvider::Pi,
            name: "Pi",
            color: MAGENTA,
            icon: "●",
        },
        ProviderEntry {
            provider: AcpProvider::Gemini,
            name: "Gemini",
            color: CYAN,
            icon: "◆",
        },
    ];

    let mut selected = 0usize;
    let _guard = TerminalGuard::enter(TerminalGuardOptions {
        alternate_screen: true,
        mouse_capture: false,
        enhanced_keys: false,
    })?;
    let mut stdout = io::stdout();

    loop {
        let (_term_w, term_h) = terminal::size().unwrap_or((80, 24));
        let mut screen = FrameBuffer::new(term_h);
        let content_height = 2 + providers.len() + 2;
        let start_y = if (term_h as usize) > content_height + 4 {
            ((term_h as usize - content_height) / 2) as u16
        } else {
            1
        };

        let mut row = start_y;

        screen.set_line(row, format!("  {BOLD_CYAN}mythcode{RESET}"));
        row += 1;
        row += 1;

        for (i, entry) in providers.iter().enumerate() {
            let is_selected = i == selected;

            if is_selected {
                screen.set_line(
                    row,
                    format!(
                        "  {CYAN}▸{RESET} {color}{icon}{RESET} {name}",
                        color = entry.color,
                        icon = entry.icon,
                        name = entry.name,
                    ),
                );
            } else {
                screen.set_line(
                    row,
                    format!(
                        "    {DIM}{icon} {name}{RESET}",
                        icon = entry.icon,
                        name = entry.name,
                    ),
                );
            }
            row += 1;
        }

        row += 1;
        screen.set_line(
            row,
            format!("  {DIM}↑↓ navigate · enter select · q quit{RESET}"),
        );
        screen.render(&mut stdout)?;

        // Read key
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected + 1 < providers.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    return Ok(Some(providers[selected].provider.clone()));
                }
                KeyCode::Char('1') => {
                    return Ok(Some(providers[0].provider.clone()));
                }
                KeyCode::Char('2') => {
                    return Ok(Some(providers[1].provider.clone()));
                }
                KeyCode::Char('3') => {
                    return Ok(Some(providers[2].provider.clone()));
                }
                KeyCode::Char('4') if providers.len() >= 4 => {
                    return Ok(Some(providers[3].provider.clone()));
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{PermissionDecision, PermissionOptionKindView, PermissionOptionView};

    use super::auto_permission_decision;

    #[test]
    fn auto_permission_prefers_accepting_option() {
        let options = vec![
            PermissionOptionView {
                option_id: "deny".into(),
                name: "Deny".into(),
                kind: PermissionOptionKindView::RejectOnce,
            },
            PermissionOptionView {
                option_id: "allow".into(),
                name: "Allow".into(),
                kind: PermissionOptionKindView::AllowOnce,
            },
        ];

        assert_eq!(
            auto_permission_decision(&options),
            PermissionDecision::Selected("allow".into())
        );
    }
}

fn resolve_cwd(project: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = if let Some(path) = project {
        if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .context("failed to determine current directory")?
                .join(path)
        }
    } else {
        std::env::current_dir().context("failed to determine current directory")?
    };

    cwd.canonicalize()
        .with_context(|| format!("failed to resolve project path {}", cwd.display()))
}

pub struct SignalState {
    #[cfg(unix)]
    sigint: Option<tokio::signal::unix::Signal>,
    #[cfg(unix)]
    sigterm: Option<tokio::signal::unix::Signal>,
}

impl SignalState {
    pub fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                sigint: Some(
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                        .context("failed to register SIGINT handler")?,
                ),
                sigterm: Some(
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .context("failed to register SIGTERM handler")?,
                ),
            })
        }

        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    pub async fn recv(&mut self) -> ShutdownSignal {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.sigint.as_mut().expect("SIGINT handler missing").recv() => ShutdownSignal::Sigint,
                _ = self.sigterm.as_mut().expect("SIGTERM handler missing").recv() => ShutdownSignal::Sigterm,
            }
        }

        #[cfg(not(unix))]
        {
            let _ = &self;
            tokio::signal::ctrl_c()
                .await
                .expect("failed to wait for Ctrl+C");
            ShutdownSignal::Sigint
        }
    }
}
