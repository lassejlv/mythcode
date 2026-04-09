use std::path::Path;

use similar::{ChangeTag, TextDiff};

use super::highlight::{self, Highlighter};
use crate::types::{DiffPreview, PlanEntryStatus, PlanView};

// Claude Code-inspired palette
const C_RESET: &str = "\x1b[0m";
const C_ACCENT: &str = "\x1b[38;5;75m";    // blue accent
#[allow(dead_code)]
const C_DIM_CYAN: &str = "\x1b[38;5;67m";
const C_GREEN: &str = "\x1b[38;5;114m";
const C_RED: &str = "\x1b[38;5;174m";
const C_YELLOW: &str = "\x1b[38;5;179m";
const C_MAGENTA: &str = "\x1b[38;5;176m";
const C_GRAY: &str = "\x1b[38;5;245m";
const C_DARK: &str = "\x1b[38;5;240m";
const C_LINE_NO: &str = "\x1b[38;5;240m";
const C_WHITE: &str = "\x1b[38;5;252m";

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
        // Auto-scroll to bottom when new content arrives
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
        // Don't scroll past the beginning — keep at least a few lines visible
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

/// Format user message echo (shown before assistant response)
pub fn format_user_message(message: &str) -> Vec<String> {
    vec![
        String::new(),
        format!("  {C_ACCENT}>{C_RESET} \x1b[1m{message}\x1b[0m"),
        String::new(),
    ]
}

/// Activity line: tool calls, mode changes, etc.
pub fn format_activity(activity: &str) -> String {
    // Shorten file paths in activity text to just filename
    let short = shorten_activity(activity);
    format!("  {C_DARK}  {short}{C_RESET}")
}

/// Warning line
pub fn format_warning(message: &str) -> String {
    format!("  {C_YELLOW}  {message}{C_RESET}")
}

/// Status/info line
pub fn format_status(message: &str) -> String {
    format!("  {C_GRAY}{message}{C_RESET}")
}

/// Shorten paths in activity strings — "Read /Users/foo/project/src/main.rs" → "Read main.rs"
fn shorten_activity(activity: &str) -> String {
    // Split into words, shorten anything that looks like a file path
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

/// Shorten a path to just filename, or dir/file for disambiguation
fn shorten_path(path: &str) -> String {
    let p = Path::new(path);
    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
        name.to_string()
    } else {
        path.to_string()
    }
}

/// Max lines to show for tool output preview
const TOOL_OUTPUT_MAX_LINES: usize = 4;

/// Format tool output with truncation and syntax highlighting
pub fn format_tool_output(title: &str, content: &str, total_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();

    // Determine tool kind from title for icon
    let (icon, label) = tool_icon_and_label(title);

    // Title header with icon and shortened path
    if !title.is_empty() {
        let short_title = shorten_activity(title);
        lines.push(format!(
            "  {C_ACCENT}{icon}{C_RESET} {C_WHITE}{label}{C_RESET} {C_GRAY}{short_title}{C_RESET}"
        ));
    }

    let preview_lines: Vec<&str> = content.lines().take(TOOL_OUTPUT_MAX_LINES).collect();
    let shown = preview_lines.len();

    if shown > 0 {
        let filename = highlight::extract_filename(title);
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let mut hl = Highlighter::new(ext);

        lines.push(format!("    {C_DARK}╭───{C_RESET}"));
        for line in &preview_lines {
            let colored = hl
                .as_mut()
                .and_then(|h| h.highlight_line(line))
                .unwrap_or_else(|| format!("{C_DARK}{line}{C_RESET}"));
            lines.push(format!("    {C_DARK}│{C_RESET}  {colored}"));
        }
        if total_lines > shown {
            let remaining = total_lines - shown;
            lines.push(format!(
                "    {C_DARK}╰─── {remaining} more lines{C_RESET}"
            ));
        } else {
            lines.push(format!("    {C_DARK}╰───{C_RESET}"));
        }
    }

    lines
}

/// Format a plan/todo list
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

/// Format a diff — clean minimal style with syntax highlighting
pub fn format_diff(diff: &DiffPreview) -> Vec<String> {
    let mut lines = Vec::new();
    let old_text = diff.old_text.as_deref().unwrap_or("");
    let text_diff = TextDiff::from_lines(old_text, &diff.new_text);
    let groups = text_diff.grouped_ops(3);

    // Set up syntax highlighter based on file extension
    let ext = diff
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mut hl = Highlighter::new(ext);

    // Count insertions and deletions
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

    // Determine if this is a new file (Write) or edit
    let is_new_file = old_text.is_empty() && insertions > 0;
    let icon = if is_new_file { "+" } else { "~" };
    let icon_color = if is_new_file { C_GREEN } else { C_ACCENT };

    // Short filename
    let filename = diff
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| diff.path.to_str().unwrap_or(""));

    // Parent directory for context (e.g. "src/")
    let parent_hint = diff
        .path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|s| format!("{s}/"))
        .unwrap_or_default();

    // Stats
    let stats = if deletions == 0 {
        format!("{C_GREEN}+{insertions}{C_RESET}")
    } else if insertions == 0 {
        format!("{C_RED}-{deletions}{C_RESET}")
    } else {
        format!("{C_GREEN}+{insertions}{C_RESET} {C_RED}-{deletions}{C_RESET}")
    };

    // File header
    lines.push(String::new());
    lines.push(format!(
        "  {icon_color}{icon}{C_RESET} {C_DARK}{parent_hint}{C_RESET}{C_MAGENTA}{filename}{C_RESET}  {stats}"
    ));

    if groups.is_empty() {
        lines.push(format!("    {C_DARK}(no changes){C_RESET}"));
        return lines;
    }

    lines.push(format!("    {C_DARK}╭───{C_RESET}"));

    for (group_idx, group) in groups.iter().enumerate() {
        if group_idx > 0 {
            lines.push(format!("    {C_DARK}│{C_RESET}  {C_DARK}⋯{C_RESET}"));
        }

        for op in group {
            for change in text_diff.iter_changes(op) {
                let line_no = match change.tag() {
                    ChangeTag::Delete => format_line_no(change.old_index()),
                    ChangeTag::Insert | ChangeTag::Equal => format_line_no(change.new_index()),
                };

                let change_str = change.to_string_lossy();
                let change_trimmed = change_str.trim_end_matches('\n');

                // Background colors for diff lines
                const BG_RED: &str = "\x1b[48;2;60;20;25m";
                const BG_GREEN: &str = "\x1b[48;2;20;50;30m";
                const BG_RESET: &str = "\x1b[49m";

                let formatted = match change.tag() {
                    ChangeTag::Delete => {
                        format!(
                            "    {C_DARK}│{C_RESET}{BG_RED} {C_LINE_NO}{line_no}{C_RESET}{BG_RED} {C_RED}- {change_trimmed}{C_RESET}{BG_RESET}"
                        )
                    }
                    ChangeTag::Insert => {
                        let highlighted = hl
                            .as_mut()
                            .and_then(|h| h.highlight_line(change_trimmed))
                            .unwrap_or_else(|| format!("{C_GREEN}{change_trimmed}{C_RESET}"));
                        format!(
                            "    {C_DARK}│{C_RESET}{BG_GREEN} {C_LINE_NO}{line_no}{C_RESET}{BG_GREEN} {C_GREEN}+ {C_RESET}{BG_GREEN}{highlighted}{BG_RESET}"
                        )
                    }
                    ChangeTag::Equal => {
                        let highlighted = hl
                            .as_mut()
                            .and_then(|h| h.highlight_line(change_trimmed))
                            .unwrap_or_else(|| format!("{C_DARK}{change_trimmed}{C_RESET}"));
                        format!(
                            "    {C_DARK}│{C_RESET} {C_LINE_NO}{line_no}{C_RESET}   {highlighted}"
                        )
                    }
                };
                lines.push(formatted);
            }
        }
    }

    lines.push(format!("    {C_DARK}╰───{C_RESET}"));
    lines
}

/// Map tool titles to icons and short labels
fn tool_icon_and_label(title: &str) -> (&'static str, &'static str) {
    let lower = title.to_lowercase();
    if lower.starts_with("read") {
        ("◇", "Read")
    } else if lower.starts_with("edit") {
        ("◆", "Edit")
    } else if lower.starts_with("write") {
        ("+", "Write")
    } else if lower.starts_with("bash") || lower.starts_with("run") {
        ("$", "Bash")
    } else if lower.starts_with("grep") || lower.starts_with("search") {
        ("⌕", "Search")
    } else if lower.starts_with("glob") || lower.starts_with("find") {
        ("⌕", "Find")
    } else if lower.starts_with("agent") || lower.starts_with("launch") {
        ("→", "Agent")
    } else if lower.starts_with("web") {
        ("↗", "Web")
    } else {
        ("▸", "Tool")
    }
}

fn format_line_no(index: Option<usize>) -> String {
    match index {
        Some(i) => format!("{:>4}", i + 1),
        None => "    ".to_string(),
    }
}
