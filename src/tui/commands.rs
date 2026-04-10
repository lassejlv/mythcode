/// Local slash-command handling.

use anyhow::Result;

use crate::acp_client::AcpClient;
use crate::input::FileIndex;
use crate::types::CommandAction;

use super::select::{SelectItem, SelectKind, SelectMode};
use super::history::{format_status, format_warning, LineType};
use super::{C_BOLD_CYAN, C_DIM, C_RESET, Tui};

impl Tui {
    pub(super) async fn handle_local_command(
        &mut self,
        client: &mut AcpClient,
        file_index: &mut FileIndex,
        line: &str,
    ) -> Result<Option<CommandAction>> {
        match line {
            "/exit" => Ok(Some(CommandAction::Exit)),
            "/clear" => {
                self.history.clear();
                Ok(Some(CommandAction::Continue))
            }
            "/cwd" => {
                let cwd = client.session_snapshot().cwd().display().to_string();
                self.history.push(format_status(&cwd), LineType::Status);
                Ok(Some(CommandAction::Continue))
            }
            "/new" => {
                let cwd = client.session_snapshot().cwd().to_path_buf();
                client.new_session(&cwd).await?;
                *file_index = crate::cli::build_file_index(&cwd);
                self.history
                    .push(format_status("new session"), LineType::Status);
                Ok(Some(CommandAction::Continue))
            }
            "/resume" => {
                match client.list_sessions().await {
                    Ok(sessions) if !sessions.is_empty() => {
                        let items: Vec<SelectItem> = sessions
                            .into_iter()
                            .map(|s| {
                                let time_hint = s.updated_at.as_deref().unwrap_or("");
                                SelectItem {
                                    id: s.id,
                                    display: format!("{} {}", s.title, C_DIM.to_string() + time_hint + C_RESET),
                                }
                            })
                            .collect();
                        self.select_mode = Some(SelectMode::new("Resume session", items, SelectKind::Resume));
                    }
                    Ok(_) => {
                        self.history.push(format_status("no sessions found"), LineType::Status);
                    }
                    Err(e) => {
                        self.history.push(format_warning(&format!("failed to list sessions: {e}")), LineType::Warning);
                    }
                }
                Ok(Some(CommandAction::Continue))
            }
            "/model" => {
                let session = client.session_snapshot();
                let models = session.models();
                if models.available.is_empty() {
                    self.history
                        .push(format_status("no models available"), LineType::Status);
                } else {
                    let items: Vec<SelectItem> = models
                        .available
                        .iter()
                        .map(|m| {
                            let current = models.current_model_id.as_deref() == Some(&m.id);
                            let marker = if current { " ●" } else { "" };
                            SelectItem {
                                id: m.id.clone(),
                                display: format!("{}{marker}", m.name),
                            }
                        })
                        .collect();
                    self.select_mode = Some(SelectMode::new("Select model", items, SelectKind::Model));
                }
                Ok(Some(CommandAction::Continue))
            }
            "/help" => {
                self.history.push(String::new(), LineType::Status);

                // Commands section
                self.history.push(
                    format!("  {C_BOLD_CYAN}Commands{C_RESET}"),
                    LineType::Status,
                );
                self.history.push(String::new(), LineType::Status);
                for (cmd, desc) in [
                    ("/help", "show this help"),
                    ("/clear", "clear the screen"),
                    ("/cwd", "show working directory"),
                    ("/new", "start a new session"),
                    ("/resume", "resume a previous session"),
                    ("/model", "select a model"),
                    ("/extensions", "show loaded extensions"),
                    ("/exit", "exit mythcode"),
                ] {
                    self.history.push(
                        format!("    \x1b[38;5;75m{cmd:<12}{C_RESET}{C_DIM}{desc}{C_RESET}"),
                        LineType::Status,
                    );
                }

                // Agent commands
                let agent_commands = client.session_snapshot().commands().to_vec();
                if !agent_commands.is_empty() {
                    self.history.push(String::new(), LineType::Status);
                    self.history.push(
                        format!("  {C_BOLD_CYAN}Agent Commands{C_RESET}"),
                        LineType::Status,
                    );
                    self.history.push(String::new(), LineType::Status);
                    for cmd in &agent_commands {
                        let name = format!("/{}", cmd.name);
                        self.history.push(
                            format!("    \x1b[38;5;75m{name:<12}{C_RESET}{C_DIM}{}{C_RESET}", cmd.description),
                            LineType::Status,
                        );
                    }
                }

                // Extension commands
                if !self.extension_commands.is_empty() {
                    self.history.push(String::new(), LineType::Status);
                    self.history.push(
                        format!("  {C_BOLD_CYAN}Extension Commands{C_RESET}"),
                        LineType::Status,
                    );
                    self.history.push(String::new(), LineType::Status);
                    for cmd in &self.extension_commands {
                        let name = format!("/{}", cmd.name);
                        self.history.push(
                            format!("    \x1b[38;5;176m{name:<12}{C_RESET}{C_DIM}{}{C_RESET}", cmd.description),
                            LineType::Status,
                        );
                    }
                }

                // Keybindings section
                self.history.push(String::new(), LineType::Status);
                self.history.push(
                    format!("  {C_BOLD_CYAN}Shortcuts{C_RESET}"),
                    LineType::Status,
                );
                self.history.push(String::new(), LineType::Status);
                for (key, desc) in [
                    ("enter", "send message"),
                    ("shift+enter", "new line"),
                    ("tab", "autocomplete / queue message"),
                    ("shift+tab", "cycle modes"),
                    ("ctrl+o", "expand last tool output"),
                    ("ctrl+c", "cancel / exit"),
                    ("ctrl+d", "exit"),
                    ("pgup/pgdn", "scroll history"),
                    ("@file", "mention a file"),
                ] {
                    self.history.push(
                        format!("    {C_DIM}{key:<14}{C_RESET}{C_DIM}{desc}{C_RESET}"),
                        LineType::Status,
                    );
                }

                self.history.push(String::new(), LineType::Status);
                Ok(Some(CommandAction::Continue))
            }
            "/extensions" | "/ext" => {
                self.history.push(String::new(), LineType::Status);
                self.history.push(
                    format!("  {C_BOLD_CYAN}Extensions{C_RESET}"),
                    LineType::Status,
                );
                self.history.push(String::new(), LineType::Status);

                if self.extension_commands.is_empty() {
                    self.history.push(
                        format!("  {C_DIM}no extensions loaded{C_RESET}"),
                        LineType::Status,
                    );
                    self.history.push(String::new(), LineType::Status);
                    self.history.push(
                        format!("  {C_DIM}add .ts files to ~/.mythcode/extensions/ or .mythcode/extensions/{C_RESET}"),
                        LineType::Status,
                    );
                } else {
                    // Group commands by showing them as registered
                    for cmd in &self.extension_commands {
                        let name = format!("/{}", cmd.name);
                        self.history.push(
                            format!("  \x1b[38;5;114m●{C_RESET} {name}  {C_DIM}{}{C_RESET}", cmd.description),
                            LineType::Status,
                        );
                    }
                }

                self.history.push(String::new(), LineType::Status);
                Ok(Some(CommandAction::Continue))
            }
            _ => Ok(None),
        }
    }
}
