use std::path::{Path, PathBuf};

use agent_client_protocol as acp;

use crate::types::{SessionModels, SlashCommand};

#[derive(Debug, Clone)]
pub struct SessionState {
    id: acp::SessionId,
    cwd: PathBuf,
    title: Option<String>,
    current_mode: Option<String>,
    commands: Vec<SlashCommand>,
    models: SessionModels,
}

impl SessionState {
    pub fn new(id: impl Into<acp::SessionId>, cwd: PathBuf) -> Self {
        Self {
            id: id.into(),
            cwd,
            title: None,
            current_mode: None,
            commands: Vec::new(),
            models: SessionModels::default(),
        }
    }

    pub fn id(&self) -> &acp::SessionId {
        &self.id
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn commands(&self) -> &[SlashCommand] {
        &self.commands
    }

    pub fn set_commands(&mut self, commands: Vec<SlashCommand>) {
        self.commands = commands;
    }

    pub fn models(&self) -> &SessionModels {
        &self.models
    }

    pub fn set_models(&mut self, models: SessionModels) {
        self.models = models;
    }

    pub fn set_current_model_id(&mut self, model_id: Option<String>) {
        self.models.current_model_id = model_id;
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    pub fn set_current_mode(&mut self, mode: Option<String>) {
        self.current_mode = mode;
    }
}
