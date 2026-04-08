use similar::{ChangeTag, TextDiff};

use crate::types::DiffPreview;

// Claude Code-inspired palette
const C_RESET: &str = "\x1b[0m";
const C_ACCENT: &str = "\x1b[38;5;75m";    // blue accent
const C_DIM_CYAN: &str = "\x1b[38;5;67m";
const C_GREEN: &str = "\x1b[38;5;114m";
const C_RED: &str = "\x1b[38;5;174m";
const C_YELLOW: &str = "\x1b[38;5;179m";
const C_MAGENTA: &str = "\x1b[38;5;176m";
const C_GRAY: &str = "\x1b[38;5;245m";
const C_DARK: &str = "\x1b[38;5;240m";
const C_LINE_NO: &str = "\x1b[38;5;240m";

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
        let max = self.lines.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
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
        format!("  {C_ACCENT}❯{C_RESET} \x1b[1m{message}\x1b[0m"),
        String::new(),
    ]
}

/// Activity line: tool calls, mode changes, etc.
pub fn format_activity(activity: &str) -> String {
    format!("    {C_DARK}▸ {activity}{C_RESET}")
}

/// Warning line
pub fn format_warning(message: &str) -> String {
    format!("  {C_YELLOW}⚠ {message}{C_RESET}")
}

/// Status/info line
pub fn format_status(message: &str) -> String {
    format!("  {C_GRAY}{message}{C_RESET}")
}

/// Max lines to show for tool output preview
const TOOL_OUTPUT_MAX_LINES: usize = 4;

/// Format tool output with truncation
pub fn format_tool_output(_title: &str, content: &str, total_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let preview_lines: Vec<&str> = content.lines().take(TOOL_OUTPUT_MAX_LINES).collect();
    let shown = preview_lines.len();

    for line in &preview_lines {
        lines.push(format!("    {C_DARK}│{C_RESET} {C_DARK}{line}{C_RESET}"));
    }

    if total_lines > shown {
        let remaining = total_lines - shown;
        lines.push(format!(
            "    {C_DARK}│ … {remaining} more lines (ctrl+o to expand){C_RESET}"
        ));
    }

    lines
}

/// Format a diff with box-drawing
pub fn format_diff(diff: &DiffPreview) -> Vec<String> {
    let mut lines = Vec::new();
    let path_display = diff.path.display();
    let old_text = diff.old_text.as_deref().unwrap_or("");
    let text_diff = TextDiff::from_lines(old_text, &diff.new_text);
    let groups = text_diff.grouped_ops(3);

    lines.push(format!(
        "  {C_DARK}╭─{C_RESET} {C_MAGENTA}{path_display}{C_RESET}"
    ));

    if groups.is_empty() {
        lines.push(format!("  {C_DARK}│{C_RESET} {C_GRAY}(no changes){C_RESET}"));
    }

    for (group_idx, group) in groups.iter().enumerate() {
        if let (Some(first), Some(last)) = (group.first(), group.last()) {
            let old_start = first.old_range().start + 1;
            let old_len = last.old_range().end - first.old_range().start;
            let new_start = first.new_range().start + 1;
            let new_len = last.new_range().end - first.new_range().start;

            if group_idx > 0 {
                lines.push(format!("  {C_DARK}│{C_RESET}"));
            }
            lines.push(format!(
                "  {C_DARK}│{C_RESET} {C_DIM_CYAN}@@ -{old_start},{old_len} +{new_start},{new_len} @@{C_RESET}"
            ));
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
                            "  {C_DARK}│{C_RESET} {C_LINE_NO}{line_no}{C_RESET} {C_RED}-{change_trimmed}{C_RESET}"
                        )
                    }
                    ChangeTag::Insert => {
                        format!(
                            "  {C_DARK}│{C_RESET} {C_LINE_NO}{line_no}{C_RESET} {C_GREEN}+{change_trimmed}{C_RESET}"
                        )
                    }
                    ChangeTag::Equal => {
                        format!(
                            "  {C_DARK}│ {C_LINE_NO}{line_no}{C_RESET}  {C_GRAY}{change_trimmed}{C_RESET}"
                        )
                    }
                };
                lines.push(formatted);
            }
        }
    }

    lines.push(format!("  {C_DARK}╰─{C_RESET}"));
    lines
}

fn format_line_no(index: Option<usize>) -> String {
    match index {
        Some(i) => format!("{:>4}", i + 1),
        None => "    ".to_string(),
    }
}
