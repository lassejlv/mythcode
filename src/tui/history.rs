use std::path::Path;

use similar::{ChangeTag, TextDiff};

use super::highlight::{self, Highlighter};
use super::markdown::wrap_ansi;
use crate::types::{DiffPreview, PlanEntryStatus, PlanView};

use super::theme;

const R: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    Assistant,
    Thinking,
    Activity,
    Diff,
    Warning,
    Status,
    Separator,
    Welcome,
    UserMessage,
}

#[derive(Debug, Clone)]
pub struct RenderedLine {
    pub content: String,
    #[allow(dead_code)]
    pub line_type: LineType,
}

pub struct History {
    lines: Vec<RenderedLine>,
    pub scroll_offset: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll_offset: 0,
        }
    }

    pub fn push(&mut self, content: String, line_type: LineType) {
        self.lines.push(RenderedLine { content, line_type });
        self.scroll_offset = 0;
    }

    pub fn push_lines(&mut self, lines: Vec<String>, line_type: LineType) {
        for line in lines {
            self.push(line, line_type);
        }
    }

    pub fn visible_lines(&self, height: usize, width: usize) -> Vec<RenderedLine> {
        if self.lines.is_empty() || height == 0 {
            return Vec::new();
        }

        let wrapped = self.wrapped_lines(width);
        let total = wrapped.len();
        let end = total.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(height);
        wrapped[start..end].to_vec()
    }

    pub fn scroll_up(&mut self, amount: usize, width: usize) {
        let max = self.wrapped_lines(width).len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn pop_n(&mut self, n: usize) {
        let len = self.lines.len();
        self.lines.truncate(len.saturating_sub(n));
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
    }

    fn wrapped_lines(&self, width: usize) -> Vec<RenderedLine> {
        let mut wrapped = Vec::new();
        for line in &self.lines {
            let segments = if line.content.is_empty() {
                vec![String::new()]
            } else {
                wrap_ansi(&line.content, width)
            };

            for content in segments {
                wrapped.push(RenderedLine {
                    content,
                    line_type: line.line_type,
                });
            }
        }
        wrapped
    }
}

#[cfg(test)]
mod tests {
    use super::{History, LineType};

    #[test]
    fn scroll_uses_wrapped_height() {
        let mut history = History::new();
        history.push("0123456789".into(), LineType::Assistant);

        history.scroll_up(1, 4);
        assert_eq!(history.scroll_offset, 1);
    }
}

pub fn format_user_message(message: &str, image_numbers: &[u32]) -> Vec<String> {
    let t = theme::theme();

    let image_suffix = if image_numbers.is_empty() {
        String::new()
    } else {
        let tags: Vec<String> = image_numbers
            .iter()
            .map(|n| format!("[Image #{n}]"))
            .collect();
        format!("  {}{}{R}", t.dark, tags.join(" "))
    };

    let line_count = message.lines().count();
    if line_count > 20 {
        let first_line = message.lines().next().unwrap_or("");
        let preview = if first_line.chars().count() > 60 {
            let truncated: String = first_line.chars().take(57).collect();
            format!("{truncated}…")
        } else {
            first_line.to_string()
        };
        vec![format!(
            "  {}❯{R} \x1b[1m{preview}\x1b[0m  {}[{line_count} lines]{R}{image_suffix}",
            t.accent, t.dark
        )]
    } else if message.is_empty() && !image_numbers.is_empty() {
        vec![format!("  {}❯{R}{image_suffix}", t.accent)]
    } else {
        let mut lines = Vec::new();
        for (i, line) in message.lines().enumerate() {
            if i == 0 {
                lines.push(format!(
                    "  {}❯{R} \x1b[1m{line}\x1b[0m{image_suffix}",
                    t.accent
                ));
            } else {
                lines.push(format!("    \x1b[1m{line}\x1b[0m"));
            }
        }
        lines
    }
}

pub fn format_turn_separator(elapsed: &str) -> Vec<String> {
    if elapsed.is_empty() {
        vec![]
    } else {
        let t = theme::theme();
        vec![format!("  {}· {elapsed}{R}", t.dark)]
    }
}

pub fn format_activity(activity: &str) -> String {
    let t = theme::theme();
    let short = shorten_activity(activity);
    let truncated = if short.chars().count() > 70 {
        let s: String = short.chars().take(67).collect();
        format!("{s}…")
    } else {
        short
    };
    format!("  {}●{R} {}{truncated}{R}", t.dot, t.dark)
}

pub fn format_warning(message: &str) -> String {
    let t = theme::theme();
    format!("  {}⚠ {message}{R}", t.yellow)
}

pub fn format_status(message: &str) -> String {
    let t = theme::theme();
    format!("  {}{message}{R}", t.gray)
}

fn shorten_activity(activity: &str) -> String {
    activity
        .split_whitespace()
        .map(|word| {
            if word.contains('/') && word.len() > 1 {
                shorten_path(word)
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn shorten_path(path: &str) -> String {
    let p = Path::new(path);
    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
        name.to_string()
    } else {
        path.to_string()
    }
}

const TOOL_OUTPUT_MAX_LINES: usize = 3;

pub fn format_tool_output(title: &str, content: &str, total_lines: usize) -> Vec<String> {
    let t = theme::theme();
    let mut lines = Vec::new();

    let short_title = shorten_activity(title);
    let display_title = if short_title.chars().count() > 70 {
        let s: String = short_title.chars().take(67).collect();
        format!("{s}…")
    } else {
        short_title
    };

    let lines_tag = if total_lines > 0 {
        format!("  {}{total_lines} lines{R}", t.dark)
    } else {
        String::new()
    };

    lines.push(format!(
        "  {}●{R} \x1b[1m{display_title}\x1b[0m{lines_tag}",
        t.dot
    ));

    // Drop the theme lock before syntax highlighting (it may take time)
    let dark = t.dark.clone();
    drop(t);

    let preview_lines: Vec<&str> = content.lines().take(TOOL_OUTPUT_MAX_LINES).collect();
    let shown = preview_lines.len();

    if shown > 0 {
        let filename = highlight::extract_filename(title);
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let mut hl = Highlighter::new(ext);

        let is_last = |i: usize| -> bool { i == shown - 1 && total_lines <= shown };

        for (i, line) in preview_lines.iter().enumerate() {
            let colored = hl
                .as_mut()
                .and_then(|h| h.highlight_line(line))
                .unwrap_or_else(|| format!("{dark}{line}{R}"));
            let connector = if is_last(i) { "└" } else { "├" };
            lines.push(format!("   {dark}{connector}{R} {colored}"));
        }
        if total_lines > shown {
            let remaining = total_lines - shown;
            lines.push(format!("   {dark}└ … {remaining} more lines{R}"));
        }
    }

    lines
}

pub fn format_plan(plan: &PlanView) -> Vec<String> {
    let t = theme::theme();
    let mut lines = Vec::new();
    lines.push(format!("  {}Plan{R}", t.accent));
    for entry in &plan.entries {
        let (icon, color) = match entry.status {
            PlanEntryStatus::Completed => ("✓", t.green.as_str()),
            PlanEntryStatus::InProgress => ("●", t.accent.as_str()),
            PlanEntryStatus::Pending => ("○", t.dark.as_str()),
        };
        lines.push(format!("   {color}{icon}{R} {}{R}", entry.content));
    }
    lines
}

pub fn format_diff(diff: &DiffPreview) -> Vec<String> {
    let t = theme::theme();
    let dot = t.dot.clone();
    let dark = t.dark.clone();
    let magenta = t.magenta.clone();
    let line_no_c = t.line_no.clone();
    let red = t.red.clone();
    let green = t.green.clone();
    let bg_add = t.diff_add_bg.clone();
    let bg_del = t.diff_del_bg.clone();
    drop(t);

    const BG_RESET: &str = "\x1b[49m";

    let mut lines = Vec::new();
    let old_text = diff.old_text.as_deref().unwrap_or("");
    let text_diff = TextDiff::from_lines(old_text, &diff.new_text);
    let groups = text_diff.grouped_ops(3);

    let ext = diff.path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut hl = Highlighter::new(ext);

    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for group in &groups {
        for op in group {
            for change in text_diff.iter_changes(op) {
                match change.tag() {
                    ChangeTag::Insert => insertions += 1,
                    ChangeTag::Delete => deletions += 1,
                    _ => {}
                }
            }
        }
    }

    let is_new_file = old_text.is_empty() && insertions > 0;
    let verb = if is_new_file { "Write" } else { "Edit" };

    let filename = diff
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| diff.path.to_str().unwrap_or(""));

    let parent_hint = diff
        .path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|s| format!("{s}/"))
        .unwrap_or_default();

    lines.push(format!(
        "  {dot}●{R} \x1b[1m{verb}\x1b[0m({dark}{parent_hint}{R}{magenta}{filename}{R})"
    ));

    let stats_text = if deletions == 0 {
        format!("Added {insertions} lines")
    } else if insertions == 0 {
        format!("Removed {deletions} lines")
    } else {
        format!("Added {insertions} lines, removed {deletions} lines")
    };
    lines.push(format!("   {dark}├{R} {dark}{stats_text}{R}"));

    if groups.is_empty() {
        lines.push(format!("   {dark}└ (no changes){R}"));
        return lines;
    }

    let mut diff_lines: Vec<String> = Vec::new();

    for (group_idx, group) in groups.iter().enumerate() {
        if group_idx > 0 {
            diff_lines.push(format!("   {dark}├{R} {dark}⋯{R}"));
        }

        for op in group {
            for change in text_diff.iter_changes(op) {
                let line_no = match change.tag() {
                    ChangeTag::Delete => format_line_no(change.old_index()),
                    ChangeTag::Insert | ChangeTag::Equal => format_line_no(change.new_index()),
                };

                let change_str = change.to_string_lossy();
                let change_trimmed = change_str.trim_end_matches('\n');

                let formatted = match change.tag() {
                    ChangeTag::Delete => {
                        format!(
                            "   {dark}├{R}{bg_del} {line_no_c}{line_no}{R}{bg_del} {red}- {change_trimmed}{R}{BG_RESET}"
                        )
                    }
                    ChangeTag::Insert => {
                        let highlighted = hl
                            .as_mut()
                            .and_then(|h| h.highlight_line(change_trimmed))
                            .unwrap_or_else(|| format!("{green}{change_trimmed}{R}"));
                        format!(
                            "   {dark}├{R}{bg_add} {line_no_c}{line_no}{R}{bg_add} {green}+ {R}{bg_add}{highlighted}{BG_RESET}"
                        )
                    }
                    ChangeTag::Equal => {
                        let highlighted = hl
                            .as_mut()
                            .and_then(|h| h.highlight_line(change_trimmed))
                            .unwrap_or_else(|| format!("{dark}{change_trimmed}{R}"));
                        format!("   {dark}├{R} {line_no_c}{line_no}{R}   {highlighted}")
                    }
                };
                diff_lines.push(formatted);
            }
        }
    }

    if let Some(last) = diff_lines.last_mut() {
        *last = last.replacen('├', "└", 1);
    }

    lines.extend(diff_lines);
    lines
}

fn format_line_no(index: Option<usize>) -> String {
    match index {
        Some(i) => format!("{:>4}", i + 1),
        None => "    ".to_string(),
    }
}
