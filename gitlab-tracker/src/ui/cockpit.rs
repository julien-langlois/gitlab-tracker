use crate::app::App;
use chrono::Utc;
use cockpit_summary::{release_summaries, DashboardSummary, ReleaseSummary};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

#[path = "cockpit_summary.rs"]
mod cockpit_summary;

fn release_lines(app: &App, releases: &[ReleaseSummary]) -> Vec<Line<'static>> {
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
        let prefix = if at_risk { "⚠ " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(title_color)),
            Span::styled(
                format!("{} ({})", release.title, due_label),
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(release_metric_line(
            "Merged",
            release.merged,
            app.theme.accent_green,
            app,
        ));
        lines.push(release_metric_line(
            "WIP",
            release.in_progress,
            app.theme.fg,
            app,
        ));
        lines.push(release_metric_line(
            "Blocked",
            release.blocked,
            alert_color(release.blocked, app),
            app,
        ));
        lines.push(release_metric_line(
            "Review",
            release.waiting_review,
            warning_color(release.waiting_review, app),
            app,
        ));
    }

    if lines.is_empty() {
        lines.push(cockpit_metric_optional(
            "Releases",
            Some("n/a".to_string()),
            app.theme.muted_inactive,
            app,
        ));
    }

    lines
}

pub fn render_cockpit(f: &mut Frame, app: &App, area: Rect) {
    let summary = DashboardSummary::from_app(app);
    let releases = release_summaries(app);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    f.render_widget(
        cockpit_block(" Flow ", flow_lines(summary, app)),
        columns[0],
    );
    f.render_widget(
        cockpit_block(" Attention ", attention_lines(summary, app)),
        columns[1],
    );
    f.render_widget(
        cockpit_block(" Delivery Health ", delivery_health_lines(summary, app)),
        columns[2],
    );
    f.render_widget(
        cockpit_block(" Quality / Scope ", quality_scope_lines(summary, app)),
        columns[3],
    );

    f.render_widget(
        cockpit_block(" Releases ", release_lines(app, &releases)),
        columns[4],
    );
}

fn cockpit_block(title: &'static str, lines: Vec<Line<'static>>) -> Paragraph<'static> {
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
}

fn flow_lines(summary: DashboardSummary, app: &App) -> Vec<Line<'static>> {
    vec![
        cockpit_metric("Open", summary.open, app.theme.fg, app),
        cockpit_metric("Mergeable", summary.mergeable, app.theme.accent_green, app),
        cockpit_metric("Draft", summary.draft, app.theme.muted_dim, app),
        cockpit_metric("Merged", summary.merged, app.theme.accent_cyan, app),
        cockpit_metric("Closed", summary.closed, app.theme.muted_dim, app),
        cockpit_metric(
            "Merged today",
            summary.merged_today,
            app.theme.accent_green,
            app,
        ),
        cockpit_metric(
            "Merged this week",
            summary.merged_this_week,
            app.theme.accent_green,
            app,
        ),
        cockpit_metric(
            "Merged this month",
            summary.merged_this_month,
            app.theme.accent_green,
            app,
        ),
        cockpit_metric(
            "Merged last 7d",
            summary.merged_last_7_days,
            app.theme.accent_green,
            app,
        ),
        cockpit_metric(
            "Merged last 30d",
            summary.merged_last_30_days,
            app.theme.accent_green,
            app,
        ),
    ]
}

fn attention_lines(summary: DashboardSummary, app: &App) -> Vec<Line<'static>> {
    let total_blocked = summary.total_blocked();

    vec![
        cockpit_metric(
            "Blocked",
            total_blocked,
            alert_color(total_blocked, app),
            app,
        ),
        cockpit_metric(
            "Conflicts",
            summary.conflicts,
            alert_color(summary.conflicts, app),
            app,
        ),
        cockpit_metric(
            "Needs rebase",
            summary.needs_rebase,
            warning_color(summary.needs_rebase, app),
            app,
        ),
        cockpit_metric(
            "CI failing",
            summary.ci_failing,
            alert_color(summary.ci_failing, app),
            app,
        ),
        cockpit_metric(
            "Discussions",
            summary.discussions,
            warning_color(summary.discussions, app),
            app,
        ),
        cockpit_metric(
            "Changes req.",
            summary.requested_changes,
            alert_color(summary.requested_changes, app),
            app,
        ),
        cockpit_metric(
            "Needs review",
            summary.needs_review,
            warning_color(summary.needs_review, app),
            app,
        ),
        cockpit_metric(
            "Review by me",
            summary.review_by_me,
            app.theme.accent_yellow,
            app,
        ),
        cockpit_metric(
            "No reviewer",
            summary.no_reviewer,
            warning_color(summary.no_reviewer, app),
            app,
        ),
        cockpit_metric(
            "No assignee",
            summary.no_assignee,
            warning_color(summary.no_assignee, app),
            app,
        ),
        cockpit_metric("Flagged", summary.flagged, app.theme.accent_yellow, app),
    ]
}

fn delivery_health_lines(summary: DashboardSummary, app: &App) -> Vec<Line<'static>> {
    let thresholds = &app.config.cockpit_thresholds;
    let mut lines = vec![
        cockpit_metric_dynamic(
            format!("Stale > {}d", thresholds.stale_days),
            summary.stale_7_days,
            warning_color(summary.stale_7_days, app),
            app,
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
            app,
        ),
        cockpit_metric(
            "No milestone",
            summary.no_milestone,
            warning_color(summary.no_milestone, app),
            app,
        ),
        cockpit_metric_dynamic(
            format!("Due next {}d", thresholds.due_soon_days),
            summary.due_this_week,
            warning_color(summary.due_this_week, app),
            app,
        ),
        cockpit_metric(
            "Overdue",
            summary.overdue,
            alert_color(summary.overdue, app),
            app,
        ),
        cockpit_metric(
            "Behind target",
            summary.behind_target,
            warning_color(summary.behind_target, app),
            app,
        ),
        cockpit_metric(
            "Hot threads",
            summary.hot_threads,
            warning_color(summary.hot_threads, app),
            app,
        ),
    ];

    if app.tracker.is_some() {
        lines.push(cockpit_metric(
            "With ticket",
            summary.with_ticket,
            app.theme.accent_cyan,
            app,
        ));
        lines.push(cockpit_metric(
            "No ticket",
            summary.no_ticket,
            warning_color(summary.no_ticket, app),
            app,
        ));
        lines.push(cockpit_metric(
            "Over estimate",
            summary.over_estimate,
            alert_color(summary.over_estimate, app),
            app,
        ));
    } else {
        lines.push(cockpit_metric_optional(
            "Tracker",
            Some("n/a".to_string()),
            app.theme.muted_inactive,
            app,
        ));
    }

    lines
}

fn quality_scope_lines(summary: DashboardSummary, app: &App) -> Vec<Line<'static>> {
    vec![
        cockpit_metric(
            "Complex",
            summary.complex,
            warning_color(summary.complex, app),
            app,
        ),
        cockpit_metric_optional(
            "Avg diff lines",
            summary.avg_diff_lines().map(|lines| lines.to_string()),
            app.theme.accent_cyan,
            app,
        ),
        cockpit_metric(
            "Large diff",
            summary.large_diff,
            warning_color(summary.large_diff, app),
            app,
        ),
        cockpit_metric(
            "Many commits",
            summary.many_commits,
            warning_color(summary.many_commits, app),
            app,
        ),
        cockpit_metric(
            "Many files",
            summary.many_files,
            warning_color(summary.many_files, app),
            app,
        ),
        cockpit_metric(
            "No diff stats",
            summary.no_diff_stats,
            warning_color(summary.no_diff_stats, app),
            app,
        ),
        cockpit_metric(
            "Pipeline unknown",
            summary.pipeline_unknown,
            warning_color(summary.pipeline_unknown, app),
            app,
        ),
        cockpit_metric(
            "CI skipped",
            summary.ci_skipped,
            warning_color(summary.ci_skipped, app),
            app,
        ),
        cockpit_metric(
            "Recently updated",
            summary.recently_updated,
            app.theme.accent_green,
            app,
        ),
    ]
}

fn cockpit_metric(label: &'static str, value: usize, color: Color, app: &App) -> Line<'static> {
    cockpit_metric_optional(label, Some(value.to_string()), color, app)
}

fn cockpit_metric_dynamic(label: String, value: usize, color: Color, app: &App) -> Line<'static> {
    cockpit_metric_optional_dynamic(label, Some(value.to_string()), color, app)
}

fn release_metric_line(
    label: &'static str,
    value: usize,
    color: Color,
    app: &App,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("    {:<8}", label),
            Style::default().fg(app.theme.fg),
        ),
        Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn cockpit_metric_optional(
    label: &'static str,
    value: Option<String>,
    color: Color,
    app: &App,
) -> Line<'static> {
    cockpit_metric_optional_dynamic(label.to_string(), value, color, app)
}

fn cockpit_metric_optional_dynamic(
    label: String,
    value: Option<String>,
    color: Color,
    app: &App,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<18}", label),
            Style::default().fg(app.theme.fg),
        ),
        Span::styled(
            value.unwrap_or_else(|| "—".to_string()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
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
