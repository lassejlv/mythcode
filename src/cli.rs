use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::acp_client::AcpClient;
use crate::input::{self, FileIndex};
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
        Some(other) => {
            anyhow::bail!("unknown provider `{other}`. Use `opencode`, `codex`, or `claude`.");
        }
        None if input::is_interactive_terminal() => pick_provider()?,
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
            let connected = AcpClient::connect(&config).await?;
            let mut client = connected.client;
            let mut events = connected.events;
            let mut signals = SignalState::new()?;

            if let Some(prompt) = &config.prompt {
                // One-shot mode: just print output directly
                run_one_shot(&client, &mut events, &mut signals, prompt).await
            } else if input::is_interactive_terminal() {
                // Interactive TUI mode
                let mut file_index = build_file_index(client.session_snapshot().cwd());
                let mut tui = Tui::new();
                let result = tui.run(&mut client, &mut events, &mut signals, &mut file_index).await;
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
    let prompt_future = client.prompt(prompt);
    tokio::pin!(prompt_future);

    let mut cancel_sent = false;
    loop {
        tokio::select! {
            result = &mut prompt_future => {
                let _result = result?;
                // Drain remaining events
                while let Ok(event) = events.try_recv() {
                    match event {
                        AppEvent::AssistantText(text) => print!("{text}"),
                        AppEvent::ThinkingText(_) => {}
                        AppEvent::PermissionRequest(req) => {
                            let _ = req.responder.send(PermissionDecision::Cancelled);
                        }
                        _ => {}
                    }
                }
                let _ = io::stdout().flush();
                println!();
                return Ok(());
            }
            Some(event) = events.recv() => {
                match event {
                    AppEvent::AssistantText(text) => {
                        print!("{text}");
                        let _ = io::stdout().flush();
                    }
                    AppEvent::PermissionRequest(req) => {
                        // Auto-accept in one-shot
                        let decision = req.options.iter()
                            .find(|o| o.kind.is_accept())
                            .map(|o| PermissionDecision::Selected(o.option_id.clone()))
                            .unwrap_or(PermissionDecision::Cancelled);
                        let _ = req.responder.send(decision);
                    }
                    _ => {}
                }
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

    loop {
        let cwd = client.session_snapshot().cwd().to_path_buf();
        let label = cwd.file_name().and_then(|n| n.to_str()).unwrap_or(".");
        print!("{label}> ");
        io::stdout().flush()?;

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
                        if let AppEvent::AssistantText(text) = event {
                            print!("{text}");
                        }
                    }
                    println!();
                    break;
                }
                Some(event) = events.recv() => {
                    match event {
                        AppEvent::AssistantText(text) => {
                            print!("{text}");
                            let _ = io::stdout().flush();
                        }
                        AppEvent::PermissionRequest(req) => {
                            let decision = req.options.iter()
                                .find(|o| o.kind.is_accept())
                                .map(|o| PermissionDecision::Selected(o.option_id.clone()))
                                .unwrap_or(PermissionDecision::Cancelled);
                            let _ = req.responder.send(decision);
                        }
                        _ => {}
                    }
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

fn pick_provider() -> Result<AcpProvider> {
    use crossterm::event::{self, Event, KeyCode};
    use crossterm::{cursor, execute, terminal};

    let providers = [
        (AcpProvider::OpenCode, "opencode", "OpenCode ACP server"),
        (AcpProvider::Codex, "codex", "Codex ACP (Zed)"),
        (AcpProvider::Claude, "claude", "Claude Code ACP"),
    ];

    let mut selected = 0usize;
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;

    loop {
        // Draw menu
        execute!(stdout, cursor::MoveTo(0, 0))?;
        write!(stdout, "\x1b[2J\x1b[H")?; // clear screen
        writeln!(stdout, "\r")?;
        writeln!(stdout, "  \x1b[1;38;5;75mmythcode\x1b[0m\r")?;
        writeln!(stdout, "  \x1b[38;5;245mSelect an ACP provider:\x1b[0m\r")?;
        writeln!(stdout, "\r")?;

        for (i, (_, name, desc)) in providers.iter().enumerate() {
            if i == selected {
                writeln!(stdout, "  \x1b[38;5;75m▸ {name}\x1b[0m  \x1b[38;5;245m{desc}\x1b[0m\r")?;
            } else {
                writeln!(stdout, "    \x1b[38;5;240m{name}  {desc}\x1b[0m\r")?;
            }
        }

        writeln!(stdout, "\r")?;
        writeln!(stdout, "  \x1b[38;5;240m↑↓ select · enter confirm · q quit\x1b[0m\r")?;
        stdout.flush()?;

        // Read key
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    if selected > 0 {
                        selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if selected + 1 < providers.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter => {
                    terminal::disable_raw_mode()?;
                    write!(stdout, "\x1b[2J\x1b[H")?; // clear screen
                    stdout.flush()?;
                    return Ok(providers[selected].0.clone());
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    terminal::disable_raw_mode()?;
                    write!(stdout, "\x1b[2J\x1b[H")?;
                    stdout.flush()?;
                    std::process::exit(0);
                }
                _ => {}
            }
        }
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
    sigint: Option<tokio::signal::unix::Signal>,
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
            Ok(Self {
                sigint: None,
                sigterm: None,
            })
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
