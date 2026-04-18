//! Unicode indicators (no emoji) with semantic colors.
//!
//! Design rule: indicator SHAPE encodes kind, COLOR encodes strength/status.
//!
//! Some helpers marked `#[allow(dead_code)]` are part of the public indicator
//! catalog — used by graph rendering, TUI extensions, etc. They may be unused
//! in the current build but kept for consistency.

#![allow(dead_code)]

use ratatui::style::{Color, Style};

/// Filled circle — active / strong / delivered.
pub const DOT_FILLED: &str = "●";
/// Empty circle — inactive / weak / wiki-link.
pub const DOT_EMPTY: &str = "○";
/// Filled square — important / delivered / selected.
pub const SQ_FILLED: &str = "■";
/// Empty square — pending / normal.
pub const SQ_EMPTY: &str = "□";
/// Checkmark — done / learned.
pub const CHECK: &str = "✓";
/// Cross — skipped / cancelled / error.
pub const CROSS: &str = "✗";
/// Arrow right — flow / link.
pub const ARROW_RIGHT: &str = "→";
/// Triangle right — play / deliver / active.
pub const PLAY: &str = "▶";

/// Semantic colors.
pub fn strong() -> Style {
    Style::default().fg(Color::Green)
}
pub fn medium() -> Style {
    Style::default().fg(Color::Yellow)
}
pub fn weak() -> Style {
    Style::default().fg(Color::Gray)
}
pub fn link() -> Style {
    Style::default().fg(Color::Cyan)
}
pub fn overdue() -> Style {
    Style::default().fg(Color::Red)
}
pub fn info() -> Style {
    Style::default().fg(Color::Blue)
}
pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Pick color based on score (for graph semantic edges).
pub fn score_color(score: f64) -> Style {
    if score >= 0.8 {
        strong()
    } else if score >= 0.6 {
        medium()
    } else {
        weak()
    }
}

/// Pick indicator + style for a lesson status.
pub fn lesson_status(status: &str) -> (&'static str, Style) {
    match status {
        "learned" => (CHECK, strong()),
        "delivered" => (PLAY, info()),
        "skipped" => (CROSS, dim()),
        _ => (SQ_EMPTY, medium()), // pending
    }
}

/// Pick indicator + style for a todo (done?).
pub fn todo_status(done: bool) -> (&'static str, Style) {
    if done {
        (CHECK, strong())
    } else {
        (SQ_EMPTY, medium())
    }
}

/// Pick indicator + style for a reminder (fired?).
pub fn reminder_status(fired: bool) -> (&'static str, Style) {
    if fired {
        (DOT_FILLED, strong())
    } else {
        (DOT_EMPTY, medium())
    }
}
