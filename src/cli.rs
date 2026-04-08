use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::acp_client::AcpClient;
use crate::input::{self, FileIndex, ReadLineOutcome};
use crate::render::Renderer;
use crate::session::SessionState;
use crate::types::{
    AppConfig, AppEvent, CommandAction, PermissionDecision, PermissionOptionView, PromptOutcome,
    ShutdownSignal, SlashCommand, SlashCommandSource,
};

#[derive(Debug, Parser)]
#[command(name = "mythcode", version, about = "Minimal ACP client for OpenCode")]
struct Args {
    #[arg(value_name = "PROMPT", trailing_var_arg = true)]
    prompt: Vec<String>,
    #[arg(short = 'p', long = "project", value_name = "PATH")]
    project: Option<PathBuf>,
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,
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
    let config = AppConfig {
        cwd,
        debug: args.debug,
        model: args.model,
        prompt,
    };

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let use_color = input::is_interactive_terminal() && std::env::var("NO_COLOR").is_err();

            let connected = AcpClient::connect(&config).await?;
            let mut client = connected.client;
            let mut events = connected.events;
            let mut renderer = Renderer::new(config.debug, use_color);
            let mut signals = SignalState::new()?;

            let result = if let Some(prompt) = &config.prompt {
                run_one_shot(&client, &mut events, &mut renderer, &mut signals, prompt).await
            } else {
                run_repl(
                    &mut client,
                    &mut events,
                    &mut renderer,
                    &mut signals,
                    use_color,
                )
                .await
            };

            client.shutdown().await;
            result
        })
        .await
}

async fn run_one_shot(
    client: &AcpClient,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    renderer: &mut Renderer,
    signals: &mut SignalState,
    prompt: &str,
) -> Result<()> {
    match run_prompt(client, events, renderer, signals, prompt).await? {
        PromptOutcome::Completed | PromptOutcome::ExitRequested => Ok(()),
    }
}

async fn run_repl(
    client: &mut AcpClient,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    renderer: &mut Renderer,
    signals: &mut SignalState,
    use_color: bool,
) -> Result<()> {
    let mut pending_exit = false;
    let mut file_index = build_file_index(client.session_snapshot().cwd(), renderer);

    if input::is_interactive_terminal() {
        input::init_theme();

        let session = client.session_snapshot();
        let project_name = session
            .cwd()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".");
        let model_name = session
            .models()
            .current_model_id
            .as_deref()
            .or_else(|| session.models().available.first().map(|m| m.name.as_str()));
        renderer.print_welcome(project_name, model_name);

        loop {
            let session = client.session_snapshot();
            let prompt = prompt_label(session.cwd(), use_color);
            let commands = prompt_commands(&session);

            let input = match input::read_line(&prompt, &commands, &file_index)? {
                ReadLineOutcome::Input(line) => {
                    pending_exit = false;
                    line
                }
                ReadLineOutcome::Interrupt => {
                    if pending_exit {
                        println!();
                        break;
                    }
                    if use_color {
                        println!("\n  \x1b[2mpress ctrl+c again to exit\x1b[0m");
                    } else {
                        println!("\n  Press Ctrl+C again to exit.");
                    }
                    pending_exit = true;
                    continue;
                }
                ReadLineOutcome::EndOfFile => {
                    println!();
                    break;
                }
            };

            let line = input.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(action) =
                handle_local_command(client, renderer, &mut file_index, line).await?
            {
                match action {
                    CommandAction::Continue => continue,
                    CommandAction::Exit => break,
                }
            }

            if matches!(
                run_prompt(client, events, renderer, signals, line).await?,
                PromptOutcome::ExitRequested
            ) {
                break;
            }
        }

        return Ok(());
    }

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    loop {
        print_prompt(client.session_snapshot().cwd())?;

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

        if let Some(action) = handle_local_command(client, renderer, &mut file_index, line).await? {
            match action {
                CommandAction::Continue => continue,
                CommandAction::Exit => break,
            }
        }

        if matches!(
            run_prompt(client, events, renderer, signals, line).await?,
            PromptOutcome::ExitRequested
        ) {
            break;
        }
    }

    Ok(())
}

async fn run_prompt(
    client: &AcpClient,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    renderer: &mut Renderer,
    signals: &mut SignalState,
    prompt: &str,
) -> Result<PromptOutcome> {
    renderer.start_turn();

    let prompt_future = client.prompt(prompt);
    tokio::pin!(prompt_future);

    let mut cancel_sent = false;
    loop {
        tokio::select! {
            result = &mut prompt_future => {
                let result = result?;
                drain_events(events, renderer);
                renderer.finish_turn(&result);
                return Ok(PromptOutcome::Completed);
            }
            maybe_event = events.recv() => {
                if let Some(event) = maybe_event {
                    match event {
                        AppEvent::PermissionRequest(request) => {
                            handle_permission_request(renderer, request)?;
                        }
                        other => renderer.render_event(other),
                    }
                }
            }
            signal = signals.recv() => {
                match signal {
                    ShutdownSignal::Sigint if !cancel_sent => {
                        client.cancel_current_turn().await?;
                        renderer.render_event(AppEvent::Activity("cancelling".to_string()));
                        cancel_sent = true;
                    }
                    ShutdownSignal::Sigint | ShutdownSignal::Sigterm => {
                        return Ok(PromptOutcome::ExitRequested);
                    }
                }
            }
        }
    }
}

async fn handle_local_command(
    client: &mut AcpClient,
    renderer: &mut Renderer,
    file_index: &mut FileIndex,
    line: &str,
) -> Result<Option<CommandAction>> {
    match line {
        "/exit" => Ok(Some(CommandAction::Exit)),
        "/clear" => {
            renderer.clear_screen();
            Ok(Some(CommandAction::Continue))
        }
        "/cwd" => {
            renderer.print_status(&client.session_snapshot().cwd().display().to_string());
            Ok(Some(CommandAction::Continue))
        }
        "/new" => {
            let cwd = client.session_snapshot().cwd().to_path_buf();
            client.new_session(&cwd).await?;
            *file_index = build_file_index(&cwd, renderer);
            renderer.print_status("new session");
            Ok(Some(CommandAction::Continue))
        }
        "/model" => {
            let session = client.session_snapshot();
            match input::pick_model(session.models())? {
                Some(selection) => {
                    client.set_model(&selection.id).await?;
                    renderer.print_status(&format!("model: {}", selection.name));
                }
                None => renderer.print_status("model unchanged"),
            }
            Ok(Some(CommandAction::Continue))
        }
        "/help" => {
            renderer.print_status("/exit /clear /cwd /new /model /help");
            Ok(Some(CommandAction::Continue))
        }
        _ => Ok(None),
    }
}

fn handle_permission_request(
    renderer: &mut Renderer,
    request: crate::types::PermissionRequestView,
) -> Result<()> {
    renderer.prepare_for_input();

    let summary_options = request.options.clone();
    let use_color = std::env::var("NO_COLOR").is_err() && input::is_interactive_terminal();
    let decision = if input::is_interactive_terminal() {
        input::pick_permission(&request, use_color)?
    } else {
        fallback_permission(&request.options)
    };

    let summary = permission_summary(&decision, &summary_options);
    let _ = request.responder.send(decision);
    renderer.print_status(&summary);
    Ok(())
}

fn fallback_permission(options: &[PermissionOptionView]) -> PermissionDecision {
    options
        .iter()
        .find(|option| option.kind.is_accept())
        .or_else(|| options.first())
        .map(|option| PermissionDecision::Selected(option.option_id.clone()))
        .unwrap_or(PermissionDecision::Cancelled)
}

fn permission_summary(decision: &PermissionDecision, options: &[PermissionOptionView]) -> String {
    match decision {
        PermissionDecision::Selected(option_id) => options
            .iter()
            .find(|option| &option.option_id == option_id)
            .map(|option| option.name.to_lowercase())
            .unwrap_or_else(|| "selected permission option".to_string()),
        PermissionDecision::Cancelled => "cancelled permission request".to_string(),
    }
}

fn prompt_commands(session: &SessionState) -> Vec<SlashCommand> {
    let mut commands = local_commands();
    commands.extend_from_slice(session.commands());
    commands
}

fn local_commands() -> Vec<SlashCommand> {
    vec![
        local_command("help", "show local commands", None),
        local_command("model", "change the active model", None),
        local_command("new", "start a fresh session", None),
        local_command("cwd", "print the current working directory", None),
        local_command("clear", "clear the terminal", None),
        local_command("exit", "exit mini-code", None),
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

fn drain_events(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    renderer: &mut Renderer,
) {
    while let Ok(event) = events.try_recv() {
        match event {
            AppEvent::PermissionRequest(request) => {
                let _ = request.responder.send(PermissionDecision::Cancelled);
            }
            other => renderer.render_event(other),
        }
    }
}

fn build_file_index(cwd: &Path, renderer: &mut Renderer) -> FileIndex {
    match FileIndex::build(cwd) {
        Ok(index) => index,
        Err(error) => {
            renderer.print_status(&format!("warning: failed to index files: {error}"));
            FileIndex::default()
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

fn prompt_label(cwd: &Path, color: bool) -> String {
    let label = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(cwd.to_str().unwrap_or("."));
    if color {
        format!("\x1b[1;36m{label}\x1b[0m \x1b[36m>\x1b[0m ")
    } else {
        format!("{label}> ")
    }
}

fn print_prompt(cwd: &Path) -> Result<()> {
    let prompt = prompt_label(cwd, false);
    print!("{prompt}");
    io::stdout().flush().context("failed to flush prompt")
}

struct SignalState {
    sigint: Option<tokio::signal::unix::Signal>,
    sigterm: Option<tokio::signal::unix::Signal>,
}

impl SignalState {
    fn new() -> Result<Self> {
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

    async fn recv(&mut self) -> ShutdownSignal {
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
