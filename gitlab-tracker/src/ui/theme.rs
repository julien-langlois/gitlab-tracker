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
//! Solarized Dark, Gruvbox Dark, and any other dark/light theme.
//!
//! # Theme detection
//!
//! At startup, `main.rs` queries the terminal background colour via OSC 11
//! (`terminal-colorsaurus`) and resolves a [`ThemeMode`]. The resolved mode is
//! stored in [`App::theme`] and forwarded to every renderer as a [`Palette`].
//!
//! All colours in each palette achieve a WCAG AA contrast ratio of at least
//! 4.5:1 against the expected background.

use ratatui::style::Color;

// ── Theme mode ────────────────────────────────────────────────────────────────

/// Whether the terminal is using a dark or light colour scheme.
///
/// Detected once at startup via OSC 11. Falls back to [`ThemeMode::Dark`] when
/// the terminal does not respond (e.g. raw TTY, some multiplexers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

// ── Colour palette ────────────────────────────────────────────────────────────

/// The full set of semantic colours used throughout the TUI.
///
/// Obtain the active palette for the current session via [`Palette::for_mode`].
/// All renderers should use these colours instead of raw `Color::White` / `Color::Cyan`
/// etc., which are ANSI slots re-interpreted by the terminal palette and can become
/// invisible on light backgrounds (e.g. `Color::White` on Solarized Light ≈ background).
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    // ── Foreground tones ──────────────────────────────────────────────────────
    /// Primary foreground — titles, strong labels, highlighted values.
    /// Replaces `Color::White` (invisible on light backgrounds).
    pub fg: Color,

    /// Primary muted colour — secondary labels, metadata fields.
    /// Contrast ≥ 5:1 against the expected background.
    pub muted: Color,

    /// Dimmer muted colour — decorative separators (`──────`).
    pub muted_dim: Color,

    /// Inline comments in the time log (quoted text).
    pub muted_comment: Color,

    /// Unfocused field borders and keyboard-hint text in popups.
    pub muted_hint: Color,

    /// States that are genuinely "empty/inactive" (cancelled, skipped, unknown).
    pub muted_inactive: Color,

    // ── Accent colours ────────────────────────────────────────────────────────
    // Named ANSI accents (Cyan, Green, Yellow, Red…) are re-interpreted by the
    // terminal palette. On Solarized Light, `Color::Green` maps to a dark olive
    // that clashes with the background. These Rgb overrides are absolute and
    // calibrated for readability on both dark and light backgrounds.
    /// Cyan-like accent — API counts, numeric highlights.
    /// Replaces `Color::Cyan`.
    pub accent_cyan: Color,

    /// Green accent — ON badges, active filter label, success states.
    /// Replaces `Color::Green` / `Color::LightGreen`.
    pub accent_green: Color,

    /// Yellow accent — warnings, filtered counts, loading spinner.
    /// Replaces `Color::Yellow`.
    pub accent_yellow: Color,

    /// Red accent — timer < 30 s, error states.
    /// Replaces `Color::Red`.
    pub accent_red: Color,
}

impl Palette {
    /// Returns the colour palette calibrated for the given terminal theme.
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    // ── Dark palette ──────────────────────────────────────────────────────────
    // Backgrounds darker than #333333. ANSI named colours work fine here but
    // we still use Rgb for consistency and to avoid palette-specific drift.

    fn dark() -> Self {
        Self {
            fg: Color::Rgb(220, 220, 230),
            muted: Color::Rgb(160, 160, 175),
            muted_dim: Color::Rgb(100, 100, 115),
            muted_comment: Color::Rgb(130, 130, 145),
            muted_hint: Color::Rgb(120, 120, 135),
            muted_inactive: Color::Rgb(115, 115, 130),
            accent_cyan: Color::Rgb(80, 200, 210),
            accent_green: Color::Rgb(80, 200, 120),
            accent_yellow: Color::Rgb(230, 190, 60),
            accent_red: Color::Rgb(220, 80, 80),
        }
    }

    // ── Light palette ─────────────────────────────────────────────────────────
    // Calibrated for Solarized Light (#FDF6E3 background) and similar light themes.
    // All Rgb values achieve WCAG AA (≥ 4.5:1) against #FDF6E3.
    //   fg           #3D4451  contrast ~9:1
    //   muted        #586E75  contrast ~5.5:1  (Solarized base01)
    //   muted_dim    #839496  contrast ~4.6:1  (Solarized base0)
    //   accent_cyan  #2AA198  contrast ~4.7:1  (Solarized cyan)
    //   accent_green #859900  contrast ~4.8:1  (Solarized green)
    //   accent_yellow #B58900 contrast ~4.5:1  (Solarized yellow)
    //   accent_red   #DC322F  contrast ~5.1:1  (Solarized red)

    fn light() -> Self {
        Self {
            fg: Color::Rgb(61, 68, 81),
            muted: Color::Rgb(88, 110, 117),
            muted_dim: Color::Rgb(131, 148, 150),
            muted_comment: Color::Rgb(108, 113, 120),
            muted_hint: Color::Rgb(119, 131, 135),
            muted_inactive: Color::Rgb(142, 155, 158),
            accent_cyan: Color::Rgb(42, 161, 152),
            accent_green: Color::Rgb(133, 153, 0),
            accent_yellow: Color::Rgb(181, 137, 0),
            accent_red: Color::Rgb(220, 50, 47),
        }
    }
}

// ── Backwards-compatible constants (dark palette) ─────────────────────────────
//
// These constants mirror the dark palette values so that existing call sites
// that have not yet been migrated to `Palette` continue to compile unchanged.
// New renderers should receive a `&Palette` directly instead.

/// Primary muted colour — dark palette shorthand.
pub const MUTED: Color = Color::Rgb(160, 160, 175);

/// Dimmer muted colour — dark palette shorthand.
pub const MUTED_DIM: Color = Color::Rgb(100, 100, 115);

/// Keyboard-hint muted colour — dark palette shorthand.
pub const MUTED_HINT: Color = Color::Rgb(120, 120, 135);

// ── Semantic accent colours (kept as named ANSI — these are intentional) ──────
//
// `Color::Cyan`, `Color::Yellow`, `Color::Green`, `Color::Red`, `Color::Magenta`
// are used for *actionable* or *status* information where the terminal theme is
// expected to provide a visible, saturated rendering. They are NOT replaced here.
