/// Layout and rendering.

use std::io::{self, Write};

use crossterm::{cursor, execute};

use super::{
    C_BOLD_CYAN, C_CYAN, C_DARK, C_DIM, C_RESET, C_SPINNER, INPUT_BOX_MIN_HEIGHT, MARGIN_TOP,
    Tui,
};

impl Tui {
    pub(super) fn input_box_height(&self) -> u16 {
        // Grow with content lines, capped at 8
        let lines = self.input.line_count();
        (lines + 2).max(INPUT_BOX_MIN_HEIGHT).min(8) // +2 for borders
    }

    pub(super) fn redraw(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        let w = self.term_width;
        let h = self.term_height;
        let input_box_h = self.input_box_height();

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
        let max_content = h.saturating_sub(MARGIN_TOP + input_box_h);

        // History gets the space minus what partials need
        let max_history = max_content.saturating_sub(extra_lines);
        let visible = self.history.visible_lines(max_history as usize);
        let history_rows = visible.len() as u16;

        // Non-sticky: input follows content with a 1-line gap
        let content_rows = history_rows + extra_lines;
        let gap = if content_rows > 0 { 1u16 } else { 0 };
        let input_row = (MARGIN_TOP + content_rows + gap).min(h.saturating_sub(input_box_h));
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

        // Spinner with timer and fun messages
        if self.spinner_active
            && self.partial_line.is_none()
            && self.thinking_partial.is_none()
            && extra_row < input_row
        {
            let elapsed_secs = self.turn_start
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            let frame = crate::spinner::frame(self.spinner_frame);
            let timer = crate::spinner::format_elapsed(elapsed_secs);

            let tool_hint = if self.tool_active {
                self.last_activity.as_deref().unwrap_or("")
            } else {
                ""
            };

            let status_msg = if self.tool_active {
                crate::spinner::tool_message_for(tool_hint, elapsed_secs)
            } else {
                crate::spinner::thinking_message(elapsed_secs)
            };
            let shimmer = crate::spinner::shimmer(self.spinner_frame, status_msg);

            execute!(stdout, cursor::MoveTo(0, extra_row))?;
            if tool_hint.is_empty() {
                write!(stdout, "\x1b[2K  {C_SPINNER}{frame}{C_RESET}  {shimmer}  {C_DARK}{timer}{C_RESET}")?;
            } else {
                write!(stdout, "\x1b[2K  {C_SPINNER}{frame}{C_RESET}  {shimmer}  {C_DIM}{tool_hint}{C_RESET}  {C_DARK}{timer}{C_RESET}")?;
            }
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
        let mut title = if let Some(ref mode) = self.current_mode {
            format!("{} · {}", self.project_name, mode)
        } else {
            self.project_name.clone()
        };
        if !self.message_queue.is_empty() {
            title.push_str(&format!(" ({} queued)", self.message_queue.len()));
        }
        let is_active = self.pending_permission.is_none() && self.select_mode.is_none();
        self.input.render(input_row, w, input_box_h, &title, is_active)?;

        // Model info line below input box
        let model_line_h: u16 = if self.current_model.is_some() { 1 } else { 0 };
        let model_row = input_row + input_box_h;
        if let Some(ref model) = self.current_model {
            if model_row < h {
                let mut stdout = io::stdout();
                let mode_hint = if self.current_mode.is_some() {
                    format!("  {C_DARK}shift+tab to switch mode{C_RESET}")
                } else {
                    String::new()
                };
                execute!(stdout, cursor::MoveTo(0, model_row))?;
                write!(stdout, "\x1b[2K   {C_DARK}{model}{mode_hint}{C_RESET}")?;
                stdout.flush()?;
            }
        }

        // Suggestions: render below if room, otherwise above the input box
        let suggestion_count = self.suggestions.len().min(8) as u16;
        let below_start = input_row + input_box_h + model_line_h;
        let room_below = h.saturating_sub(below_start);
        let render_above = suggestion_count > 0 && room_below < suggestion_count;

        if !self.suggestions.is_empty() {
            let mut stdout = io::stdout();
            let base_row = if render_above {
                // Render above the input box
                input_row.saturating_sub(suggestion_count)
            } else {
                below_start
            };

            for (i, suggestion) in self.suggestions.iter().enumerate().take(8) {
                let row = base_row + i as u16;
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
            // Clear remaining rows below input if rendering below
            if !render_above {
                let clear_from = base_row + suggestion_count;
                for row in clear_from..h {
                    execute!(stdout, cursor::MoveTo(0, row))?;
                    write!(stdout, "\x1b[2K")?;
                }
            }
            stdout.flush()?;
        } else if self.select_mode.is_none() {
            // Clear area below input
            let mut stdout = io::stdout();
            for row in below_start..h {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
            }
            stdout.flush()?;
        }

        // Select mode overlay — above or below input box
        if let Some(ref sel) = self.select_mode {
            let mut stdout = io::stdout();
            let items_shown = sel.filtered.len().min(10) as u16;
            let sel_total = items_shown + 1; // items + title row
            let sel_below_start = input_row + input_box_h;
            let room_below = h.saturating_sub(sel_below_start);
            let sel_start = if room_below >= sel_total {
                sel_below_start
            } else {
                input_row.saturating_sub(sel_total)
            };

            // Title + filter
            if sel_start < h {
                execute!(stdout, cursor::MoveTo(0, sel_start))?;
                let filter_display = if sel.filter.is_empty() {
                    format!("{C_DIM}type to filter…{C_RESET}")
                } else {
                    format!("\x1b[1m{}\x1b[0m", sel.filter)
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
            let clear_from = sel_start + 1 + items_shown;
            for row in clear_from..h {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
            }
            stdout.flush()?;
        }

        // Reposition cursor inside the input box (rendering below may have moved it)
        self.input.reposition_cursor(input_row, w, input_box_h)?;

        Ok(())
    }
}
