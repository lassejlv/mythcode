/// Layout and rendering.
use std::io;

use crate::terminal_ui::FrameBuffer;

use super::{
    C_BOLD_CYAN, C_CYAN, C_DARK, C_DIM, C_RESET, C_SPINNER, INPUT_BOX_MIN_HEIGHT, MARGIN_TOP, Tui,
    TurnState,
};

impl Tui {
    pub(super) fn input_box_height(&self) -> u16 {
        let lines = self.input.line_count();
        (lines + 2).clamp(INPUT_BOX_MIN_HEIGHT, 8)
    }

    pub(super) fn redraw(&mut self) -> io::Result<()> {
        let w = self.term_width;
        let h = self.term_height;
        let input_box_h = self.input_box_height();
        let model_line_h: u16 = 0;
        let bottom_chrome = input_box_h + model_line_h;
        let mut frame = FrameBuffer::new(h);

        let live_thinking = if self.history.scroll_offset == 0 {
            self.live_thinking_lines()
        } else {
            Vec::new()
        };
        let live_assistant = if self.history.scroll_offset == 0 {
            self.live_assistant_lines()
        } else {
            Vec::new()
        };
        let status_lines = self.status_lines();
        let extra_lines = (live_thinking.len() + live_assistant.len() + status_lines.len()) as u16;

        let max_content = h.saturating_sub(MARGIN_TOP + bottom_chrome);
        let max_history = max_content.saturating_sub(extra_lines);
        let visible = self.history.visible_lines(max_history as usize, w as usize);
        let history_rows = visible.len() as u16;

        let content_rows = history_rows + extra_lines;
        let gap = 0;
        let input_row = (MARGIN_TOP + content_rows + gap).min(h.saturating_sub(bottom_chrome));

        for (row, line) in visible.iter().enumerate() {
            frame.set_line(MARGIN_TOP + row as u16, line.content.clone());
        }

        let mut extra_row = MARGIN_TOP + history_rows;
        for line in &live_thinking {
            if extra_row >= input_row {
                break;
            }
            frame.set_line(extra_row, line.clone());
            extra_row += 1;
        }
        for line in &live_assistant {
            if extra_row >= input_row {
                break;
            }
            frame.set_line(extra_row, line.clone());
            extra_row += 1;
        }
        for line in &status_lines {
            if extra_row >= input_row {
                break;
            }
            frame.set_line(extra_row, line.clone());
            extra_row += 1;
        }

        let mut title = if let Some(ref mode) = self.current_mode {
            format!("{} · {}", self.project_name, mode)
        } else {
            self.project_name.clone()
        };
        if let Some(ref model) = self.current_model {
            title.push_str(&format!(" · {}", super::shorten_model_name(model)));
        }
        for value in self.status_items.values() {
            title.push_str(&format!(" · {value}"));
        }
        for img in &self.pending_images {
            title.push_str(&format!(" [Image #{}]", img.number));
        }
        if !self.message_queue.is_empty() {
            title.push_str(&format!(" ({} queued)", self.message_queue.len()));
        }
        let is_active = self.pending_permission.is_none() && self.select_mode.is_none();
        let input_frame = self.input.render_frame(w, input_box_h, &title, is_active);
        for (offset, line) in input_frame.lines.iter().enumerate() {
            frame.set_line(input_row + offset as u16, line.clone());
        }
        if is_active {
            frame.set_cursor(
                input_frame.cursor_x,
                input_row + input_frame.cursor_y,
                input_frame.cursor_shape,
            );
        }

        self.render_suggestions(&mut frame, input_row, bottom_chrome, h);
        self.render_select_mode(&mut frame, input_row, bottom_chrome, h);

        let mut stdout = io::stdout();
        frame.render(&mut stdout)
    }

    fn render_suggestions(
        &self,
        frame: &mut FrameBuffer,
        input_row: u16,
        bottom_chrome: u16,
        height: u16,
    ) {
        if self.suggestions.is_empty() {
            return;
        }

        let suggestion_count = self.suggestions.len().min(6) as u16;
        let menu_height = suggestion_count;
        let below_start = input_row + bottom_chrome;
        let room_below = height.saturating_sub(below_start);
        let render_above = room_below < menu_height;
        let base_row = if render_above {
            input_row.saturating_sub(menu_height)
        } else {
            below_start
        };

        for (i, suggestion) in self.suggestions.iter().enumerate().take(6) {
            let row = base_row + i as u16;
            if row >= height {
                break;
            }
            let line = if self.selected_suggestion == Some(i) {
                format!("  {C_CYAN}›{C_RESET} {}", suggestion.label)
            } else {
                format!("    {C_DIM}{}{C_RESET}", suggestion.label)
            };
            frame.set_line(row, line);
        }
    }

    fn render_select_mode(
        &self,
        frame: &mut FrameBuffer,
        input_row: u16,
        bottom_chrome: u16,
        height: u16,
    ) {
        let Some(ref sel) = self.select_mode else {
            return;
        };

        let items_shown = sel.filtered.len().min(10) as u16;
        let sel_total = items_shown + 1;
        let sel_below_start = input_row + bottom_chrome;
        let room_below = height.saturating_sub(sel_below_start);
        let sel_start = if room_below >= sel_total {
            sel_below_start
        } else {
            input_row.saturating_sub(sel_total)
        };

        let filter_display = if sel.filter.is_empty() {
            format!("{C_DIM}type to filter…{C_RESET}")
        } else {
            format!("\x1b[1m{}\x1b[0m", sel.filter)
        };
        frame.set_line(
            sel_start,
            format!("  {C_BOLD_CYAN}{}{C_RESET}  {filter_display}", sel.title),
        );

        for (visible_idx, &item_idx) in sel.filtered.iter().enumerate().take(10) {
            let row = sel_start + 1 + visible_idx as u16;
            if row >= height {
                break;
            }
            let item = &sel.items[item_idx];
            let line = if visible_idx == sel.selected {
                format!(
                    "  {C_CYAN}▸ {}{C_RESET} {C_DIM}({}){C_RESET}",
                    item.display, item.id
                )
            } else {
                format!("    {C_DIM}{} ({}){C_RESET}", item.display, item.id)
            };
            frame.set_line(row, line);
        }
    }

    pub(super) fn status_lines(&self) -> Vec<String> {
        if let Some(ref perm) = self.pending_permission {
            const BG_GREEN: &str = "\x1b[48;2;20;50;30m";
            const BG_RED: &str = "\x1b[48;2;60;20;25m";
            const FG_GREEN: &str = "\x1b[38;2;166;227;161m";
            const FG_RED: &str = "\x1b[38;2;243;139;168m";
            const FG_WHITE: &str = "\x1b[38;2;205;214;244m";
            const BG_RESET: &str = "\x1b[49m";

            let mut lines = vec![format!(
                "  {C_DIM}↑↓ select · enter confirm · esc cancel{C_RESET}"
            )];
            for (i, opt) in perm.options.iter().enumerate() {
                let is_accept = opt.kind.is_accept();
                let label = &opt.name;
                if i == perm.selected {
                    let (bg, fg) = if is_accept {
                        (BG_GREEN, FG_WHITE)
                    } else {
                        (BG_RED, FG_WHITE)
                    };
                    let padded = format!(" ▸ {label} ");
                    lines.push(format!("  {bg}{fg}{padded}{BG_RESET}{C_RESET}"));
                } else {
                    let fg = if is_accept { FG_GREEN } else { FG_RED };
                    lines.push(format!("    {fg}{label}{C_RESET}"));
                }
            }
            return lines;
        }

        if !self.spinner_active || self.turn_state == TurnState::Idle {
            return Vec::new();
        }

        let elapsed_secs = self.turn_start.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let timer = crate::spinner::format_elapsed(elapsed_secs);
        let tool_hint = if self.turn_state == TurnState::ToolRunning {
            self.last_activity.as_deref().unwrap_or("")
        } else {
            ""
        };
        let status_msg = match self.turn_state {
            TurnState::ToolRunning => crate::spinner::tool_message_for(tool_hint, elapsed_secs),
            TurnState::Responding => "Responding…",
            TurnState::Thinking | TurnState::AwaitingPermission => {
                crate::spinner::thinking_message(elapsed_secs)
            }
            TurnState::Idle => "",
        };
        let shimmer = crate::spinner::shimmer(self.spinner_frame, status_msg);
        let short_hint = if tool_hint.len() > 40 {
            let truncated: String = tool_hint.chars().take(37).collect();
            format!("{truncated}…")
        } else {
            tool_hint.to_string()
        };

        let line = if short_hint.is_empty() {
            format!("  {C_SPINNER}{shimmer}{C_RESET}  {C_DARK}{timer}{C_RESET}")
        } else {
            format!(
                "  {C_SPINNER}{shimmer}{C_RESET}  {C_DIM}{short_hint}{C_RESET}  {C_DARK}{timer}{C_RESET}"
            )
        };

        vec![line]
    }
}
