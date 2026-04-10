/// Markdown-to-ANSI helpers used by the TUI.
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use unicode_width::UnicodeWidthChar;

use super::highlight::Highlighter;

const C_RESET: &str = "\x1b[0m";
const C_BOLD: &str = "\x1b[1m";
const C_ITALIC: &str = "\x1b[3m";
const C_CODE_INLINE: &str = "\x1b[38;2;166;227;161m";
const C_CODE_INLINE_BG: &str = "\x1b[48;2;30;40;35m";
const C_CODE_BLOCK: &str = "\x1b[38;5;248m";
const C_CODE_FENCE: &str = "\x1b[38;5;240m";
const C_HEADER1: &str = "\x1b[1;38;2;137;180;250m";
const C_HEADER2: &str = "\x1b[1;38;2;205;214;244m";
const C_HEADER3: &str = "\x1b[1;38;5;249m";
const C_BULLET: &str = "\x1b[38;2;137;180;250m";
const C_LINK: &str = "\x1b[38;5;117m";
const C_THINKING: &str = "\x1b[38;2;88;91;112m";
const C_THINKING_BAR: &str = "\x1b[38;2;69;71;90m";

pub fn render_markdown(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_highlighter: Option<Highlighter> = None;

    for raw_line in text.split('\n') {
        let trimmed = raw_line.trim_start();
        let indent = raw_line.len().saturating_sub(trimmed.len());

        if trimmed.starts_with("```") {
            if in_code_block {
                in_code_block = false;
                code_highlighter = None;
                lines.push(format!("  {C_CODE_FENCE}  ╰───{C_RESET}"));
            } else {
                in_code_block = true;
                let lang = trimmed.trim_start_matches('`').trim();
                code_highlighter = if lang.is_empty() {
                    None
                } else {
                    Highlighter::new(lang)
                };
                let lang_label = if lang.is_empty() {
                    String::new()
                } else {
                    format!(" {C_CODE_FENCE}{lang}{C_RESET}")
                };
                lines.push(format!("  {C_CODE_FENCE}  ╭───{lang_label}{C_RESET}"));
            }
            continue;
        }

        if in_code_block {
            let colored = code_highlighter
                .as_mut()
                .and_then(|highlighter| highlighter.highlight_line(raw_line))
                .unwrap_or_else(|| format!("{C_CODE_BLOCK}{raw_line}{C_RESET}"));
            lines.push(format!("  {C_CODE_FENCE}  │{C_RESET} {colored}"));
            continue;
        }

        if trimmed.is_empty() {
            lines.push(String::new());
            continue;
        }

        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            lines.push(format!(
                "  {C_CODE_FENCE}─────────────────────────{C_RESET}"
            ));
            continue;
        }

        let rendered = if let Some(rest) = trimmed.strip_prefix("# ") {
            format!("  {C_HEADER1}{}{C_RESET}", render_inline(rest))
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            format!("  {C_HEADER2}{}{C_RESET}", render_inline(rest))
        } else if let Some(rest) = trimmed.strip_prefix("### ") {
            format!("  {C_HEADER3}{}{C_RESET}", render_inline(rest))
        } else if let Some(rest) = blockquote_body(trimmed) {
            format!(
                "{} {}{C_ITALIC}{}{C_RESET}",
                quote_prefix(blockquote_depth(trimmed)),
                C_RESET,
                render_inline(rest)
            )
        } else if let Some((prefix, rest)) = list_prefix(indent, trimmed) {
            format!("{prefix} {}", render_inline(rest))
        } else {
            format!("{}{}", " ".repeat(2 + indent), render_inline(trimmed))
        };

        lines.extend(wrap_ansi(&rendered, width));
    }

    if lines.last().is_some_and(String::is_empty) {
        while lines.last().is_some_and(String::is_empty) && lines.len() > 1 {
            lines.pop();
        }
    }

    lines
}

pub fn render_thinking(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        let rendered = if raw_line.is_empty() {
            format!("  {C_THINKING_BAR}│{C_RESET}")
        } else {
            format!("  {C_THINKING_BAR}│{C_RESET} {C_THINKING}{raw_line}{C_RESET}")
        };
        lines.extend(wrap_ansi(&rendered, width));
    }
    lines
}

fn blockquote_depth(trimmed: &str) -> usize {
    trimmed.chars().take_while(|ch| *ch == '>').count()
}

fn blockquote_body(trimmed: &str) -> Option<&str> {
    let depth = blockquote_depth(trimmed);
    if depth == 0 {
        return None;
    }
    trimmed[depth..]
        .trim_start()
        .strip_prefix("")
        .map(|_| trimmed[depth..].trim_start())
}

fn quote_prefix(depth: usize) -> String {
    let mut prefix = String::from("  ");
    for _ in 0..depth.max(1) {
        prefix.push_str(C_THINKING_BAR);
        prefix.push('│');
        prefix.push_str(C_RESET);
        prefix.push(' ');
    }
    prefix
}

fn list_prefix(indent: usize, trimmed: &str) -> Option<(String, &str)> {
    let list_indent = " ".repeat(3 + indent);

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return Some((format!("{list_indent}{C_BULLET}•{C_RESET}"), rest));
    }

    if let Some(dot_pos) = trimmed.find(". ")
        && dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit())
    {
        let prefix = format!("{list_indent}{C_BULLET}{}{C_RESET}", &trimmed[..=dot_pos]);
        return Some((prefix, &trimmed[dot_pos + 2..]));
    }

    None
}

fn render_inline(text: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(text, options);
    let mut out = String::new();
    let mut stack: Vec<&'static str> = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Emphasis) => {
                stack.push(C_ITALIC);
                out.push_str(C_ITALIC);
            }
            Event::Start(Tag::Strong) => {
                stack.push(C_BOLD);
                out.push_str(C_BOLD);
            }
            Event::Start(Tag::Link { .. }) => {
                stack.push(C_LINK);
                out.push_str(C_LINK);
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link) => {
                let _ = stack.pop();
                out.push_str(C_RESET);
                for style in &stack {
                    out.push_str(style);
                }
            }
            Event::Code(code) => {
                out.push_str(C_CODE_INLINE_BG);
                out.push_str(C_CODE_INLINE);
                out.push(' ');
                out.push_str(&code);
                out.push(' ');
                out.push_str(C_RESET);
                for style in &stack {
                    out.push_str(style);
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => out.push_str(&text),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            Event::TaskListMarker(checked) => out.push_str(if checked { "[x] " } else { "[ ] " }),
            Event::Rule => out.push_str("─────────────────────────"),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented))
            | Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
            | Event::End(TagEnd::CodeBlock)
            | Event::FootnoteReference(_)
            | Event::Start(_)
            | Event::End(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
        }
    }

    if !stack.is_empty() {
        out.push_str(C_RESET);
    }

    out
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

#[cfg(test)]
mod tests {
    use super::{render_markdown, render_thinking};

    #[test]
    fn renders_markdown_lists_and_fences() {
        let text = "# Header\n- one\n- two\n```rust\nlet x = 1;\n```";
        let rendered = render_markdown(text, 80).join("\n");
        assert!(rendered.contains("Header"));
        assert!(rendered.contains("•"));
        assert!(rendered.contains("╭───"));
        assert!(rendered.contains("╰───"));
    }

    #[test]
    fn wraps_thinking_output() {
        let rendered = render_thinking("this is a fairly long thinking line", 12);
        assert!(rendered.len() > 1);
        assert!(rendered[0].contains("│"));
    }
}
