use crate::app::App;
use crate::models::{GitlabMrState, MergeabilityStatus, PipelineState};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

#[derive(Debug, Clone, Copy, Default)]
struct DashboardSummary {
    open: usize,
    merged: usize,
    closed: usize,
    mergeable: usize,
    draft: usize,
    merged_today: usize,
    merged_this_week: usize,
    merged_this_month: usize,
    merged_last_7_days: usize,
    merged_last_30_days: usize,
    conflicts: usize,
    needs_rebase: usize,
    ci_failing: usize,
    ci_running: usize,
    discussions: usize,
    requested_changes: usize,
    needs_review: usize,
    assigned_to_me: usize,
    review_by_me: usize,
    no_reviewer: usize,
    no_assignee: usize,
    flagged: usize,
    stale_7_days: usize,
    oldest_open_days: Option<i64>,
    no_milestone: usize,
    due_this_week: usize,
    overdue: usize,
    behind_target: usize,
    hot_threads: usize,
    with_ticket: usize,
    no_ticket: usize,
    over_estimate: usize,
    complex: usize,
    large_diff: usize,
    many_commits: usize,
    many_files: usize,
    no_diff_stats: usize,
    diff_stats_count: usize,
    total_diff_lines: u64,
    pipeline_unknown: usize,
    ci_skipped: usize,
    recently_updated: usize,
}

impl DashboardSummary {
    fn from_app(app: &App) -> Self {
        let mut summary = Self::default();
        let now = Utc::now();
        let today = now.date_naive();
        let current_iso_week = now.iso_week();
        let current_year = now.year();
        let current_month = now.month();
        let username = app.config.gitlab_username.as_deref();
        let tracker_enabled = app.tracker.is_some();

        for mr in app.visible_mrs() {
            match mr.state {
                GitlabMrState::Opened => summary.open += 1,
                GitlabMrState::Merged => {
                    summary.merged += 1;
                    if let Some(merged_at) = parse_gitlab_datetime(mr.merged_at.as_deref()) {
                        if merged_at.date_naive() == today {
                            summary.merged_today += 1;
                        }
                        if merged_at.iso_week() == current_iso_week {
                            summary.merged_this_week += 1;
                        }
                        if merged_at.year() == current_year && merged_at.month() == current_month {
                            summary.merged_this_month += 1;
                        }
                        if merged_at >= now - Duration::days(7) {
                            summary.merged_last_7_days += 1;
                        }
                        if merged_at >= now - Duration::days(30) {
                            summary.merged_last_30_days += 1;
                        }
                    }
                }
                GitlabMrState::Closed => summary.closed += 1,
            }

            if mr.flagged {
                summary.flagged += 1;
            }
            if mr.recently_updated {
                summary.recently_updated += 1;
            }

            if tracker_enabled {
                if let Some(ticket) = &mr.linked_ticket {
                    summary.with_ticket += 1;
                    if is_over_estimate(ticket.time_estimate, ticket.time_spent) {
                        summary.over_estimate += 1;
                    }
                } else {
                    summary.no_ticket += 1;
                }
            }

            if mr.state != GitlabMrState::Opened {
                continue;
            }

            match mr.mergeability {
                MergeabilityStatus::Mergeable => summary.mergeable += 1,
                MergeabilityStatus::Draft => summary.draft += 1,
                MergeabilityStatus::Conflict => summary.conflicts += 1,
                MergeabilityStatus::NeedsRebase => summary.needs_rebase += 1,
                MergeabilityStatus::DiscussionsNotResolved => summary.discussions += 1,
                MergeabilityStatus::RequestedChanges => summary.requested_changes += 1,
                MergeabilityStatus::CiStillRunning => summary.ci_running += 1,
                MergeabilityStatus::CiMustPass => {}
                MergeabilityStatus::NotApproved => summary.needs_review += 1,
                _ => {}
            }

            match mr.pipelines.first().map(|pipeline| &pipeline.status) {
                Some(PipelineState::Failed) => summary.ci_failing += 1,
                Some(PipelineState::Skipped) => summary.ci_skipped += 1,
                None => summary.pipeline_unknown += 1,
                _ => {}
            }

            if mr.reviewers.is_empty() {
                summary.no_reviewer += 1;
            }
            if mr.assignee == "None" || mr.assignee.trim().is_empty() {
                summary.no_assignee += 1;
            }

            if let Some(username) = username {
                let needle = format!("@{}", username);
                if mr.assignee.contains(&needle) {
                    summary.assigned_to_me += 1;
                }
                if mr
                    .reviewers
                    .iter()
                    .any(|reviewer| reviewer.contains(&needle))
                {
                    summary.review_by_me += 1;
                }
            }

            if parse_gitlab_datetime(mr.updated_at.as_deref())
                .is_some_and(|updated_at| updated_at < now - Duration::days(7))
            {
                summary.stale_7_days += 1;
            }

            if let Some(created_at) = parse_gitlab_datetime(mr.created_at.as_deref()) {
                let age_days = (now - created_at).num_days().max(0);
                summary.oldest_open_days = Some(
                    summary
                        .oldest_open_days
                        .map_or(age_days, |oldest| oldest.max(age_days)),
                );
            }

            if let Some(stats) = &mr.diff_stats {
                let diff_lines = u64::from(stats.additions + stats.deletions);
                summary.diff_stats_count += 1;
                summary.total_diff_lines += diff_lines;

                if stats.difficulty(&app.config.complexity_profile) >= 0.66 {
                    summary.complex += 1;
                }
                if diff_lines >= u64::from(app.config.complexity_profile.hard_threshold) {
                    summary.large_diff += 1;
                }
                if stats.commits_count >= 10 {
                    summary.many_commits += 1;
                }
                if stats.files_changed >= 20 {
                    summary.many_files += 1;
                }
                if stats.commits_behind.is_some_and(|behind| behind > 0) {
                    summary.behind_target += 1;
                }
            } else {
                summary.no_diff_stats += 1;
            }

            if mr.milestone.trim().is_empty() || mr.milestone == "None" {
                summary.no_milestone += 1;
            }
            if let Some(due_date) = parse_gitlab_date(mr.milestone_due_date.as_deref()) {
                if due_date < today {
                    summary.overdue += 1;
                } else if due_date <= today + Duration::days(7) {
                    summary.due_this_week += 1;
                }
            }

            if mr.user_notes_count >= 10 {
                summary.hot_threads += 1;
            }
        }

        summary
    }

    fn total_blocked(self) -> usize {
        self.conflicts
            + self.needs_rebase
            + self.ci_failing
            + self.discussions
            + self.requested_changes
    }

    fn avg_diff_lines(self) -> Option<u64> {
        if self.diff_stats_count == 0 {
            None
        } else {
            Some(self.total_diff_lines / self.diff_stats_count as u64)
        }
    }
}

pub fn render_cockpit(f: &mut Frame, app: &App, area: Rect) {
    #[derive(Debug, Default)]
    struct ReleaseSummary {
        title: String,
        due_date: Option<NaiveDate>,
        merged: usize,
        in_progress: usize,
        blocked: usize,
        waiting_review: usize,
    }

    impl ReleaseSummary {
        fn remaining(&self) -> usize {
            self.in_progress + self.blocked + self.waiting_review
        }

        fn is_at_risk(&self, today: NaiveDate) -> bool {
            let Some(due_date) = self.due_date else {
                return false;
            };

            let days_left = (due_date - today).num_days();
            self.remaining() > 0
                && (days_left < 0
                    || (days_left <= 3 && self.remaining() >= 2)
                    || (days_left <= 7 && self.remaining() >= 5))
        }
    }

    fn release_summaries(app: &App) -> Vec<ReleaseSummary> {
        let today = Utc::now().date_naive();
        let mut releases = std::collections::BTreeMap::<String, ReleaseSummary>::new();

        for mr in app.visible_mrs() {
            let milestone = mr.milestone.trim();
            if milestone.is_empty() || milestone == "None" {
                continue;
            }

            let entry = releases
                .entry(milestone.to_string())
                .or_insert_with(|| ReleaseSummary {
                    title: milestone.to_string(),
                    due_date: parse_gitlab_date(mr.milestone_due_date.as_deref()),
                    ..ReleaseSummary::default()
                });

            if entry.due_date.is_none() {
                entry.due_date = parse_gitlab_date(mr.milestone_due_date.as_deref());
            }

            match mr.state {
                GitlabMrState::Merged => entry.merged += 1,
                GitlabMrState::Closed => {}
                GitlabMrState::Opened => match mr.mergeability {
                    MergeabilityStatus::Conflict | MergeabilityStatus::NeedsRebase => {
                        entry.blocked += 1;
                    }
                    MergeabilityStatus::NotApproved | MergeabilityStatus::RequestedChanges => {
                        entry.waiting_review += 1;
                    }
                    _ => entry.in_progress += 1,
                },
            }
        }

        let mut releases: Vec<ReleaseSummary> = releases.into_values().collect();
        releases.sort_by_key(|release| {
            release
                .due_date
                .map(|due_date| ((due_date - today).num_days().abs(), due_date))
                .unwrap_or((i64::MAX, NaiveDate::MAX))
        });
        releases.truncate(3);
        releases
    }

    fn release_lines(app: &App, releases: &[ReleaseSummary]) -> Vec<Line<'static>> {
        let today = Utc::now().date_naive();
        let mut lines = Vec::new();

        for release in releases {
            let at_risk = release.is_at_risk(today);
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

    let summary = DashboardSummary::from_app(app);
    let releases = release_summaries(app);
    let has_releases = !releases.is_empty();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if has_releases {
            vec![
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ]
        } else {
            vec![
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ]
        })
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

    if has_releases {
        f.render_widget(
            cockpit_block(" Releases ", release_lines(app, &releases)),
            columns[4],
        );
    }
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
    vec![
        cockpit_metric(
            "Blocked",
            summary.total_blocked(),
            alert_color(summary.total_blocked(), app),
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
        cockpit_metric("Flagged", summary.flagged, app.theme.accent_yellow, app),
    ]
}

fn delivery_health_lines(summary: DashboardSummary, app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        cockpit_metric(
            "Stale > 7d",
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
                    if days >= 14 {
                        app.theme.accent_red
                    } else if days >= 7 {
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
        cockpit_metric(
            "Due next 7d",
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

fn is_over_estimate(estimate: Option<u32>, spent: Option<u32>) -> bool {
    matches!((estimate, spent), (Some(estimate), Some(spent)) if estimate > 0 && spent > estimate)
}

fn parse_gitlab_datetime(value: Option<&str>) -> Option<DateTime<Utc>> {
    value?.parse::<DateTime<Utc>>().ok()
}

fn parse_gitlab_date(value: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value?, "%Y-%m-%d").ok()
}
