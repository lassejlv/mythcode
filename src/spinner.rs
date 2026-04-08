/// Spinner + shimmer for the "Thinking…" state.

// All frames are exactly 11 display columns wide — no text shift.
const FRAMES: &[&str] = &[
    "     ◆     ",
    "    ◆·◆    ",
    "   ◆· ·◆   ",
    "  ◆·   ·◆  ",
    " ◆·     ·◆ ",
    "◆·       ·◆",
    " ◇·     ·◇ ",
    "  ◇·   ·◇  ",
    "   ◇· ·◇   ",
    "    ◇·◇    ",
    "     ◇     ",
    "    ◇·◇    ",
    "   ◇· ·◇   ",
    "  ◇·   ·◇  ",
    " ◇·     ·◇ ",
    "◇·       ·◇",
    " ◆·     ·◆ ",
    "  ◆·   ·◆  ",
    "   ◆· ·◆   ",
    "    ◆·◆    ",
];

pub const INTERVAL_MS: u64 = 60;

const SPINNER_DIVISOR: usize = 3;

pub fn frame(tick: usize) -> &'static str {
    FRAMES[(tick / SPINNER_DIVISOR) % FRAMES.len()]
}

// ── Status messages ─────────────────────────────────────────────

const THINKING_MESSAGES: &[&str] = &[
    "Thinking…",
    "Cooking…",
    "Brewing ideas…",
    "Pondering…",
    "Conjuring…",
    "Dreaming up…",
    "Crafting…",
    "Weaving…",
    "Scheming…",
    "Imagining…",
];

const TOOL_MESSAGES: &[&str] = &[
    "Working…",
    "Running…",
    "Processing…",
    "Executing…",
    "Crunching…",
    "Building…",
    "Hammering…",
    "Wiring…",
];

/// How often status messages rotate (in seconds).
const MESSAGE_ROTATE_SECS: u64 = 10;

/// Pick a status message based on elapsed seconds.
pub fn thinking_message(elapsed_secs: u64) -> &'static str {
    let idx = (elapsed_secs / MESSAGE_ROTATE_SECS) as usize % THINKING_MESSAGES.len();
    THINKING_MESSAGES[idx]
}

pub fn tool_message(elapsed_secs: u64) -> &'static str {
    let idx = (elapsed_secs / MESSAGE_ROTATE_SECS) as usize % TOOL_MESSAGES.len();
    TOOL_MESSAGES[idx]
}

/// Pick a message based on the active tool name. Falls back to generic pool.
pub fn tool_message_for(tool_hint: &str, elapsed_secs: u64) -> &'static str {
    let lower = tool_hint.to_lowercase();
    // Match on common tool name prefixes/keywords
    if lower.contains("read") || lower.contains("cat") {
        "Reading…"
    } else if lower.contains("edit") || lower.contains("write") || lower.contains("patch") {
        "Editing…"
    } else if lower.contains("bash") || lower.contains("shell") || lower.contains("exec") {
        "Running…"
    } else if lower.contains("grep") || lower.contains("search") || lower.contains("glob") || lower.contains("find") {
        "Searching…"
    } else if lower.contains("list") || lower.contains("ls") {
        "Listing…"
    } else if lower.contains("test") {
        "Testing…"
    } else if lower.contains("build") || lower.contains("compile") {
        "Building…"
    } else if lower.contains("install") {
        "Installing…"
    } else if lower.contains("fetch") || lower.contains("download") || lower.contains("curl") {
        "Fetching…"
    } else if lower.contains("git") {
        "Committing…"
    } else if lower.contains("lint") || lower.contains("format") || lower.contains("fmt") {
        "Formatting…"
    } else {
        tool_message(elapsed_secs)
    }
}

/// Format elapsed time as "Xs" or "Xm Ys"
pub fn format_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        format!("{elapsed_secs}s")
    } else {
        let m = elapsed_secs / 60;
        let s = elapsed_secs % 60;
        format!("{m}m {s}s")
    }
}

// ── Shimmer ─────────────────────────────────────────────────────

/// Render text with a smooth light wave rolling left to right.
pub fn shimmer(tick: usize, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    let cycle = len + 4;
    let peak = tick % cycle;

    let mut result = String::with_capacity(len * 20);
    for (i, ch) in chars.iter().enumerate() {
        let dist = if peak >= i { peak - i } else { i - peak };
        let color = match dist {
            0 => 159,
            1 => 117,
            2 => 75,
            3 => 68,
            4 => 60,
            _ => 240,
        };
        result.push_str("\x1b[38;5;");
        result.push_str(&color.to_string());
        result.push('m');
        result.push(*ch);
    }
    result.push_str("\x1b[0m");
    result
}
