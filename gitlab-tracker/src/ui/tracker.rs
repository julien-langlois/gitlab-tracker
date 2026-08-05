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
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::collections::HashMap;

// ── Colour maps ───────────────────────────────────────────────────────────────

/// Colour maps (background + foreground) for tracker badge labels shown in the
/// Tracker pane.
///
/// Populated at startup from the active tracker's config file (e.g. `redmine.yaml`)
/// and forwarded to renderers so they stay fully agnostic of each tracker's data model.
///
/// Any future tracker plugin (Jira, Linear, …) simply fills the same two maps —
/// no change to the renderers is needed.
///
/// `#[allow(dead_code)]` suppresses Clippy false-positives in feature-less builds
/// where no tracker is compiled in.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct TrackerLabelColors {
    /// Colour map for tracker-type labels (e.g. "Bug", "Evolution").
    /// Keys are stored lowercased; `"*"` is a catch-all fallback.
    pub tracker_type: HashMap<String, (Color, Color)>,
    /// Colour map for priority labels (e.g. "Normale", "Haute").
    /// Keys are stored lowercased; `"*"` is a catch-all fallback.
    pub priority: HashMap<String, (Color, Color)>,
}

#[allow(dead_code)]
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
        (theme::MUTED_DIM, Color::White)
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

/// Formats a duration in seconds as "Xh Ym". Returns "—" for zero.
fn format_duration(secs: u32) -> String {
    if secs == 0 {
        return "—".to_string();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    match (h, m) {
        (0, m) => format!("{}m", m),
        (h, 0) => format!("{}h", h),
        (h, m) => format!("{}h {}m", h, m),
    }
}

/// Formats hours (f32) as "Xh Ym" for display in time-log entries.
fn fmt_hours(hours: f32) -> String {
    let total_mins = (hours * 60.0).round() as u32;
    let h = total_mins / 60;
    let m = total_mins % 60;
    match (h, m) {
        (0, m) => format!("{}m", m),
        (h, 0) => format!("{}h", h),
        (h, m) => format!("{}h {}m", h, m),
    }
}

// ── Renderers ─────────────────────────────────────────────────────────────────

/// Renders the Tracker pane (lower-right) — ticket details for the linked ticket.
///
/// Contains: ID, subject, type/priority badges, status, assignees, version,
/// start date, progress bar, and time tracking (estimate/spent/remaining).
///
/// `tracker_colors` is only exercised when a tracker feature flag is compiled in
/// and a ticket is linked. `#[allow(unused_variables)]` suppresses Clippy
/// false-positives in feature-less builds.
#[allow(unused_variables)]
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
                .fg(Color::White)
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
        lines.push(Line::from(vec![
            Span::raw("Start    : "),
            Span::styled(start.clone(), Style::default().fg(theme::MUTED)),
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
    let has_estimate = ticket.time_estimate.map(|v| v > 0).unwrap_or(false);
    let has_spent = ticket.time_spent.map(|v| v > 0).unwrap_or(false);
    let has_remaining = ticket.time_remaining.map(|v| v > 0).unwrap_or(false);

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
                        .fg(theme::MUTED)
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
    entries: &[gitlab_tracker_core::TimeEntry],
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
            let total_hours: f32 = entries.iter().map(|e| e.hours).sum();
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
        Span::styled("── ", Style::default().fg(theme::MUTED_DIM)),
        Span::styled(
            "Entries",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ──────────────────────────────",
            Style::default().fg(theme::MUTED_DIM),
        ),
    ]));

    if entries.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No time entries recorded yet.",
            Style::default().fg(theme::MUTED),
        )]));
    } else {
        for entry in entries {
            // Line 1: date | duration | activity | user
            lines.push(Line::from(vec![
                Span::styled(entry.spent_on.clone(), Style::default().fg(theme::MUTED)),
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
                Span::styled(entry.user.clone(), Style::default().fg(Color::White)),
            ]));
            // Line 2: comment (indented), shown only when non-empty.
            if !entry.comment.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("\"{}\"", entry.comment),
                        Style::default().fg(theme::MUTED_COMMENT),
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
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));
    }

    Text::from(lines)
}
