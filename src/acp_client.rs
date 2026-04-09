use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use agent_client_protocol::{self as acp, Agent as _};
use anyhow::{Context, Result, anyhow};
use tokio::sync::{mpsc, oneshot};

use crate::process::AcpProcess;
use crate::session::SessionState;
use crate::types::{
    AppConfig, AppEvent, DiffPreview, PermissionDecision, PermissionOptionKindView,
    PermissionOptionView, PermissionRequestView, PlanEntryStatus, PlanEntryView, PlanView,
    PromptResult, SessionModels, SlashCommand, SlashCommandSource, ToolOutputView,
};

pub struct AcpClient {
    conn: acp::ClientSideConnection,
    process: AcpProcess,
    state: Rc<RefCell<SessionState>>,
    turn_cancelled: Rc<Cell<bool>>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
    model: Option<String>,
}

pub struct ConnectedClient {
    pub client: AcpClient,
    pub events: mpsc::UnboundedReceiver<AppEvent>,
}

struct ClientHandler {
    event_tx: mpsc::UnboundedSender<AppEvent>,
    turn_cancelled: Rc<Cell<bool>>,
    state: Rc<RefCell<SessionState>>,
    tool_calls: Rc<RefCell<HashMap<String, acp::ToolCall>>>,
}

impl AcpClient {
    pub async fn connect(config: &AppConfig) -> Result<ConnectedClient> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (process, transport) = AcpProcess::spawn(config, event_tx.clone()).await?;
        let turn_cancelled = Rc::new(Cell::new(false));
        let tool_calls = Rc::new(RefCell::new(HashMap::new()));

        let initial_state = Rc::new(RefCell::new(SessionState::new(
            acp::SessionId::new("pending"),
            config.cwd.clone(),
        )));

        let handler = ClientHandler {
            event_tx: event_tx.clone(),
            turn_cancelled: Rc::clone(&turn_cancelled),
            state: Rc::clone(&initial_state),
            tool_calls,
        };

        let (conn, io_task) =
            acp::ClientSideConnection::new(handler, transport.stdin, transport.stdout, |future| {
                tokio::task::spawn_local(future);
            });

        let io_events = event_tx.clone();
        tokio::task::spawn_local(async move {
            if let Err(error) = io_task.await {
                let _ = io_events.send(AppEvent::Warning(format!("ACP I/O ended: {error}")));
            }
        });

        if config.debug {
            let mut subscription = conn.subscribe();
            let debug_tx = event_tx.clone();
            tokio::task::spawn_local(async move {
                while let Ok(message) = subscription.recv().await {
                    let arrow = match message.direction {
                        acp::StreamMessageDirection::Incoming => "<-",
                        acp::StreamMessageDirection::Outgoing => "->",
                    };
                    let summary = match message.message {
                        acp::StreamMessageContent::Request { method, .. } => {
                            format!("{arrow} request {method}")
                        }
                        acp::StreamMessageContent::Response { id, result } => match result {
                            Ok(_) => format!("{arrow} response #{id:?} ok"),
                            Err(error) => format!("{arrow} response #{id:?} error {error}"),
                        },
                        acp::StreamMessageContent::Notification { method, .. } => {
                            format!("{arrow} notification {method}")
                        }
                    };
                    let _ = debug_tx.send(AppEvent::DebugProtocol(summary));
                }
            });
        }

        let initialize = acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_info(
            acp::Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                .title("mythcode"),
        );

        if let Err(error) = conn.initialize(initialize).await {
            return Err(attach_startup_context(error, &process, &config.cwd).await);
        }

        let session = new_session_inner(
            &conn,
            &process,
            &event_tx,
            &config.cwd,
            config.model.as_deref(),
        )
        .await?;
        *initial_state.borrow_mut() = session;

        Ok(ConnectedClient {
            client: Self {
                conn,
                process,
                state: initial_state,
                turn_cancelled,
                event_tx,
                model: config.model.clone(),
            },
            events: event_rx,
        })
    }

    pub async fn prompt(&self, prompt: &str) -> Result<PromptResult> {
        self.turn_cancelled.set(false);
        let session_id = self.state.borrow().id().clone();
        let response = self
            .conn
            .prompt(acp::PromptRequest::new(
                session_id,
                vec![prompt.to_string().into()],
            ))
            .await
            .context("ACP prompt failed")?;

        self.turn_cancelled.set(false);
        Ok(PromptResult {
            stop_reason: response.stop_reason,
        })
    }

    pub async fn cancel_current_turn(&self) -> Result<()> {
        self.turn_cancelled.set(true);
        let session_id = self.state.borrow().id().clone();
        self.conn
            .cancel(acp::CancelNotification::new(session_id))
            .await
            .context("failed to send session/cancel")
    }

    pub async fn new_session(&mut self, cwd: &Path) -> Result<()> {
        self.turn_cancelled.set(false);
        let session = new_session_inner(
            &self.conn,
            &self.process,
            &self.event_tx,
            cwd,
            self.model.as_deref(),
        )
        .await?;
        *self.state.borrow_mut() = session;
        Ok(())
    }

    pub async fn set_model(&mut self, model_id: &str) -> Result<()> {
        let snapshot = self.state.borrow().clone();
        set_model_inner(&self.conn, &snapshot, &self.event_tx, model_id).await?;
        self.state
            .borrow_mut()
            .set_current_model_id(Some(model_id.to_string()));
        self.model = Some(model_id.to_string());
        Ok(())
    }

    pub async fn set_mode(&self, mode_id: &str) -> Result<()> {
        let session_id = self.state.borrow().id().clone();
        self.conn
            .set_session_mode(acp::SetSessionModeRequest::new(
                session_id,
                mode_id.to_string(),
            ))
            .await
            .context("failed to set mode")?;
        self.state
            .borrow_mut()
            .set_current_mode(Some(mode_id.to_string()));
        Ok(())
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionListItem>> {
        let cwd = self.state.borrow().cwd().to_path_buf();
        let response = self
            .conn
            .list_sessions(acp::ListSessionsRequest::new().cwd(cwd))
            .await
            .context("failed to list sessions")?;
        Ok(response
            .sessions
            .into_iter()
            .map(|s| SessionListItem {
                id: s.session_id.to_string(),
                title: s.title.unwrap_or_else(|| "(untitled)".to_string()),
                updated_at: s.updated_at,
            })
            .collect())
    }

    pub async fn resume_session(&mut self, session_id: &str) -> Result<()> {
        let cwd = self.state.borrow().cwd().to_path_buf();
        let response = self
            .conn
            .resume_session(acp::ResumeSessionRequest::new(
                session_id.to_string(),
                cwd.clone(),
            ))
            .await
            .context("failed to resume session")?;

        let mut session = SessionState::new(session_id.to_string(), cwd);

        if let Some(models) = &response.models {
            session.set_models(SessionModels {
                current_model_id: Some(models.current_model_id.to_string()),
                available: models
                    .available_models
                    .iter()
                    .map(|m| crate::types::ModelOption {
                        id: m.model_id.to_string(),
                        name: m.name.clone(),
                        description: m.description.clone(),
                    })
                    .collect(),
            });
        }

        if let Some(modes) = &response.modes {
            session.set_current_mode(Some(modes.current_mode_id.to_string()));
            session.set_available_modes(
                modes
                    .available_modes
                    .iter()
                    .map(|m| crate::session::ModeOption {
                        id: m.id.to_string(),
                        name: m.name.clone(),
                    })
                    .collect(),
            );
        }

        // Re-apply model override if set
        if let Some(ref model) = self.model {
            if let Err(e) = set_model_inner(&self.conn, &session, &self.event_tx, model).await {
                let _ = self.event_tx.send(AppEvent::Warning(e.to_string()));
            } else {
                session.set_current_model_id(Some(model.clone()));
            }
        }

        *self.state.borrow_mut() = session;
        Ok(())
    }

    pub fn session_snapshot(&self) -> SessionState {
        self.state.borrow().clone()
    }

    pub async fn shutdown(&self) {
        self.process.shutdown().await;
    }
}

#[derive(Debug, Clone)]
pub struct SessionListItem {
    pub id: String,
    pub title: String,
    pub updated_at: Option<String>,
}

async fn new_session_inner(
    conn: &acp::ClientSideConnection,
    process: &AcpProcess,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    cwd: &Path,
    model: Option<&str>,
) -> Result<SessionState> {
    let response = match conn
        .new_session(acp::NewSessionRequest::new(cwd.to_path_buf()))
        .await
    {
        Ok(response) => response,
        Err(error) => return Err(attach_startup_context(error, process, cwd).await),
    };

    let mut session = SessionState::new(response.session_id.clone(), cwd.to_path_buf());
    session.set_models(session_models_from_response(&response));

    // Extract modes if available
    if let Some(modes) = &response.modes {
        session.set_current_mode(Some(modes.current_mode_id.to_string()));
        session.set_available_modes(
            modes
                .available_modes
                .iter()
                .map(|m| crate::session::ModeOption {
                    id: m.id.to_string(),
                    name: m.name.clone(),
                })
                .collect(),
        );
    }

    if let Some(model) = model {
        if let Err(error) = set_model_inner(conn, &session, event_tx, model).await {
            let _ = event_tx.send(AppEvent::Warning(error.to_string()));
        } else {
            session.set_current_model_id(Some(model.to_string()));
        }
    }

    Ok(session)
}

async fn set_model_inner(
    conn: &acp::ClientSideConnection,
    session: &SessionState,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    model: &str,
) -> Result<()> {
    let result = conn
        .set_session_model(acp::SetSessionModelRequest::new(
            session.id().clone(),
            model.to_string(),
        ))
        .await;

    match result {
        Ok(_) => {
            let label = session
                .models()
                .available
                .iter()
                .find(|candidate| candidate.id == model)
                .map(|candidate| candidate.name.clone())
                .unwrap_or_else(|| model.to_string());
            let _ = event_tx.send(AppEvent::Activity(format!("model: {label}")));
            Ok(())
        }
        Err(error) => Err(anyhow!(
            "model override `{model}` was not accepted by the agent: {error}"
        )),
    }
}

async fn attach_startup_context(
    error: acp::Error,
    process: &AcpProcess,
    cwd: &Path,
) -> anyhow::Error {
    match process.try_wait().await {
        Ok(Some(_)) => anyhow!("{error}\n{}", process.startup_context(cwd).await),
        Ok(None) => anyhow!(error),
        Err(wait_error) => anyhow!("{error}\nfailed to inspect opencode process: {wait_error}"),
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for ClientHandler {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        if self.turn_cancelled.get() {
            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ));
        }

        let (response_tx, response_rx) = oneshot::channel();
        let request = PermissionRequestView {
            title: tool_update_title(&args.tool_call),
            subtitle: args
                .tool_call
                .fields
                .kind
                .as_ref()
                .map(|kind| format!("{kind:?}").replace('_', " ").to_lowercase()),
            options: args
                .options
                .iter()
                .map(|option| PermissionOptionView {
                    option_id: option.option_id.to_string(),
                    name: option.name.clone(),
                    kind: permission_kind(&option.kind),
                })
                .collect(),
            locations: args
                .tool_call
                .fields
                .locations
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|location| location.path.display().to_string())
                .collect(),
            responder: response_tx,
        };

        if self
            .event_tx
            .send(AppEvent::PermissionRequest(request))
            .is_err()
        {
            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ));
        }

        let outcome = match response_rx.await {
            Ok(PermissionDecision::Selected(option_id)) => acp::RequestPermissionOutcome::Selected(
                acp::SelectedPermissionOutcome::new(option_id),
            ),
            Ok(PermissionDecision::Cancelled) | Err(_) => acp::RequestPermissionOutcome::Cancelled,
        };

        Ok(acp::RequestPermissionResponse::new(outcome))
    }

    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal(
        &self,
        _args: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        match args.update {
            acp::SessionUpdate::UserMessageChunk(chunk) => {
                if let Some(text) = content_text(&chunk.content) {
                    let _ = self.event_tx.send(AppEvent::UserMessage(text));
                }
            }
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                if let Some(text) = content_text(&chunk.content) {
                    let _ = self.event_tx.send(AppEvent::AssistantText(text));
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                self.tool_calls
                    .borrow_mut()
                    .insert(tool_call.tool_call_id.to_string(), tool_call.clone());
                let title = tool_call_title(&tool_call);
                let _ = self.event_tx.send(AppEvent::Activity(title.clone()));
                emit_diffs(&self.event_tx, extract_diff_previews(&tool_call.content));
                emit_tool_output(&self.event_tx, &title, &tool_call.content);
            }
            acp::SessionUpdate::ToolCallUpdate(update) => {
                let mut tool_calls = self.tool_calls.borrow_mut();
                let key = update.tool_call_id.to_string();
                let previous = tool_calls.get(&key).cloned();
                let next = if let Some(existing) = tool_calls.get_mut(&key) {
                    existing.update(update.fields.clone());
                    existing.clone()
                } else if let Ok(tool_call) = acp::ToolCall::try_from(update.clone()) {
                    tool_calls.insert(key.clone(), tool_call.clone());
                    tool_call
                } else {
                    drop(tool_calls);
                    if let Some(title) = update.fields.title.clone() {
                        let _ = self.event_tx.send(AppEvent::Activity(title));
                    }
                    return Ok(());
                };
                drop(tool_calls);

                if let Some(title) = update.fields.title {
                    let _ = self.event_tx.send(AppEvent::Activity(title));
                }

                if previous.as_ref().map(|tool| &tool.content) != Some(&next.content) {
                    emit_diffs(&self.event_tx, extract_diff_previews(&next.content));
                    let title = next.title.clone();
                    emit_tool_output(&self.event_tx, &title, &next.content);
                }
            }
            acp::SessionUpdate::AvailableCommandsUpdate(update) => {
                self.state
                    .borrow_mut()
                    .set_commands(commands_from_update(update.available_commands));
            }
            acp::SessionUpdate::CurrentModeUpdate(update) => {
                self.state
                    .borrow_mut()
                    .set_current_mode(Some(update.current_mode_id.to_string()));
                let _ = self
                    .event_tx
                    .send(AppEvent::ModeChanged(update.current_mode_id.to_string()));
            }
            acp::SessionUpdate::SessionInfoUpdate(update) => {
                let title = update.title.value().cloned();
                self.state.borrow_mut().set_title(title.clone());
                if let Some(title) = title {
                    let _ = self.event_tx.send(AppEvent::SessionTitle(title));
                }
            }
            acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                if let Some(text) = content_text(&chunk.content) {
                    let _ = self.event_tx.send(AppEvent::ThinkingText(text));
                }
            }
            acp::SessionUpdate::Plan(plan) => {
                let entries = plan
                    .entries
                    .iter()
                    .map(|e| PlanEntryView {
                        content: e.content.clone(),
                        status: match e.status {
                            acp::PlanEntryStatus::Pending => PlanEntryStatus::Pending,
                            acp::PlanEntryStatus::InProgress => PlanEntryStatus::InProgress,
                            acp::PlanEntryStatus::Completed => PlanEntryStatus::Completed,
                            _ => PlanEntryStatus::Pending,
                        },
                    })
                    .collect();
                let _ = self.event_tx.send(AppEvent::PlanUpdate(PlanView { entries }));
            }
            _ => {}
        }

        Ok(())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Err(acp::Error::method_not_found())
    }
}

fn session_models_from_response(response: &acp::NewSessionResponse) -> SessionModels {
    if let Some(models) = &response.models {
        return SessionModels {
            current_model_id: Some(models.current_model_id.to_string()),
            available: models
                .available_models
                .iter()
                .map(|model| crate::types::ModelOption {
                    id: model.model_id.to_string(),
                    name: model.name.clone(),
                    description: model.description.clone(),
                })
                .collect(),
        };
    }

    SessionModels::default()
}

fn commands_from_update(commands: Vec<acp::AvailableCommand>) -> Vec<SlashCommand> {
    commands
        .into_iter()
        .map(|command| SlashCommand {
            name: command.name,
            description: command.description,
            hint: command.input.and_then(command_hint),
            source: SlashCommandSource::Agent,
        })
        .collect()
}

fn command_hint(input: acp::AvailableCommandInput) -> Option<String> {
    match input {
        acp::AvailableCommandInput::Unstructured(input) => Some(input.hint),
        _ => None,
    }
}

fn permission_kind(kind: &acp::PermissionOptionKind) -> PermissionOptionKindView {
    match kind {
        acp::PermissionOptionKind::AllowOnce => PermissionOptionKindView::AllowOnce,
        acp::PermissionOptionKind::AllowAlways => PermissionOptionKindView::AllowAlways,
        acp::PermissionOptionKind::RejectOnce => PermissionOptionKindView::RejectOnce,
        acp::PermissionOptionKind::RejectAlways => PermissionOptionKindView::RejectAlways,
        _ => PermissionOptionKindView::Other,
    }
}

fn emit_tool_output(
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    title: &str,
    content: &[acp::ToolCallContent],
) {
    let mut text = String::new();
    for item in content {
        if let acp::ToolCallContent::Content(c) = item {
            if let acp::ContentBlock::Text(t) = &c.content {
                text.push_str(&t.text);
            }
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    // Filter out lines that are just XML-like tags or structured metadata
    let clean_lines: Vec<&str> = trimmed
        .lines()
        .filter(|line| {
            let t = line.trim();
            // Skip XML-like tags
            if t.starts_with('<') && t.ends_with('>') {
                return false;
            }
            // Skip empty lines at this stage
            if t.is_empty() {
                return false;
            }
            true
        })
        .collect();

    if clean_lines.is_empty() {
        return;
    }

    let clean_text = clean_lines.join("\n");
    let total_lines = clean_lines.len();

    let _ = event_tx.send(AppEvent::ToolOutput(ToolOutputView {
        title: title.to_string(),
        content: clean_text,
        total_lines,
    }));
}

fn emit_diffs(event_tx: &mpsc::UnboundedSender<AppEvent>, diffs: Vec<DiffPreview>) {
    for diff in diffs {
        let _ = event_tx.send(AppEvent::ToolDiff(diff));
    }
}

fn extract_diff_previews(content: &[acp::ToolCallContent]) -> Vec<DiffPreview> {
    content
        .iter()
        .filter_map(|item| match item {
            acp::ToolCallContent::Diff(diff) => Some(DiffPreview {
                path: diff.path.clone(),
                old_text: diff.old_text.clone(),
                new_text: diff.new_text.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn content_text(content: &acp::ContentBlock) -> Option<String> {
    match content {
        acp::ContentBlock::Text(text) => Some(text.text.clone()),
        acp::ContentBlock::ResourceLink(resource) => Some(resource.uri.clone()),
        acp::ContentBlock::Image(_) => Some("<image>".to_string()),
        acp::ContentBlock::Audio(_) => Some("<audio>".to_string()),
        acp::ContentBlock::Resource(_) => Some("<resource>".to_string()),
        _ => None,
    }
}

fn tool_update_title(tool_call: &acp::ToolCallUpdate) -> String {
    if let Some(title) = &tool_call.fields.title {
        return title.clone();
    }

    if let Some(kind) = &tool_call.fields.kind {
        return format!("{kind:?}").replace('_', " ").to_lowercase();
    }

    "tool activity".to_string()
}

fn tool_call_title(tool_call: &acp::ToolCall) -> String {
    if !tool_call.title.is_empty() {
        return tool_call.title.clone();
    }

    format!("{:?}", tool_call.kind)
        .replace('_', " ")
        .to_lowercase()
}
