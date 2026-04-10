use std::path::Path;

use similar::{ChangeTag, TextDiff};

use super::highlight::{self, Highlighter};
use crate::types::{DiffPreview, PlanEntryStatus, PlanView};

const C_RESET: &str = "\x1b[0m";
const C_ACCENT: &str = "\x1b[38;5;75m";
const C_GREEN: &str = "\x1b[38;5;114m";
const C_RED: &str = "\x1b[38;5;174m";
const C_YELLOW: &str = "\x1b[38;5;179m";
const C_MAGENTA: &str = "\x1b[38;5;176m";
const C_GRAY: &str = "\x1b[38;5;245m";
const C_DARK: &str = "\x1b[38;5;240m";
const C_LINE_NO: &str = "\x1b[38;5;240m";
#[allow(dead_code)]
const C_WHITE: &str = "\x1b[38;5;252m";
const C_DOT: &str = "\x1b[38;5;179m"; // yellow dot like Claude Code

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

    pub fn visible_lines(&self, height: usize) -> &[RenderedLine] {
        if self.lines.is_empty() || height == 0 {
            return &[];
        }
        let total = self.lines.len();
        let end = total.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(height);
        &self.lines[start..end]
    }

    pub fn scroll_up(&mut self, amount: usize) {
        let max = self.lines.len().saturating_sub(3);
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
}

pub fn format_user_message(message: &str) -> Vec<String> {
    vec![
        String::new(),
        format!("  {C_ACCENT}❯{C_RESET} \x1b[1m{message}\x1b[0m"),
    ]
}

pub fn format_turn_separator(elapsed: &str) -> Vec<String> {
    if elapsed.is_empty() {
        vec![String::new()]
    } else {
        vec![
            format!("  {C_DARK}· {elapsed}{C_RESET}"),
            String::new(),
        ]
    }
}

pub fn format_activity(activity: &str) -> String {
    let short = shorten_activity(activity);
    let truncated = if short.chars().count() > 70 {
        let s: String = short.chars().take(67).collect();
        format!("{s}…")
    } else {
        short
    };
    format!("  {C_DOT}●{C_RESET} {C_DARK}{truncated}{C_RESET}")
}

pub fn format_warning(message: &str) -> String {
    format!("  {C_YELLOW}⚠ {message}{C_RESET}")
}

pub fn format_status(message: &str) -> String {
    format!("  {C_GRAY}{message}{C_RESET}")
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

const TOOL_OUTPUT_MAX_LINES: usize = 4;

pub fn format_tool_output(title: &str, content: &str, total_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();

    let short_title = shorten_activity(title);
    let display_title = if short_title.chars().count() > 70 {
        let s: String = short_title.chars().take(67).collect();
        format!("{s}…")
    } else {
        short_title
    };

    let lines_tag = if total_lines > 0 {
        format!("  {C_DARK}{total_lines} lines{C_RESET}")
    } else {
        String::new()
    };

    lines.push(format!(
        "  {C_DOT}●{C_RESET} \x1b[1m{display_title}\x1b[0m{lines_tag}"
    ));

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
                .unwrap_or_else(|| format!("{C_DARK}{line}{C_RESET}"));
            let connector = if is_last(i) { "└" } else { "├" };
            lines.push(format!(
                "    {C_DARK}{connector}{C_RESET}  {colored}"
            ));
        }
        if total_lines > shown {
            let remaining = total_lines - shown;
            lines.push(format!(
                "    {C_DARK}└ … {remaining} more lines{C_RESET}"
            ));
        }
    }

    lines
}

pub fn format_plan(plan: &PlanView) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push(format!("  {C_ACCENT}Plan{C_RESET}"));
    for entry in &plan.entries {
        let (icon, color) = match entry.status {
            PlanEntryStatus::Completed => ("✓", C_GREEN),
            PlanEntryStatus::InProgress => ("●", C_ACCENT),
            PlanEntryStatus::Pending => ("○", C_DARK),
        };
        lines.push(format!("    {color}{icon}{C_RESET} {}{C_RESET}", entry.content));
    }
    lines.push(String::new());
    lines
}

pub fn format_diff(diff: &DiffPreview) -> Vec<String> {
    let mut lines = Vec::new();
    let old_text = diff.old_text.as_deref().unwrap_or("");
    let text_diff = TextDiff::from_lines(old_text, &diff.new_text);
    let groups = text_diff.grouped_ops(3);

    let ext = diff
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
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

    // Header: ● Edit src/main.rs
    lines.push(format!(
        "  {C_DOT}●{C_RESET} \x1b[1m{verb}\x1b[0m({C_DARK}{parent_hint}{C_RESET}{C_MAGENTA}{filename}{C_RESET})"
    ));

    // Stats line
    let stats_text = if deletions == 0 {
        format!("Added {insertions} lines")
    } else if insertions == 0 {
        format!("Removed {deletions} lines")
    } else {
        format!("Added {insertions} lines, removed {deletions} lines")
    };
    lines.push(format!(
        "    {C_DARK}├{C_RESET} {C_DARK}{stats_text}{C_RESET}"
    ));

    if groups.is_empty() {
        lines.push(format!("    {C_DARK}└ (no changes){C_RESET}"));
        return lines;
    }

    const BG_RED: &str = "\x1b[48;2;60;20;25m";
    const BG_GREEN: &str = "\x1b[48;2;20;50;30m";
    const BG_RESET: &str = "\x1b[49m";

    // Collect all diff lines first so we know which is last
    let mut diff_lines: Vec<String> = Vec::new();

    for (group_idx, group) in groups.iter().enumerate() {
        if group_idx > 0 {
            diff_lines.push(format!("    {C_DARK}├{C_RESET}  {C_DARK}⋯{C_RESET}"));
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
                            "    {C_DARK}├{C_RESET}{BG_RED} {C_LINE_NO}{line_no}{C_RESET}{BG_RED} {C_RED}- {change_trimmed}{C_RESET}{BG_RESET}"
                        )
                    }
                    ChangeTag::Insert => {
                        let highlighted = hl
                            .as_mut()
                            .and_then(|h| h.highlight_line(change_trimmed))
                            .unwrap_or_else(|| format!("{C_GREEN}{change_trimmed}{C_RESET}"));
                        format!(
                            "    {C_DARK}├{C_RESET}{BG_GREEN} {C_LINE_NO}{line_no}{C_RESET}{BG_GREEN} {C_GREEN}+ {C_RESET}{BG_GREEN}{highlighted}{BG_RESET}"
                        )
                    }
                    ChangeTag::Equal => {
                        let highlighted = hl
                            .as_mut()
                            .and_then(|h| h.highlight_line(change_trimmed))
                            .unwrap_or_else(|| format!("{C_DARK}{change_trimmed}{C_RESET}"));
                        format!(
                            "    {C_DARK}├{C_RESET} {C_LINE_NO}{line_no}{C_RESET}   {highlighted}"
                        )
                    }
                };
                diff_lines.push(formatted);
            }
        }
    }

    // Replace last ├ with └
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
