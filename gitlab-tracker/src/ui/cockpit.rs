use crate::app::App;
use chrono::Utc;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use summary::{release_summaries, DashboardSummary, ReleaseSummary};

mod summary;

fn release_lines(app: &App, releases: &[ReleaseSummary]) -> Vec<CockpitRow> {
    let today = Utc::now().date_naive();
    let mut lines = Vec::new();

    for release in releases {
        let at_risk = release.is_at_risk(today, &app.config.cockpit_thresholds);
        let title_color = if at_risk {
            app.theme.accent_red
        } else {
            app.theme.accent_cyan
        };
        let due_label = release
            .due_date
            .map(|due_date| {
                let days_left = (due_date - today).num_days();
                if days_left < 0 {
                    format!("D+{}", days_left.abs())
                } else {
                    format!("D-{}", days_left)
                }
            })
            .unwrap_or_else(|| "no due date".to_string());
        lines.push(CockpitRow::Heading {
            text: format!("{} ({})", release.title, due_label),
            color: title_color,
            alert: at_risk,
        });
        lines.push(release_metric_line(
            "Merged",
            release.merged,
            app.theme.accent_green,
        ));
        lines.push(release_metric_line(
            "WIP",
            release.in_progress,
            app.theme.fg,
        ));
        lines.push(release_metric_line(
            "Blocked",
            release.blocked,
            alert_color(release.blocked, app),
        ));
        lines.push(release_metric_line(
            "Review",
            release.waiting_review,
            warning_color(release.waiting_review, app),
        ));
    }

    if lines.is_empty() {
        lines.push(cockpit_metric_optional(
            "Releases",
            Some("n/a".to_string()),
            app.theme.muted_inactive,
        ));
    }

    lines
}

/// One line of a cockpit card.
#[derive(Debug, Clone)]
pub enum CockpitRow {
    /// A release title; `alert` marks one at risk.
    Heading {
        text: String,
        color: Color,
        alert: bool,
    },
    /// A counter: `nested` ones belong to the heading above them.
    Metric {
        label: String,
        value: String,
        color: Color,
        nested: bool,
    },
}

impl CockpitRow {
    pub fn to_line(&self, app: &App) -> Line<'static> {
        match self {
            CockpitRow::Heading { text, color, alert } => {
                let prefix = if *alert { "⚠ " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix.to_string(), Style::default().fg(*color)),
                    Span::styled(
                        text.clone(),
                        Style::default().fg(*color).add_modifier(Modifier::BOLD),
                    ),
                ])
            }
            CockpitRow::Metric {
                label,
                value,
                color,
                nested,
            } => {
                let label = if *nested {
                    format!("    {label:<8}")
                } else {
                    format!("  {label:<18}")
                };
                Line::from(vec![
                    Span::styled(label, Style::default().fg(app.theme.fg)),
                    Span::styled(
                        value.clone(),
                        Style::default().fg(*color).add_modifier(Modifier::BOLD),
                    ),
                ])
            }
        }
    }
}

/// The cockpit's cards in display order, as (title, rows). Shared by the terminal
/// layout below and the desktop window.
pub fn cockpit_cards(app: &App) -> Vec<(&'static str, Vec<CockpitRow>)> {
    let summary = DashboardSummary::from_app(app);
    let releases = release_summaries(app);
    let mut cards = Vec::with_capacity(5);
    #[cfg(feature = "stats")]
    if let Some(flow_summary) = DashboardSummary::flow_from_stats_report(app, &summary) {
        cards.push(("Flow", flow_lines(flow_summary, app)));
    }
    cards.push(("Attention", attention_lines(summary, app)));
    cards.push(("Delivery Health", delivery_health_lines(summary, app)));
    cards.push(("Quality / Scope", quality_scope_lines(summary, app)));
    cards.push(("Releases", release_lines(app, &releases)));
    cards
}

pub fn render_cockpit(f: &mut Frame, app: &App, area: Rect) {
    let cards = cockpit_cards(app);
    let share = 100 / cards.len().max(1) as u16;
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Percentage(share); cards.len()])
        .split(area);
    for ((title, rows), column) in cards.into_iter().zip(columns.iter()) {
        let lines = rows.iter().map(|row| row.to_line(app)).collect();
        f.render_widget(cockpit_block(format!(" {title} "), lines), *column);
    }
}

fn cockpit_block(title: String, lines: Vec<Line<'static>>) -> Paragraph<'static> {
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
}

#[cfg(feature = "stats")]
fn flow_lines(summary: DashboardSummary, app: &App) -> Vec<CockpitRow> {
    vec![
        cockpit_metric("Open", summary.open, app.theme.fg),
        cockpit_metric("Mergeable", summary.mergeable, app.theme.accent_green),
        cockpit_metric("Draft", summary.draft, app.theme.muted_dim),
        cockpit_metric("Merged", summary.merged, app.theme.accent_cyan),
        cockpit_metric("Closed", summary.closed, app.theme.muted_dim),
        cockpit_metric("Merged today", summary.merged_today, app.theme.accent_green),
        cockpit_metric(
            "Merged this week",
            summary.merged_this_week,
            app.theme.accent_green,
        ),
        cockpit_metric(
            "Merged this month",
            summary.merged_this_month,
            app.theme.accent_green,
        ),
        cockpit_metric(
            "Merged last 7d",
            summary.merged_last_7_days,
            app.theme.accent_green,
        ),
        cockpit_metric(
            "Merged last 30d",
            summary.merged_last_30_days,
            app.theme.accent_green,
        ),
    ]
}

fn attention_lines(summary: DashboardSummary, app: &App) -> Vec<CockpitRow> {
    let total_blocked = summary.total_blocked();

    vec![
        cockpit_metric("Blocked", total_blocked, alert_color(total_blocked, app)),
        cockpit_metric(
            "Conflicts",
            summary.conflicts,
            alert_color(summary.conflicts, app),
        ),
        cockpit_metric(
            "Needs rebase",
            summary.needs_rebase,
            warning_color(summary.needs_rebase, app),
        ),
        cockpit_metric(
            "CI failing",
            summary.ci_failing,
            alert_color(summary.ci_failing, app),
        ),
        cockpit_metric(
            "Discussions",
            summary.discussions,
            warning_color(summary.discussions, app),
        ),
        cockpit_metric(
            "Changes req.",
            summary.requested_changes,
            alert_color(summary.requested_changes, app),
        ),
        cockpit_metric(
            "Needs review",
            summary.needs_review,
            warning_color(summary.needs_review, app),
        ),
        cockpit_metric_optional_dynamic(
            "To review (me)".to_string(),
            // Count, then the age of the oldest request (short: narrow column).
            Some(match summary.oldest_review_wait_days {
                Some(0) if summary.awaiting_my_review > 0 => {
                    format!("{} (<1d)", summary.awaiting_my_review)
                }
                Some(days) if summary.awaiting_my_review > 0 => {
                    format!("{} ({days}d)", summary.awaiting_my_review)
                }
                _ => summary.awaiting_my_review.to_string(),
            }),
            app.theme.accent_yellow,
        ),
        cockpit_metric(
            "No reviewer",
            summary.no_reviewer,
            warning_color(summary.no_reviewer, app),
        ),
        cockpit_metric(
            "No assignee",
            summary.no_assignee,
            warning_color(summary.no_assignee, app),
        ),
        cockpit_metric("Flagged", summary.flagged, app.theme.accent_yellow),
    ]
}

fn delivery_health_lines(summary: DashboardSummary, app: &App) -> Vec<CockpitRow> {
    let thresholds = &app.config.cockpit_thresholds;
    let mut lines = vec![
        cockpit_metric_dynamic(
            format!("Stale > {}d", thresholds.stale_days),
            summary.stale_7_days,
            warning_color(summary.stale_7_days, app),
        ),
        cockpit_metric_optional(
            "Oldest open",
            summary.oldest_open_days.map(|days| format!("{}d", days)),
            summary
                .oldest_open_days
                .map_or(app.theme.muted_inactive, |days| {
                    if days >= thresholds.old_open_alert_days {
                        app.theme.accent_red
                    } else if days >= thresholds.old_open_warning_days {
                        app.theme.accent_yellow
                    } else {
                        app.theme.accent_green
                    }
                }),
        ),
        cockpit_metric(
            "No milestone",
            summary.no_milestone,
            warning_color(summary.no_milestone, app),
        ),
        cockpit_metric_dynamic(
            format!("Due next {}d", thresholds.due_soon_days),
            summary.due_this_week,
            warning_color(summary.due_this_week, app),
        ),
        cockpit_metric(
            "Overdue",
            summary.overdue,
            alert_color(summary.overdue, app),
        ),
        cockpit_metric(
            "Behind target",
            summary.behind_target,
            warning_color(summary.behind_target, app),
        ),
        cockpit_metric(
            "Hot threads",
            summary.hot_threads,
            warning_color(summary.hot_threads, app),
        ),
    ];

    if app.tracker.is_some() {
        lines.push(cockpit_metric(
            "With ticket",
            summary.with_ticket,
            app.theme.accent_cyan,
        ));
        lines.push(cockpit_metric(
            "No ticket",
            summary.no_ticket,
            warning_color(summary.no_ticket, app),
        ));
        lines.push(cockpit_metric(
            "Over estimate",
            summary.over_estimate,
            alert_color(summary.over_estimate, app),
        ));
    } else {
        lines.push(cockpit_metric_optional(
            "Tracker",
            Some("n/a".to_string()),
            app.theme.muted_inactive,
        ));
    }

    lines
}

fn quality_scope_lines(summary: DashboardSummary, app: &App) -> Vec<CockpitRow> {
    vec![
        cockpit_metric(
            "Complex",
            summary.complex,
            warning_color(summary.complex, app),
        ),
        cockpit_metric_optional(
            "Avg diff lines",
            summary.avg_diff_lines().map(|lines| lines.to_string()),
            app.theme.accent_cyan,
        ),
        cockpit_metric(
            "Large diff",
            summary.large_diff,
            warning_color(summary.large_diff, app),
        ),
        cockpit_metric(
            "Many commits",
            summary.many_commits,
            warning_color(summary.many_commits, app),
        ),
        cockpit_metric(
            "Many files",
            summary.many_files,
            warning_color(summary.many_files, app),
        ),
        cockpit_metric(
            "No diff stats",
            summary.no_diff_stats,
            warning_color(summary.no_diff_stats, app),
        ),
        cockpit_metric(
            "Pipeline unknown",
            summary.pipeline_unknown,
            warning_color(summary.pipeline_unknown, app),
        ),
        cockpit_metric(
            "CI skipped",
            summary.ci_skipped,
            warning_color(summary.ci_skipped, app),
        ),
        cockpit_metric(
            "Recently updated",
            summary.recently_updated,
            app.theme.accent_green,
        ),
    ]
}

fn cockpit_metric(label: &'static str, value: usize, color: Color) -> CockpitRow {
    cockpit_metric_optional(label, Some(value.to_string()), color)
}

fn cockpit_metric_dynamic(label: String, value: usize, color: Color) -> CockpitRow {
    cockpit_metric_optional_dynamic(label, Some(value.to_string()), color)
}

fn release_metric_line(label: &'static str, value: usize, color: Color) -> CockpitRow {
    CockpitRow::Metric {
        label: label.to_string(),
        value: value.to_string(),
        color,
        nested: true,
    }
}

fn cockpit_metric_optional(label: &'static str, value: Option<String>, color: Color) -> CockpitRow {
    cockpit_metric_optional_dynamic(label.to_string(), value, color)
}

fn cockpit_metric_optional_dynamic(
    label: String,
    value: Option<String>,
    color: Color,
) -> CockpitRow {
    CockpitRow::Metric {
        label,
        value: value.unwrap_or_else(|| "—".to_string()),
        color,
        nested: false,
    }
}

fn alert_color(value: usize, app: &App) -> Color {
    if value > 0 {
        app.theme.accent_red
    } else {
        app.theme.muted_inactive
    }
}

fn warning_color(value: usize, app: &App) -> Color {
    if value > 0 {
        app.theme.accent_yellow
    } else {
        app.theme.muted_inactive
    }
}
