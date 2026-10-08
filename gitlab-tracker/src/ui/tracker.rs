//! Tracker pane renderers — provider-agnostic TUI presentation layer.
//!
//! This module owns everything that is *visually* specific to the Tracker pane
//! (lower-right split), independently of which tracker backend is active
//! (Redmine, Jira, Linear, …).
//!
//! Design constraints:
//!  - Only `gitlab-tracker-core` types are imported here (no Redmine/Jira crates).
//!  - `ratatui` **is** allowed — this crate is the terminal binary.
//!  - Zero coupling to MR Inspector logic; `inspector.rs` is untouched.

use crate::models::TrackedMr;
use crate::ui::theme;
use crate::utils::format_relative_date;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::collections::HashMap;

// ── Colour maps ───────────────────────────────────────────────────────────────

/// Colour maps (background + foreground) for tracker badge labels shown in the
/// Tracker pane.
///
/// Populated at startup from the active tracker's config (e.g. `[project.tracker]` in `projects.toml`)
/// and forwarded to renderers so they stay fully agnostic of each tracker's data model.
///
/// Any future tracker plugin (Jira, Linear, …) simply fills the same two maps —
/// no change to the renderers is needed.
#[derive(Debug, Clone, Default)]
pub struct TrackerLabelColors {
    /// Colour map for tracker-type labels (e.g. "Bug", "Evolution").
    /// Keys are stored lowercased; `"*"` is a catch-all fallback.
    pub tracker_type: HashMap<String, (Color, Color)>,
    /// Colour map for priority labels (e.g. "Normale", "Haute").
    /// Keys are stored lowercased; `"*"` is a catch-all fallback.
    pub priority: HashMap<String, (Color, Color)>,
}

impl TrackerLabelColors {
    /// Resolves a label against a colour map with case-insensitive exact match,
    /// then a wildcard `"*"` fallback, then a hard-coded default (dark_gray / white).
    fn resolve(map: &HashMap<String, (Color, Color)>, label: &str) -> (Color, Color) {
        let label_lower = label.to_lowercase();
        if let Some(&colors) = map.get(&label_lower).or_else(|| {
            map.iter()
                .find(|(k, _)| k.to_lowercase() == label_lower)
                .map(|(_, v)| v)
        }) {
            return colors;
        }
        if let Some(&colors) = map.get("*") {
            return colors;
        }
        (theme::muted_dim(), Color::White)
    }

    /// Resolves the colour pair for a tracker-type label.
    pub fn get_tracker_type_color(&self, label: &str) -> (Color, Color) {
        Self::resolve(&self.tracker_type, label)
    }

    /// Resolves the colour pair for a priority label.
    pub fn get_priority_color(&self, label: &str) -> (Color, Color) {
        Self::resolve(&self.priority, label)
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Formats a number of minutes as "Xh Ym" ("45m", "2h", "1h 30m").
fn format_minutes(total_mins: u32) -> String {
    match (total_mins / 60, total_mins % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Formats a duration in seconds as "Xh Ym". Returns "—" for zero.
/// Also used by the table's Tracker column.
pub(crate) fn format_duration(secs: u32) -> String {
    if secs == 0 {
        return "—".to_string();
    }
    format_minutes(secs / 60)
}

/// Formats hours (f32) as "Xh Ym" for display in time-log entries.
fn fmt_hours(hours: f32) -> String {
    format_minutes((hours * 60.0).round() as u32)
}

// ── Renderers ─────────────────────────────────────────────────────────────────

/// Renders the Tracker pane (lower-right) — ticket details for the linked ticket.
///
/// Contains: ID, subject, type/priority badges, status, assignees, version,
/// start date, progress bar, and time tracking (estimate/spent/remaining).
pub fn render_ticket_info(mr: &TrackedMr, tracker_colors: &TrackerLabelColors) -> Text<'static> {
    let Some(ticket) = &mr.linked_ticket else {
        return Text::from(vec![Line::from(vec![Span::styled(
            "No linked ticket detected.",
            Style::default().fg(Color::DarkGray),
        )])]);
    };

    let mut lines: Vec<Line> = vec![];

    // ── ID + subject ──────────────────────────────────────────────────────────
    lines.push(Line::from(vec![
        Span::styled(
            format!("#{}", ticket.id),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            ticket.subject.clone(),
            Style::default()
                .fg(crate::ui::theme::fg())
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![Span::raw("")]));

    // ── Type badge ────────────────────────────────────────────────────────────
    if let Some(tracker_type) = &ticket.tracker_type {
        let (type_bg, type_fg) = tracker_colors.get_tracker_type_color(tracker_type);
        lines.push(Line::from(vec![
            Span::raw("Type     : "),
            Span::styled(
                format!(" {} ", tracker_type),
                Style::default()
                    .bg(type_bg)
                    .fg(type_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // ── Priority badge ────────────────────────────────────────────────────────
    if let Some(priority) = &ticket.priority {
        let (prio_bg, prio_fg) = tracker_colors.get_priority_color(priority);
        lines.push(Line::from(vec![
            Span::raw("Priority : "),
            Span::styled(
                format!(" {} ", priority),
                Style::default()
                    .bg(prio_bg)
                    .fg(prio_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // ── Status ────────────────────────────────────────────────────────────────
    lines.push(Line::from(vec![
        Span::raw("Status   : "),
        Span::styled(
            ticket.status.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // ── Author / Assignee ─────────────────────────────────────────────────────
    lines.push(Line::from(vec![
        Span::raw("Author   : "),
        Span::styled(
            ticket
                .author
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Assignee : "),
        Span::styled(
            ticket
                .assignee
                .clone()
                .unwrap_or_else(|| "Unassigned".to_string()),
            Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // ── Version / Start ───────────────────────────────────────────────────────
    if let Some(version) = &ticket.version {
        lines.push(Line::from(vec![
            Span::raw("Version  : "),
            Span::styled(
                version.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if let Some(start) = &ticket.start_date {
        // Append T00:00:00Z so format_relative_date can parse the date-only string.
        let iso_for_relative = format!("{}T00:00:00Z", start);
        let relative = format_relative_date(&iso_for_relative);
        lines.push(Line::from(vec![
            Span::raw("Start    : "),
            Span::styled(
                format!("{}  ({})", start, relative),
                Style::default().fg(theme::muted()),
            ),
        ]));
    }

    // ── Progress bar ──────────────────────────────────────────────────────────
    if let Some(ratio) = ticket.done_ratio {
        let bar_width: usize = 20;
        let filled = (ratio as f32 / 100.0 * bar_width as f32).round() as usize;
        let bar = format!(
            "{}{}",
            "█".repeat(filled),
            "░".repeat(bar_width.saturating_sub(filled))
        );
        let bar_color = if ratio >= 100 {
            Color::Green
        } else if ratio >= 75 {
            Color::Cyan
        } else {
            Color::Yellow
        };
        lines.push(Line::from(vec![
            Span::raw("Progress : "),
            Span::styled(bar, Style::default().fg(bar_color)),
            Span::styled(
                format!("  {}%", ratio),
                Style::default().fg(bar_color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // ── Time tracking ─────────────────────────────────────────────────────────
    let has_estimate = ticket.time_estimate.is_some_and(|v| v > 0);
    let has_spent = ticket.time_spent.is_some_and(|v| v > 0);
    let has_remaining = ticket.time_remaining.is_some_and(|v| v > 0);

    if has_estimate || has_spent || has_remaining {
        let spent_color = match (ticket.time_estimate, ticket.time_spent) {
            (Some(est), Some(spent)) if est > 0 => {
                // Colour encodes burn-rate against estimate: green = on track,
                // yellow = approaching limit (≥80%), red = over budget.
                let ratio = spent as f32 / est as f32;
                if ratio >= 1.0 {
                    Color::Red
                } else if ratio >= 0.8 {
                    Color::Yellow
                } else {
                    Color::Green
                }
            }
            // No estimate available — display spent time in cyan (neutral/informational)
            // rather than an invisible muted grey.
            _ => Color::Cyan,
        };
        if has_estimate {
            lines.push(Line::from(vec![
                Span::raw("Estimate : "),
                Span::styled(
                    ticket
                        .time_estimate
                        .map(format_duration)
                        .unwrap_or_else(|| "—".to_string()),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        if has_spent {
            lines.push(Line::from(vec![
                Span::raw("Spent    : "),
                Span::styled(
                    ticket
                        .time_spent
                        .map(format_duration)
                        .unwrap_or_else(|| "—".to_string()),
                    Style::default()
                        .fg(spent_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        if has_remaining {
            lines.push(Line::from(vec![
                Span::raw("Remaining: "),
                Span::styled(
                    ticket
                        .time_remaining
                        .map(format_duration)
                        .unwrap_or_else(|| "—".to_string()),
                    Style::default()
                        .fg(theme::muted())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    Text::from(lines)
}

/// Renders the Time Log view for the Tracker pane.
///
/// Shows a progress bar (spent vs estimate), then the list of time entries
/// fetched from the tracker backend for the linked ticket.
pub fn render_time_log(
    mr: &TrackedMr,
    // `None` before the first fetch was even started.
    state: Option<&crate::app::TimeLogState>,
    muted_comment: ratatui::style::Color,
) -> Text<'static> {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            format!("Time Log — MR !{}", mr.id),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![Span::raw("")]),
    ];

    // ── Progress bar (estimate vs spent) ─────────────────────────────────────
    if let Some(ticket) = &mr.linked_ticket {
        let estimate_secs = ticket.time_estimate.unwrap_or(0);
        let spent_secs = ticket.time_spent.unwrap_or(0);

        if estimate_secs > 0 {
            let ratio = (spent_secs as f32 / estimate_secs as f32).min(1.0);
            let bar_width: usize = 30;
            let filled = (ratio * bar_width as f32).round() as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

            let pct = (spent_secs as f32 / estimate_secs as f32 * 100.0).round() as u32;
            let bar_color = if spent_secs > estimate_secs {
                Color::Red
            } else if pct >= 80 {
                Color::Yellow
            } else {
                Color::Green
            };

            lines.push(Line::from(vec![
                Span::raw("Estimate : "),
                Span::styled(
                    format_duration(estimate_secs),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("Spent    : "),
                Span::styled(
                    format_duration(spent_secs),
                    Style::default().fg(bar_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(bar_color)),
                Span::raw(format!("  {}%", pct)),
            ]));
        } else {
            // No estimate — just show total spent.
            let total_hours: f32 = match state {
                Some(crate::app::TimeLogState::Loaded(entries)) => {
                    entries.iter().map(|e| e.hours).sum()
                }
                _ => 0.0,
            };
            lines.push(Line::from(vec![
                Span::raw("Total    : "),
                Span::styled(
                    fmt_hours(total_hours),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        lines.push(Line::from(vec![Span::raw("")]));
    }

    // ── Entries list ──────────────────────────────────────────────────────────
    lines.push(Line::from(vec![
        Span::styled("── ", Style::default().fg(theme::muted_dim())),
        Span::styled(
            "Entries",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ──────────────────────────────",
            Style::default().fg(theme::muted_dim()),
        ),
    ]));

    let entries = match state {
        Some(crate::app::TimeLogState::Loaded(entries)) => entries.as_slice(),
        Some(crate::app::TimeLogState::Failed(error)) => {
            lines.push(Line::from(vec![Span::styled(
                format!("Failed to load time entries: {error}"),
                Style::default().fg(Color::Red),
            )]));
            lines.push(Line::from(vec![Span::styled(
                "Retried at the next refresh — or press [R] now.",
                Style::default().fg(theme::muted()),
            )]));
            return Text::from(lines);
        }
        Some(crate::app::TimeLogState::Loading) | None => {
            lines.push(Line::from(vec![Span::styled(
                "Loading time entries…",
                Style::default().fg(theme::muted()),
            )]));
            return Text::from(lines);
        }
    };
    if entries.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No time entries recorded yet.",
            Style::default().fg(theme::muted()),
        )]));
    } else {
        for entry in entries {
            // Line 1: date | duration | activity | user
            lines.push(Line::from(vec![
                Span::styled(entry.spent_on.clone(), Style::default().fg(theme::muted())),
                Span::raw("  "),
                Span::styled(
                    fmt_hours(entry.hours),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    entry.activity.name.clone(),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    entry.user.clone(),
                    Style::default().fg(crate::ui::theme::fg()),
                ),
            ]));
            // Line 2: comment (indented), shown only when non-empty.
            if !entry.comment.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("\"{}\"", entry.comment),
                        Style::default().fg(muted_comment),
                    ),
                ]));
            }
        }

        // Summary footer.
        let total_hours: f32 = entries.iter().map(|e| e.hours).sum();
        lines.push(Line::from(vec![Span::raw("")]));
        lines.push(Line::from(vec![Span::styled(
            "─────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!(
                "Total: {} entries — {} logged",
                entries.len(),
                fmt_hours(total_hours)
            ),
            Style::default()
                .fg(crate::ui::theme::fg())
                .add_modifier(Modifier::BOLD),
        )]));
    }

    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TimeLogState;
    use gitlab_tracker_core::{Activity, LinkedTicket, TimeEntry, LINKED_TICKET_SCHEMA_VERSION};

    fn ticket() -> LinkedTicket {
        LinkedTicket {
            schema_version: LINKED_TICKET_SCHEMA_VERSION,
            id: "4271".into(),
            subject: "Expose MR metadata".into(),
            status: "In Progress".into(),
            url: "https://tracker.example/issues/4271".into(),
            author: Some("Marina".into()),
            assignee: Some("Thomas".into()),
            time_estimate: Some(28_800), // 8 h
            time_spent: Some(18_000),    // 5 h
            time_remaining: Some(10_800),
            tracker_type: Some("Evolution".into()),
            priority: Some("High".into()),
            version: Some("v2.4.0".into()),
            start_date: Some("2024-04-15".into()),
            done_ratio: Some(65),
        }
    }

    fn mr_with(ticket: Option<LinkedTicket>) -> TrackedMr {
        let mut mr = TrackedMr::placeholder("104".into(), "feat: GraphQL".into(), None);
        mr.linked_ticket = ticket;
        mr
    }

    fn entry(hours: f32, comment: &str) -> TimeEntry {
        TimeEntry {
            id: 1,
            hours,
            activity: Activity {
                id: 9,
                name: "Development".into(),
            },
            comment: comment.into(),
            user: "Thomas".into(),
            spent_on: "2024-04-16".into(),
        }
    }

    fn rows(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// The row starting with `label`, as text.
    fn row(text: &Text<'_>, label: &str) -> String {
        rows(text)
            .into_iter()
            .find(|row| row.starts_with(label))
            .unwrap_or_else(|| panic!("no `{label}` row in {:#?}", rows(text)))
    }

    /// Foreground of the first value span of the row starting with `label`.
    fn value_colour(text: &Text<'_>, label: &str) -> Option<Color> {
        let line = text
            .lines
            .iter()
            .find(|line| line.spans[0].content.starts_with(label))
            .unwrap_or_else(|| panic!("no `{label}` row"));
        line.spans[1].style.fg
    }

    #[test]
    fn durations_are_shown_as_hours_and_minutes() {
        assert_eq!(format_duration(0), "—");
        assert_eq!(format_duration(59), "0m", "under a minute");
        assert_eq!(format_duration(2_700), "45m");
        assert_eq!(format_duration(7_200), "2h");
        assert_eq!(format_duration(5_400), "1h 30m");
        assert_eq!(fmt_hours(0.25), "15m");
        assert_eq!(fmt_hours(1.5), "1h 30m");
        // Rounded to the minute, not truncated.
        assert_eq!(fmt_hours(0.999), "1h");
    }

    #[test]
    fn label_colours_resolve_exact_then_wildcard_then_default() {
        let bug = (Color::Red, Color::White);
        let other = (Color::Blue, Color::Black);
        let mut colors = TrackerLabelColors {
            tracker_type: HashMap::from([("bug".to_string(), bug), ("*".to_string(), other)]),
            // A key that was not lowercased at load time still matches.
            priority: HashMap::from([("Haute".to_string(), bug)]),
        };
        assert_eq!(colors.get_tracker_type_color("Bug"), bug);
        assert_eq!(colors.get_tracker_type_color("Evolution"), other);
        assert_eq!(colors.get_priority_color("haute"), bug);
        assert_eq!(
            colors.get_priority_color("Basse"),
            (theme::muted_dim(), Color::White)
        );
        colors.tracker_type.clear();
        assert_eq!(
            colors.get_tracker_type_color("Bug"),
            (theme::muted_dim(), Color::White)
        );
    }

    #[test]
    fn ticket_pane_lists_the_ticket_fields() {
        let none = render_ticket_info(&mr_with(None), &TrackerLabelColors::default());
        assert_eq!(rows(&none), ["No linked ticket detected."]);

        let colors = TrackerLabelColors {
            tracker_type: HashMap::from([("evolution".to_string(), (Color::Blue, Color::Black))]),
            priority: HashMap::new(),
        };
        let text = render_ticket_info(&mr_with(Some(ticket())), &colors);
        assert_eq!(rows(&text)[0], "#4271  Expose MR metadata");
        assert_eq!(row(&text, "Type"), "Type     :  Evolution ");
        assert_eq!(row(&text, "Priority"), "Priority :  High ");
        assert_eq!(row(&text, "Status"), "Status   : In Progress");
        assert_eq!(row(&text, "Author"), "Author   : Marina");
        assert_eq!(row(&text, "Assignee"), "Assignee : Thomas");
        assert_eq!(row(&text, "Version"), "Version  : v2.4.0");
        assert!(row(&text, "Start").starts_with("Start    : 2024-04-15  ("));
        // 65 % of a 20-cell bar.
        let progress = row(&text, "Progress");
        assert_eq!(progress.matches('█').count(), 13);
        assert_eq!(progress.matches('░').count(), 7);
        assert!(progress.ends_with("65%"));
        assert_eq!(row(&text, "Estimate"), "Estimate : 8h");
        assert_eq!(row(&text, "Spent"), "Spent    : 5h");
        assert_eq!(row(&text, "Remaining"), "Remaining: 3h");

        let type_badge = &text
            .lines
            .iter()
            .find(|l| l.spans[0].content.starts_with("Type"))
            .unwrap()
            .spans[1];
        assert_eq!(
            (type_badge.style.bg, type_badge.style.fg),
            (Some(Color::Blue), Some(Color::Black))
        );
    }

    #[test]
    fn ticket_pane_skips_what_the_tracker_did_not_provide() {
        let bare = LinkedTicket {
            author: None,
            assignee: None,
            time_estimate: None,
            time_spent: Some(0),
            time_remaining: None,
            tracker_type: None,
            priority: None,
            version: None,
            start_date: None,
            done_ratio: None,
            ..ticket()
        };
        let text = render_ticket_info(&mr_with(Some(bare)), &TrackerLabelColors::default());
        assert_eq!(row(&text, "Author"), "Author   : Unknown");
        assert_eq!(row(&text, "Assignee"), "Assignee : Unassigned");
        let labels = rows(&text).join("\n");
        for absent in [
            "Type",
            "Priority",
            "Version",
            "Start",
            "Progress",
            "Estimate",
            "Spent",
            "Remaining",
        ] {
            assert!(
                !labels.contains(&format!("{absent} ")),
                "`{absent}` row shown:\n{labels}"
            );
        }
    }

    #[test]
    fn spent_time_and_progress_colours_follow_the_budget() {
        let with = |estimate: Option<u32>, spent: u32, done: u32| {
            let ticket = LinkedTicket {
                time_estimate: estimate,
                time_spent: Some(spent),
                done_ratio: Some(done),
                ..ticket()
            };
            render_ticket_info(&mr_with(Some(ticket)), &TrackerLabelColors::default())
        };
        assert_eq!(
            value_colour(&with(Some(1000), 799, 50), "Spent"),
            Some(Color::Green)
        );
        assert_eq!(
            value_colour(&with(Some(1000), 800, 50), "Spent"),
            Some(Color::Yellow)
        );
        assert_eq!(
            value_colour(&with(Some(1000), 1000, 50), "Spent"),
            Some(Color::Red)
        );
        // No estimate to compare with: neutral.
        assert_eq!(
            value_colour(&with(None, 5000, 50), "Spent"),
            Some(Color::Cyan)
        );

        assert_eq!(
            value_colour(&with(None, 1, 74), "Progress"),
            Some(Color::Yellow)
        );
        assert_eq!(
            value_colour(&with(None, 1, 75), "Progress"),
            Some(Color::Cyan)
        );
        assert_eq!(
            value_colour(&with(None, 1, 100), "Progress"),
            Some(Color::Green)
        );
    }

    #[test]
    fn time_log_shows_its_fetch_state() {
        let mr = mr_with(Some(ticket()));
        for state in [None, Some(TimeLogState::Loading)] {
            let text = render_time_log(&mr, state.as_ref(), Color::Gray);
            assert_eq!(rows(&text)[0], "Time Log — MR !104");
            assert_eq!(rows(&text).last().unwrap(), "Loading time entries…");
        }

        let failed = TimeLogState::Failed("401 Unauthorized".into());
        let text = render_time_log(&mr, Some(&failed), Color::Gray);
        let all = rows(&text);
        assert_eq!(
            all[all.len() - 2],
            "Failed to load time entries: 401 Unauthorized"
        );
        assert!(all[all.len() - 1].contains("[R]"));

        let empty = TimeLogState::Loaded(Vec::new());
        let text = render_time_log(&mr, Some(&empty), Color::Gray);
        assert_eq!(rows(&text).last().unwrap(), "No time entries recorded yet.");
    }

    #[test]
    fn time_log_lists_entries_with_their_total() {
        let state = TimeLogState::Loaded(vec![entry(1.5, "Schema design"), entry(0.75, "")]);
        let text = render_time_log(&mr_with(Some(ticket())), Some(&state), Color::Gray);
        let all = rows(&text);
        let first = all
            .iter()
            .position(|row| row.starts_with("2024-04-16"))
            .unwrap();
        assert_eq!(all[first], "2024-04-16  1h 30m  Development  Thomas");
        assert_eq!(all[first + 1], "  \"Schema design\"");
        // No comment: no quoted line under the second entry.
        assert_eq!(all[first + 2], "2024-04-16  45m  Development  Thomas");
        assert_eq!(all.iter().filter(|row| row.contains('"')).count(), 1);
        assert_eq!(all.last().unwrap(), "Total: 2 entries — 2h 15m logged");
        let comment = &text.lines[first + 1].spans[1];
        assert_eq!(comment.style.fg, Some(Color::Gray));
    }

    #[test]
    fn time_log_header_compares_spent_time_with_the_estimate() {
        let header = |estimate: Option<u32>, spent: u32, state: Option<&TimeLogState>| {
            let ticket = LinkedTicket {
                time_estimate: estimate,
                time_spent: Some(spent),
                ..ticket()
            };
            render_time_log(&mr_with(Some(ticket)), state, Color::Gray)
        };
        // 5 h of 8 h: 63 %, 19 of 30 cells.
        let text = header(Some(28_800), 18_000, None);
        assert_eq!(row(&text, "Estimate"), "Estimate : 8h");
        let spent = row(&text, "Spent");
        assert!(
            spent.starts_with("Spent    : 5h") && spent.ends_with("63%"),
            "{spent}"
        );
        assert_eq!(spent.matches('█').count(), 19);
        assert_eq!(value_colour(&text, "Spent"), Some(Color::Green));

        assert_eq!(
            value_colour(&header(Some(1000), 800, None), "Spent"),
            Some(Color::Yellow)
        );
        // Over budget: the bar is full, the percentage is not capped.
        let over = header(Some(1000), 1500, None);
        assert_eq!(value_colour(&over, "Spent"), Some(Color::Red));
        let spent = row(&over, "Spent");
        assert!(spent.ends_with("150%") && !spent.contains('░'), "{spent}");

        // No estimate: the total comes from the loaded entries.
        let state = TimeLogState::Loaded(vec![entry(2.0, ""), entry(0.5, "")]);
        assert_eq!(
            row(&header(None, 0, Some(&state)), "Total"),
            "Total    : 2h 30m"
        );
        assert_eq!(row(&header(None, 0, None), "Total"), "Total    : 0m");
        // No ticket at all: no header, straight to the entries section.
        let text = render_time_log(&mr_with(None), None, Color::Gray);
        assert!(!rows(&text)
            .iter()
            .any(|row| row.starts_with("Estimate") || row.starts_with("Total")));
    }
}
