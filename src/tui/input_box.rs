use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders, Padding, Widget};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::terminal_ui::CursorShape;

pub struct InputBox {
    content: String,
    cursor_byte: usize,
    input_history: Vec<String>,
    history_index: Option<usize>,
}

pub struct InputRender {
    pub lines: Vec<String>,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_shape: CursorShape,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor_byte: 0,
            input_history: Vec::new(),
            history_index: None,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn take_content(&mut self) -> String {
        let text = std::mem::take(&mut self.content);
        self.cursor_byte = 0;
        self.history_index = None;
        if !text.trim().is_empty() {
            self.input_history.push(text.clone());
        }
        text
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor_byte = 0;
        self.history_index = None;
    }

    pub fn set_content(&mut self, s: &str) {
        self.content = s.to_string();
        self.cursor_byte = self.content.len();
    }

    pub fn insert_char(&mut self, ch: char) {
        self.content.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
    }

    pub fn insert_newline(&mut self) {
        self.content.insert(self.cursor_byte, '\n');
        self.cursor_byte += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.content.insert_str(self.cursor_byte, s);
        self.cursor_byte += s.len();
    }

    pub fn line_count(&self) -> u16 {
        (self.content.matches('\n').count() + 1).min(6) as u16
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        let prev = prev_boundary(&self.content, self.cursor_byte);
        self.content.remove(prev);
        self.cursor_byte = prev;
    }

    pub fn delete_char_after(&mut self) {
        if self.cursor_byte < self.content.len() {
            self.content.remove(self.cursor_byte);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        self.cursor_byte = prev_boundary(&self.content, self.cursor_byte);
    }

    pub fn move_right(&mut self) {
        if self.cursor_byte >= self.content.len() {
            return;
        }
        if let Some(ch) = self.content[self.cursor_byte..].chars().next() {
            self.cursor_byte += ch.len_utf8();
        }
    }

    pub fn move_home(&mut self) {
        let (line_start, _) = self.current_line_bounds();
        self.cursor_byte = line_start;
    }

    pub fn move_end(&mut self) {
        let (_, line_end) = self.current_line_bounds();
        self.cursor_byte = line_end;
    }

    pub fn delete_word_before(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        let before = &self.content[..self.cursor_byte];
        let trimmed = before.trim_end();
        let new_end = trimmed
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        self.content.drain(new_end..self.cursor_byte);
        self.cursor_byte = new_end;
    }

    pub fn history_prev(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            None => self.input_history.len() - 1,
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.history_index = Some(idx);
        self.set_content(&self.input_history[idx].clone());
    }

    pub fn history_next(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx + 1 >= self.input_history.len() {
            self.history_index = None;
            self.clear();
        } else {
            self.history_index = Some(idx + 1);
            self.set_content(&self.input_history[idx + 1].clone());
        }
    }

    pub fn render_frame(
        &self,
        width: u16,
        height: u16,
        title: &str,
        is_active: bool,
    ) -> InputRender {
        let area = Rect::new(1, 0, width.saturating_sub(2), height);
        let mut buf = Buffer::empty(area);

        let border_color = if is_active {
            Color::Indexed(237)
        } else {
            Color::Indexed(234)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(border_color))
            .title(format!(" {title} "))
            .title_style(Style::default().fg(Color::Indexed(245)))
            .padding(Padding::horizontal(1));

        let inner = block.inner(area);
        block.render(area, &mut buf);

        let prompt_col = inner.x;
        if inner.width > 2 {
            let cell = &mut buf[(prompt_col, inner.y)];
            cell.set_char('❯');
            cell.set_fg(Color::Indexed(75));
        }

        let text_start = prompt_col + 2;
        let text_width = inner.width.saturating_sub(2) as usize;
        let lines: Vec<&str> = self.content.split('\n').collect();

        let before_cursor = &self.content[..self.cursor_byte];
        let cursor_line_idx = before_cursor.matches('\n').count();
        let cursor_line_start = before_cursor.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let cursor_col_in_line = display_width(&self.content[cursor_line_start..self.cursor_byte]);

        for (line_idx, line) in lines.iter().enumerate() {
            let row_in_box = line_idx as u16;
            if row_in_box >= inner.height {
                break;
            }

            let y = inner.y + row_in_box;
            let x_start = if line_idx == 0 {
                text_start
            } else {
                prompt_col + 2
            };
            let viewport_width = if line_idx == 0 {
                text_width
            } else {
                inner.width.saturating_sub(2) as usize
            };

            if viewport_width == 0 {
                continue;
            }

            let scroll = if line_idx == cursor_line_idx {
                horizontal_scroll(cursor_col_in_line, viewport_width)
            } else {
                0
            };

            let visible = visible_slice(line, scroll, viewport_width);
            let mut x = x_start;
            for ch in visible.chars() {
                if x >= area.right() || y >= area.bottom() {
                    break;
                }
                let cell = &mut buf[(x, y)];
                cell.set_char(ch);
                cell.set_fg(Color::Reset);
                x = x.saturating_add(ch.width().unwrap_or(1) as u16);
            }

            if line_idx > 0 && inner.width > 2 {
                let cell = &mut buf[(prompt_col, y)];
                cell.set_char('·');
                cell.set_fg(Color::Indexed(240));
            }
        }

        if self.content.is_empty() {
            let placeholder = "…";
            let mut x = text_start;
            for ch in placeholder.chars() {
                if x >= area.right() {
                    break;
                }
                let cell = &mut buf[(x, inner.y)];
                cell.set_char(ch);
                cell.set_fg(Color::Indexed(238));
                x = x.saturating_add(ch.width().unwrap_or(1) as u16);
            }
        }

        let (cursor_x, cursor_y) = self.cursor_screen_pos(inner, text_start);
        let mut rendered_lines = Vec::with_capacity(height as usize);
        for row in 0..height {
            rendered_lines.push(buffer_row_to_ansi(&buf, area, row));
        }

        InputRender {
            lines: rendered_lines,
            cursor_x,
            cursor_y,
            cursor_shape: CursorShape::Block,
        }
    }

    fn current_line_bounds(&self) -> (usize, usize) {
        let before = &self.content[..self.cursor_byte];
        let line_start = before.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        let line_end = self.content[self.cursor_byte..]
            .find('\n')
            .map(|idx| self.cursor_byte + idx)
            .unwrap_or(self.content.len());
        (line_start, line_end)
    }

    fn cursor_screen_pos(&self, inner: Rect, text_start: u16) -> (u16, u16) {
        if self.content.is_empty() {
            return (text_start, inner.y);
        }

        let before = &self.content[..self.cursor_byte];
        let line_idx = before.matches('\n').count() as u16;
        let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col = display_width(&self.content[line_start..self.cursor_byte]);
        let viewport_width = inner.width.saturating_sub(2) as usize;
        let scroll = horizontal_scroll(col, viewport_width);
        let x = text_start + col.saturating_sub(scroll) as u16;
        let y = inner.y + line_idx.min(inner.height.saturating_sub(1));
        (x, y)
    }
}

fn prev_boundary(text: &str, idx: usize) -> usize {
    text[..idx]
        .char_indices()
        .next_back()
        .map(|(pos, _)| pos)
        .unwrap_or(0)
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn horizontal_scroll(cursor_col: usize, viewport_width: usize) -> usize {
    if viewport_width == 0 {
        0
    } else if cursor_col > viewport_width.saturating_sub(4) {
        cursor_col.saturating_sub(viewport_width.saturating_sub(4))
    } else {
        0
    }
}

fn visible_slice(text: &str, scroll_cols: usize, viewport_width: usize) -> String {
    let mut skipped = 0usize;
    let mut used = 0usize;
    let mut out = String::new();

    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(1).max(1);
        if skipped + ch_width <= scroll_cols {
            skipped += ch_width;
            continue;
        }
        if skipped < scroll_cols {
            skipped += ch_width;
            continue;
        }
        if used + ch_width > viewport_width {
            break;
        }

        out.push(ch);
        used += ch_width;
    }

    out
}

fn buffer_row_to_ansi(buf: &Buffer, area: Rect, row: u16) -> String {
    let y = area.top() + row;
    let mut out = String::from(" ");
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;

    for x in area.left()..area.right() {
        let cell = &buf[(x, y)];
        if cell.fg != fg {
            out.push_str(&ansi_color(cell.fg, false));
            fg = cell.fg;
        }
        if cell.bg != bg {
            out.push_str(&ansi_color(cell.bg, true));
            bg = cell.bg;
        }
        out.push_str(cell.symbol());
    }

    if fg != Color::Reset || bg != Color::Reset {
        out.push_str("\x1b[0m");
    }

    out
}

fn ansi_color(color: Color, background: bool) -> String {
    let prefix = if background { 48 } else { 38 };
    match color {
        Color::Reset => {
            if background {
                "\x1b[49m".to_string()
            } else {
                "\x1b[39m".to_string()
            }
        }
        Color::Black => format!("\x1b[{prefix};5;0m"),
        Color::Red => format!("\x1b[{prefix};5;1m"),
        Color::Green => format!("\x1b[{prefix};5;2m"),
        Color::Yellow => format!("\x1b[{prefix};5;3m"),
        Color::Blue => format!("\x1b[{prefix};5;4m"),
        Color::Magenta => format!("\x1b[{prefix};5;5m"),
        Color::Cyan => format!("\x1b[{prefix};5;6m"),
        Color::Gray => format!("\x1b[{prefix};5;7m"),
        Color::DarkGray => format!("\x1b[{prefix};5;8m"),
        Color::LightRed => format!("\x1b[{prefix};5;9m"),
        Color::LightGreen => format!("\x1b[{prefix};5;10m"),
        Color::LightYellow => format!("\x1b[{prefix};5;11m"),
        Color::LightBlue => format!("\x1b[{prefix};5;12m"),
        Color::LightMagenta => format!("\x1b[{prefix};5;13m"),
        Color::LightCyan => format!("\x1b[{prefix};5;14m"),
        Color::White => format!("\x1b[{prefix};5;15m"),
        Color::Rgb(r, g, b) => format!("\x1b[{prefix};2;{r};{g};{b}m"),
        Color::Indexed(i) => format!("\x1b[{prefix};5;{i}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::{InputBox, display_width, visible_slice};

    #[test]
    fn home_and_end_stay_on_current_line() {
        let mut input = InputBox::new();
        input.set_content("abc\ndef");
        input.move_left();
        input.move_home();
        assert_eq!(input.cursor_byte, 4);
        input.move_end();
        assert_eq!(input.cursor_byte, 7);
    }

    #[test]
    fn cursor_position_uses_display_width_for_wide_chars() {
        let mut input = InputBox::new();
        input.set_content("ab界");
        let render = input.render_frame(20, 3, "title", true);
        assert_eq!(render.cursor_x, 9);
        assert_eq!(display_width("ab界"), 4);
    }

    #[test]
    fn visible_slice_scrolls_by_columns_not_chars() {
        assert_eq!(visible_slice("ab界cd", 3, 3), "cd");
    }
}
