mod commands;
mod events;
mod highlight;
mod history;
mod input_box;
mod keys;
mod markdown;
mod permission;
mod render;
mod select;
pub mod theme;

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use tokio::sync::mpsc;

use crate::acp_client::AcpClient;
use crate::input::FileIndex;
use crate::terminal_ui::{TerminalGuard, TerminalGuardOptions};
use crate::types::{AppEvent, PermissionDecision, ShutdownSignal, ToolOutputView};

use history::{History, LineType, format_activity, format_user_message};
use input_box::InputBox;
use permission::PendingPermission;
use select::SelectMode;

/// A suggestion entry for the autocomplete menu.
#[derive(Clone)]
pub struct Suggestion {
    /// The value inserted into the input when accepted (e.g. "/help")
    pub value: String,
    /// Display label shown in the menu (e.g. "/help")
    pub label: String,
    /// Optional description shown alongside
    pub description: String,
    /// Source tag (e.g. "local", "agent", "file")
    #[allow(dead_code)]
    pub source: &'static str,
}

const INPUT_BOX_MIN_HEIGHT: u16 = 3;
const MARGIN_TOP: u16 = 1;

// Claude Code-inspired palette
const C_RESET: &str = "\x1b[0m";
const C_DIM: &str = "\x1b[38;5;245m";
const C_CYAN: &str = "\x1b[38;5;75m";
const C_BOLD_CYAN: &str = "\x1b[1;38;5;75m";
const C_DARK: &str = "\x1b[38;5;240m";
const C_SPINNER: &str = "\x1b[38;5;75m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnState {
    Idle,
    Thinking,
    Responding,
    ToolRunning,
    AwaitingPermission,
}

pub struct Tui {
    history: History,
    input: InputBox,
    term_width: u16,
    term_height: u16,
    assistant_buffer: String,
    thinking_buffer: String,
    last_activity: Option<String>,
    activity_line_count: u16,
    pending_permission: Option<PendingPermission>,
    suspended_turn_state: Option<TurnState>,
    select_mode: Option<SelectMode>,
    turn_count: u32,
    spinner_frame: usize,
    spinner_active: bool,
    turn_start: Option<Instant>,
    turn_state: TurnState,
    suggestions: Vec<Suggestion>,
    selected_suggestion: Option<usize>,
    printed_text: bool,
    project_name: String,
    current_mode: Option<String>,
    current_model: Option<String>,
    last_tool_outputs: Vec<ToolOutputView>,
    live_output_lines: usize,
    message_queue: Vec<String>,
    extension_commands: Vec<crate::types::SlashCommand>,
    status_items: std::collections::HashMap<String, String>,
}

enum KeyAction {
    Continue,
    Exit,
    Submit(String),
}

impl Tui {
    pub fn new() -> Self {
        let (w, h) = terminal::size().unwrap_or((80, 24));
        Self {
            history: History::new(),
            input: InputBox::new(),
            term_width: w,
            term_height: h,
            assistant_buffer: String::new(),
            thinking_buffer: String::new(),
            last_activity: None,
            activity_line_count: 0,
            pending_permission: None,
            suspended_turn_state: None,
            select_mode: None,
            turn_count: 0,
            spinner_frame: 0,
            spinner_active: false,
            turn_start: None,
            turn_state: TurnState::Idle,
            suggestions: Vec::new(),
            selected_suggestion: None,
            printed_text: false,
            project_name: String::new(),
            current_mode: None,
            current_model: None,
            last_tool_outputs: Vec::new(),
            live_output_lines: 0,
            message_queue: Vec::new(),
            extension_commands: Vec::new(),
            status_items: std::collections::HashMap::new(),
        }
    }

    pub async fn run(
        &mut self,
        client: &mut AcpClient,
        events: &mut mpsc::UnboundedReceiver<AppEvent>,
        signals: &mut super::cli::SignalState,
        file_index: &mut FileIndex,
        ext_host: &mut Option<crate::extensions::ExtensionHost>,
    ) -> Result<()> {
        let session = client.session_snapshot();
        self.project_name = session
            .cwd()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .to_string();
        let model_name = session
            .models()
            .current_model_id
            .as_deref()
            .or_else(|| session.models().available.first().map(|m| m.name.as_str()))
            .map(|s| s.to_string());

        // Set initial mode and model
        self.current_mode = session.current_mode().map(|s| s.to_string());
        self.current_model = model_name.clone();

        // Welcome
        self.history.push(
            format!(
                "  {C_BOLD_CYAN}mythcode{C_RESET} {C_DARK}·{C_RESET} \x1b[1m{}\x1b[0m",
                self.project_name
            ),
            LineType::Welcome,
        );
        if let Some(model) = &model_name {
            let short_model = shorten_model_name(model);
            self.history.push(
                format!("  {C_DIM}{short_model} · /help · shift+tab{C_RESET}"),
                LineType::Welcome,
            );
        } else {
            self.history.push(
                format!("  {C_DIM}/help · shift+tab{C_RESET}"),
                LineType::Welcome,
            );
        }

        let _terminal_guard = TerminalGuard::enter(TerminalGuardOptions {
            alternate_screen: true,
            mouse_capture: true,
            enhanced_keys: true,
        })?;

        let (term_tx, mut term_rx) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            loop {
                match crossterm::event::read() {
                    Ok(event) => {
                        if term_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Load extension commands if host is running
        if let Some(host) = ext_host.as_ref() {
            self.extension_commands = host.commands().await;
        }

        self.redraw()?;

        let mut pending_exit = false;
        let mut spinner_interval =
            tokio::time::interval(Duration::from_millis(crate::spinner::INTERVAL_MS));
        spinner_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        'outer: loop {
            // === INPUT PHASE ===
            let submitted_line = if let Some(queued) = self.take_queued_submission() {
                queued
            } else {
                loop {
                    tokio::select! {
                        Some(event) = term_rx.recv() => {
                            match event {
                                Event::Key(key) if key.kind == KeyEventKind::Press => {
                                    match self.handle_key(key, client, &mut pending_exit, file_index).await? {
                                        KeyAction::Continue => {}
                                        KeyAction::Exit => break 'outer,
                                        KeyAction::Submit(line) => {
                                            pending_exit = false;
                                            self.suggestions.clear();
                                            self.selected_suggestion = None;
                                            break line;
                                        }
                                    }
                                }
                                Event::Mouse(mouse) => {
                                    if let crossterm::event::MouseEventKind::ScrollUp = mouse.kind {
                                        self.history.scroll_up(3, self.term_width as usize);
                                        self.redraw()?;
                                    } else if let crossterm::event::MouseEventKind::ScrollDown = mouse.kind {
                                        self.history.scroll_down(3);
                                        self.redraw()?;
                                    }
                                }
                                Event::Resize(w, h) => {
                                    self.term_width = w;
                                    self.term_height = h;
                                    self.redraw()?;
                                }
                                _ => {}
                            }
                        }
                        Some(app_event) = events.recv() => {
                            // Refresh extension commands when new events arrive
                            if let Some(host) = ext_host.as_ref() {
                                self.extension_commands = host.commands().await;
                            }
                            self.dispatch_app_event(app_event);
                            self.redraw()?;
                        }
                        signal = signals.recv() => {
                            match signal {
                                ShutdownSignal::Sigint => {
                                    if pending_exit { break 'outer; }
                                    self.history.push(
                                        format!("  {C_DIM}press ctrl+c again to exit{C_RESET}"),
                                        LineType::Status,
                                    );
                                    pending_exit = true;
                                    self.redraw()?;
                                }
                                ShutdownSignal::Sigterm => break 'outer,
                            }
                        }
                    }
                }
            };

            let mut trimmed = submitted_line.trim().to_string();
            if trimmed.is_empty() {
                self.redraw()?;
                continue;
            }

            // Extension input hook
            if let Some(host) = ext_host.as_ref() {
                if let Ok(result) = host
                    .request("lifecycle/input", serde_json::json!({ "text": &trimmed }))
                    .await
                {
                    if result
                        .get("handled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        self.redraw()?;
                        continue;
                    }
                    if let Some(new_text) = result.get("text").and_then(|v| v.as_str()) {
                        trimmed = new_text.to_string();
                    }
                }
            }

            // Check extension commands
            if trimmed.starts_with('/') {
                if let Some(host) = ext_host.as_ref() {
                    let cmd_name = trimmed
                        .trim_start_matches('/')
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    let cmd_args = trimmed
                        .trim_start_matches('/')
                        .trim_start_matches(cmd_name)
                        .trim();
                    let ext_commands = host.commands().await;
                    if ext_commands.iter().any(|c| c.name == cmd_name) {
                        let _ = host.execute_command(cmd_name, cmd_args).await;
                        self.redraw()?;
                        continue;
                    }
                }
            }

            if let Some(action) = self
                .handle_local_command(client, file_index, &trimmed)
                .await?
            {
                match action {
                    crate::types::CommandAction::Continue => {
                        self.redraw()?;
                        continue;
                    }
                    crate::types::CommandAction::Exit => break,
                }
            }

            // Echo user message
            let user_lines = format_user_message(&trimmed);
            self.history.push_lines(user_lines, LineType::UserMessage);

            // Extension beforePrompt hook
            if let Some(host) = ext_host.as_ref() {
                if let Ok(result) = host
                    .request(
                        "lifecycle/beforePrompt",
                        serde_json::json!({ "prompt": &trimmed }),
                    )
                    .await
                {
                    if result
                        .get("skip")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        self.redraw()?;
                        continue;
                    }
                    if let Some(new_prompt) = result.get("prompt").and_then(|v| v.as_str()) {
                        trimmed = new_prompt.to_string();
                    }
                }
            }

            // === PROMPT PHASE ===
            self.start_turn();
            if let Some(host) = ext_host.as_ref() {
                host.notify("lifecycle/agentStart", serde_json::json!({}));
            }
            let mut cancel_sent = false;
            self.redraw()?;

            let prompt_future = client.prompt(&trimmed);
            tokio::pin!(prompt_future);

            loop {
                tokio::select! {
                    result = &mut prompt_future => {
                        let result = result?;
                        while let Ok(event) = events.try_recv() {
                            match event {
                                AppEvent::PermissionRequest(req) => {
                                    let _ = req.responder.send(PermissionDecision::Cancelled);
                                }
                                other => self.handle_app_event(other),
                            }
                        }
                        self.finish_turn(&result);
                        if let Some(host) = ext_host.as_ref() {
                            host.notify("lifecycle/agentEnd", serde_json::json!({
                                "stopReason": format!("{:?}", result.stop_reason)
                            }));
                        }
                        self.redraw()?;
                        break;
                    }
                    Some(app_event) = events.recv() => {
                        self.dispatch_app_event(app_event);
                        self.redraw()?;
                    }
                    Some(term_event) = term_rx.recv() => {
                        match term_event {
                            Event::Key(key) if key.kind != KeyEventKind::Press => {
                                // Ignore Release/Repeat events
                            }
                            Event::Key(key) if self.pending_permission.is_some() => {
                                // Handle permission dialog during prompt phase
                                match key.code {
                                    KeyCode::Up => {
                                        if let Some(ref mut perm) = self.pending_permission {
                                            if perm.selected > 0 { perm.selected -= 1; }
                                        }
                                        self.redraw()?;
                                    }
                                    KeyCode::Down => {
                                        if let Some(ref mut perm) = self.pending_permission {
                                            if perm.selected + 1 < perm.options.len() { perm.selected += 1; }
                                        }
                                        self.redraw()?;
                                    }
                                    KeyCode::Enter => {
                                        self.accept_pending_permission();
                                        self.redraw()?;
                                    }
                                    KeyCode::Esc => {
                                        self.cancel_pending_permission();
                                        self.redraw()?;
                                    }
                                    _ => {}
                                }
                            }
                            Event::Key(key) => match key.code {
                                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    if !cancel_sent {
                                        client.cancel_current_turn().await?;
                                        self.history.push(format_activity("cancelling"), LineType::Activity);
                                        cancel_sent = true;
                                        self.redraw()?;
                                    } else {
                                        break 'outer;
                                    }
                                }
                                KeyCode::Esc => {
                                    if !cancel_sent {
                                        client.cancel_current_turn().await?;
                                        self.history.push(format_activity("cancelling"), LineType::Activity);
                                        cancel_sent = true;
                                        self.redraw()?;
                                    }
                                }
                                KeyCode::PageUp => {
                                    self.history
                                        .scroll_up(self.term_height as usize / 2, self.term_width as usize);
                                    self.redraw()?;
                                }
                                KeyCode::PageDown => {
                                    self.history.scroll_down(self.term_height as usize / 2);
                                    self.redraw()?;
                                }
                                // Allow typing to queue messages during prompt
                                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    self.input.insert_char(ch);
                                    self.redraw()?;
                                }
                                KeyCode::Backspace => {
                                    self.input.delete_char_before();
                                    self.redraw()?;
                                }
                                KeyCode::Left => { self.input.move_left(); self.redraw()?; }
                                KeyCode::Right => { self.input.move_right(); self.redraw()?; }
                                KeyCode::Home => { self.input.move_home(); self.redraw()?; }
                                KeyCode::End => { self.input.move_end(); self.redraw()?; }
                                KeyCode::Enter if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                                    self.input.insert_newline();
                                    self.redraw()?;
                                }
                                KeyCode::Enter => {
                                    if self.queue_current_input() {
                                        self.redraw()?;
                                    }
                                }
                                KeyCode::Tab => {
                                    self.update_suggestions_with_client(file_index, client);
                                    if !self.suggestions.is_empty() {
                                        self.selected_suggestion = Some(0);
                                        self.input.set_content(&self.suggestions[0].value.clone());
                                        self.redraw()?;
                                    }
                                }
                                _ => {}
                            },
                            Event::Mouse(mouse) => {
                                if let crossterm::event::MouseEventKind::ScrollUp = mouse.kind {
                                    self.history.scroll_up(3, self.term_width as usize);
                                    self.redraw()?;
                                } else if let crossterm::event::MouseEventKind::ScrollDown = mouse.kind {
                                    self.history.scroll_down(3);
                                    self.redraw()?;
                                }
                            }
                            Event::Resize(w, h) => {
                                self.term_width = w;
                                self.term_height = h;
                                self.redraw()?;
                            }
                            _ => {}
                        }
                    }
                    signal = signals.recv() => {
                        match signal {
                            ShutdownSignal::Sigint if !cancel_sent => {
                                client.cancel_current_turn().await?;
                                self.history.push(format_activity("cancelling"), LineType::Activity);
                                cancel_sent = true;
                                self.redraw()?;
                            }
                            ShutdownSignal::Sigint | ShutdownSignal::Sigterm => {
                                break 'outer;
                            }
                        }
                    }
                    _ = spinner_interval.tick(), if self.spinner_active && self.pending_permission.is_none() => {
                        self.spinner_frame += 1;
                        self.redraw()?;
                    }
                }
            }
        }

        Ok(())
    }

    fn queue_current_input(&mut self) -> bool {
        let text = self.input.take_content();
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return false;
        }

        self.message_queue.push(trimmed.clone());
        self.history.push(
            format!("  {C_DIM}queued: {trimmed}{C_RESET}"),
            LineType::Status,
        );
        true
    }

    fn take_queued_submission(&mut self) -> Option<String> {
        if self.message_queue.is_empty() {
            return None;
        }

        Some(std::mem::take(&mut self.message_queue).join("\n\n"))
    }

    fn clear_queue(&mut self) {
        self.message_queue.clear();
    }
}

/// Shorten ugly model IDs like "fireworks-ai/accounts/fireworks/routers/kimi-k2p5-turbo"
/// to just the last segment: "kimi-k2p5-turbo"
fn shorten_model_name(model: &str) -> String {
    // If it contains slashes, take the last segment
    if model.contains('/') {
        model.rsplit('/').next().unwrap_or(model).to_string()
    } else {
        model.to_string()
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use crate::types::{
        AppEvent, PermissionOptionKindView, PermissionOptionView, PermissionRequestView,
    };

    use super::{Tui, TurnState};

    #[test]
    fn queued_messages_preserve_fifo_order() {
        let mut tui = Tui::new();
        tui.input.set_content("first");
        assert!(tui.queue_current_input());
        tui.input.set_content("second");
        assert!(tui.queue_current_input());

        assert_eq!(
            tui.take_queued_submission().as_deref(),
            Some("first\n\nsecond")
        );
        assert!(tui.take_queued_submission().is_none());
    }

    #[test]
    fn spinner_status_survives_streaming_text() {
        let mut tui = Tui::new();
        tui.start_turn();
        tui.handle_app_event(AppEvent::AssistantText("# Header\n- item".to_string()));

        assert!(!tui.live_assistant_lines().is_empty());
        assert!(!tui.status_lines().is_empty());
        assert_eq!(tui.turn_state, TurnState::Responding);
    }

    #[test]
    fn permission_restores_previous_turn_state() {
        let mut tui = Tui::new();
        tui.start_turn();
        tui.turn_state = TurnState::ToolRunning;

        let (tx, _rx) = oneshot::channel();
        tui.dispatch_app_event(AppEvent::PermissionRequest(PermissionRequestView {
            title: "Need approval".into(),
            subtitle: None,
            options: vec![PermissionOptionView {
                option_id: "allow".into(),
                name: "Allow once".into(),
                kind: PermissionOptionKindView::AllowOnce,
            }],
            locations: Vec::new(),
            responder: tx,
        }));

        assert_eq!(tui.turn_state, TurnState::AwaitingPermission);
        tui.accept_pending_permission();
        assert_eq!(tui.turn_state, TurnState::ToolRunning);
    }
}
