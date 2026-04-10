/// Permission dialog state and handling.
use crate::types::{PermissionDecision, PermissionOptionView};

pub(super) struct PendingPermission {
    #[allow(dead_code)]
    pub title: String,
    #[allow(dead_code)]
    pub subtitle: Option<String>,
    pub options: Vec<PermissionOptionView>,
    #[allow(dead_code)]
    pub locations: Vec<String>,
    pub selected: usize,
    pub responder: tokio::sync::oneshot::Sender<PermissionDecision>,
}
