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

use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::{cursor, execute, terminal};
use tokio::sync::mpsc;

use crate::acp_client::AcpClient;
use crate::input::FileIndex;
use crate::types::{AppEvent, PermissionDecision, ShutdownSignal, ToolOutputView};

use history::{
    format_activity, format_status, format_user_message, History, LineType,
};
use input_box::InputBox;
use markdown::MarkdownParser;
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

pub struct Tui {
    history: History,
    input: InputBox,
    md_parser: MarkdownParser,
    term_width: u16,
    term_height: u16,
    assistant_buffer: String,
    thinking_buffer: String,
    assistant_open: bool,
    thinking_open: bool,
    last_activity: Option<String>,
    pending_permission: Option<PendingPermission>,
    select_mode: Option<SelectMode>,
    turn_count: u32,
    spinner_frame: usize,
    spinner_active: bool,
    turn_start: Option<Instant>,
    tool_active: bool,
    partial_line: Option<String>,
    thinking_partial: Option<String>,
    suggestions: Vec<Suggestion>,
    selected_suggestion: Option<usize>,
    printed_text: bool,
    project_name: String,
    current_mode: Option<String>,
    current_model: Option<String>,
    last_tool_outputs: Vec<ToolOutputView>,
    live_output_lines: usize,
    message_queue: Vec<String>,
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
            md_parser: MarkdownParser::new(),
            term_width: w,
            term_height: h,
            assistant_buffer: String::new(),
            thinking_buffer: String::new(),
            assistant_open: false,
            thinking_open: false,
            last_activity: None,
            pending_permission: None,
            select_mode: None,
            turn_count: 0,
            spinner_frame: 0,
            spinner_active: false,
            turn_start: None,
            tool_active: false,
            partial_line: None,
            thinking_partial: None,
            suggestions: Vec::new(),
            selected_suggestion: None,
            printed_text: false,
            project_name: String::new(),
            current_mode: None,
            current_model: None,
            last_tool_outputs: Vec::new(),
            live_output_lines: 0,
            message_queue: Vec::new(),
        }
    }

    pub async fn run(
        &mut self,
        client: &mut AcpClient,
        events: &mut mpsc::UnboundedReceiver<AppEvent>,
        signals: &mut super::cli::SignalState,
        file_index: &mut FileIndex,
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
        self.history.push(String::new(), LineType::Welcome);
        self.history.push(
            format!(
                "  {C_BOLD_CYAN}mythcode{C_RESET} {C_DARK}·{C_RESET} \x1b[1m{}\x1b[0m",
                self.project_name
            ),
            LineType::Welcome,
        );
        if let Some(model) = &model_name {
            self.history
                .push(format!("  {C_DIM}{model}{C_RESET}"), LineType::Welcome);
        }
        self.history.push(String::new(), LineType::Welcome);

        // Terminal setup
        terminal::enable_raw_mode()?;
        execute!(
            io::stdout(),
            terminal::EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Show,
        )?;

        // Enable enhanced keyboard protocol (Kitty) so Shift+Enter is distinguishable.
        // Gracefully ignored on terminals that don't support it.
        let has_enhanced_keys = crossterm::terminal::supports_keyboard_enhancement()
            .unwrap_or(false);
        if has_enhanced_keys {
            let _ = execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
            );
        }

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            if has_enhanced_keys {
                let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
            }
            let _ = execute!(
                io::stdout(),
                crossterm::event::DisableMouseCapture,
                terminal::LeaveAlternateScreen,
                cursor::SetCursorStyle::DefaultUserShape,
            );
            original_hook(info);
        }));

        let (term_tx, mut term_rx) = mpsc::unbounded_channel();
        std::thread::spawn(move || loop {
            match crossterm::event::read() {
                Ok(event) => {
                    if term_tx.send(event).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        });

        self.redraw()?;

        let mut pending_exit = false;
        let mut spinner_interval = tokio::time::interval(Duration::from_millis(crate::spinner::INTERVAL_MS));
        spinner_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        'outer: loop {
            // === INPUT PHASE ===
            let submitted_line = loop {
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
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        self.history.scroll_up(3);
                                        self.redraw()?;
                                    }
                                    MouseEventKind::ScrollDown => {
                                        self.history.scroll_down(3);
                                        self.redraw()?;
                                    }
                                    _ => {}
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
            };

            let trimmed = submitted_line.trim().to_string();
            if trimmed.is_empty() {
                self.redraw()?;
                continue;
            }

            if let Some(action) =
                self.handle_local_command(client, file_index, &trimmed).await?
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

            // === PROMPT PHASE ===
            self.start_turn();
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
                                        let perm = self.pending_permission.take().unwrap();
                                        let option = &perm.options[perm.selected];
                                        let decision = PermissionDecision::Selected(option.option_id.clone());
                                        let summary = option.name.to_lowercase();
                                        let _ = perm.responder.send(decision);
                                        self.history.push(format_status(&summary), LineType::Status);
                                        self.redraw()?;
                                    }
                                    KeyCode::Esc => {
                                        let perm = self.pending_permission.take().unwrap();
                                        let _ = perm.responder.send(PermissionDecision::Cancelled);
                                        self.history.push(format_status("cancelled"), LineType::Status);
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
                                    self.history.scroll_up(self.term_height as usize / 2);
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
                                KeyCode::Tab => {
                                    // Queue message while agent is working
                                    let text = self.input.take_content();
                                    let trimmed = text.trim().to_string();
                                    if !trimmed.is_empty() {
                                        self.message_queue.push(trimmed.clone());
                                        self.history.push(
                                            format!("  {C_DIM}queued: {trimmed}{C_RESET}"),
                                            LineType::Status,
                                        );
                                        self.redraw()?;
                                    }
                                }
                                _ => {}
                            },
                            Event::Mouse(mouse) => {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        self.history.scroll_up(3);
                                        self.redraw()?;
                                    }
                                    MouseEventKind::ScrollDown => {
                                        self.history.scroll_down(3);
                                        self.redraw()?;
                                    }
                                    _ => {}
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

        // Restore terminal
        let _ = terminal::disable_raw_mode();
        if has_enhanced_keys {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            terminal::LeaveAlternateScreen,
            cursor::SetCursorStyle::DefaultUserShape,
        );

        Ok(())
    }
}
