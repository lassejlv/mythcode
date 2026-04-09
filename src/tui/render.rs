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

        // Reserve space for the model info line below the input box
        let model_line_h: u16 = if self.current_model.is_some() { 1 } else { 0 };
        // Total fixed bottom chrome: input box + model line
        let bottom_chrome = input_box_h + model_line_h;

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
        let max_content = h.saturating_sub(MARGIN_TOP + bottom_chrome);

        // History gets the space minus what partials need
        let max_history = max_content.saturating_sub(extra_lines);
        let visible = self.history.visible_lines(max_history as usize);
        let history_rows = visible.len() as u16;

        // Non-sticky: input follows content with a 1-line gap
        let content_rows = history_rows + extra_lines;
        let gap = if content_rows > 0 { 1u16 } else { 0 };
        // Clamp so that input_box + model_line always fit on screen
        let input_row = (MARGIN_TOP + content_rows + gap).min(h.saturating_sub(bottom_chrome));
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

            // Truncate tool hint to keep the spinner line clean
            let short_hint = if tool_hint.len() > 40 {
                let truncated: String = tool_hint.chars().take(37).collect();
                format!("{truncated}…")
            } else {
                tool_hint.to_string()
            };

            execute!(stdout, cursor::MoveTo(0, extra_row))?;
            if short_hint.is_empty() {
                write!(stdout, "\x1b[2K  {C_SPINNER}{frame}{C_RESET}  {shimmer}  {C_DARK}{timer}{C_RESET}")?;
            } else {
                write!(stdout, "\x1b[2K  {C_SPINNER}{frame}{C_RESET}  {shimmer}  {C_DIM}{short_hint}{C_RESET}  {C_DARK}{timer}{C_RESET}")?;
            }
        }

        // Permission options (rendered between history and input)
        if let Some(ref perm) = self.pending_permission {
            const BG_GREEN: &str = "\x1b[48;2;20;50;30m";
            const BG_RED: &str = "\x1b[48;2;60;20;25m";
            const FG_GREEN: &str = "\x1b[38;2;166;227;161m";
            const FG_RED: &str = "\x1b[38;2;243;139;168m";
            const FG_WHITE: &str = "\x1b[38;2;205;214;244m";
            const BG_RESET: &str = "\x1b[49m";

            // Layout: blank + hint + blank + options (each padded)
            let total_rows = perm.options.len() as u16 + 3; // hint + blank + options + blank
            let perm_start = input_row.saturating_sub(total_rows);

            let mut row = perm_start;

            // Blank line
            if row < input_row {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
                row += 1;
            }

            // Hint
            if row < input_row {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K  {C_DIM}↑↓ select · enter confirm · esc cancel{C_RESET}")?;
                row += 1;
            }

            // Blank line
            if row < input_row {
                execute!(stdout, cursor::MoveTo(0, row))?;
                write!(stdout, "\x1b[2K")?;
                row += 1;
            }

            // Options
            for (i, opt) in perm.options.iter().enumerate() {
                if row >= input_row {
                    break;
                }
                execute!(stdout, cursor::MoveTo(0, row))?;
                let is_accept = opt.kind.is_accept();
                let label = &opt.name;

                if i == perm.selected {
                    let (bg, fg) = if is_accept {
                        (BG_GREEN, FG_WHITE)
                    } else {
                        (BG_RED, FG_WHITE)
                    };
                    // Pad the label to create a wider background pill
                    let padded = format!(" ▸ {label} ");
                    write!(stdout, "\x1b[2K  {bg}{fg}{padded}{BG_RESET}{C_RESET}")?;
                } else {
                    let fg = if is_accept { FG_GREEN } else { FG_RED };
                    write!(stdout, "\x1b[2K    {fg}{label}{C_RESET}")?;
                }
                row += 1;
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

        // Suggestions: floating palette above the input box
        let suggestion_count = self.suggestions.len().min(8) as u16;
        // menu = top border + items + bottom border/hint
        let suggestion_menu_height = if suggestion_count > 0 { suggestion_count + 2 } else { 0 };
        let below_start = input_row + bottom_chrome;
        let room_below = h.saturating_sub(below_start);
        let render_above = suggestion_count > 0 && room_below < suggestion_menu_height;

        if !self.suggestions.is_empty() {
            let mut stdout = io::stdout();
            let menu_height = suggestion_menu_height;
            let base_row = if render_above {
                input_row.saturating_sub(menu_height)
            } else {
                below_start
            };

            const BG_SEL: &str = "\x1b[48;5;237m"; // subtle selection bg
            const BG_RESET: &str = "\x1b[49m";

            // Find max label width for alignment
            let max_label = self.suggestions.iter().take(8).map(|s| s.label.len()).max().unwrap_or(0);
            // Find max description width to size the box
            let max_desc = self.suggestions.iter().take(8).map(|s| s.description.len()).max().unwrap_or(0);
            let inner_width = (max_label + 2 + max_desc + 4).max(20); // 4 for "  ▸ " prefix padding
            let box_width = inner_width + 6; // borders + padding
            let box_width = box_width.min(w.saturating_sub(2) as usize);

            // Top border with header
            let top_row = base_row;
            if top_row < h {
                execute!(stdout, cursor::MoveTo(1, top_row))?;
                let is_command = self.suggestions.first().map_or(false, |s| s.label.starts_with('/'));
                let header_label = if is_command { " Commands " } else { " Files " };
                let border_rest = box_width.saturating_sub(header_label.len() + 1); // 1 for ╭
                write!(
                    stdout,
                    "\x1b[2K{C_DARK}╭{C_RESET}{C_DIM}{header_label}{C_RESET}{C_DARK}{}{C_RESET}",
                    "─".repeat(border_rest)
                )?;
            }

            // Max space available for description text
            // Layout: │ [▸/ ] label  desc [pad] │
            //          1  3   label  2          1
            let desc_budget = box_width.saturating_sub(4 + max_label + 2 + 2); // prefix + label + gap + borders

            // Items
            for (i, suggestion) in self.suggestions.iter().enumerate().take(8) {
                let row = top_row + 1 + i as u16;
                if row >= h {
                    break;
                }
                execute!(stdout, cursor::MoveTo(1, row))?;
                let padded_label = format!("{:<width$}", suggestion.label, width = max_label);

                // Truncate description to fit in the box
                let desc = if suggestion.description.len() > desc_budget {
                    let mut truncated: String = suggestion.description.chars().take(desc_budget.saturating_sub(1)).collect();
                    truncated.push('…');
                    truncated
                } else {
                    suggestion.description.clone()
                };

                // Visible content width (without ANSI codes)
                let visible_len = if desc.is_empty() {
                    4 + padded_label.len()
                } else {
                    4 + padded_label.len() + 2 + desc.len()
                };
                let pad = box_width.saturating_sub(visible_len + 2); // 2 for │ borders

                if self.selected_suggestion == Some(i) {
                    write!(stdout, "\x1b[2K{C_DARK}│{C_RESET}{BG_SEL} {C_CYAN}▸ {padded_label}{C_RESET}{BG_SEL}")?;
                    if !desc.is_empty() {
                        write!(stdout, "  {C_DIM}{desc}{C_RESET}{BG_SEL}")?;
                    }
                    write!(stdout, "{}{BG_RESET}{C_DARK}│{C_RESET}", " ".repeat(pad))?;
                } else {
                    write!(stdout, "\x1b[2K{C_DARK}│{C_RESET}   {C_DARK}{padded_label}{C_RESET}")?;
                    if !desc.is_empty() {
                        write!(stdout, "  {C_DIM}{desc}{C_RESET}")?;
                    }
                    write!(stdout, "{}{C_DARK}│{C_RESET}", " ".repeat(pad + 1))?; // +1 for ▸ vs space
                }
            }

            // Bottom border with hint
            let bottom_row = top_row + 1 + suggestion_count;
            if bottom_row < h {
                execute!(stdout, cursor::MoveTo(1, bottom_row))?;
                let hint = " tab/↑↓ navigate · enter select · esc close ";
                let border_rest = box_width.saturating_sub(hint.len() + 1);
                write!(
                    stdout,
                    "\x1b[2K{C_DARK}╰{}{C_RESET}{C_DARK}{hint}{C_RESET}",
                    "─".repeat(border_rest)
                )?;
            }

            // Clear remaining rows
            if !render_above {
                let clear_from = base_row + menu_height;
                for row in clear_from..h {
                    execute!(stdout, cursor::MoveTo(0, row))?;
                    write!(stdout, "\x1b[2K")?;
                }
            }
            stdout.flush()?;
        } else if self.select_mode.is_none() {
            // Clear area below bottom chrome
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
            let sel_below_start = input_row + bottom_chrome;
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
