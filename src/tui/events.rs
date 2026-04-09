/// App event handling, streaming buffer management, and turn management.

use std::time::Instant;

use crate::types::{AppEvent, PermissionDecision};

use super::history::{
    format_activity, format_diff, format_plan, format_tool_output,
    format_turn_separator, format_user_message, format_warning, LineType,
};
use super::markdown::wrap_ansi;
use super::permission::PendingPermission;
use super::{C_DIM, C_RESET, Tui};

impl Tui {
    // ── Event handling ──────────────────────────────────────────────

    pub(super) fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::UserMessage(text) => {
                // User messages replayed from session history
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                self.live_output_lines = 0;
                self.last_activity = None;
                let lines = format_user_message(text.trim());
                self.history.push_lines(lines, LineType::UserMessage);
            }
            AppEvent::AssistantText(text) => {
                self.stop_spinner();
                self.tool_active = false;
                self.live_output_lines = 0;
                // Add spacing when transitioning from thinking to assistant
                if self.thinking_open {
                    self.flush_thinking();
                    self.history.push(String::new(), LineType::Separator);
                }
                self.assistant_open = true;
                self.assistant_buffer.push_str(&text);
                self.printed_text = true;
                self.flush_complete_assistant_lines();
            }
            AppEvent::ThinkingText(text) => {
                self.stop_spinner();
                self.tool_active = false;
                self.flush_assistant();
                self.thinking_open = true;
                self.thinking_buffer.push_str(&text);
                self.flush_complete_thinking_lines();
            }
            AppEvent::Activity(activity) => {
                if self.last_activity.as_deref() == Some(&activity) {
                    return;
                }
                self.flush_assistant();
                self.flush_thinking();
                self.live_output_lines = 0; // new tool, reset live output
                // Add spacing before tool activity (unless it's the first one)
                if self.last_activity.is_some() {
                    self.history.push(String::new(), LineType::Separator);
                }
                self.history.push(format_activity(&activity), LineType::Activity);
                self.last_activity = Some(activity);
                self.tool_active = true;
                self.spinner_active = true;
            }
            AppEvent::ModeChanged(mode) => {
                // Don't stop the spinner — mode changes often arrive at
                // the start of a turn before any content.
                self.current_mode = Some(mode);
            }
            AppEvent::SessionTitle(_title) => {
                // Title shown in input box border, no need to log
            }
            AppEvent::ToolDiff(diff) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                let lines = format_diff(&diff);
                self.history.push_lines(lines, LineType::Diff);
            }
            AppEvent::ToolOutput(output) => {
                // Replace previous live output lines (including the activity line)
                let pop_count = if self.live_output_lines > 0 {
                    self.live_output_lines
                } else {
                    // First output for this tool — pop the activity announcement line
                    1
                };
                self.history.pop_n(pop_count);
                let lines = format_tool_output(&output.title, &output.content, output.total_lines);
                self.live_output_lines = lines.len();
                self.history.push_lines(lines, LineType::Activity);
                self.last_tool_outputs.push(output);
            }
            AppEvent::PlanUpdate(plan) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();
                let lines = format_plan(&plan);
                self.history.push_lines(lines, LineType::Activity);
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

    pub(super) fn dispatch_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::PermissionRequest(req) => {
                self.stop_spinner();
                self.flush_assistant();
                self.flush_thinking();

                // Cancel any existing pending permission before replacing
                if let Some(old_perm) = self.pending_permission.take() {
                    let _ = old_perm.responder.send(PermissionDecision::Cancelled);
                }

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

    pub(super) fn flush_complete_assistant_lines(&mut self) {
        let w = self.term_width as usize;
        while let Some(newline_pos) = self.assistant_buffer.find('\n') {
            let line = self.assistant_buffer[..newline_pos].to_string();
            let rendered = self.md_parser.render_line(&line);
            for wrapped in wrap_ansi(&rendered, w) {
                self.history.push(wrapped, LineType::Assistant);
            }
            self.assistant_buffer = self.assistant_buffer[newline_pos + 1..].to_string();
        }
        self.partial_line = if self.assistant_buffer.is_empty() {
            None
        } else {
            Some(self.md_parser.render_line(&self.assistant_buffer))
        };
    }

    pub(super) fn flush_complete_thinking_lines(&mut self) {
        let w = self.term_width as usize;
        while let Some(newline_pos) = self.thinking_buffer.find('\n') {
            let line = self.thinking_buffer[..newline_pos].to_string();
            let rendered = self.md_parser.render_thinking_line(&line);
            for wrapped in wrap_ansi(&rendered, w) {
                self.history.push(wrapped, LineType::Thinking);
            }
            self.thinking_buffer = self.thinking_buffer[newline_pos + 1..].to_string();
        }
        self.thinking_partial = if self.thinking_buffer.is_empty() {
            None
        } else {
            Some(self.md_parser.render_thinking_line(&self.thinking_buffer))
        };
    }

    pub(super) fn flush_assistant(&mut self) {
        if !self.assistant_buffer.is_empty() {
            let w = self.term_width as usize;
            let line = std::mem::take(&mut self.assistant_buffer);
            let rendered = self.md_parser.render_line(&line);
            for wrapped in wrap_ansi(&rendered, w) {
                self.history.push(wrapped, LineType::Assistant);
            }
            self.partial_line = None;
        }
        self.assistant_open = false;
    }

    pub(super) fn flush_thinking(&mut self) {
        if !self.thinking_buffer.is_empty() {
            let w = self.term_width as usize;
            let line = std::mem::take(&mut self.thinking_buffer);
            let rendered = self.md_parser.render_thinking_line(&line);
            for wrapped in wrap_ansi(&rendered, w) {
                self.history.push(wrapped, LineType::Thinking);
            }
            self.thinking_partial = None;
        }
        self.thinking_open = false;
    }

    // ── Turn management ─────────────────────────────────────────────

    pub(super) fn start_turn(&mut self) {
        self.stop_spinner();
        self.flush_assistant();
        self.flush_thinking();
        self.turn_count += 1;
        self.assistant_open = false;
        self.thinking_open = false;
        self.printed_text = false;
        self.last_activity = None;
        self.last_tool_outputs.clear();
        self.live_output_lines = 0;
        self.spinner_active = true;
        self.spinner_frame = 0;
        self.turn_start = Some(Instant::now());
        self.tool_active = false;
    }

    pub(super) fn finish_turn(&mut self, result: &crate::types::PromptResult) {
        self.stop_spinner();
        self.flush_assistant();
        self.flush_thinking();

        let elapsed = self.turn_start
            .map(|t| crate::spinner::format_elapsed(t.elapsed().as_secs()))
            .unwrap_or_default();

        let display_elapsed = if matches!(
            result.stop_reason,
            agent_client_protocol::StopReason::Cancelled
        ) {
            format!("cancelled · {elapsed}")
        } else {
            elapsed
        };
        let sep_lines = format_turn_separator(&display_elapsed);
        self.history.push_lines(sep_lines, LineType::Separator);
        self.turn_start = None;
        self.tool_active = false;
    }

    pub(super) fn stop_spinner(&mut self) {
        self.spinner_active = false;
    }
}
