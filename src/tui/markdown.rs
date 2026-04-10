/// Streaming markdown-to-ANSI renderer.
/// Handles: **bold**, *italic*, `inline code`, # headers, - lists, fenced code blocks.
/// Uses Catppuccin Mocha palette for a clean, modern look.

use unicode_width::UnicodeWidthChar;

use super::highlight::Highlighter;

// Catppuccin Mocha palette
const C_RESET: &str = "\x1b[0m";
const C_BOLD: &str = "\x1b[1m";
const C_ITALIC: &str = "\x1b[3m";
const C_CODE_INLINE: &str = "\x1b[38;2;166;227;161m";        // green — stands out inline
const C_CODE_INLINE_BG: &str = "\x1b[48;2;30;40;35m";        // subtle green tint bg
const C_CODE_BLOCK: &str = "\x1b[38;5;248m";
const C_CODE_FENCE: &str = "\x1b[38;5;240m";
const C_HEADER1: &str = "\x1b[1;38;2;137;180;250m";          // bold blue
const C_HEADER2: &str = "\x1b[1;38;2;205;214;244m";          // bold text
const C_HEADER3: &str = "\x1b[1;38;5;249m";
const C_BULLET: &str = "\x1b[38;2;137;180;250m";             // blue bullets
const C_THINKING: &str = "\x1b[38;2;88;91;112m";             // overlay0 — subtle
const C_THINKING_BAR: &str = "\x1b[38;2;69;71;90m";          // surface1

pub struct MarkdownParser {
    in_code_block: bool,
    code_lang: String,
    code_highlighter: Option<Highlighter>,
}

impl MarkdownParser {
    pub fn new() -> Self {
        Self {
            in_code_block: false,
            code_lang: String::new(),
            code_highlighter: None,
        }
    }

    /// Render a complete line of markdown to an ANSI string.
    pub fn render_line(&mut self, line: &str) -> String {
        // Fenced code block toggle
        if line.trim_start().starts_with("```") {
            if self.in_code_block {
                // Closing fence
                self.in_code_block = false;
                self.code_lang.clear();
                self.code_highlighter = None;
                return format!("  {C_CODE_FENCE}  ╰───{C_RESET}");
            } else {
                // Opening fence — extract language
                self.in_code_block = true;
                let lang = line.trim_start().trim_start_matches('`').trim();
                self.code_lang = lang.to_string();
                self.code_highlighter = if !lang.is_empty() {
                    Highlighter::new(lang)
                } else {
                    None
                };
                let lang_label = if lang.is_empty() {
                    String::new()
                } else {
                    format!(" {C_CODE_FENCE}{lang}{C_RESET}")
                };
                return format!("  {C_CODE_FENCE}  ╭───{lang_label}{C_RESET}");
            }
        }

        // Inside code block: syntax highlighted
        if self.in_code_block {
            let colored = self.code_highlighter
                .as_mut()
                .and_then(|h| h.highlight_line(line))
                .unwrap_or_else(|| format!("{C_CODE_BLOCK}{line}{C_RESET}"));
            return format!("  {C_CODE_FENCE}  │{C_RESET} {colored}");
        }

        let trimmed = line.trim_start();

        // Headers
        if let Some(rest) = trimmed.strip_prefix("### ") {
            return format!("  {C_HEADER3}{rest}{C_RESET}");
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            return format!("  {C_HEADER2}{rest}{C_RESET}");
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            return format!("  {C_HEADER1}{rest}{C_RESET}");
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            return format!("  {C_CODE_FENCE}─────────────────────────{C_RESET}");
        }

        // Unordered list items
        if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            let rendered = render_inline(rest);
            return format!("    {C_BULLET}•{C_RESET} {rendered}");
        }

        // Nested list items (  - or   *)
        if let Some(rest) = trimmed.strip_prefix("  - ").or_else(|| trimmed.strip_prefix("  * ")) {
            let rendered = render_inline(rest);
            return format!("      {C_BULLET}•{C_RESET} {rendered}");
        }

        // Ordered list items (1. 2. etc)
        if let Some(dot_pos) = trimmed.find(". ") {
            if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                let num = &trimmed[..dot_pos];
                let rest = &trimmed[dot_pos + 2..];
                let rendered = render_inline(rest);
                return format!("    {C_BULLET}{num}.{C_RESET} {rendered}");
            }
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let rendered = render_inline(rest);
            return format!("  {C_THINKING_BAR}│{C_RESET} {C_ITALIC}{rendered}{C_RESET}");
        }

        // Regular paragraph text
        let rendered = render_inline(trimmed);
        if line.is_empty() {
            String::new()
        } else {
            format!("  {rendered}")
        }
    }

    /// Render thinking text — dim with a left bar
    pub fn render_thinking_line(&self, line: &str) -> String {
        if line.is_empty() {
            return format!("  {C_THINKING_BAR}│{C_RESET}");
        }
        format!("  {C_THINKING_BAR}│{C_RESET} {C_THINKING}{line}{C_RESET}")
    }
}

/// Render inline markdown: **bold**, *italic*, `code`
fn render_inline(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 64);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // **bold**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_closing(&chars, i + 2, &['*', '*']) {
                result.push_str(C_BOLD);
                let inner: String = chars[i + 2..end].iter().collect();
                result.push_str(&inner);
                result.push_str(C_RESET);
                i = end + 2;
                continue;
            }
        }

        // *italic*
        if chars[i] == '*' && (i + 1 < len && chars[i + 1] != ' ') {
            if let Some(end) = find_closing_single(&chars, i + 1, '*') {
                if end > i + 1 {
                    result.push_str(C_ITALIC);
                    let inner: String = chars[i + 1..end].iter().collect();
                    result.push_str(&inner);
                    result.push_str(C_RESET);
                    i = end + 1;
                    continue;
                }
            }
        }

        // `inline code` — with background tint
        if chars[i] == '`' {
            if let Some(end) = find_closing_single(&chars, i + 1, '`') {
                result.push_str(C_CODE_INLINE_BG);
                result.push_str(C_CODE_INLINE);
                result.push(' ');
                let inner: String = chars[i + 1..end].iter().collect();
                result.push_str(&inner);
                result.push(' ');
                result.push_str(C_RESET);
                i = end + 1;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

fn find_closing(chars: &[char], start: usize, pattern: &[char; 2]) -> Option<usize> {
    let len = chars.len();
    for i in start..len.saturating_sub(1) {
        if chars[i] == pattern[0] && chars[i + 1] == pattern[1] {
            return Some(i);
        }
    }
    None
}

fn find_closing_single(chars: &[char], start: usize, ch: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == ch {
            return Some(i);
        }
    }
    None
}

/// Wrap a rendered ANSI line to fit within `width` columns.
/// Returns one or more lines; continuation lines get the same indent.
pub fn wrap_ansi(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }

    // Measure the visible indent (leading spaces) to replicate on wrapped lines.
    let indent = visible_indent(line);
    let indent_str: String = " ".repeat(indent);

    // Split into segments: either ANSI escapes (zero-width) or visible chars.
    let segments = parse_segments(line);

    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut col: usize = 0;
    // Track active ANSI state so we can re-apply on continuation lines.
    let mut active_ansi: Vec<String> = Vec::new();

    for seg in &segments {
        match seg {
            Segment::Ansi(code) => {
                cur.push_str(code);
                // Track active styling so we can replay on wrapped lines
                if code.contains("[0m") || code.contains("[0;") {
                    active_ansi.clear();
                } else {
                    active_ansi.push(code.clone());
                }
            }
            Segment::Text(text) => {
                for ch in text.chars() {
                    let ch_w = ch.width().unwrap_or(0);
                    if ch == ' ' && col + ch_w > width {
                        // Break at this space
                        cur.push_str(C_RESET);
                        lines.push(cur);
                        cur = format!("{indent_str}{}", active_ansi.join(""));
                        col = indent;
                        continue;
                    }
                    if col + ch_w > width && col > indent {
                        // Hard break mid-word
                        cur.push_str(C_RESET);
                        lines.push(cur);
                        cur = format!("{indent_str}{}", active_ansi.join(""));
                        col = indent;
                    }
                    cur.push(ch);
                    col += ch_w;
                }
            }
        }
    }

    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Count visible leading spaces in an ANSI string.
fn visible_indent(s: &str) -> usize {
    let mut count = 0;
    let mut in_esc = false;
    for ch in s.chars() {
        if in_esc {
            if ch.is_ascii_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_esc = true;
            continue;
        }
        if ch == ' ' {
            count += 1;
        } else {
            break;
        }
    }
    count
}

enum Segment {
    Ansi(String),
    Text(String),
}

/// Parse a string into ANSI escape sequences and visible text segments.
fn parse_segments(s: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut chars = s.chars().peekable();
    let mut text_buf = String::new();

    while let Some(&ch) = chars.peek() {
        if ch == '\x1b' {
            // Flush text
            if !text_buf.is_empty() {
                segments.push(Segment::Text(std::mem::take(&mut text_buf)));
            }
            let mut esc = String::new();
            esc.push(chars.next().unwrap()); // \x1b
            while let Some(&next) = chars.peek() {
                esc.push(chars.next().unwrap());
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            segments.push(Segment::Ansi(esc));
        } else {
            text_buf.push(chars.next().unwrap());
        }
    }
    if !text_buf.is_empty() {
        segments.push(Segment::Text(text_buf));
    }
    segments
}
