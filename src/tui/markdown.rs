/// Streaming markdown-to-ANSI renderer.
/// Handles: **bold**, *italic*, `inline code`, # headers, - lists, fenced code blocks.
/// Uses 256-color ANSI for a muted, clean palette.

// Claude Code-inspired palette
const C_RESET: &str = "\x1b[0m";
const C_BOLD: &str = "\x1b[1m";             // terminal default bold white
const C_ITALIC: &str = "\x1b[3m";
const C_CODE_INLINE: &str = "\x1b[38;5;209m"; // warm orange like Claude Code
const C_CODE_BLOCK: &str = "\x1b[38;5;248m";
const C_CODE_FENCE: &str = "\x1b[38;5;240m";
const C_HEADER1: &str = "\x1b[1;38;5;75m";  // bright blue
const C_HEADER2: &str = "\x1b[1m";
const C_HEADER3: &str = "\x1b[1;38;5;249m";
const C_BULLET: &str = "\x1b[38;5;245m";
const C_THINKING: &str = "\x1b[3;38;5;239m"; // dark italic — clearly subordinate to body text

pub struct MarkdownParser {
    in_code_block: bool,
}

impl MarkdownParser {
    pub fn new() -> Self {
        Self {
            in_code_block: false,
        }
    }

    /// Render a complete line of markdown to an ANSI string.
    pub fn render_line(&mut self, line: &str) -> String {
        // Fenced code block toggle
        if line.trim_start().starts_with("```") {
            self.in_code_block = !self.in_code_block;
            return format!("  {C_CODE_FENCE}{line}{C_RESET}");
        }

        // Inside code block: monospace gray
        if self.in_code_block {
            return format!("  {C_CODE_BLOCK}  {line}{C_RESET}");
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
            return format!("  {C_CODE_FENCE}────────────────────{C_RESET}");
        }

        // Unordered list items
        if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            let rendered = render_inline(rest);
            return format!("  {C_BULLET}•{C_RESET} {rendered}");
        }

        // Ordered list items (1. 2. etc)
        if let Some(dot_pos) = trimmed.find(". ") {
            if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                let num = &trimmed[..dot_pos];
                let rest = &trimmed[dot_pos + 2..];
                let rendered = render_inline(rest);
                return format!("  {C_BULLET}{num}.{C_RESET} {rendered}");
            }
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let rendered = render_inline(rest);
            return format!("  {C_CODE_FENCE}│{C_RESET} {C_HEADER3}{rendered}{C_RESET}");
        }

        // Regular paragraph text
        let rendered = render_inline(trimmed);
        if line.is_empty() {
            String::new()
        } else {
            format!("  {rendered}")
        }
    }

    /// Render thinking text (dim + italic + muted)
    pub fn render_thinking_line(&self, line: &str) -> String {
        if line.is_empty() {
            return String::new();
        }
        format!("  {C_THINKING}{line}{C_RESET}")
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

        // `inline code`
        if chars[i] == '`' {
            if let Some(end) = find_closing_single(&chars, i + 1, '`') {
                result.push_str(C_CODE_INLINE);
                let inner: String = chars[i + 1..end].iter().collect();
                result.push_str(&inner);
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
