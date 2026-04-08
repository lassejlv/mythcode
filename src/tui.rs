use std::cmp::{max, min};
use std::io::{self, Stdout};

use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use futures_util::future::LocalBoxFuture;
use pulldown_cmark::{CodeBlockKind, Event as MarkdownEvent, HeadingLevel, Options, Parser, Tag};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use similar::{ChangeTag, TextDiff};
use unicode_width::UnicodeWidthChar;

use crate::acp_client::AcpClient;
use crate::cli::SignalState;
use crate::session::SessionState;
use crate::types::{
    AppEvent, DiffPreview, ModelOption, PermissionDecision, PermissionRequestView, PromptResult,
};

const HELP_TEXT: &str =
    "Enter send  Shift+Enter newline  PgUp/PgDn scroll  Ctrl+C cancel/exit  Ctrl+L clear";
const THINKING_STYLE: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::ITALIC);
const STATUS_STYLE: Style = Style::new().fg(Color::DarkGray);
const USER_STYLE: Style = Style::new().fg(Color::Cyan);
const AGENT_STYLE: Style = Style::new().fg(Color::Green);
const CODE_STYLE: Style = Style::new().fg(Color::Yellow).bg(Color::Rgb(24, 24, 24));

pub async fn run_repl(
    client: &mut AcpClient,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    signals: &mut SignalState,
) -> Result<()> {
    let mut terminal = TerminalSession::enter()?;
    let mut ui = ChatUi::new();
    let mut event_stream = EventStream::new();
    let mut pending_prompt: Option<LocalBoxFuture<'_, Result<PromptResult>>> = None;
    let mut pending_exit = false;

    loop {
        let snapshot = client.session_snapshot();
        terminal.draw(|frame| ui.draw(frame, &snapshot))?;

        tokio::select! {
            maybe_term = event_stream.next() => {
                let event = match maybe_term {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => return Err(error).context("failed to read terminal event"),
                    None => continue,
                };
                let action = ui.handle_terminal_event(event, pending_prompt.is_some(), &snapshot);

                match action {
                    UiAction::None => {}
                    UiAction::Exit => break,
                    UiAction::CloseOverlay => ui.close_overlay(),
                    UiAction::ClearTranscript => ui.clear_transcript(),
                    UiAction::CancelTurn => {
                        if pending_prompt.is_some() {
                            client.cancel_current_turn().await?;
                            ui.push_status("cancelling");
                            pending_exit = false;
                        } else if pending_exit {
                            break;
                        } else {
                            ui.push_status("press ctrl+c again to exit");
                            pending_exit = true;
                        }
                    }
                    UiAction::OpenModelPicker => ui.open_model_picker(&snapshot),
                    UiAction::SelectModel(model_id) => {
                        client.set_model(&model_id).await?;
                        let label = snapshot
                            .models()
                            .available
                            .iter()
                            .find(|candidate| candidate.id == model_id)
                            .map(|candidate| candidate.name.clone())
                            .unwrap_or(model_id);
                        ui.push_status(&format!("model: {label}"));
                        pending_exit = false;
                    }
                    UiAction::PermissionDecision(decision) => {
                        let summary = ui.respond_permission(decision);
                        ui.push_status(&summary);
                        pending_exit = false;
                    }
                    UiAction::Submit(input) => {
                        pending_exit = false;
                        match LocalCommand::parse(&input) {
                            Some(LocalCommand::Exit) => break,
                            Some(LocalCommand::Clear) => ui.clear_transcript(),
                            Some(LocalCommand::Help) => {
                                ui.push_status("/help /model /new /cwd /clear /exit");
                            }
                            Some(LocalCommand::Cwd) => {
                                ui.push_status(&snapshot.cwd().display().to_string());
                            }
                            Some(LocalCommand::Model) => ui.open_model_picker(&snapshot),
                            Some(LocalCommand::New) => {
                                let cwd = snapshot.cwd().to_path_buf();
                                client.new_session(&cwd).await?;
                                ui.push_status("new session");
                            }
                            None => {
                                ui.push_user_message(input.clone());
                                ui.start_assistant_turn();
                                pending_prompt = Some(Box::pin(client.prompt_owned(input)));
                            }
                        }
                    }
                }
            }
            maybe_event = events.recv() => {
                if let Some(event) = maybe_event {
                    ui.handle_app_event(event);
                    pending_exit = false;
                }
            }
            signal = signals.recv() => {
                match signal {
                    crate::types::ShutdownSignal::Sigint => {
                        if pending_prompt.is_some() {
                            client.cancel_current_turn().await?;
                            ui.push_status("cancelling");
                            pending_exit = false;
                        } else if pending_exit {
                            break;
                        } else {
                            ui.push_status("press ctrl+c again to exit");
                            pending_exit = true;
                        }
                    }
                    crate::types::ShutdownSignal::Sigterm => break,
                }
            }
            result = pending_prompt.as_mut().expect("pending prompt missing"), if pending_prompt.is_some() => {
                let result = result?;
                pending_prompt = None;
                ui.finish_turn(&result);
                if matches!(result.stop_reason, agent_client_protocol::StopReason::Cancelled) {
                    ui.push_status("cancelled");
                }
            }
        }
    }

    if let Some(decision) = ui.close_open_permission() {
        let _ = decision;
    }

    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)
            .context("failed to enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("failed to create terminal backend")?;
        terminal.clear().context("failed to clear terminal")?;
        Ok(Self { terminal })
    }

    fn draw<F>(&mut self, render: F) -> Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.terminal
            .draw(render)
            .context("failed to draw terminal")?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

enum UiAction {
    None,
    Submit(String),
    Exit,
    CloseOverlay,
    CancelTurn,
    ClearTranscript,
    OpenModelPicker,
    SelectModel(String),
    PermissionDecision(PermissionDecision),
}

enum LocalCommand {
    Exit,
    Clear,
    Help,
    Model,
    New,
    Cwd,
}

impl LocalCommand {
    fn parse(input: &str) -> Option<Self> {
        match input.trim() {
            "/exit" => Some(Self::Exit),
            "/clear" => Some(Self::Clear),
            "/help" => Some(Self::Help),
            "/model" => Some(Self::Model),
            "/new" => Some(Self::New),
            "/cwd" => Some(Self::Cwd),
            _ => None,
        }
    }
}

struct ChatUi {
    transcript: Vec<TranscriptEntry>,
    composer: Composer,
    overlay: Option<Overlay>,
    scroll_top: u16,
    follow_output: bool,
    transcript_width: u16,
    transcript_height: u16,
}

impl ChatUi {
    fn new() -> Self {
        Self {
            transcript: Vec::new(),
            composer: Composer::default(),
            overlay: None,
            scroll_top: 0,
            follow_output: true,
            transcript_width: 0,
            transcript_height: 0,
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>, session: &SessionState) {
        let area = frame.area();
        let composer_height = self.composer_block_height(area.width.saturating_sub(2));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(composer_height),
            ])
            .split(area);

        self.render_header(frame, chunks[0], session);
        self.render_transcript(frame, chunks[1]);
        self.render_composer(frame, chunks[2]);

        if let Some(overlay) = &self.overlay {
            self.render_overlay(frame, overlay, area);
        }
    }

    fn handle_terminal_event(
        &mut self,
        event: Event,
        turn_in_progress: bool,
        session: &SessionState,
    ) -> UiAction {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if let Some(overlay) = &mut self.overlay {
                    return overlay.handle_key(key);
                }
                self.handle_key(key, turn_in_progress, session)
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.scroll_up(3);
                    UiAction::None
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_down(3);
                    UiAction::None
                }
                _ => UiAction::None,
            },
            Event::Paste(text) => {
                self.composer.insert_str(&text);
                UiAction::None
            }
            Event::Resize(_, _) => UiAction::None,
            _ => UiAction::None,
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        turn_in_progress: bool,
        session: &SessionState,
    ) -> UiAction {
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => UiAction::CancelTurn,
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => UiAction::ClearTranscript,
            (KeyCode::Enter, modifiers)
                if modifiers.contains(KeyModifiers::SHIFT)
                    || modifiers.contains(KeyModifiers::ALT) =>
            {
                self.composer.insert_newline();
                UiAction::None
            }
            (KeyCode::Enter, _) if turn_in_progress => UiAction::None,
            (KeyCode::Enter, _) => {
                let input = self.composer.take();
                if input.trim().is_empty() {
                    UiAction::None
                } else {
                    UiAction::Submit(input)
                }
            }
            (KeyCode::Up, _) => {
                self.composer.move_up();
                UiAction::None
            }
            (KeyCode::Down, _) => {
                self.composer.move_down();
                UiAction::None
            }
            (KeyCode::Left, _) => {
                self.composer.move_left();
                UiAction::None
            }
            (KeyCode::Right, _) => {
                self.composer.move_right();
                UiAction::None
            }
            (KeyCode::Home, _) => {
                self.composer.move_home();
                UiAction::None
            }
            (KeyCode::End, _) => {
                self.composer.move_end();
                UiAction::None
            }
            (KeyCode::Backspace, _) => {
                self.composer.backspace();
                UiAction::None
            }
            (KeyCode::Delete, _) => {
                self.composer.delete();
                UiAction::None
            }
            (KeyCode::Tab, _) => {
                self.composer.insert_str("    ");
                UiAction::None
            }
            (KeyCode::PageUp, _) => {
                self.scroll_up(max(1, self.transcript_height / 2));
                UiAction::None
            }
            (KeyCode::PageDown, _) => {
                self.scroll_down(max(1, self.transcript_height / 2));
                UiAction::None
            }
            (KeyCode::Char('m'), KeyModifiers::CONTROL) => {
                if session.models().available.is_empty() {
                    self.push_status("no models available");
                    UiAction::None
                } else {
                    UiAction::OpenModelPicker
                }
            }
            (KeyCode::Esc, _) => UiAction::Exit,
            (KeyCode::Char(ch), modifiers)
                if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.composer.insert_char(ch);
                UiAction::None
            }
            _ => UiAction::None,
        }
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect, session: &SessionState) {
        let title = session
            .cwd()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(".");
        let model = session
            .models()
            .current_model_id
            .as_deref()
            .unwrap_or("default");
        let line = Line::from(vec![
            Span::styled(
                "mythcode-code",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                title.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("model {model}"), STATUS_STYLE),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_transcript(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.transcript_width = area.width.saturating_sub(2);
        self.transcript_height = area.height.saturating_sub(2);

        let lines = self.transcript_lines();
        let max_scroll = max_scroll(&lines, self.transcript_width, self.transcript_height);
        if self.follow_output {
            self.scroll_top = max_scroll;
        } else {
            self.scroll_top = min(self.scroll_top, max_scroll);
        }

        let block = Block::default().borders(Borders::ALL).title("Conversation");
        let transcript = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_top, 0));
        frame.render_widget(transcript, area);
    }

    fn render_composer(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default().borders(Borders::ALL).title("Prompt");
        let inner = block.inner(area);
        let text = self.composer.render_text();
        frame.render_widget(
            Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
            area,
        );

        if inner.width > 0 && inner.height > 0 && self.overlay.is_none() {
            let (cursor_x, cursor_y) = self.composer.visual_cursor(inner.width);
            let x = inner.x + min(cursor_x as u16, inner.width.saturating_sub(1));
            let y = inner.y + min(cursor_y as u16, inner.height.saturating_sub(1));
            frame.set_cursor_position((x, y));
        }

        let help_y = area.bottom().saturating_sub(1);
        let help_area = Rect::new(area.x + 1, help_y, area.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::styled(HELP_TEXT, STATUS_STYLE)),
            help_area,
        );
    }

    fn render_overlay(&self, frame: &mut Frame<'_>, overlay: &Overlay, area: Rect) {
        let popup = centered_rect(70, 60, area);
        frame.render_widget(Clear, popup);

        match overlay {
            Overlay::Permission(prompt) => {
                let locations = if prompt.request.locations.is_empty() {
                    None
                } else {
                    Some(prompt.request.locations.join(", "))
                };

                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(3),
                        Constraint::Length(locations.as_ref().map(|_| 1).unwrap_or(0)),
                    ])
                    .split(popup);

                let header = Text::from(vec![
                    Line::styled(
                        prompt.request.title.clone(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(
                        prompt
                            .request
                            .subtitle
                            .clone()
                            .unwrap_or_else(|| "permission request".to_string()),
                        STATUS_STYLE,
                    ),
                ]);
                frame.render_widget(
                    Paragraph::new(header)
                        .block(Block::default().borders(Borders::ALL).title("Approval")),
                    vertical[0],
                );

                let items = prompt
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        let marker = if index == prompt.selected {
                            "› "
                        } else {
                            "  "
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(marker, Style::default().fg(Color::Cyan)),
                            Span::raw(option.name.clone()),
                            Span::styled(format!(" ({})", option.kind), STATUS_STYLE),
                        ]))
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    List::new(items).block(Block::default().borders(Borders::ALL).title("Options")),
                    vertical[1],
                );

                if let Some(locations) = locations {
                    frame.render_widget(
                        Paragraph::new(Line::styled(locations, STATUS_STYLE))
                            .block(Block::default().borders(Borders::ALL).title("Locations")),
                        vertical[2],
                    );
                }
            }
            Overlay::Model(prompt) => {
                let items = prompt
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        let marker = if index == prompt.selected {
                            "› "
                        } else {
                            "  "
                        };
                        let description = option.description.clone().unwrap_or_default();
                        ListItem::new(Text::from(vec![
                            Line::from(vec![
                                Span::styled(marker, Style::default().fg(Color::Cyan)),
                                Span::raw(option.name.clone()),
                                Span::styled(format!(" ({})", option.id), STATUS_STYLE),
                            ]),
                            Line::styled(description, STATUS_STYLE),
                        ]))
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    List::new(items).block(Block::default().borders(Borders::ALL).title("Models")),
                    popup,
                );
            }
        }
    }

    fn composer_block_height(&self, width: u16) -> u16 {
        let inner_width = max(1, width as usize);
        let rows = self.composer.visual_lines(inner_width);
        min(12, max(4, rows as u16 + 2))
    }

    fn transcript_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (index, entry) in self.transcript.iter().enumerate() {
            if index > 0 {
                lines.push(Line::raw(""));
            }
            entry.push_lines(&mut lines);
        }
        if lines.is_empty() {
            lines.push(Line::styled(
                "Conversation will appear here.",
                STATUS_STYLE.add_modifier(Modifier::ITALIC),
            ));
        }
        lines
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::AssistantText(text) => self.push_assistant_text(&text),
            AppEvent::Activity(activity) => self.push_thinking(&activity),
            AppEvent::ModeChanged(mode) => self.push_status(&format!("mode: {mode}")),
            AppEvent::SessionTitle(title) => self.push_status(&format!("session: {title}")),
            AppEvent::ToolDiff(diff) => self.push_diff(diff),
            AppEvent::PermissionRequest(request) => self.open_permission(request),
            AppEvent::Warning(message) => self.push_status(&format!("warning: {message}")),
            AppEvent::DebugProtocol(message) => self.push_status(&format!("debug: {message}")),
            AppEvent::ProcessStderr(message) => self.push_status(&format!("stderr: {message}")),
        }
    }

    fn push_user_message(&mut self, text: String) {
        self.transcript.push(TranscriptEntry::User(text));
        self.follow_output = true;
    }

    fn start_assistant_turn(&mut self) {
        self.transcript
            .push(TranscriptEntry::Assistant(String::new()));
        self.follow_output = true;
    }

    fn push_assistant_text(&mut self, text: &str) {
        match self.transcript.last_mut() {
            Some(TranscriptEntry::Assistant(existing)) => existing.push_str(text),
            _ => self
                .transcript
                .push(TranscriptEntry::Assistant(text.to_string())),
        }
        self.follow_output = true;
    }

    fn push_thinking(&mut self, text: &str) {
        self.transcript
            .push(TranscriptEntry::Thinking(text.to_string()));
        self.follow_output = true;
    }

    fn push_status(&mut self, text: &str) {
        self.transcript
            .push(TranscriptEntry::Status(text.to_string()));
        self.follow_output = true;
    }

    fn push_diff(&mut self, diff: DiffPreview) {
        self.transcript.push(TranscriptEntry::Diff(diff));
        self.follow_output = true;
    }

    fn finish_turn(&mut self, _result: &PromptResult) {
        self.follow_output = true;
    }

    fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.scroll_top = 0;
        self.follow_output = true;
    }

    fn open_permission(&mut self, request: PermissionRequestView) {
        self.overlay = Some(Overlay::Permission(PermissionPrompt::new(request)));
    }

    fn open_model_picker(&mut self, session: &SessionState) {
        if session.models().available.is_empty() {
            self.push_status("no models available");
            return;
        }
        self.overlay = Some(Overlay::Model(ModelPrompt::new(
            session.models().available.clone(),
            session.models().current_model_id.as_deref(),
        )));
    }

    fn respond_permission(&mut self, decision: PermissionDecision) -> String {
        let Some(Overlay::Permission(prompt)) = self.overlay.take() else {
            return "no permission request pending".to_string();
        };

        let summary = permission_summary(&decision, &prompt.options);
        let _ = prompt.request.responder.send(decision);
        summary
    }

    fn close_open_permission(&mut self) -> Option<PermissionDecision> {
        let Some(Overlay::Permission(prompt)) = self.overlay.take() else {
            return None;
        };
        let _ = prompt.request.responder.send(PermissionDecision::Cancelled);
        Some(PermissionDecision::Cancelled)
    }

    fn close_overlay(&mut self) {
        if matches!(self.overlay, Some(Overlay::Model(_))) {
            self.overlay = None;
        }
    }

    fn scroll_up(&mut self, amount: u16) {
        self.scroll_top = self.scroll_top.saturating_sub(amount);
        self.follow_output = false;
    }

    fn scroll_down(&mut self, amount: u16) {
        self.scroll_top = self.scroll_top.saturating_add(amount);
        self.follow_output = self.scroll_top
            >= max_scroll(
                &self.transcript_lines(),
                self.transcript_width,
                self.transcript_height,
            );
    }
}

enum Overlay {
    Permission(PermissionPrompt),
    Model(ModelPrompt),
}

impl Overlay {
    fn handle_key(&mut self, key: KeyEvent) -> UiAction {
        match self {
            Overlay::Permission(prompt) => prompt.handle_key(key),
            Overlay::Model(prompt) => prompt.handle_key(key),
        }
    }
}

struct PermissionPrompt {
    request: PermissionRequestView,
    options: Vec<crate::types::PermissionOptionView>,
    selected: usize,
}

impl PermissionPrompt {
    fn new(request: PermissionRequestView) -> Self {
        let mut options = request.options.clone();
        options.sort_by_key(|option| !option.kind.is_accept());
        Self {
            request,
            options,
            selected: 0,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> UiAction {
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                UiAction::None
            }
            KeyCode::Down => {
                self.selected = min(self.selected + 1, self.options.len().saturating_sub(1));
                UiAction::None
            }
            KeyCode::Enter => UiAction::PermissionDecision(PermissionDecision::Selected(
                self.options[self.selected].option_id.clone(),
            )),
            KeyCode::Esc => UiAction::PermissionDecision(PermissionDecision::Cancelled),
            _ => UiAction::None,
        }
    }
}

struct ModelPrompt {
    options: Vec<ModelOption>,
    selected: usize,
}

impl ModelPrompt {
    fn new(options: Vec<ModelOption>, current_model_id: Option<&str>) -> Self {
        let selected = current_model_id
            .and_then(|current| options.iter().position(|option| option.id == current))
            .unwrap_or(0);
        Self { options, selected }
    }

    fn handle_key(&mut self, key: KeyEvent) -> UiAction {
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                UiAction::None
            }
            KeyCode::Down => {
                self.selected = min(self.selected + 1, self.options.len().saturating_sub(1));
                UiAction::None
            }
            KeyCode::Enter => UiAction::SelectModel(self.options[self.selected].id.clone()),
            KeyCode::Esc => UiAction::CloseOverlay,
            _ => UiAction::None,
        }
    }
}

enum TranscriptEntry {
    User(String),
    Assistant(String),
    Thinking(String),
    Status(String),
    Diff(DiffPreview),
}

impl TranscriptEntry {
    fn push_lines(&self, out: &mut Vec<Line<'static>>) {
        match self {
            Self::User(text) => {
                out.push(Line::styled("You", USER_STYLE.add_modifier(Modifier::BOLD)));
                out.extend(plain_text_lines(text, Style::default()));
            }
            Self::Assistant(text) => {
                out.push(Line::styled(
                    "Agent",
                    AGENT_STYLE.add_modifier(Modifier::BOLD),
                ));
                out.extend(markdown_to_lines(text));
            }
            Self::Thinking(text) => {
                out.push(Line::from(vec![
                    Span::styled("thinking ", THINKING_STYLE.add_modifier(Modifier::BOLD)),
                    Span::styled(text.clone(), THINKING_STYLE),
                ]));
            }
            Self::Status(text) => {
                out.push(Line::styled(text.clone(), STATUS_STYLE));
            }
            Self::Diff(diff) => {
                out.push(Line::styled(
                    format!("diff {}", diff.path.display()),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ));
                out.extend(diff_to_lines(diff));
            }
        }
    }
}

#[derive(Default)]
struct Composer {
    text: String,
    cursor: usize,
}

impl Composer {
    fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn insert_str(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    fn backspace(&mut self) {
        if let Some(prev) = previous_boundary(&self.text, self.cursor) {
            self.text.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    fn delete(&mut self) {
        if let Some(next) = next_boundary(&self.text, self.cursor) {
            self.text.drain(self.cursor..next);
        }
    }

    fn move_left(&mut self) {
        if let Some(prev) = previous_boundary(&self.text, self.cursor) {
            self.cursor = prev;
        }
    }

    fn move_right(&mut self) {
        if let Some(next) = next_boundary(&self.text, self.cursor) {
            self.cursor = next;
        }
    }

    fn move_home(&mut self) {
        self.cursor = line_start(&self.text, self.cursor);
    }

    fn move_end(&mut self) {
        self.cursor = line_end(&self.text, self.cursor);
    }

    fn move_up(&mut self) {
        let start = line_start(&self.text, self.cursor);
        if start == 0 {
            return;
        }
        let column = char_column(&self.text[start..self.cursor]);
        let previous_end = start.saturating_sub(1);
        let previous_start = line_start(&self.text, previous_end);
        self.cursor = byte_index_for_column(
            &self.text[previous_start..previous_end],
            previous_start,
            column,
        );
    }

    fn move_down(&mut self) {
        let end = line_end(&self.text, self.cursor);
        if end >= self.text.len() {
            return;
        }
        let column = char_column(&self.text[line_start(&self.text, self.cursor)..self.cursor]);
        let next_start = end + 1;
        let next_end = line_end(&self.text, next_start);
        self.cursor = byte_index_for_column(&self.text[next_start..next_end], next_start, column);
    }

    fn render_text(&self) -> Text<'static> {
        if self.text.is_empty() {
            return Text::from(Line::styled(
                "Ask the agent something…",
                STATUS_STYLE.add_modifier(Modifier::ITALIC),
            ));
        }

        Text::from(
            self.text
                .split('\n')
                .map(|line| Line::raw(line.to_string()))
                .collect::<Vec<_>>(),
        )
    }

    fn visual_lines(&self, width: usize) -> usize {
        visual_position(&self.text, self.text.len(), width).1 + 1
    }

    fn visual_cursor(&self, width: u16) -> (usize, usize) {
        visual_position(&self.text, self.cursor, max(1, width as usize))
    }
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let popup_width = area.width * width_percent / 100;
    let popup_height = area.height * height_percent / 100;
    Rect::new(
        area.x + (area.width.saturating_sub(popup_width)) / 2,
        area.y + (area.height.saturating_sub(popup_height)) / 2,
        max(10, popup_width),
        max(6, popup_height),
    )
}

fn permission_summary(
    decision: &PermissionDecision,
    options: &[crate::types::PermissionOptionView],
) -> String {
    match decision {
        PermissionDecision::Selected(option_id) => options
            .iter()
            .find(|option| &option.option_id == option_id)
            .map(|option| option.name.to_lowercase())
            .unwrap_or_else(|| "selected permission option".to_string()),
        PermissionDecision::Cancelled => "cancelled permission request".to_string(),
    }
}

fn diff_to_lines(diff: &DiffPreview) -> Vec<Line<'static>> {
    let old_text = diff.old_text.as_deref().unwrap_or("");
    let text_diff = TextDiff::from_lines(old_text, &diff.new_text);
    let mut lines = Vec::new();

    for group in text_diff.grouped_ops(3) {
        for op in group {
            for change in text_diff.iter_changes(&op) {
                let style = match change.tag() {
                    ChangeTag::Delete => Style::default().fg(Color::Red),
                    ChangeTag::Insert => Style::default().fg(Color::Green),
                    ChangeTag::Equal => Style::default(),
                };
                let prefix = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                let content = change.to_string_lossy().trim_end_matches('\n').to_string();
                lines.push(Line::styled(format!("{prefix}{content}"), style));
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::styled("(no changes)", STATUS_STYLE));
    }

    lines
}

fn plain_text_lines(text: &str, style: Style) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| Line::styled(line.to_string(), style))
        .collect()
}

fn markdown_to_lines(markdown: &str) -> Vec<Line<'static>> {
    let parser = Parser::new_ext(markdown, Options::all());
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut styles = vec![Style::default()];
    let mut lists: Vec<ListState> = Vec::new();
    let mut in_code_block = false;

    for event in parser {
        match event {
            MarkdownEvent::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    ensure_blank_line(&mut lines);
                    styles.push(heading_style(level));
                }
                Tag::BlockQuote(_) => {
                    ensure_blank_line(&mut lines);
                    current.push(Span::styled("> ", STATUS_STYLE));
                    styles.push(STATUS_STYLE);
                }
                Tag::CodeBlock(kind) => {
                    ensure_blank_line(&mut lines);
                    in_code_block = true;
                    let label = match kind {
                        CodeBlockKind::Fenced(lang) if !lang.is_empty() => format!("```{lang}"),
                        _ => "```".to_string(),
                    };
                    lines.push(Line::styled(label, STATUS_STYLE));
                }
                Tag::List(start) => lists.push(ListState::new(start)),
                Tag::Item => {
                    if !current.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current)));
                    }
                    current.extend(list_prefix(&mut lists));
                }
                Tag::Emphasis => styles.push(current_style(&styles).add_modifier(Modifier::ITALIC)),
                Tag::Strong => styles.push(current_style(&styles).add_modifier(Modifier::BOLD)),
                Tag::Strikethrough => {
                    styles.push(current_style(&styles).add_modifier(Modifier::CROSSED_OUT))
                }
                Tag::Link { .. } => {
                    styles.push(
                        current_style(&styles)
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
                _ => {}
            },
            MarkdownEvent::End(tag_end) => match tag_end {
                pulldown_cmark::TagEnd::Paragraph
                | pulldown_cmark::TagEnd::Heading(_)
                | pulldown_cmark::TagEnd::Item => {
                    if !current.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current)));
                    }
                }
                pulldown_cmark::TagEnd::BlockQuote(_) => {
                    if !current.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current)));
                    }
                    let _ = styles.pop();
                }
                pulldown_cmark::TagEnd::CodeBlock => {
                    in_code_block = false;
                    lines.push(Line::styled("```", STATUS_STYLE));
                }
                pulldown_cmark::TagEnd::List(_) => {
                    if !current.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current)));
                    }
                    let _ = lists.pop();
                }
                pulldown_cmark::TagEnd::Emphasis
                | pulldown_cmark::TagEnd::Strong
                | pulldown_cmark::TagEnd::Strikethrough
                | pulldown_cmark::TagEnd::Link => {
                    let _ = styles.pop();
                }
                _ => {}
            },
            MarkdownEvent::Text(text) => {
                if in_code_block {
                    for line in text.split('\n') {
                        lines.push(Line::styled(line.to_string(), CODE_STYLE));
                    }
                } else {
                    current.push(Span::styled(text.to_string(), current_style(&styles)));
                }
            }
            MarkdownEvent::Code(code) => {
                current.push(Span::styled(code.to_string(), CODE_STYLE));
            }
            MarkdownEvent::SoftBreak => {
                current.push(Span::raw(" "));
            }
            MarkdownEvent::HardBreak => {
                lines.push(Line::from(std::mem::take(&mut current)));
            }
            MarkdownEvent::Rule => {
                ensure_blank_line(&mut lines);
                lines.push(Line::styled("─".repeat(32), STATUS_STYLE));
            }
            MarkdownEvent::Html(html) | MarkdownEvent::InlineHtml(html) => {
                current.push(Span::styled(html.to_string(), STATUS_STYLE));
            }
            MarkdownEvent::FootnoteReference(reference) => {
                current.push(Span::styled(format!("[{reference}]"), STATUS_STYLE));
            }
            MarkdownEvent::TaskListMarker(checked) => {
                current.push(Span::raw(if checked { "[x] " } else { "[ ] " }));
            }
            _ => {}
        }
    }

    if !current.is_empty() {
        lines.push(Line::from(current));
    }

    if lines.is_empty() {
        lines.push(Line::raw(String::new()));
    }

    lines
}

fn ensure_blank_line(lines: &mut Vec<Line<'static>>) {
    if lines.last().map(|line| line.width()).unwrap_or(1) != 0 {
        lines.push(Line::raw(""));
    }
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H2 => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().add_modifier(Modifier::BOLD),
    }
}

fn current_style(styles: &[Style]) -> Style {
    styles.last().copied().unwrap_or_default()
}

fn list_prefix(lists: &mut [ListState]) -> Vec<Span<'static>> {
    let depth = lists.len().saturating_sub(1);
    let mut spans = Vec::new();
    if depth > 0 {
        spans.push(Span::raw("  ".repeat(depth)));
    }
    let marker = lists
        .last_mut()
        .map(ListState::next_marker)
        .unwrap_or_else(|| "- ".to_string());
    spans.push(Span::styled(marker, STATUS_STYLE));
    spans
}

enum ListState {
    Bullet,
    Ordered(usize),
}

impl ListState {
    fn new(start: Option<u64>) -> Self {
        match start {
            Some(value) => Self::Ordered(value as usize),
            None => Self::Bullet,
        }
    }

    fn next_marker(&mut self) -> String {
        match self {
            Self::Bullet => "- ".to_string(),
            Self::Ordered(next) => {
                let marker = format!("{next}. ");
                *next += 1;
                marker
            }
        }
    }
}

fn max_scroll(lines: &[Line<'_>], width: u16, height: u16) -> u16 {
    if width == 0 || height == 0 {
        return 0;
    }

    let total_height: usize = lines
        .iter()
        .map(|line| wrapped_line_height(line, width as usize))
        .sum();
    total_height.saturating_sub(height as usize) as u16
}

fn wrapped_line_height(line: &Line<'_>, width: usize) -> usize {
    let content_width = line.width();
    max(
        1,
        (content_width.max(1) + width.saturating_sub(1)) / width.max(1),
    )
}

fn visual_position(text: &str, cursor: usize, width: usize) -> (usize, usize) {
    let mut x = 0usize;
    let mut y = 0usize;
    let width = width.max(1);

    for ch in text[..cursor].chars() {
        if ch == '\n' {
            x = 0;
            y += 1;
            continue;
        }

        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if x + ch_width > width {
            x = 0;
            y += 1;
        }
        x += ch_width;
        if x >= width {
            x = 0;
            y += 1;
        }
    }

    (x, y)
}

fn previous_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .or_else(|| (cursor < text.len()).then_some(text.len()))
}

fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len())
}

fn char_column(text: &str) -> usize {
    text.chars().count()
}

fn byte_index_for_column(line: &str, offset: usize, column: usize) -> usize {
    let byte_offset = line
        .char_indices()
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(line.len());
    offset + byte_offset
}
