use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

#[allow(dead_code)]
pub struct Theme {
    pub accent: String,
    pub green: String,
    pub red: String,
    pub yellow: String,
    pub magenta: String,
    pub gray: String,
    pub dark: String,
    pub line_no: String,
    pub dot: String,
    pub white: String,
    pub dim: String,
    pub bold_cyan: String,
    pub spinner: String,
    pub cyan: String,
    // Markdown
    pub code_fg: String,
    pub code_bg: String,
    pub code_block: String,
    pub code_fence: String,
    pub header1: String,
    pub header2: String,
    pub header3: String,
    pub bullet: String,
    pub thinking: String,
    pub thinking_bar: String,
    // Diff backgrounds
    pub diff_add_bg: String,
    pub diff_del_bg: String,
    // Permission
    pub perm_green_bg: String,
    pub perm_red_bg: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: "\x1b[38;5;75m".into(),
            green: "\x1b[38;5;114m".into(),
            red: "\x1b[38;5;174m".into(),
            yellow: "\x1b[38;5;179m".into(),
            magenta: "\x1b[38;5;176m".into(),
            gray: "\x1b[38;5;245m".into(),
            dark: "\x1b[38;5;240m".into(),
            line_no: "\x1b[38;5;240m".into(),
            dot: "\x1b[38;5;179m".into(),
            white: "\x1b[38;5;252m".into(),
            dim: "\x1b[38;5;245m".into(),
            bold_cyan: "\x1b[1;38;5;75m".into(),
            spinner: "\x1b[38;5;75m".into(),
            cyan: "\x1b[38;5;75m".into(),
            code_fg: "\x1b[38;2;166;227;161m".into(),
            code_bg: "\x1b[48;2;30;40;35m".into(),
            code_block: "\x1b[38;5;248m".into(),
            code_fence: "\x1b[38;5;240m".into(),
            header1: "\x1b[1;38;2;137;180;250m".into(),
            header2: "\x1b[1;38;2;205;214;244m".into(),
            header3: "\x1b[1;38;5;249m".into(),
            bullet: "\x1b[38;2;137;180;250m".into(),
            thinking: "\x1b[38;2;88;91;112m".into(),
            thinking_bar: "\x1b[38;2;69;71;90m".into(),
            diff_add_bg: "\x1b[48;2;20;50;30m".into(),
            diff_del_bg: "\x1b[48;2;60;20;25m".into(),
            perm_green_bg: "\x1b[48;2;20;50;30m".into(),
            perm_red_bg: "\x1b[48;2;60;20;25m".into(),
        }
    }
}

fn hex_to_ansi_fg(hex: &str) -> Option<String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(format!("\x1b[38;2;{r};{g};{b}m"))
}

fn hex_to_ansi_bg(hex: &str) -> Option<String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(format!("\x1b[48;2;{r};{g};{b}m"))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThemeOverride {
    pub accent: Option<String>,
    pub green: Option<String>,
    pub red: Option<String>,
    pub yellow: Option<String>,
    pub magenta: Option<String>,
    pub gray: Option<String>,
    pub dark: Option<String>,
    pub dot: Option<String>,
    pub code_fg: Option<String>,
    pub code_bg: Option<String>,
    pub header1: Option<String>,
    pub bullet: Option<String>,
    pub thinking: Option<String>,
    pub diff_add_bg: Option<String>,
    pub diff_del_bg: Option<String>,
}

static THEME: OnceLock<Mutex<Theme>> = OnceLock::new();

pub fn theme() -> std::sync::MutexGuard<'static, Theme> {
    THEME
        .get_or_init(|| Mutex::new(Theme::default()))
        .lock()
        .unwrap()
}

pub fn apply_override(overrides: &ThemeOverride) {
    let mut t = theme();
    macro_rules! apply_fg {
        ($field:ident) => {
            if let Some(hex) = &overrides.$field {
                if let Some(ansi) = hex_to_ansi_fg(hex) {
                    t.$field = ansi;
                }
            }
        };
    }
    macro_rules! apply_bg {
        ($field:ident) => {
            if let Some(hex) = &overrides.$field {
                if let Some(ansi) = hex_to_ansi_bg(hex) {
                    t.$field = ansi;
                }
            }
        };
    }

    apply_fg!(accent);
    apply_fg!(green);
    apply_fg!(red);
    apply_fg!(yellow);
    apply_fg!(magenta);
    apply_fg!(gray);
    apply_fg!(dark);
    apply_fg!(dot);
    apply_fg!(code_fg);
    apply_fg!(header1);
    apply_fg!(bullet);
    apply_fg!(thinking);
    apply_bg!(code_bg);
    apply_bg!(diff_add_bg);
    apply_bg!(diff_del_bg);

    // Sync derived colors
    t.line_no = t.dark.clone();
    t.dim = t.gray.clone();
    t.bold_cyan = format!("\x1b[1m{}", t.accent);
    t.spinner = t.accent.clone();
    t.cyan = t.accent.clone();
    t.white = "\x1b[38;5;252m".into();
    t.thinking_bar = t.thinking.clone();
    t.perm_green_bg = t.diff_add_bg.clone();
    t.perm_red_bg = t.diff_del_bg.clone();
}
