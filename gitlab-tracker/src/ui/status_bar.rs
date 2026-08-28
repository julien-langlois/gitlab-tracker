//! Status bar renderer — displays metadata above the main MR table.
//!
//! Each piece of information is a coloured [`Span`] separated by a dim `│` divider,
//! which allows per-segment colours (e.g. red timer when < 30 s) without cramming
//! everything into a plain `.title()` string on the [`Block`].

use crate::app::{App, SortColumn, SortOrder};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Dim separator used between status bar segments.
fn sep() -> Span<'static> {
    Span::styled(" │ ", Style::default().fg(Color::DarkGray))
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
    let mut spans: Vec<Span<'static>> = Vec::new();

    // ── 1. Project label ──────────────────────────────────────────────────────
    let project_label = match &app.project_name {
        Some(name) if !name.is_empty() => format!(" {} ({})", name, app.base_url),
        _ => format!(" {}", app.base_url),
    };
    spans.push(Span::styled(
        project_label,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    // ── 2. Next refresh countdown ─────────────────────────────────────────────
    let mins = app.time_left / 60;
    let secs = app.time_left % 60;
    let timer_color = if app.time_left < 30 {
        Color::Red
    } else if app.time_left < 60 {
        Color::Yellow
    } else {
        Color::Green
    };
    spans.push(sep());
    spans.push(Span::styled("🔄 ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        format!("{:02}:{:02}", mins, secs),
        Style::default()
            .fg(timer_color)
            .add_modifier(Modifier::BOLD),
    ));

    // ── 3. API call estimates ─────────────────────────────────────────────────
    let gl = app.estimated_gitlab_calls;
    let gl_startup = app.startup_gitlab_estimate.unwrap_or(gl);

    spans.push(sep());
    spans.push(Span::styled(
        "🌐 GL ~",
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::styled(
        gl.to_string(),
        Style::default().fg(Color::Cyan),
    ));
    if gl != gl_startup {
        spans.push(Span::styled(
            format!(" ({}🚀)", gl_startup),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Tracker segment — only when a provider is configured.
    if let Some(provider) = &app.tracker {
        let tr = app.estimated_tracker_calls;
        let tr_startup = app.startup_tracker_estimate.unwrap_or(tr);

        spans.push(sep());
        spans.push(Span::styled(
            format!("{} ~", provider.name()),
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            tr.to_string(),
            Style::default().fg(Color::Cyan),
        ));
        if tr != tr_startup {
            spans.push(Span::styled(
                format!(" ({}🚀)", tr_startup),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    // ── 4. Auto-polling badge ─────────────────────────────────────────────────
    spans.push(sep());
    if app.discovery_enabled {
        spans.push(Span::styled(
            "🔍 Auto-polling ",
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            "ON",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            "🔍 Auto-polling ",
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            "OFF",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // ── 5. MR count ───────────────────────────────────────────────────────────
    let total = app.mrs.len();
    let visible = app.visible_mrs().count();
    spans.push(sep());
    if visible < total {
        spans.push(Span::styled(
            format!("{}", visible),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("/{} MRs", total),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::styled(
            format!("{}", total),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" MRs", Style::default().fg(Color::DarkGray)));
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
    spans.push(sep());
    spans.push(Span::styled("Sort: ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        format!("{} {}", sort_col, sort_arrow),
        Style::default().fg(Color::White),
    ));

    // ── 7. Active filter ──────────────────────────────────────────────────────
    let filter_label = app.active_filter.label(&app.filter_defs);
    spans.push(sep());
    spans.push(Span::styled(
        "Filter: ",
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::styled(
        filter_label.to_string(),
        Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD),
    ));

    // ── 8. Loading spinner ────────────────────────────────────────────────────
    let pending = app.pending_initial_fetches + app.pending_refresh_fetches;
    if pending > 0 {
        const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = SPINNER_FRAMES[(app.spinner_frame / 3) % SPINNER_FRAMES.len()];
        spans.push(sep());
        spans.push(Span::styled(
            format!("{} Loading ({} pending)…", frame, pending),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    Paragraph::new(Line::from(spans))
}
