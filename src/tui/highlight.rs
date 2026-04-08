/// Syntax highlighting for file content using syntect.

use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Highlight all lines, returning a Vec of ANSI strings.
pub fn highlight_content(content: &str, filename: &str) -> Vec<String> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mut hl = match Highlighter::new(ext) {
        Some(h) => h,
        None => return content.lines().map(|l| l.to_string()).collect(),
    };
    content
        .lines()
        .map(|line| hl.highlight_line(line).unwrap_or_else(|| line.to_string()))
        .collect()
}

/// Extract a filename/extension from a tool title like "Read src/main.rs"
/// or a path like "/foo/bar.ts".
pub fn extract_filename(title: &str) -> &str {
    // Try the last whitespace-separated token (covers "Read file.rs", "Write file.rs", etc.)
    title.split_whitespace().last().unwrap_or(title)
}

pub struct Highlighter {
    hl: HighlightLines<'static>,
    ss: &'static SyntaxSet,
}

struct SyntectStatics {
    ss: SyntaxSet,
    ts: ThemeSet,
}

fn statics() -> &'static SyntectStatics {
    use std::sync::OnceLock;
    static STATICS: OnceLock<SyntectStatics> = OnceLock::new();
    STATICS.get_or_init(|| SyntectStatics {
        ss: SyntaxSet::load_defaults_newlines(),
        ts: ThemeSet::load_defaults(),
    })
}

impl Highlighter {
    /// Create a highlighter for the given file extension.
    /// Returns None if no syntax definition is found.
    pub fn new(extension: &str) -> Option<Self> {
        let st = statics();
        let syntax = st.ss.find_syntax_by_extension(extension)?;
        // Use a dark theme that works well on dark terminals
        let theme = st.ts.themes.get("base16-ocean.dark")?;
        let hl = HighlightLines::new(syntax, theme);
        // Safety: statics() returns &'static, so we can transmute the lifetime.
        // The SyntaxSet and Theme outlive everything.
        let ss: &'static SyntaxSet = &st.ss;
        Some(Self { hl, ss })
    }

    /// Highlight a single line, returning an ANSI string or None on failure.
    pub fn highlight_line(&mut self, line: &str) -> Option<String> {
        let input = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{line}\n")
        };
        let ranges = self.hl.highlight_line(&input, self.ss).ok()?;
        Some(styled_to_ansi(&ranges))
    }
}

/// Convert syntect styled spans to ANSI escape sequences.
fn styled_to_ansi(ranges: &[(Style, &str)]) -> String {
    let mut out = String::new();
    for &(style, text) in ranges {
        let r = style.foreground.r;
        let g = style.foreground.g;
        let b = style.foreground.b;
        // Use 24-bit true color
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
        // Trim trailing newline added for syntect
        out.push_str(text.trim_end_matches('\n'));
    }
    out.push_str("\x1b[0m");
    out
}
