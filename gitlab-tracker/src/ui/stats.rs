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
            .style(Style::default().fg(theme::muted()))
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

    let clamped_scroll =
        render_stats_tab(f, report, active_tab, content_area, app.stats_view.scroll);

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
            .border_style(Style::default().fg(theme::muted_dim()))
            .title(Span::styled(
                " STATS │ [Tab]/[Shift+Tab]: Tabs │ [1-5]: Jump │ [W]: Window │ [R]: Refresh │ [↑/↓]: Scroll │ [G/Esc]: Close ",
                Style::default().fg(Color::Cyan),
            )),
    )
    .style(Style::default().fg(theme::muted_dim()));
    f.render_widget(status_bar, status_area);
}

/// Renders the content of one stats tab into `area`, scrolled by `scroll` lines
/// where the tab scrolls; returns the scroll clamped to the content.
/// Shared by the overlay above and the desktop window.
pub fn render_stats_tab(
    f: &mut Frame,
    report: &gitlab_tracker_stats::StatReport,
    tab: StatsTab,
    area: Rect,
    scroll: u16,
) -> u16 {
    match tab {
        StatsTab::Overview => {
            render_overview_tab(f, report, area);
            0
        }
        StatsTab::Flow => {
            render_flow_tab(f, report, area);
            0
        }
        StatsTab::Quality => render_quality_tab(f, report, area, scroll),
        StatsTab::Forecasts => render_forecasts_tab(f, report, area, scroll),
        StatsTab::Correlations => render_correlations_tab(f, report, area, scroll),
    }
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
                Style::default().fg(theme::muted())
            };
            [Span::raw(" "), Span::styled(format!(" {label} "), style)]
        })
        .collect::<Vec<_>>();

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::muted_dim()))
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
/// `AppEvent::StatsReportLoaded`. This keeps the main event loop non-blocking.
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

        let generation = app.stats_view.next_generation();
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

            let result = gitlab_tracker_stats::StatReport::load(db.as_ref(), &filter)
                .await
                .map(Box::new);
            let _ = tx2.send(crate::models::AppEvent::StatsReportLoaded { generation, result });

            // The cockpit reads the whole retained history, whatever the window
            // selected in the overlay.
            let cockpit_filter = QueryFilter {
                window: None,
                ..filter
            };
            let result = gitlab_tracker_stats::aggregator::aggregate(db.as_ref(), &cockpit_filter)
                .await
                .map(Box::new);
            let _ = tx2.send(crate::models::AppEvent::StatsCockpitLoaded { generation, result });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::test_app;
    use gitlab_tracker_stats::{QueryFilter, StatReport};
    use ratatui::{backend::TestBackend, Terminal};

    /// The report computed from the demo dataset (all time): every block has data.
    pub(super) async fn demo_report() -> StatReport {
        let db = crate::demo::seed_demo_stats_db("123456")
            .await
            .expect("demo stats DB");
        let filter = QueryFilter {
            project_id: Some("123456".into()),
            sprint_weeks: Some(2),
            ..QueryFilter::default()
        };
        StatReport::load(db.as_ref(), &filter).await.unwrap()
    }

    pub(super) fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// Draws the overlay and returns the screen, one line per row.
    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render_stats_overlay(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn overlay_shows_loading_error_and_empty_states() {
        let mut app = test_app();
        assert!(screen(&mut app, 100, 20).contains("No stats available yet."));

        app.stats_view.error = Some("database is locked".into());
        let text = screen(&mut app, 100, 20);
        assert!(text.contains("database is locked") && !text.contains("No stats available"));

        // Loading wins: a previous error is not shown while a new report computes.
        app.stats_view.loading = true;
        let text = screen(&mut app, 100, 20);
        assert!(text.contains("Computing statistics") && !text.contains("database is locked"));
        assert!(text.contains(app.stats_view.window.label()));
    }

    #[tokio::test]
    async fn every_tab_renders_its_blocks_from_the_report() {
        let mut app = test_app();
        app.stats_view.report = Some(demo_report().await);
        let expected: [(StatsTab, &[&str]); 5] = [
            (
                StatsTab::Overview,
                &[
                    "Throughput · Quality",
                    "Cycle Time (created → merged)",
                    "Backlog Health",
                    "Poisson · Queue · Signals",
                    "Summary insights",
                ],
            ),
            (
                StatsTab::Flow,
                &[
                    "Promotion Lead Time (merge → branch)",
                    "Cycle Time by Author (median)",
                    "Cycle Time by Reviewer (median)",
                    "Cycle Time by Milestone (median)",
                ],
            ),
            (
                StatsTab::Quality,
                &["Data confidence", "MR size buckets", "Overall"],
            ),
            (StatsTab::Forecasts, &["Forecasts · Signals", "Signals"]),
            (StatsTab::Correlations, &["Spearman Correlations", "ρ = "]),
        ];
        for (tab, markers) in expected {
            app.stats_view.tab = tab;
            let text = screen(&mut app, 170, 50);
            for marker in markers {
                assert!(
                    text.contains(marker),
                    "{tab:?}: `{marker}` missing in\n{text}"
                );
            }
            // Chrome shared by every tab.
            assert!(text.contains(&format!("Tab: {}", tab.label())), "{tab:?}");
            assert!(text.contains("MRs in sample") && text.contains("5 Correlations"));
            for other in expected.iter().filter(|(other, _)| *other != tab) {
                assert!(
                    !text.contains(other.1[0]),
                    "{tab:?} also shows {:?}",
                    other.0
                );
            }
        }
    }

    #[tokio::test]
    async fn scroll_is_clamped_by_the_tab_being_rendered() {
        let mut app = test_app();
        app.stats_view.report = Some(demo_report().await);

        // List tabs keep at most what is left to scroll.
        app.stats_view.tab = StatsTab::Quality;
        app.stats_view.scroll = u16::MAX;
        screen(&mut app, 170, 50);
        let max = app.stats_view.scroll;
        assert!(max < 50, "clamped to the content: {max}");
        app.stats_view.scroll = 0;
        screen(&mut app, 170, 50);
        assert_eq!(app.stats_view.scroll, 0);

        // Dashboard tabs do not scroll.
        app.stats_view.tab = StatsTab::Overview;
        app.stats_view.scroll = 12;
        screen(&mut app, 170, 50);
        assert_eq!(app.stats_view.scroll, 0);
    }

    #[tokio::test]
    async fn tiny_terminals_do_not_panic() {
        let mut app = test_app();
        app.stats_view.report = Some(demo_report().await);
        for tab in [
            StatsTab::Overview,
            StatsTab::Flow,
            StatsTab::Quality,
            StatsTab::Forecasts,
            StatsTab::Correlations,
        ] {
            app.stats_view.tab = tab;
            for (width, height) in [(1, 1), (10, 4), (30, 9), (60, 12)] {
                screen(&mut app, width, height);
            }
        }
    }
}
