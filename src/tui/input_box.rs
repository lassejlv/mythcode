use std::io::{self, Write};

use crossterm::{cursor, execute, style};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders, Padding, Widget};

pub struct InputBox {
    content: String,
    cursor_byte: usize,
    cursor_col: usize,
    input_history: Vec<String>,
    history_index: Option<usize>,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor_byte: 0,
            cursor_col: 0,
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
        self.cursor_col = 0;
        self.history_index = None;
        if !text.trim().is_empty() {
            self.input_history.push(text.clone());
        }
        text
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor_byte = 0;
        self.cursor_col = 0;
        self.history_index = None;
    }

    pub fn set_content(&mut self, s: &str) {
        self.content = s.to_string();
        self.cursor_byte = self.content.len();
        self.cursor_col = self.content.chars().count();
    }

    pub fn insert_char(&mut self, ch: char) {
        self.content.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
        self.cursor_col += 1;
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        let prev = self.content[..self.cursor_byte]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.content.remove(prev);
        self.cursor_byte = prev;
        self.cursor_col = self.cursor_col.saturating_sub(1);
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
        let prev = self.content[..self.cursor_byte]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.cursor_byte = prev;
        self.cursor_col = self.cursor_col.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.cursor_byte >= self.content.len() {
            return;
        }
        if let Some(ch) = self.content[self.cursor_byte..].chars().next() {
            self.cursor_byte += ch.len_utf8();
            self.cursor_col += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_byte = 0;
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_byte = self.content.len();
        self.cursor_col = self.content.chars().count();
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
        let removed_chars = self.content[new_end..self.cursor_byte].chars().count();
        self.content.drain(new_end..self.cursor_byte);
        self.cursor_byte = new_end;
        self.cursor_col = self.cursor_col.saturating_sub(removed_chars);
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

    pub fn render(
        &self,
        row: u16,
        width: u16,
        height: u16,
        title: &str,
        is_active: bool,
    ) -> io::Result<()> {
        let area = Rect::new(1, row, width.saturating_sub(2), height);
        let mut buf = Buffer::empty(area);

        let border_color = if is_active {
            Color::Indexed(240) // subtle gray
        } else {
            Color::Indexed(236) // darker gray
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(border_color))
            .title(format!(" {} ", title))
            .title_style(Style::default().fg(Color::Indexed(245)))
            .padding(Padding::horizontal(1));

        let inner = block.inner(area);
        block.render(area, &mut buf);

        // Render the ">" prompt indicator
        let prompt_col = inner.x;
        if inner.width > 2 {
            let cell = &mut buf[(prompt_col, inner.y)];
            cell.set_char('>');
            cell.set_fg(Color::Indexed(73));
        }

        // Render input text after the prompt
        let text_start = prompt_col + 2; // "> " takes 2 cols
        let text_width = inner.width.saturating_sub(2) as usize;

        if !self.content.is_empty() && text_width > 0 {
            let text_chars: Vec<char> = self.content.chars().collect();
            let start = if self.cursor_col > text_width.saturating_sub(4) {
                self.cursor_col.saturating_sub(text_width.saturating_sub(4))
            } else {
                0
            };
            let end = (start + text_width).min(text_chars.len());

            for (col_offset, idx) in (start..end).enumerate() {
                let x = text_start + col_offset as u16;
                if x < area.right() {
                    let cell = &mut buf[(x, inner.y)];
                    cell.set_char(text_chars[idx]);
                    cell.set_fg(Color::Indexed(250));
                }
            }
        }

        // Write buffer to terminal
        let mut stdout = io::stdout();
        for y in area.top()..area.bottom() {
            execute!(stdout, cursor::MoveTo(area.left(), y))?;
            for x in area.left()..area.right() {
                let cell = &buf[(x, y)];
                let ct_fg = ratatui_to_crossterm_color(cell.fg);
                let ct_bg = ratatui_to_crossterm_color(cell.bg);
                execute!(
                    stdout,
                    style::SetForegroundColor(ct_fg),
                    style::SetBackgroundColor(ct_bg),
                    style::Print(cell.symbol()),
                )?;
            }
            execute!(stdout, style::ResetColor)?;
        }

        // Show placeholder if empty
        if self.content.is_empty() {
            execute!(stdout, cursor::MoveTo(text_start, inner.y))?;
            write!(stdout, "\x1b[38;5;242mType a message...\x1b[0m")?;
        }

        // Position cursor after "> "
        let cursor_display_col = if self.content.is_empty() {
            0
        } else {
            let start = if self.cursor_col > text_width.saturating_sub(4) {
                self.cursor_col.saturating_sub(text_width.saturating_sub(4))
            } else {
                0
            };
            self.cursor_col - start
        };

        execute!(
            stdout,
            cursor::MoveTo(text_start + cursor_display_col as u16, inner.y),
            cursor::Show,
            cursor::SetCursorStyle::SteadyBlock,
        )?;

        stdout.flush()
    }

    /// Reposition the cursor into the input box without re-rendering.
    /// Call after any drawing below the input box that moves the cursor.
    pub fn reposition_cursor(&self, row: u16, width: u16, height: u16) -> io::Result<()> {
        let area = Rect::new(1, row, width.saturating_sub(2), height);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding::horizontal(1));
        let inner = block.inner(area);
        let text_start = inner.x + 2; // "> " takes 2 cols
        let text_width = inner.width.saturating_sub(2) as usize;

        let cursor_display_col = if self.content.is_empty() {
            0
        } else {
            let start = if self.cursor_col > text_width.saturating_sub(4) {
                self.cursor_col.saturating_sub(text_width.saturating_sub(4))
            } else {
                0
            };
            self.cursor_col - start
        };

        let mut stdout = io::stdout();
        execute!(
            stdout,
            cursor::MoveTo(text_start + cursor_display_col as u16, inner.y),
            cursor::Show,
            cursor::SetCursorStyle::SteadyBlock,
        )?;
        stdout.flush()
    }
}

fn ratatui_to_crossterm_color(color: ratatui::style::Color) -> crossterm::style::Color {
    match color {
        Color::Reset => crossterm::style::Color::Reset,
        Color::Black => crossterm::style::Color::Black,
        Color::Red => crossterm::style::Color::DarkRed,
        Color::Green => crossterm::style::Color::DarkGreen,
        Color::Yellow => crossterm::style::Color::DarkYellow,
        Color::Blue => crossterm::style::Color::DarkBlue,
        Color::Magenta => crossterm::style::Color::DarkMagenta,
        Color::Cyan => crossterm::style::Color::DarkCyan,
        Color::Gray => crossterm::style::Color::Grey,
        Color::DarkGray => crossterm::style::Color::DarkGrey,
        Color::LightRed => crossterm::style::Color::Red,
        Color::LightGreen => crossterm::style::Color::Green,
        Color::LightYellow => crossterm::style::Color::Yellow,
        Color::LightBlue => crossterm::style::Color::Blue,
        Color::LightMagenta => crossterm::style::Color::Magenta,
        Color::LightCyan => crossterm::style::Color::Cyan,
        Color::White => crossterm::style::Color::White,
        Color::Rgb(r, g, b) => crossterm::style::Color::Rgb { r, g, b },
        Color::Indexed(i) => crossterm::style::Color::AnsiValue(i),
    }
}
