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
///   9. Flash message (action feedback, only for a few seconds)
pub fn render_status_bar(app: &App, width: u16) -> Paragraph<'static> {
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

    // Where each segment starts in `spans`, and how long it stays when the bar
    // is too narrow.
    let mut marks: Vec<(usize, u8)> = vec![(0, Priority::ESSENTIAL)];

    // ── 1. Project label ──────────────────────────────────────────────────────
    let bold = Style::default().fg(fg).add_modifier(Modifier::BOLD);
    match &app.project_name {
        Some(name) if !name.is_empty() => {
            spans.push(Span::styled(format!(" {name}"), bold));
            // The URL goes first when space runs out: the name says which project.
            marks.push((spans.len(), Priority::URL));
            spans.push(Span::styled(format!(" ({})", app.base_url), bold));
        }
        _ => spans.push(Span::styled(format!(" {}", app.base_url), bold)),
    }

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
    marks.push((spans.len(), Priority::REFRESH));
    spans.push(sep(muted_dim));
    spans.push(Span::styled("🔄 ", Style::default().fg(muted)));
    spans.push(Span::styled(
        format!("{:02}:{:02}", mins, secs),
        Style::default()
            .fg(timer_color)
            .add_modifier(Modifier::BOLD),
    ));
    // The sentinel's light check (`0`: off).
    if app.sentinel_interval_secs > 0 {
        let left = app.sentinel_left;
        spans.push(Span::styled("  👁 ", Style::default().fg(muted)));
        spans.push(Span::styled(
            if app.sentinel.in_flight {
                "…".to_string()
            } else {
                format!("{:02}:{:02}", left / 60, left % 60)
            },
            Style::default().fg(accent_cyan),
        ));
    }

    // ── 3. API call estimates ─────────────────────────────────────────────────
    let gl = app.estimated_gitlab_calls;
    let gl_startup = app.startup_gitlab_estimate.unwrap_or(gl);

    marks.push((spans.len(), Priority::API));
    spans.push(sep(muted_dim));
    spans.push(Span::styled("🌐 GL ~", Style::default().fg(muted)));
    spans.push(Span::styled(
        gl.to_string(),
        Style::default().fg(accent_cyan),
    ));
    let gl_percent = crate::app::saved_percent(gl, app.saved_gitlab_calls);
    if gl_percent > 0 {
        spans.push(Span::styled(
            format!(" −{gl_percent}%"),
            Style::default().fg(accent_green),
        ));
    }
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

        marks.push((spans.len(), Priority::API));
        spans.push(sep(muted_dim));
        spans.push(Span::styled(
            format!("{} ~", provider.name()),
            Style::default().fg(muted),
        ));
        spans.push(Span::styled(
            tr.to_string(),
            Style::default().fg(accent_cyan),
        ));
        let tr_percent = crate::app::saved_percent(tr, app.saved_tracker_calls);
        if tr_percent > 0 {
            spans.push(Span::styled(
                format!(" −{tr_percent}%"),
                Style::default().fg(accent_green),
            ));
        }
        if tr != tr_startup {
            spans.push(Span::styled(
                format!(" ({}🚀)", tr_startup),
                Style::default().fg(muted_dim),
            ));
        }
    }

    // ── 4. Auto-polling badge ─────────────────────────────────────────────────
    marks.push((spans.len(), Priority::POLLING));
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
    marks.push((spans.len(), Priority::ESSENTIAL));
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
        SortColumn::MergedAt => "Merged",
    };
    let sort_arrow = match app.sort_order {
        SortOrder::Ascending => "↑",
        SortOrder::Descending => "↓",
    };
    marks.push((spans.len(), Priority::SORT));
    spans.push(sep(muted_dim));
    spans.push(Span::styled("Sort: ", Style::default().fg(muted)));
    spans.push(Span::styled(
        format!("{} {}", sort_col, sort_arrow),
        Style::default().fg(fg),
    ));

    // ── 7. Active filter ──────────────────────────────────────────────────────
    let filter_label = app.active_filter.label(&app.filter_defs);
    marks.push((spans.len(), Priority::FILTER));
    spans.push(sep(muted_dim));
    spans.push(Span::styled("Filter: ", Style::default().fg(muted)));
    spans.push(Span::styled(
        filter_label.to_string(),
        Style::default()
            .fg(accent_green)
            .add_modifier(Modifier::BOLD),
    ));

    if app.config.hide_merged {
        spans.push(Span::styled(
            " · merged hidden",
            Style::default().fg(accent_yellow),
        ));
    }

    // ── 8. Loading spinner ────────────────────────────────────────────────────
    let pending = app
        .pending_initial_fetches
        .union(&app.pending_refresh_fetches)
        .count();
    if pending > 0 {
        const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = SPINNER_FRAMES[(app.layout.spinner_frame / 3) % SPINNER_FRAMES.len()];
        marks.push((spans.len(), Priority::ESSENTIAL));
        spans.push(sep(muted_dim));
        spans.push(Span::styled(
            format!("{} Loading ({} pending)…", frame, pending),
            Style::default()
                .fg(accent_yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // ── 9. Flash message (action feedback, a few seconds) ─────────────────────
    if let Some(flash) = &app.flash {
        let colour = if flash.is_error {
            accent_red
        } else {
            accent_green
        };
        marks.push((spans.len(), Priority::ESSENTIAL));
        spans.push(sep(muted_dim));
        spans.push(Span::styled(
            flash.text.clone(),
            Style::default().fg(colour).add_modifier(Modifier::BOLD),
        ));
    }

    Paragraph::new(Line::from(fit(spans, &marks, width)))
}

/// How long a segment stays when the bar is too narrow: the highest goes first.
struct Priority;

impl Priority {
    /// The project, the MR count, loading and action feedback: never dropped.
    const ESSENTIAL: u8 = 0;
    const FILTER: u8 = 1;
    const REFRESH: u8 = 2;
    const SORT: u8 = 3;
    const POLLING: u8 = 4;
    const API: u8 = 5;
    const URL: u8 = 6;
}

/// Drops whole segments, least useful first (rightmost first among equals), until
/// the bar fits in `width` columns: a segment is shown entirely or not at all,
/// never cut in the middle of a number.
fn fit(spans: Vec<Span<'static>>, marks: &[(usize, u8)], width: u16) -> Vec<Span<'static>> {
    // (first span, end, priority, columns) per segment.
    let ends = marks
        .iter()
        .skip(1)
        .map(|&(start, _)| start)
        .chain(std::iter::once(spans.len()));
    let segments: Vec<(usize, usize, u8, usize)> = marks
        .iter()
        .zip(ends)
        .map(|(&(start, priority), end)| {
            let cols = spans[start..end].iter().map(Span::width).sum();
            (start, end, priority, cols)
        })
        .collect();
    let mut kept = vec![true; segments.len()];
    let mut total: usize = segments.iter().map(|s| s.3).sum();
    while total > usize::from(width) {
        let Some(drop) = (0..segments.len())
            .filter(|&i| kept[i] && segments[i].2 != Priority::ESSENTIAL)
            .max_by_key(|&i| (segments[i].2, i))
        else {
            break;
        };
        kept[drop] = false;
        total -= segments[drop].3;
    }
    let shown: Vec<usize> = segments
        .iter()
        .zip(&kept)
        .filter(|(_, &keep)| keep)
        .flat_map(|(s, _)| s.0..s.1)
        .collect();
    spans
        .into_iter()
        .enumerate()
        .filter(|(i, _)| shown.binary_search(i).is_ok())
        .map(|(_, span)| span)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// " Orbit" + " (url)" + " │ 🔄 14:56" + " │ GL ~3" + " │ 46 MRs" + " │ Sort: ID"
    fn bar() -> (Vec<Span<'static>>, Vec<(usize, u8)>) {
        let spans = vec![
            Span::raw(" Orbit"),
            Span::raw(" (https://gitlab.example.com)"),
            Span::raw(" │ "),
            Span::raw("🔄 14:56"),
            Span::raw(" │ "),
            Span::raw("GL ~3"),
            Span::raw(" │ "),
            Span::raw("46 MRs"),
            Span::raw(" │ "),
            Span::raw("Sort: ID"),
        ];
        let marks = vec![
            (0, Priority::ESSENTIAL),
            (1, Priority::URL),
            (2, Priority::REFRESH),
            (4, Priority::API),
            (6, Priority::ESSENTIAL),
            (8, Priority::SORT),
        ];
        (spans, marks)
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_wide_bar_shows_everything() {
        let (spans, marks) = bar();
        let all = text(&spans);
        assert_eq!(text(&fit(spans, &marks, 200)), all);
    }

    #[test]
    fn a_narrow_bar_drops_whole_segments_least_useful_first() {
        let (spans, marks) = bar();
        // Without the URL and the API estimate: 6 + 3+8 + 3+6 + 3+8 = 37.
        assert_eq!(
            text(&fit(spans.clone(), &marks, 37)),
            " Orbit │ 🔄 14:56 │ 46 MRs │ Sort: ID"
        );
        // One column less: the sort goes, the refresh and the count stay whole.
        assert_eq!(text(&fit(spans, &marks, 36)), " Orbit │ 🔄 14:56 │ 46 MRs");
    }

    #[test]
    fn the_project_and_the_count_are_never_dropped() {
        let (spans, marks) = bar();
        assert_eq!(text(&fit(spans, &marks, 5)), " Orbit │ 46 MRs");
    }
}
