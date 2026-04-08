/// Spinner + shimmer for the "Thinking…" state.
///
/// The spinner renders as a small animated glyph sequence.
/// The shimmer renders "Thinking…" with a light wave rolling across.
/// Both are driven by a shared tick counter.

// ── Spinner ─────────────────────────────────────────────────────

const FRAMES: &[&str] = &[
    "    ✦    ",
    "   ✦·✦   ",
    "  ✦·· ··✦  ",
    " ✦··  ··✦ ",
    "✦···   ···✦",
    " ✦··  ··✦ ",
    "  ✦·· ··✦  ",
    "   ✦·✦   ",
    "    ✦    ",
    "   ·✧·   ",
    "  ·✧ ✧·  ",
    " · ✧  ✧ · ",
    "·  ✧   ✧  ·",
    " · ✧  ✧ · ",
    "  ·✧ ✧·  ",
    "   ·✧·   ",
];

/// Interval between ticks in milliseconds.
/// The shimmer needs faster ticks than the spinner,
/// so we tick fast and only advance the spinner every N ticks.
pub const INTERVAL_MS: u64 = 60;

const SPINNER_DIVISOR: usize = 3; // spinner advances every 3 ticks (~180ms)

pub fn frame(tick: usize) -> &'static str {
    let idx = (tick / SPINNER_DIVISOR) % FRAMES.len();
    FRAMES[idx]
}

// ── Shimmer ─────────────────────────────────────────────────────

/// Render "Thinking…" with a smooth light wave rolling left to right.
pub fn shimmer_thinking(tick: usize) -> String {
    let text = "Thinking…";
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    // The wave peak position (fractional, in half-char steps for smoothness)
    // Cycle: sweep across text, then a short dark gap
    let cycle = len + 4;
    let peak = tick % cycle;

    let mut result = String::with_capacity(len * 20);
    for (i, ch) in chars.iter().enumerate() {
        let dist = if peak >= i { peak - i } else { i - peak };
        let color = match dist {
            0 => 195, // white-cyan glow
            1 => 152, // bright teal-white
            2 => 116, // teal
            3 => 73,  // muted teal
            4 => 66,  // dark teal
            _ => 240, // base dim
        };
        result.push_str("\x1b[38;5;");
        result.push_str(&color.to_string());
        result.push('m');
        result.push(*ch);
    }
    result.push_str("\x1b[0m");
    result
}
