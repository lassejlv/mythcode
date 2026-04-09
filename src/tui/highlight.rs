/// Syntax highlighting for file content using syntect.

use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, Style, Theme, ThemeItem, ThemeSettings,
};
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
    theme: Theme,
}

fn statics() -> &'static SyntectStatics {
    use std::sync::OnceLock;
    static STATICS: OnceLock<SyntectStatics> = OnceLock::new();
    STATICS.get_or_init(|| SyntectStatics {
        ss: SyntaxSet::load_defaults_newlines(),
        theme: catppuccin_mocha_theme(),
    })
}

impl Highlighter {
    /// Create a highlighter for the given file extension.
    /// Returns None if no syntax definition is found.
    pub fn new(extension: &str) -> Option<Self> {
        let st = statics();
        let syntax = st.ss.find_syntax_by_extension(extension)?;
        let hl = HighlightLines::new(syntax, &st.theme);
        // Safety: statics() returns &'static, so the SyntaxSet outlives everything.
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
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
        out.push_str(text.trim_end_matches('\n'));
    }
    out.push_str("\x1b[0m");
    out
}

// ── Catppuccin Mocha theme ─────────────────────────────────────

fn c(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xFF) as u8,
        g: ((hex >> 8) & 0xFF) as u8,
        b: (hex & 0xFF) as u8,
        a: 0xFF,
    }
}

fn scope(s: &str) -> ScopeSelectors {
    s.parse().unwrap()
}

fn rule(scopes: &str, color: Color, font_style: FontStyle) -> ThemeItem {
    ThemeItem {
        scope: scope(scopes),
        style: syntect::highlighting::StyleModifier {
            foreground: Some(color),
            background: None,
            font_style: Some(font_style),
        },
    }
}

fn catppuccin_mocha_theme() -> Theme {
    // Catppuccin Mocha palette
    let rosewater = c(0xf5e0dc);
    let flamingo  = c(0xf2cdcd);
    let pink      = c(0xf5c2e7);
    let mauve     = c(0xcba6f7);
    let red       = c(0xf38ba8);
    let maroon    = c(0xeba0ac);
    let peach     = c(0xfab387);
    let yellow    = c(0xf9e2af);
    let green     = c(0xa6e3a1);
    let sky       = c(0x89dceb);
    let sapphire  = c(0x74c7ec);
    let blue      = c(0x89b4fa);
    let lavender  = c(0xb4befe);
    let text      = c(0xcdd6f4);
    let overlay2  = c(0x9399b2);
    let overlay0  = c(0x6c7086);
    let surface0  = c(0x313244);
    let base      = c(0x1e1e2e);

    let n = FontStyle::empty();
    let i = FontStyle::ITALIC;
    let b = FontStyle::BOLD;

    Theme {
        name: Some("Catppuccin Mocha".to_string()),
        author: Some("Catppuccin".to_string()),
        settings: ThemeSettings {
            foreground: Some(text),
            background: Some(base),
            caret: Some(rosewater),
            line_highlight: Some(surface0),
            selection: Some(surface0),
            selection_foreground: Some(text),
            gutter: Some(overlay0),
            gutter_foreground: Some(overlay0),
            ..Default::default()
        },
        scopes: vec![
            // Comments
            rule("comment", overlay0, i),
            rule("punctuation.definition.comment", overlay0, i),

            // Strings
            rule("string", green, n),
            rule("string.regexp", peach, n),
            rule("constant.other.symbol", flamingo, n),

            // Numbers & constants
            rule("constant.numeric", peach, n),
            rule("constant.language", mauve, n),
            rule("constant.character.escape", pink, n),
            rule("constant.other.color", sapphire, n),

            // Keywords
            rule("keyword", mauve, n),
            rule("keyword.control", mauve, n),
            rule("keyword.operator", sky, n),
            rule("keyword.other.special-method", blue, n),

            // Storage / types
            rule("storage", mauve, n),
            rule("storage.type", yellow, i),
            rule("storage.modifier", mauve, n),

            // Entity (functions, classes, tags)
            rule("entity.name.function", blue, n),
            rule("entity.name.class", yellow, n),
            rule("entity.name.type", yellow, n),
            rule("entity.name.tag", mauve, n),
            rule("entity.name.section", blue, b),
            rule("entity.other.attribute-name", yellow, i),
            rule("entity.other.inherited-class", green, i),

            // Variable
            rule("variable", text, n),
            rule("variable.parameter", maroon, i),
            rule("variable.language", red, i),
            rule("variable.other", text, n),

            // Support (built-in types, functions)
            rule("support.function", blue, n),
            rule("support.class", yellow, n),
            rule("support.type", blue, n),
            rule("support.constant", peach, n),

            // Punctuation
            rule("punctuation", overlay2, n),
            rule("punctuation.definition.tag", mauve, n),
            rule("punctuation.definition.string", green, n),
            rule("punctuation.separator", overlay2, n),
            rule("punctuation.section", overlay2, n),

            // Operators
            rule("keyword.operator", sky, n),

            // Markup (markdown)
            rule("markup.heading", blue, b),
            rule("markup.bold", peach, b),
            rule("markup.italic", maroon, i),
            rule("markup.underline.link", rosewater, n),
            rule("markup.raw", green, n),
            rule("markup.list", mauve, n),
            rule("markup.inserted", green, n),
            rule("markup.deleted", red, n),
            rule("markup.changed", peach, n),

            // Meta
            rule("meta.function-call", blue, n),
            rule("meta.class", yellow, n),
            rule("meta.separator", overlay2, n),

            // Invalid
            rule("invalid", red, n),
            rule("invalid.deprecated", lavender, i),

            // Diff
            rule("markup.inserted.diff", green, n),
            rule("markup.deleted.diff", red, n),
            rule("meta.diff.header", blue, b),

            // JSON / YAML keys
            rule("support.type.property-name", blue, n),
            rule("source.json string.quoted.double", green, n),

            // Type annotations
            rule("support.type", yellow, n),
            rule("entity.name.type.class", yellow, n),

            // Decorators / annotations
            rule("meta.decorator", mauve, n),
            rule("punctuation.decorator", mauve, n),

            // Default text
            rule("text", text, n),
            rule("source", text, n),
        ],
    }
}
