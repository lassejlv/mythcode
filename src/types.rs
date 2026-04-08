use std::fmt;
use std::path::PathBuf;

use agent_client_protocol as acp;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub cwd: PathBuf,
    pub debug: bool,
    pub model: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug)]
pub enum AppEvent {
    AssistantText(String),
    Activity(String),
    ModeChanged(String),
    SessionTitle(String),
    ToolDiff(DiffPreview),
    PermissionRequest(PermissionRequestView),
    Warning(String),
    DebugProtocol(String),
    ProcessStderr(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub hint: Option<String>,
    pub source: SlashCommandSource,
}

impl SlashCommand {
    pub fn display_name(&self) -> String {
        format!("/{}", self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandSource {
    Local,
    Agent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionModels {
    pub current_model_id: Option<String>,
    pub available: Vec<ModelOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

impl fmt::Display for ModelOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPreview {
    pub path: PathBuf,
    pub old_text: Option<String>,
    pub new_text: String,
}

#[derive(Debug)]
pub struct PermissionRequestView {
    pub title: String,
    pub subtitle: Option<String>,
    pub options: Vec<PermissionOptionView>,
    pub locations: Vec<String>,
    pub responder: oneshot::Sender<PermissionDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOptionView {
    pub option_id: String,
    pub name: String,
    pub kind: PermissionOptionKindView,
}

impl fmt::Display for PermissionOptionView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOptionKindView {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Other,
}

impl PermissionOptionKindView {
    pub fn is_accept(&self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways)
    }
}

impl fmt::Display for PermissionOptionKindView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::AllowOnce => "allow once",
            Self::AllowAlways => "allow always",
            Self::RejectOnce => "deny once",
            Self::RejectAlways => "deny always",
            Self::Other => "option",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Selected(String),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    Continue,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOutcome {
    Completed,
    ExitRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
}

#[derive(Debug, Clone)]
pub struct PromptResult {
    pub stop_reason: acp::StopReason,
}
