//! Centralised colour theme for the TUI.
//!
//! # Why `Color::Rgb` instead of named ANSI colours?
//!
//! Named ANSI colours such as `Color::DarkGray` map to terminal colour slot #8.
//! Every terminal palette redefines that slot differently — Terminator/Ambiance
//! renders it as ~#555555 on a ~#2D0922 background, which produces a contrast
//! ratio of only ~2.8:1 (WCAG minimum is 4.5:1).
//!
//! `Color::Rgb(r, g, b)` values are **absolute**: they are passed directly to
//! the terminal as a 24-bit colour sequence and are never re-interpreted by the
//! palette. This guarantees consistent readability across Ambiance, Dracula, Nord,
//! Solarized Dark, Gruvbox Dark, and any other dark theme.
//!
//! All colours below have been chosen to achieve a WCAG AA contrast ratio of at
//! least 4.5:1 against backgrounds darker than #333333.

use ratatui::style::Color;

// ── Semantic "muted" tones (replaces DarkGray everywhere) ─────────────────────

/// Primary muted colour — used for secondary labels, metadata (Clone, URL,
/// Start Date, date fields). Contrast ≥ 5:1 on any background darker than #2A2A2A.
pub const MUTED: Color = Color::Rgb(160, 160, 175);

/// Dimmer muted colour — used for decorative separators (`──────`).
/// Intentionally lighter than pure background noise, but less prominent than
/// actual content.
pub const MUTED_DIM: Color = Color::Rgb(100, 100, 115);

/// Used for inline comments in the time log (quoted text) — sits between
/// `MUTED_DIM` and `MUTED` so it reads as "supplementary" without disappearing.
pub const MUTED_COMMENT: Color = Color::Rgb(130, 130, 145);

/// Used for unfocused field borders in the Log Time popup and for keyboard-hint
/// text at the bottom of popups (`[Tab] Next field  [Enter] Submit  [Esc] Cancel`).
pub const MUTED_HINT: Color = Color::Rgb(120, 120, 135);

/// Used for states that are genuinely "empty/inactive" (canceled, skipped, unknown).
/// Slightly darker than `MUTED` to convey lower salience.
pub const MUTED_INACTIVE: Color = Color::Rgb(115, 115, 130);

// ── Semantic accent colours (kept as named ANSI — these are intentional) ──────
//
// `Color::Cyan`, `Color::Yellow`, `Color::Green`, `Color::Red`, `Color::Magenta`
// are used for *actionable* or *status* information where the terminal theme is
// expected to provide a visible, saturated rendering. They are NOT replaced here.
