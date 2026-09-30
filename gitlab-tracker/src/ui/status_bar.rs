//! Status bar renderer — displays metadata above the main MR table.
//!
//! Each piece of information is a coloured [`Span`] separated by a dim `│` divider,
//! which allows per-segment colours (e.g. red timer when < 30 s) without cramming
//! everything into a plain `.title()` string on the `Block`.
//!
//! All colours are sourced from `app.theme` (a [`crate::ui::theme::Palette`]) so
//! the bar remains readable on both dark and light terminal backgrounds.
//! No raw `Color::White / Cyan / Green / Yellow / Red` — those are ANSI slots
//! re-interpreted by the terminal palette and become invisible on Solarized Light.

use crate::app::{App, SortColumn, SortOrder};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Dim separator used between status bar segments.
/// Colour is taken from the active palette so it adapts to dark/light themes.
fn sep(muted_dim: ratatui::style::Color) -> Span<'static> {
    Span::styled(" │ ", Style::default().fg(muted_dim))
}

/// Renders the one-line status bar shown above the MR table.
///
/// Segments (left → right):
///   1. Project name / URL
///   2. Next refresh countdown (coloured red when < 30 s)
///   3. API call estimates (GitLab + optional Tracker)
///   4. Auto-polling badge (ON / OFF)
///   5. MR count (filtered / total)
///   6. Active sort column + order
///   7. Active filter label
///   8. Loading spinner (only while fetches are pending)
pub fn render_status_bar(app: &App) -> Paragraph<'static> {
    let palette = app.theme;
    let fg = palette.fg;
    let muted = palette.muted;
    let muted_dim = palette.muted_dim;
    let muted_inactive = palette.muted_inactive;
    let accent_cyan = palette.accent_cyan;
    let accent_green = palette.accent_green;
    let accent_yellow = palette.accent_yellow;
    let accent_red = palette.accent_red;

    let mut spans: Vec<Span<'static>> = Vec::new();

    // ── 1. Project label ──────────────────────────────────────────────────────
    let project_label = match &app.project_name {
        Some(name) if !name.is_empty() => format!(" {} ({})", name, app.base_url),
        _ => format!(" {}", app.base_url),
    };
    spans.push(Span::styled(
        project_label,
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    ));

    // ── 2. Next refresh countdown ─────────────────────────────────────────────
    let mins = app.time_left / 60;
    let secs = app.time_left % 60;
    let timer_color = if app.time_left < 30 {
        accent_red
    } else if app.time_left < 60 {
        accent_yellow
    } else {
        accent_green
    };
    spans.push(sep(muted_dim));
    spans.push(Span::styled("🔄 ", Style::default().fg(muted)));
    spans.push(Span::styled(
        format!("{:02}:{:02}", mins, secs),
        Style::default()
            .fg(timer_color)
            .add_modifier(Modifier::BOLD),
    ));

    // ── 3. API call estimates ─────────────────────────────────────────────────
    let gl = app.estimated_gitlab_calls;
    let gl_startup = app.startup_gitlab_estimate.unwrap_or(gl);

    spans.push(sep(muted_dim));
    spans.push(Span::styled("🌐 GL ~", Style::default().fg(muted)));
    spans.push(Span::styled(
        gl.to_string(),
        Style::default().fg(accent_cyan),
    ));
    if gl != gl_startup {
        spans.push(Span::styled(
            format!(" ({}🚀)", gl_startup),
            Style::default().fg(muted_dim),
        ));
    }

    // Tracker segment — only when a provider is configured.
    if let Some(provider) = &app.tracker {
        let tr = app.estimated_tracker_calls;
        let tr_startup = app.startup_tracker_estimate.unwrap_or(tr);

        spans.push(sep(muted_dim));
        spans.push(Span::styled(
            format!("{} ~", provider.name()),
            Style::default().fg(muted),
        ));
        spans.push(Span::styled(
            tr.to_string(),
            Style::default().fg(accent_cyan),
        ));
        if tr != tr_startup {
            spans.push(Span::styled(
                format!(" ({}🚀)", tr_startup),
                Style::default().fg(muted_dim),
            ));
        }
    }

    // ── 4. Auto-polling badge ─────────────────────────────────────────────────
    spans.push(sep(muted_dim));
    spans.push(Span::styled("🔍 Auto-polling ", Style::default().fg(muted)));
    if app.discovery_enabled {
        spans.push(Span::styled(
            "ON",
            Style::default()
                .fg(accent_green)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            "OFF",
            Style::default()
                .fg(muted_inactive)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // ── 5. MR count ───────────────────────────────────────────────────────────
    let total = app.mrs.len();
    let visible = app.visible_mrs().count();
    spans.push(sep(muted_dim));
    if visible < total {
        spans.push(Span::styled(
            format!("{}", visible),
            Style::default()
                .fg(accent_yellow)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("/{} MRs", total),
            Style::default().fg(muted),
        ));
    } else {
        spans.push(Span::styled(
            format!("{}", total),
            Style::default().fg(fg).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" MRs", Style::default().fg(muted)));
    }

    // ── 6. Sort ───────────────────────────────────────────────────────────────
    let sort_col = match app.sort_column {
        SortColumn::UpdatedAt => "Updated",
        SortColumn::Id => "ID",
        SortColumn::Milestone => "Milestone",
        SortColumn::Title => "Title",
    };
    let sort_arrow = match app.sort_order {
        SortOrder::Ascending => "↑",
        SortOrder::Descending => "↓",
    };
    spans.push(sep(muted_dim));
    spans.push(Span::styled("Sort: ", Style::default().fg(muted)));
    spans.push(Span::styled(
        format!("{} {}", sort_col, sort_arrow),
        Style::default().fg(fg),
    ));

    // ── 7. Active filter ──────────────────────────────────────────────────────
    let filter_label = app.active_filter.label(&app.filter_defs);
    spans.push(sep(muted_dim));
    spans.push(Span::styled("Filter: ", Style::default().fg(muted)));
    spans.push(Span::styled(
        filter_label.to_string(),
        Style::default()
            .fg(accent_green)
            .add_modifier(Modifier::BOLD),
    ));

    // ── 8. Loading spinner ────────────────────────────────────────────────────
    let pending = app
        .pending_initial_fetches
        .union(&app.pending_refresh_fetches)
        .count();
    if pending > 0 {
        const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = SPINNER_FRAMES[(app.spinner_frame / 3) % SPINNER_FRAMES.len()];
        spans.push(sep(muted_dim));
        spans.push(Span::styled(
            format!("{} Loading ({} pending)…", frame, pending),
            Style::default()
                .fg(accent_yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    Paragraph::new(Line::from(spans))
}
