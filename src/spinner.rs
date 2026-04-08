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

/// Pick a status message based on elapsed seconds — changes every ~4s.
pub fn thinking_message(elapsed_secs: u64) -> &'static str {
    let idx = (elapsed_secs / 4) as usize % THINKING_MESSAGES.len();
    THINKING_MESSAGES[idx]
}

pub fn tool_message(elapsed_secs: u64) -> &'static str {
    let idx = (elapsed_secs / 4) as usize % TOOL_MESSAGES.len();
    TOOL_MESSAGES[idx]
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
