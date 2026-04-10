use std::io::{self, Write};

use crossterm::cursor::{self, SetCursorStyle};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::{execute, terminal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
}

#[derive(Debug, Clone, Copy)]
pub struct CursorState {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
}

pub struct FrameBuffer {
    lines: Vec<String>,
    cursor: Option<CursorState>,
}

impl FrameBuffer {
    pub fn new(height: u16) -> Self {
        Self {
            lines: vec![String::new(); height as usize],
            cursor: None,
        }
    }

    pub fn set_line(&mut self, row: u16, content: impl Into<String>) {
        if let Some(line) = self.lines.get_mut(row as usize) {
            *line = content.into();
        }
    }

    pub fn set_cursor(&mut self, x: u16, y: u16, shape: CursorShape) {
        self.cursor = Some(CursorState { x, y, shape });
    }

    pub fn render(&self, stdout: &mut impl Write) -> io::Result<()> {
        for (row, line) in self.lines.iter().enumerate() {
            execute!(stdout, cursor::MoveTo(0, row as u16))?;
            write!(stdout, "\x1b[2K")?;
            if !line.is_empty() {
                write!(stdout, "{line}")?;
            }
        }

        match self.cursor {
            Some(cursor_state) => {
                let style = match cursor_state.shape {
                    CursorShape::Block => SetCursorStyle::SteadyBlock,
                };
                execute!(
                    stdout,
                    cursor::MoveTo(cursor_state.x, cursor_state.y),
                    style,
                    cursor::Show,
                )?;
            }
            None => {
                execute!(stdout, cursor::Hide, SetCursorStyle::DefaultUserShape,)?;
            }
        }

        stdout.flush()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TerminalGuardOptions {
    pub alternate_screen: bool,
    pub mouse_capture: bool,
    pub enhanced_keys: bool,
}

pub struct TerminalGuard {
    alternate_screen: bool,
    mouse_capture: bool,
    enhanced_keys: bool,
}

impl TerminalGuard {
    pub fn enter(options: TerminalGuardOptions) -> io::Result<Self> {
        terminal::enable_raw_mode()?;

        let mut stdout = io::stdout();
        if options.alternate_screen {
            execute!(stdout, terminal::EnterAlternateScreen)?;
        }
        if options.mouse_capture {
            execute!(stdout, EnableMouseCapture)?;
        }

        let enhanced_keys = if options.enhanced_keys
            && terminal::supports_keyboard_enhancement().unwrap_or(false)
        {
            execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
            )?;
            true
        } else {
            false
        };

        Ok(Self {
            alternate_screen: options.alternate_screen,
            mouse_capture: options.mouse_capture,
            enhanced_keys,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let mut stdout = io::stdout();

        if self.enhanced_keys {
            let _ = execute!(stdout, PopKeyboardEnhancementFlags);
        }
        if self.mouse_capture {
            let _ = execute!(stdout, DisableMouseCapture);
        }
        if self.alternate_screen {
            let _ = execute!(stdout, terminal::LeaveAlternateScreen);
        }

        let _ = execute!(stdout, cursor::Show, SetCursorStyle::DefaultUserShape,);
    }
}

#[cfg(test)]
mod tests {
    use super::{CursorShape, FrameBuffer};

    #[test]
    fn frame_buffer_overwrites_lines_and_cursor() {
        let mut frame = FrameBuffer::new(4);
        frame.set_line(1, "hello");
        frame.set_line(2, "world");
        frame.set_cursor(3, 2, CursorShape::Block);

        assert_eq!(frame.lines[1], "hello");
        assert_eq!(frame.lines[2], "world");
        let cursor = frame.cursor.expect("cursor");
        assert_eq!((cursor.x, cursor.y), (3, 2));
    }
}
