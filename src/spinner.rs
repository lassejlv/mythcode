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

/// Render "Thinking…" with a smooth light wave rolling left to right.
pub fn shimmer_thinking(tick: usize) -> String {
    let text = "Thinking…";
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
