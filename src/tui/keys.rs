/// Key handling and autocomplete suggestions.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::acp_client::AcpClient;
use crate::input::FileIndex;
use crate::types::PermissionDecision;

use super::history::{format_status, LineType};
use super::select::SelectKind;
use super::{C_DIM, C_RESET, KeyAction, Tui};

impl Tui {
    pub(super) async fn handle_key(
        &mut self,
        key: KeyEvent,
        client: &mut AcpClient,
        pending_exit: &mut bool,
        file_index: &mut FileIndex,
    ) -> Result<KeyAction> {
        // Select mode (model picker, etc.)
        if let Some(ref mut sel) = self.select_mode {
            match key.code {
                KeyCode::Up => {
                    sel.selected = sel.selected.saturating_sub(1);
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Down => {
                    if sel.selected + 1 < sel.filtered.len() {
                        sel.selected += 1;
                    }
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Enter => {
                    let sel = self.select_mode.take().unwrap();
                    if let Some(item) = sel.selected_item() {
                        match sel.kind {
                            SelectKind::Model => {
                                let id = item.id.clone();
                                let name = item.display.clone();
                                client.set_model(&id).await?;
                                self.current_model = Some(id);
                                self.history.push(
                                    format_status(&format!("model → {name}")),
                                    LineType::Status,
                                );
                            }
                            SelectKind::Resume => {
                                let id = item.id.clone();
                                let name = item.display.clone();
                                client.resume_session(&id).await?;
                                self.current_mode = client.session_snapshot().current_mode().map(|s| s.to_string());
                                *file_index = crate::cli::build_file_index(client.session_snapshot().cwd());
                                self.history.clear();
                                self.history.push(String::new(), LineType::Status);
                                self.history.push(
                                    format_status(&format!("resumed: {name}")),
                                    LineType::Status,
                                );
                                self.history.push(String::new(), LineType::Status);
                            }
                        }
                    }
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Esc => {
                    self.select_mode = None;
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Backspace => {
                    let sel = self.select_mode.as_mut().unwrap();
                    sel.filter.pop();
                    sel.update_filter();
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Char(ch) => {
                    let sel = self.select_mode.as_mut().unwrap();
                    sel.filter.push(ch);
                    sel.update_filter();
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                _ => return Ok(KeyAction::Continue),
            }
        }

        // Permission mode
        if let Some(ref mut perm) = self.pending_permission {
            match key.code {
                KeyCode::Up => {
                    if perm.selected > 0 {
                        perm.selected -= 1;
                    }
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Down => {
                    if perm.selected + 1 < perm.options.len() {
                        perm.selected += 1;
                    }
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Enter => {
                    let perm = self.pending_permission.take().unwrap();
                    let option = &perm.options[perm.selected];
                    let decision = PermissionDecision::Selected(option.option_id.clone());
                    let summary = option.name.to_lowercase();
                    let _ = perm.responder.send(decision);
                    self.history.push(format_status(&summary), LineType::Status);
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Esc => {
                    let perm = self.pending_permission.take().unwrap();
                    let _ = perm.responder.send(PermissionDecision::Cancelled);
                    self.history.push(format_status("cancelled"), LineType::Status);
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                _ => return Ok(KeyAction::Continue),
            }
        }

        // Autocomplete: Enter accepts suggestion into input (does NOT submit)
        if !self.suggestions.is_empty() {
            match key.code {
                KeyCode::Tab | KeyCode::Down => {
                    let idx = match self.selected_suggestion {
                        None => 0,
                        Some(i) => (i + 1) % self.suggestions.len(),
                    };
                    self.selected_suggestion = Some(idx);
                    self.input.set_content(&self.suggestions[idx]);
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Up => {
                    if let Some(i) = self.selected_suggestion {
                        let idx = if i == 0 {
                            self.suggestions.len() - 1
                        } else {
                            i - 1
                        };
                        self.selected_suggestion = Some(idx);
                        self.input.set_content(&self.suggestions[idx]);
                    }
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Enter => {
                    // Accept the current suggestion into input, don't send
                    if let Some(idx) = self.selected_suggestion {
                        self.input.set_content(&self.suggestions[idx]);
                    }
                    self.suggestions.clear();
                    self.selected_suggestion = None;
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                KeyCode::Esc => {
                    self.suggestions.clear();
                    self.selected_suggestion = None;
                    self.redraw()?;
                    return Ok(KeyAction::Continue);
                }
                _ => {
                    self.suggestions.clear();
                    self.selected_suggestion = None;
                    // fall through
                }
            }
        }

        match key.code {
            KeyCode::BackTab => {
                // Shift+Tab: cycle through modes
                let session = client.session_snapshot();
                let modes = session.available_modes();
                if modes.len() > 1 {
                    let current = session.current_mode().unwrap_or("");
                    let current_idx = modes.iter().position(|m| m.id == current).unwrap_or(0);
                    let next_idx = (current_idx + 1) % modes.len();
                    let next_id = modes[next_idx].id.clone();
                    let next_name = modes[next_idx].name.clone();
                    client.set_mode(&next_id).await?;
                    self.history.push(
                        format_status(&format!("mode → {next_name}")),
                        LineType::Status,
                    );
                }
                self.redraw()?;
                return Ok(KeyAction::Continue);
            }
            KeyCode::Enter if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                self.input.insert_newline();
            }
            KeyCode::Enter => {
                let current = self.input.take_content();
                // Combine queued messages with current
                let mut parts = std::mem::take(&mut self.message_queue);
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    parts.push(trimmed);
                }
                let combined = parts.join("\n");
                return Ok(KeyAction::Submit(combined));
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if *pending_exit {
                    return Ok(KeyAction::Exit);
                }
                self.history.push(
                    format!("  {C_DIM}press ctrl+c again to exit{C_RESET}"),
                    LineType::Status,
                );
                *pending_exit = true;
                self.redraw()?;
                return Ok(KeyAction::Continue);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(KeyAction::Exit);
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.delete_word_before();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Expand last tool output fully
                if let Some(output) = self.last_tool_outputs.last() {
                    self.history.push(String::new(), LineType::Status);
                    self.history.push(
                        format!("  {C_DIM}── {} ({} lines) ──{C_RESET}", output.title, output.total_lines),
                        LineType::Status,
                    );
                    for line in output.content.lines() {
                        self.history.push(
                            format!("  {C_DIM}│{C_RESET} \x1b[38;5;245m{line}\x1b[0m"),
                            LineType::Activity,
                        );
                    }
                    self.history.push(
                        format!("  {C_DIM}──────────────────{C_RESET}"),
                        LineType::Status,
                    );
                    self.history.push(String::new(), LineType::Status);
                }
            }
            KeyCode::Char(ch) => {
                *pending_exit = false;
                self.input.insert_char(ch);
                self.update_suggestions_with_client(file_index, client);
            }
            KeyCode::Backspace => {
                self.input.delete_char_before();
                self.update_suggestions_with_client(file_index, client);
            }
            KeyCode::Delete => {
                self.input.delete_char_after();
            }
            KeyCode::Left => self.input.move_left(),
            KeyCode::Right => self.input.move_right(),
            KeyCode::Home => self.input.move_home(),
            KeyCode::End => self.input.move_end(),
            KeyCode::Up => self.input.history_prev(),
            KeyCode::Down => self.input.history_next(),
            KeyCode::PageUp => {
                self.history.scroll_up(self.term_height as usize / 2);
            }
            KeyCode::PageDown => {
                self.history.scroll_down(self.term_height as usize / 2);
            }
            KeyCode::Tab => {
                // If suggestions are available, cycle them
                self.update_suggestions_with_client(file_index, client);
                if !self.suggestions.is_empty() {
                    self.selected_suggestion = Some(0);
                    self.input.set_content(&self.suggestions[0].clone());
                } else {
                    // Queue the current message and clear input for the next
                    let text = self.input.take_content();
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        self.message_queue.push(trimmed.clone());
                        self.history.push(
                            format!("  {C_DIM}queued: {trimmed}{C_RESET}"),
                            LineType::Status,
                        );
                    }
                }
            }
            _ => {}
        }

        self.redraw()?;
        Ok(KeyAction::Continue)
    }

    pub(super) fn update_suggestions_with_client(&mut self, file_index: &FileIndex, client: &AcpClient) {
        let content = self.input.content().to_string();
        self.suggestions.clear();
        self.selected_suggestion = None;

        if content.starts_with('/') && !content.contains(char::is_whitespace) {
            let query = content.trim_start_matches('/').to_lowercase();
            // Merge local + ACP commands
            let mut commands = crate::cli::local_commands();
            commands.extend_from_slice(client.session_snapshot().commands());
            let mut matches: Vec<(u8, u8, String)> = commands
                .iter()
                .filter_map(|cmd| {
                    crate::input::score_command(cmd, &query).map(|(src, rank, _)| {
                        let display = if cmd.hint.is_some() {
                            format!("/{} ", cmd.name)
                        } else {
                            format!("/{}", cmd.name)
                        };
                        (src, rank, display)
                    })
                })
                .collect();
            matches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            self.suggestions = matches.into_iter().take(8).map(|m| m.2).collect();
        }

        if let Some((_start, _end, query)) = crate::input::mention_query(&content) {
            self.suggestions = file_index
                .search(query)
                .into_iter()
                .map(|path| {
                    let at_pos = content.rfind('@').unwrap_or(0);
                    let mut result = content[..at_pos].to_string();
                    result.push('@');
                    result.push_str(&path);
                    result.push(' ');
                    result
                })
                .collect();
        }
    }
}
