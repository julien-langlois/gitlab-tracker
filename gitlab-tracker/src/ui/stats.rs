use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

mod common;
mod correlations;
mod flow;
mod forecasts;
mod overview;
mod quality;

use common::inner_area;
use correlations::render_correlations_tab;
use flow::render_flow_tab;
use forecasts::render_forecasts_tab;
use overview::render_overview_tab;
use quality::render_quality_tab;

use crate::app::{App, StatsTab};
use crate::ui::theme;

/// Renders the Stats overlay as a fullscreen multi-block dashboard.
pub fn render_stats_overlay(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Clear, area);

    let view = &app.stats_view;
    let window_label = view.window.label();

    // ── Outer border with title bar ───────────────────────────────────────────
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" Stats ─ {window_label} "));

    // Loading / error / empty states are rendered inside the outer block.
    if view.loading {
        f.render_widget(
            Paragraph::new("  ⟳  Computing statistics…")
                .block(outer_block)
                .style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }

    if let Some(err) = &view.error {
        f.render_widget(
            Paragraph::new(format!("  ✘  {err}"))
                .block(outer_block)
                .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    let Some(report) = &view.report else {
        f.render_widget(
            Paragraph::new(
                "  No stats available yet.\n\
                 \n\
                 Stats are recorded automatically as MRs are merged or refreshed.\n\
                 Open this view again after a few MR events have been processed.",
            )
            .block(outer_block)
            .style(Style::default().fg(theme::MUTED))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };

    // Render the outer frame first, then compute inner area for content.
    f.render_widget(outer_block, area);
    let inner = inner_area(area);

    // ── Fixed chrome: tabs on top, status bar at bottom ───────────────────────
    let [tabs_area, content_area, status_area] = {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(inner);
        [chunks[0], chunks[1], chunks[2]]
    };

    let active_tab = app.stats_view.tab;
    render_stats_tab_bar(f, tabs_area, active_tab);

    let current_scroll = app.stats_view.scroll;
    let clamped_scroll = match active_tab {
        StatsTab::Overview => {
            render_overview_tab(f, report, content_area);
            0
        }
        StatsTab::Flow => {
            render_flow_tab(f, report, content_area);
            0
        }
        StatsTab::Quality => render_quality_tab(f, report, content_area, current_scroll),
        StatsTab::Forecasts => render_forecasts_tab(f, report, content_area, current_scroll),
        StatsTab::Correlations => render_correlations_tab(f, report, content_area, current_scroll),
    };

    let generated_at = report.generated_at.clone();
    let total_mrs = report.aggregated.total_mrs;
    let window_lbl = window_label.to_string();
    let tab_lbl = active_tab.label();

    app.stats_view.scroll = clamped_scroll;

    // ── Fixed status bar (full width, like the main input bar) ────────────────
    let status_bar = Paragraph::new(format!(
        " Generated {generated_at}  ·  {total_mrs} MRs in sample  ·  Window: {window_lbl}  ·  Tab: {tab_lbl}"
    ))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::MUTED_DIM))
            .title(Span::styled(
                " STATS │ [Tab]/[Shift+Tab]: Tabs │ [1-5]: Jump │ [W]: Window │ [R]: Refresh │ [↑/↓]: Scroll │ [G/Esc]: Close ",
                Style::default().fg(Color::Cyan),
            )),
    )
    .style(Style::default().fg(theme::MUTED_DIM));
    f.render_widget(status_bar, status_area);
}

// ── Tab renderers ─────────────────────────────────────────────────────────────

fn render_stats_tab_bar(f: &mut Frame, area: Rect, active: StatsTab) {
    let tabs = [
        (StatsTab::Overview, "1 Overview"),
        (StatsTab::Flow, "2 Flow"),
        (StatsTab::Quality, "3 Quality"),
        (StatsTab::Forecasts, "4 Forecasts"),
        (StatsTab::Correlations, "5 Correlations"),
    ];

    let spans = tabs
        .iter()
        .flat_map(|(tab, label)| {
            let style = if *tab == active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::MUTED)
            };
            [Span::raw(" "), Span::styled(format!(" {label} "), style)]
        })
        .collect::<Vec<_>>();

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::MUTED_DIM))
                .title(Span::styled(
                    " Tabs ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
        ),
        area,
    );
}

// ── Public API re-exported from the overlay module ────────────────────────────

/// Called by `events.rs` to trigger an async stats recomputation.
///
/// Marks `stats_view.loading = true` immediately so the overlay shows a spinner,
/// then spawns a Tokio task that queries the DB and sends the result back via
/// `AppEvent::StatsReportReady`. This keeps the main event loop non-blocking.
pub fn trigger_stats_refresh(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::models::AppEvent>,
) {
    trigger_stats_report_refresh(app, tx, true);
}

/// Refreshes the stats report for cockpit consumers without opening or marking the
/// fullscreen stats overlay as loading.
pub fn trigger_background_stats_refresh(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::models::AppEvent>,
) {
    trigger_stats_report_refresh(app, tx, false);
}

fn trigger_stats_report_refresh(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::models::AppEvent>,
    mark_overlay_loading: bool,
) {
    #[cfg(feature = "stats")]
    {
        use gitlab_tracker_stats::aggregator::QueryFilter;

        let Some(db) = app.stats_db.clone() else {
            if mark_overlay_loading {
                app.stats_view.report = None;
                app.stats_view.error =
                    Some("Stats DB not available — check startup logs.".to_string());
            }
            return;
        };

        if mark_overlay_loading {
            app.stats_view.loading = true;
            app.stats_view.error = None;
            app.stats_view.scroll = 0;
        }

        let window = app.stats_view.window.to_query_window();
        let project_id = app.project_id.clone();
        let sprint_weeks = app.stats_view.sprint_weeks;
        let tx2 = tx.clone();

        tokio::spawn(async move {
            let filter = QueryFilter {
                window,
                project_id: Some(project_id),
                sprint_weeks: Some(sprint_weeks),
                ..Default::default()
            };

            match gitlab_tracker_stats::StatReport::load(db.as_ref(), &filter).await {
                Ok(report) => {
                    let _ = tx2.send(crate::models::AppEvent::StatsReportReady(Box::new(report)));
                }
                Err(e) => {
                    let _ = tx2.send(crate::models::AppEvent::StatsReportFailed(e.to_string()));
                }
            }
        });
    }
}
