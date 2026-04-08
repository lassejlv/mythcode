mod history;
mod input_box;
mod markdown;

use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use crossterm::{cursor, execute, terminal};
use tokio::sync::mpsc;

use crate::acp_client::AcpClient;
use crate::input::FileIndex;
use crate::types::{AppEvent, CommandAction, PermissionDecision, PermissionOptionView, ShutdownSignal, ToolOutputView};

use history::{
    format_activity, format_diff, format_status, format_tool_output, format_user_message,
    format_warning, History, LineType,
};
use input_box::InputBox;
use markdown::MarkdownParser;

const INPUT_BOX_HEIGHT: u16 = 3;
const MARGIN_TOP: u16 = 1;

// Claude Code-inspired palette
const C_RESET: &str = "\x1b[0m";
const C_DIM: &str = "\x1b[38;5;245m";
const C_CYAN: &str = "\x1b[38;5;75m";
const C_BOLD_CYAN: &str = "\x1b[1;38;5;75m";
const C_DARK: &str = "\x1b[38;5;240m";
const C_SPINNER: &str = "\x1b[38;5;75m";

struct PendingPermission {
    #[allow(dead_code)]
    title: String,
    #[allow(dead_code)]
    subtitle: Option<String>,
    options: Vec<PermissionOptionView>,
    #[allow(dead_code)]
    locations: Vec<String>,
    selected: usize,
    responder: tokio::sync::oneshot::Sender<PermissionDecision>,
}

/// Interactive select mode for model picker, file picker, etc.
struct SelectMode {
    title: String,
    items: Vec<SelectItem>,
    filtered: Vec<usize>,
    filter: String,
    selected: usize,
    kind: SelectKind,
}

#[derive(Clone)]
struct SelectItem {
    id: String,
    display: String,
}

enum SelectKind {
    Model,
}

impl SelectMode {
    fn new(title: &str, items: Vec<SelectItem>, kind: SelectKind) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self {
            title: title.to_string(),
            items,
            filtered,
            filter: String::new(),
            selected: 0,
            kind,
        }
    }

    fn update_filter(&mut self) {
        let q = self.filter.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                q.is_empty()
                    || item.display.to_lowercase().contains(&q)
                    || item.id.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }

    fn selected_item(&self) -> Option<&SelectItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.items.get(i))
    }
}

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
    partial_line: Option<String>,
    thinking_partial: Option<String>,
    suggestions: Vec<String>,
    selected_suggestion: Option<usize>,
    printed_text: bool,
    project_name: String,
    current_mode: Option<String>,
    last_tool_outputs: Vec<ToolOutputView>,
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
            partial_line: None,
            thinking_partial: None,
            suggestions: Vec::new(),
            selected_suggestion: None,
            printed_text: false,
            project_name: String::new(),
            current_mode: None,
            last_tool_outputs: Vec::new(),
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

        // Set initial mode
        self.current_mode = session.current_mode().map(|s| s.to_string());

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
            crossterm::event::EnableMouseCapture,
            cursor::Show,
        )?;

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
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
                            Event::Key(key) => {
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
                            Event::Resize(w, h) => {
                                self.term_width = w;
                                self.term_height = h;
                                self.redraw()?;
                            }
                            Event::Mouse(mouse) => {
                                self.handle_mouse(mouse);
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
                    CommandAction::Continue => {
                        self.redraw()?;
                        continue;
                    }
                    CommandAction::Exit => break,
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
                                KeyCode::PageUp => {
                                    self.history.scroll_up(self.term_height as usize / 2);
                                    self.redraw()?;
                                }
                                KeyCode::PageDown => {
                                    self.history.scroll_down(self.term_height as usize / 2);
                                    self.redraw()?;
                                }
                                _ => {}
                            },
                            Event::Resize(w, h) => {
                                self.term_width = w;
                                self.term_height = h;
                                self.redraw()?;
                            }
                            Event::Mouse(mouse) => {
                                self.handle_mouse(mouse);
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
                    _ = spinner_interval.tick(), if self.spinner_active => {
                        self.spinner_frame += 1;
                        self.redraw()?;
                    }
                }
            }
        }

        // Restore terminal
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            crossterm::event::DisableMouseCapture,
            terminal::LeaveAlternateScreen,
            cursor::SetCursorStyle::DefaultUserShape,
        );

        Ok(())
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::AssistantText(text) => {
                self.stop_spinner();
                self.flush_thinking();
                self.assistant_open = true;
                self.assistant_buffer.push_str(&text);
                self.printed_text = true;
                self.flush_complete_assistant_lines();
            }
            AppEvent::ThinkingText(text) => {
                self.stop_spinner();
                self.flush_assistant();
                self.thinking_open = true;
                self.thinking_buffer.push_str(&text);
                self.flush_complete_thinking_lines();
            }
            AppEvent::Activity(activity) => {
                if self.last_activity.as_deref() == Some(&activity) {
                    return;
                }
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                self.history.push(format_activity(&activity), LineType::Activity);
                self.last_activity = Some(activity);
            }
            AppEvent::ModeChanged(mode) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                self.current_mode = Some(mode.clone());
                self.history.push(
                    format_activity(&format!("mode → {mode}")),
                    LineType::Activity,
                );
            }
            AppEvent::SessionTitle(title) => {
                self.history.push(
                    format_activity(&format!("session: {title}")),
                    LineType::Activity,
                );
            }
            AppEvent::ToolDiff(diff) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                let lines = format_diff(&diff);
                self.history.push_lines(lines, LineType::Diff);
                self.history.push(String::new(), LineType::Diff);
            }
            AppEvent::ToolOutput(output) => {
                self.stop_spinner();
                let lines = format_tool_output(&output.title, &output.content, output.total_lines);
                self.history.push_lines(lines, LineType::Activity);
                self.last_tool_outputs.push(output);
            }
            AppEvent::Warning(msg) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                self.history.push(format_warning(&msg), LineType::Warning);
            }
            AppEvent::DebugProtocol(_) | AppEvent::ProcessStderr(_) => {}
            AppEvent::PermissionRequest(_) => {}
        }
    }

    fn dispatch_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::PermissionRequest(req) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();

                let mut title_line = format!("  \x1b[38;5;222m⚠ {}\x1b[0m", req.title);
                if let Some(sub) = &req.subtitle {
                    title_line.push_str(&format!(" {C_DIM}{sub}{C_RESET}"));
                }
                self.history.push(title_line, LineType::Warning);

                if !req.locations.is_empty() {
                    self.history.push(
                        format!("    {C_DIM}{}{C_RESET}", req.locations.join(", ")),
                        LineType::Status,
                    );
                }

                self.pending_permission = Some(PendingPermission {
                    title: req.title,
                    subtitle: req.subtitle,
                    options: req.options,
                    locations: req.locations,
                    selected: 0,
                    responder: req.responder,
                });
            }
            other => self.handle_app_event(other),
        }
    }

    // ── Streaming buffer management ─────────────────────────────────

    fn flush_complete_assistant_lines(&mut self) {
        while let Some(newline_pos) = self.assistant_buffer.find('\n') {
            let line = self.assistant_buffer[..newline_pos].to_string();
            let rendered = self.md_parser.render_line(&line);
            self.history.push(rendered, LineType::Assistant);
            self.assistant_buffer = self.assistant_buffer[newline_pos + 1..].to_string();
        }
        self.partial_line = if self.assistant_buffer.is_empty() {
            None
        } else {
            Some(self.md_parser.render_line(&self.assistant_buffer))
        };
    }

    fn flush_complete_thinking_lines(&mut self) {
        while let Some(newline_pos) = self.thinking_buffer.find('\n') {
            let line = self.thinking_buffer[..newline_pos].to_string();
            let rendered = self.md_parser.render_thinking_line(&line);
            self.history.push(rendered, LineType::Thinking);
            self.thinking_buffer = self.thinking_buffer[newline_pos + 1..].to_string();
        }
        self.thinking_partial = if self.thinking_buffer.is_empty() {
            None
        } else {
            Some(self.md_parser.render_thinking_line(&self.thinking_buffer))
        };
    }

    fn flush_assistant(&mut self) {
        if !self.assistant_buffer.is_empty() {
            let line = std::mem::take(&mut self.assistant_buffer);
            let rendered = self.md_parser.render_line(&line);
            self.history.push(rendered, LineType::Assistant);
            self.partial_line = None;
        }
        self.assistant_open = false;
    }

    fn flush_thinking(&mut self) {
        if !self.thinking_buffer.is_empty() {
            let line = std::mem::take(&mut self.thinking_buffer);
            let rendered = self.md_parser.render_thinking_line(&line);
            self.history.push(rendered, LineType::Thinking);
            self.thinking_partial = None;
        }
        self.thinking_open = false;
    }

    // ── Turn management ─────────────────────────────────────────────

    fn start_turn(&mut self) {
        self.stop_spinner();
        self.flush_assistant();
        self.flush_thinking();
        self.turn_count += 1;
        self.assistant_open = false;
        self.thinking_open = false;
        self.printed_text = false;
        self.last_activity = None;
        self.last_tool_outputs.clear();
        self.spinner_active = true;
        self.spinner_frame = 0;
    }

    fn finish_turn(&mut self, result: &crate::types::PromptResult) {
        self.stop_spinner();
        self.flush_assistant();
        self.flush_thinking();

        if matches!(
            result.stop_reason,
            agent_client_protocol::StopReason::Cancelled
        ) {
            self.history.push(format_status("cancelled"), LineType::Status);
        }
        self.history.push(String::new(), LineType::Separator);
    }

    fn stop_spinner(&mut self) {
        self.spinner_active = false;
    }

    // ── Key handling ────────────────────────────────────────────────

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        client: &mut AcpClient,
        pending_exit: &mut bool,
        file_index: &FileIndex,
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
                                self.history.push(
                                    format_status(&format!("model → {name}")),
                                    LineType::Status,
                                );
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
            KeyCode::Enter => {
                let content = self.input.take_content();
                return Ok(KeyAction::Submit(content));
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
                self.update_suggestions_with_client(file_index, client);
                if !self.suggestions.is_empty() {
                    self.selected_suggestion = Some(0);
                    self.input.set_content(&self.suggestions[0].clone());
                }
            }
            _ => {}
        }

        self.redraw()?;
        Ok(KeyAction::Continue)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.history.scroll_up(3),
            MouseEventKind::ScrollDown => self.history.scroll_down(3),
            _ => {}
        }
    }

    fn update_suggestions_with_client(&mut self, file_index: &FileIndex, client: &AcpClient) {
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

    // ── Local commands ──────────────────────────────────────────────

    async fn handle_local_command(
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
                self.history.push(
                    format!("  {C_BOLD_CYAN}Commands{C_RESET}"),
                    LineType::Status,
                );
                for (cmd, desc) in [
                    ("/exit", "exit mythcode"),
                    ("/clear", "clear the screen"),
                    ("/cwd", "show working directory"),
                    ("/new", "start a new session"),
                    ("/model", "select a model"),
                ] {
                    self.history.push(
                        format!("  \x1b[0m{cmd:<9}{C_DIM}{desc}{C_RESET}"),
                        LineType::Status,
                    );
                }
                self.history.push(String::new(), LineType::Status);
                Ok(Some(CommandAction::Continue))
            }
            _ => Ok(None),
        }
    }

    // ── Layout & rendering ──────────────────────────────────────────

    fn redraw(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        let w = self.term_width;
        let h = self.term_height;

        execute!(stdout, cursor::Hide)?;

        // Calculate how many extra rows we need for partials/spinner
        let mut extra_lines: u16 = 0;
        if self.history.scroll_offset == 0 {
            if self.thinking_partial.is_some() {
                extra_lines += 1;
            }
            if self.partial_line.is_some() {
                extra_lines += 1;
            }
            if self.spinner_active && self.partial_line.is_none() && self.thinking_partial.is_none()
            {
                extra_lines += 1;
            }
        }

        // Max space for all content (history + extras)
        let max_content = h.saturating_sub(MARGIN_TOP + INPUT_BOX_HEIGHT);

        // History gets the space minus what partials need
        let max_history = max_content.saturating_sub(extra_lines);
        let visible = self.history.visible_lines(max_history as usize);
        let history_rows = visible.len() as u16;

        // Non-sticky: input follows content
        let content_rows = history_rows + extra_lines;
        let input_row = (MARGIN_TOP + content_rows).min(h.saturating_sub(INPUT_BOX_HEIGHT));
        let draw_area = input_row.saturating_sub(MARGIN_TOP);

        // Clear margin
        for row in 0..MARGIN_TOP {
            execute!(stdout, cursor::MoveTo(0, row))?;
            write!(stdout, "\x1b[2K")?;
        }

        // Draw history lines
        for row in 0..draw_area {
            execute!(stdout, cursor::MoveTo(0, MARGIN_TOP + row))?;
            write!(stdout, "\x1b[2K")?;
            if let Some(line) = visible.get(row as usize) {
                write!(stdout, "{}", &line.content)?;
            }
        }

        // Streaming partials (space is already reserved)
        let mut extra_row = MARGIN_TOP + history_rows;

        if let Some(ref partial) = self.thinking_partial {
            if self.history.scroll_offset == 0 && extra_row < input_row {
                execute!(stdout, cursor::MoveTo(0, extra_row))?;
                write!(stdout, "\x1b[2K{partial}")?;
                extra_row += 1;
            }
        }

        if let Some(ref partial) = self.partial_line {
            if self.history.scroll_offset == 0 && extra_row < input_row {
                execute!(stdout, cursor::MoveTo(0, extra_row))?;
                write!(stdout, "\x1b[2K{partial}")?;
                extra_row += 1;
            }
        }

        // Spinner
        if self.spinner_active
            && self.partial_line.is_none()
            && self.thinking_partial.is_none()
            && extra_row < input_row
        {
            let frame = crate::spinner::frame(self.spinner_frame);
            let shimmer = crate::spinner::shimmer_thinking(self.spinner_frame);
            execute!(stdout, cursor::MoveTo(0, extra_row))?;
            write!(stdout, "\x1b[2K  {C_SPINNER}{frame}{C_RESET}  {shimmer}")?;
        }

        // Permission options (rendered between history and input)
        if let Some(ref perm) = self.pending_permission {
            let hint = format!("{C_DIM}↑↓ select · enter confirm · esc cancel{C_RESET}");
            let perm_start = input_row.saturating_sub(perm.options.len() as u16 + 1);
            execute!(stdout, cursor::MoveTo(0, perm_start))?;
            write!(stdout, "\x1b[2K  {hint}")?;
            for (i, opt) in perm.options.iter().enumerate() {
                let row = perm_start + 1 + i as u16;
                if row >= input_row {
                    break;
                }
                execute!(stdout, cursor::MoveTo(0, row))?;
                if i == perm.selected {
                    write!(stdout, "\x1b[2K  {C_CYAN}▸ {opt}{C_RESET}")?;
                } else {
                    write!(stdout, "\x1b[2K    {C_DIM}{opt}{C_RESET}")?;
                }
            }
        }

        stdout.flush()?;

        // Input box (non-sticky: positioned after content)
        let title = if let Some(ref mode) = self.current_mode {
            format!("{} · {}", self.project_name, mode)
        } else {
            self.project_name.clone()
        };
        let is_active = self.pending_permission.is_none() && self.select_mode.is_none();
        self.input.render(input_row, w, INPUT_BOX_HEIGHT, &title, is_active)?;

        // Suggestions BELOW the input box
        let suggestions_start = input_row + INPUT_BOX_HEIGHT;
        if !self.suggestions.is_empty() {
            let mut stdout = io::stdout();
            for (i, suggestion) in self.suggestions.iter().enumerate().take(8) {
                let row = suggestions_start + i as u16;
                if row >= h {
                    break;
                }
                execute!(stdout, cursor::MoveTo(0, row))?;
                if self.selected_suggestion == Some(i) {
                    write!(stdout, "\x1b[2K  {C_CYAN}▸ {suggestion}{C_RESET}")?;
                } else {
                    write!(stdout, "\x1b[2K    {C_DIM}{suggestion}{C_RESET}")?;
                }
            }
            // Clear remaining rows below suggestions
            let clear_from = suggestions_start + self.suggestions.len().min(8) as u16;
            for row in clear_from..h {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
            }
            stdout.flush()?;
        } else if self.select_mode.is_none() {
            // Clear area below input
            let mut stdout = io::stdout();
            for row in suggestions_start..h {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
            }
            stdout.flush()?;
        }

        // Select mode overlay (below input box)
        if let Some(ref sel) = self.select_mode {
            let mut stdout = io::stdout();
            let sel_start = input_row + INPUT_BOX_HEIGHT;

            // Title + filter
            if sel_start < h {
                execute!(stdout, cursor::MoveTo(0, sel_start))?;
                let filter_display = if sel.filter.is_empty() {
                    format!("{C_DIM}type to filter…{C_RESET}")
                } else {
                    format!("{C_TEXT}{}{C_RESET}", sel.filter)
                };
                write!(
                    stdout,
                    "\x1b[2K  {C_BOLD_CYAN}{}{C_RESET}  {filter_display}",
                    sel.title
                )?;
            }

            // Items
            for (vi, &item_idx) in sel.filtered.iter().enumerate().take(10) {
                let row = sel_start + 1 + vi as u16;
                if row >= h {
                    break;
                }
                let item = &sel.items[item_idx];
                execute!(stdout, cursor::MoveTo(0, row))?;
                if vi == sel.selected {
                    write!(
                        stdout,
                        "\x1b[2K  {C_CYAN}▸ {}{C_RESET} {C_DIM}({}){C_RESET}",
                        item.display, item.id
                    )?;
                } else {
                    write!(
                        stdout,
                        "\x1b[2K    {C_DIM}{} ({}){C_RESET}",
                        item.display, item.id
                    )?;
                }
            }

            // Clear below
            let clear_from = sel_start + 1 + sel.filtered.len().min(10) as u16;
            for row in clear_from..h {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
            }
            stdout.flush()?;
        }

        // Reposition cursor inside the input box (rendering below may have moved it)
        self.input.reposition_cursor(input_row, w, INPUT_BOX_HEIGHT)?;

        Ok(())
    }
}

enum KeyAction {
    Continue,
    Exit,
    Submit(String),
}
