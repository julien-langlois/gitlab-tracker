use crate::config::AppConfig;
use crate::models::{GitlabMrState, MergeabilityStatus, Pipeline, PipelineState, TrackedMr};
use crate::ui::table::badge_label;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::collections::HashMap;

/// Colour maps (background + foreground) for tracker badge labels shown in the Inspector.
///
/// Populated at startup from the active tracker's config file (e.g. `redmine.yaml`)
/// and forwarded to `render_safe_inspector_text` so the renderer stays fully agnostic
/// of every tracker's internal data model.
///
/// Any future tracker plugin (Jira, Linear, …) simply fills the same two maps —
/// no change to the renderer is needed.
///
/// Fields and methods are used only when a tracker feature flag is enabled at build
/// time, so `#[allow]` suppresses Clippy false-positives in feature-less builds.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct TrackerLabelColors {
    /// Colour map for tracker-type labels (e.g. "Bug", "Evolution").
    /// Keys are stored lowercased; `"*"` is a catch-all fallback.
    pub tracker_type: HashMap<String, (Color, Color)>,
    /// Colour map for priority labels (e.g. "Normal", "High").
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
        (Color::DarkGray, Color::White)
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

/// Renders the pipeline list view for the Inspector panel.
///
/// Shows the last fetched pipelines for the selected MR with their jobs,
/// grouped by pipeline run. Displayed when the user presses [P].
/// Renders the Time Log view for the Inspector panel (redmine feature only).
///
/// Shows a progress bar (spent vs estimate), then the list of time entries
/// fetched from Redmine for the linked ticket.
pub fn render_time_log_text(
    mr: &TrackedMr,
    entries: &[gitlab_tracker_core::TimeEntry],
) -> ratatui::text::Text<'static> {
    /// Formats hours (f32) as "Xh Ym" for display.
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

    /// Formats seconds (u32) as "Xh Ym" for display.
    fn fmt_secs(secs: u32) -> String {
        fmt_hours(secs as f32 / 3600.0)
    }

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
                    fmt_secs(estimate_secs),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("Spent    : "),
                Span::styled(
                    fmt_secs(spent_secs),
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
        Span::styled("── ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Entries",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ──────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    if entries.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No time entries recorded yet.",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        for entry in entries {
            // Line 1: date | duration | activity | user
            lines.push(Line::from(vec![
                Span::styled(entry.spent_on.clone(), Style::default().fg(Color::DarkGray)),
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
                        Style::default().fg(Color::DarkGray),
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

    ratatui::text::Text::from(lines)
}

pub fn render_pipelines_text(mr: &TrackedMr) -> Text<'static> {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            format!("Pipelines — MR !{}", mr.id),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![Span::raw("")]),
    ];

    if mr.pipelines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No pipelines found for this MR.",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        for pipeline in &mr.pipelines {
            lines.extend(render_pipeline_block(pipeline));
            lines.push(Line::from(vec![Span::raw("")]));
        }
    }

    Text::from(lines)
}

/// Renders a single pipeline block with its status header and job list.
fn render_pipeline_block(pipeline: &Pipeline) -> Vec<Line<'static>> {
    let (status_icon, status_color) = pipeline_status_style(&pipeline.status);

    // Format the creation timestamp to a compact readable form (drop sub-seconds and timezone).
    let date_display = pipeline
        .created_at
        .as_deref()
        .map(|s| s.get(..19).unwrap_or(s).replace('T', " "))
        .unwrap_or_else(|| "unknown date".to_string());

    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("#{} ", pipeline.id),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            status_icon,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", date_display),
            Style::default().fg(Color::DarkGray),
        ),
    ])];

    if pipeline.jobs.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No jobs.",
            Style::default().fg(Color::DarkGray),
        )]));
        return lines;
    }

    // Group jobs by stage for readability.
    let mut current_stage = String::new();
    for job in &pipeline.jobs {
        if job.stage != current_stage {
            current_stage = job.stage.clone();
            lines.push(Line::from(vec![Span::styled(
                format!("  ▸ {}", current_stage),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
        }

        let (job_icon, job_color) = job_status_style(&job.status);
        let duration = job
            .duration
            .map(|d| format!(" ({:.0}s)", d))
            .unwrap_or_default();

        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(job_icon, Style::default().fg(job_color)),
            Span::raw(format!(" {}{}", job.name, duration)),
        ]));
    }

    lines
}

/// Maps a `PipelineState` to a display icon and its colour.
fn pipeline_status_style(state: &PipelineState) -> (&'static str, Color) {
    match state {
        PipelineState::Success => ("✔ passed", Color::Green),
        PipelineState::Failed => ("✘ failed", Color::Red),
        PipelineState::Running => ("⟳ running", Color::Cyan),
        PipelineState::Pending => ("◔ pending", Color::Yellow),
        PipelineState::Canceled => ("⊘ canceled", Color::DarkGray),
        PipelineState::Skipped => ("⊝ skipped", Color::DarkGray),
        PipelineState::Created => ("○ created", Color::White),
        PipelineState::Unknown => ("? unknown", Color::DarkGray),
    }
}

/// Maps a job status string (as returned by the GitLab API) to icon + colour.
fn job_status_style(status: &str) -> (&'static str, Color) {
    match status {
        "success" => ("✔", Color::Green),
        "failed" => ("✘", Color::Red),
        "running" => ("⟳", Color::Cyan),
        "pending" => ("◔", Color::Yellow),
        "canceled" => ("⊘", Color::DarkGray),
        "skipped" => ("⊝", Color::DarkGray),
        "created" => ("○", Color::White),
        _ => ("?", Color::DarkGray),
    }
}

pub fn create_chip_span(label: &str, config: &AppConfig) -> Span<'static> {
    let (bg, fg) = config.get_label_style(label);
    Span::styled(
        format!(" {} ", label),
        Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
    )
}

/// Renders a section header separator with a coloured title.
fn section_header(title: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled("── ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ──────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

// `tracker_colors` is only exercised when a tracker feature (e.g. `redmine`) is compiled in
// and a ticket is linked. Clippy sees it as unused in feature-less builds — suppressed here
// rather than prefixing with `_` so call sites stay readable.
#[allow(unused_variables)]
pub fn render_safe_inspector_text(
    mr: &TrackedMr,
    config: &AppConfig,
    tracker_colors: &TrackerLabelColors,
) -> Text<'static> {
    // Format ISO 8601 timestamp to a more readable form (date + time, drop sub-seconds).
    let updated_at_display = mr
        .updated_at
        .as_deref()
        .map(|s| s.get(..19).unwrap_or(s).replace('T', " "))
        .unwrap_or_else(|| "Unknown".to_string());

    // Compute the activity badge (icon + color) based on configurable thresholds.
    let (badge_icon, badge_color) = config.activity_badge(mr.updated_at.as_deref());

    // Build the state badge using the same colour coding as the table column.
    // badge_label() centers the text to BADGE_WIDTH chars — no manual padding needed.
    let (state_text, state_fg, state_bg) = match mr.state {
        GitlabMrState::Opened => ("OPEN", Color::Black, Color::Green),
        GitlabMrState::Merged => ("MERGED", Color::Black, Color::Magenta),
        GitlabMrState::Closed => ("CLOSED", Color::Black, Color::Red),
    };
    let state_label = badge_label(state_text);

    // Build the git clone command for the source branch — used in the [Y]ank hint.
    // Derives the SSH clone URL from the web URL: replaces the HTTPS scheme and host
    // with the git@ SSH equivalent (standard GitLab convention).
    let git_clone_cmd = {
        // e.g. "https://gitlab.com/org/project/-/merge_requests/42"
        //   -> "git clone -b feat/my-branch git@gitlab.com:org/project.git"
        let ssh_url = mr
            .web_url
            .split("/-/")
            .next()
            .unwrap_or("")
            .replacen("https://", "git@", 1)
            .replacen('/', ":", 1);
        format!("git clone -b {} {}.git", mr.source_branch, ssh_url)
    };

    // ── SECTION 1: Identity ──────────────────────────────────────────────────
    let mut lines = vec![
        section_header("Identity"),
        Line::from(vec![Span::styled(
            format!("MR ID    : !{}", mr.id),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![
            Span::raw("State    : "),
            Span::styled(
                state_label,
                Style::default()
                    .fg(state_fg)
                    .bg(state_bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("Branch   : "),
            Span::styled(
                mr.source_branch.clone(),
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  →  "),
            Span::styled(
                mr.target_branch.clone(),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::raw("Clone    : "),
            Span::styled(git_clone_cmd.clone(), Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(
                "[Y] copy",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("URL      : "),
            Span::styled(mr.web_url.clone(), Style::default().fg(Color::DarkGray)),
        ]),
    ];

    // ── SECTION 2: People ────────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::raw("")]));
    lines.push(section_header("People"));
    lines.push(Line::from(vec![
        Span::raw("Author   : "),
        Span::styled(
            mr.author.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Assignee : "),
        Span::styled(
            mr.assignee.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));

    // Reviewers: listed inline, or dimmed "None" if empty.
    if mr.reviewers.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("Reviewers: "),
            Span::styled("None", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        for (i, reviewer) in mr.reviewers.iter().enumerate() {
            let label = if i == 0 { "Reviewers: " } else { "           " };
            lines.push(Line::from(vec![
                Span::raw(label),
                Span::styled(
                    reviewer.clone(),
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    // Notes / comments indicator — always shown, highlights when non-zero.
    let (notes_text, notes_fg, notes_bg) = if mr.user_notes_count == 0 {
        (
            "  ✔ No comments  ".to_string(),
            Color::DarkGray,
            Color::Black,
        )
    } else {
        (
            format!("  💬 {} comment(s) — review pending  ", mr.user_notes_count),
            Color::Black,
            Color::Yellow,
        )
    };
    lines.push(Line::from(vec![
        Span::raw("Notes    : "),
        Span::styled(
            notes_text,
            Style::default()
                .fg(notes_fg)
                .bg(notes_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // ── SECTION 3: Planning ──────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::raw("")]));
    lines.push(section_header("Planning"));
    lines.push(Line::from(vec![
        Span::raw("Milestone: "),
        Span::styled(mr.milestone.clone(), Style::default().fg(Color::Cyan)),
    ]));

    // Milestone due date — show with urgency colouring when set.
    let (due_text, due_color) = match mr.milestone_due_date.as_deref() {
        None | Some("") => ("Not set".to_string(), Color::DarkGray),
        Some(date) => {
            // Colour the date based on proximity: red if past, yellow if within 7 days,
            // green otherwise. We do a simple lexicographic comparison against today's date
            // (YYYY-MM-DD format sorts correctly without parsing).
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let in_7_days = (chrono::Utc::now() + chrono::Duration::days(7))
                .format("%Y-%m-%d")
                .to_string();
            let color = if date < today.as_str() {
                Color::Red
            } else if date <= in_7_days.as_str() {
                Color::Yellow
            } else {
                Color::Green
            };
            (date.to_string(), color)
        }
    };
    lines.push(Line::from(vec![
        Span::raw("Due date : "),
        Span::styled(
            due_text,
            Style::default().fg(due_color).add_modifier(Modifier::BOLD),
        ),
    ]));

    // ── SECTION 4: Status ────────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::raw("")]));
    lines.push(section_header("Status"));

    // Mergeability — only meaningful for open MRs.
    // badge_label() centers the text to BADGE_WIDTH, matching the State badge width.
    if mr.state == GitlabMrState::Opened {
        let (merge_text, merge_fg, merge_bg) = match mr.mergeability {
            MergeabilityStatus::Mergeable => ("MERGEABLE", Color::Black, Color::LightGreen),
            MergeabilityStatus::Conflict => ("CONFLICT", Color::White, Color::Red),
            MergeabilityStatus::NeedsRebase => ("REBASE", Color::Black, Color::Yellow),
            MergeabilityStatus::NotOpen => ("CLOSED", Color::Black, Color::Red),
            MergeabilityStatus::Draft => ("DRAFT", Color::White, Color::Rgb(80, 80, 80)),
            MergeabilityStatus::DiscussionsNotResolved => {
                ("DISCUSSIONS", Color::Black, Color::LightMagenta)
            }
            MergeabilityStatus::CiMustPass => ("CI MUST PASS", Color::Black, Color::LightYellow),
            MergeabilityStatus::CiStillRunning => ("CI STILL RUNNING", Color::Black, Color::Yellow),
            MergeabilityStatus::NotApproved => ("NOT APPROVED", Color::Black, Color::LightRed),
            MergeabilityStatus::RequestedChanges => ("REQUESTED CHANGES", Color::White, Color::Red),
            MergeabilityStatus::Unknown => ("UNKNOWN", Color::DarkGray, Color::Black),
        };
        lines.push(Line::from(vec![
            Span::raw("Merge    : "),
            Span::styled(
                badge_label(merge_text),
                Style::default()
                    .fg(merge_fg)
                    .bg(merge_bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // merged_by / merged_at — only shown for merged MRs.
    if mr.state == GitlabMrState::Merged {
        let merged_by_display = mr.merged_by.as_deref().unwrap_or("Unknown");
        let merged_at_display = mr
            .merged_at
            .as_deref()
            .map(|s| s.get(..19).unwrap_or(s).replace('T', " "))
            .unwrap_or_else(|| "Unknown".to_string());
        lines.push(Line::from(vec![
            Span::raw("Merged by: "),
            Span::styled(
                merged_by_display.to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw("Merged at: "),
            Span::styled(merged_at_display, Style::default().fg(Color::Magenta)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::raw("Updated  : "),
        Span::styled(updated_at_display, Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(
            badge_icon,
            Style::default()
                .fg(badge_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // ── SECTION 5: Labels ────────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::raw("")]));
    lines.push(section_header("Labels"));

    if !mr.labels.is_empty() {
        let mut label_spans: Vec<Span<'static>> = vec![];
        for label in &mr.labels {
            label_spans.push(create_chip_span(label, config));
            label_spans.push(Span::raw(" "));
        }
        lines.push(Line::from(label_spans));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "None",
            Style::default().dark_gray(),
        )]));
    }

    // ── Tracker ticket ───────────────────────────────────────────────────────
    // Rendered for any active tracker provider — field is None when none is configured.
    {
        /// Formats a duration in seconds as "Xh Ym" for display.
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

        lines.push(Line::from(vec![Span::raw("")]));
        lines.push(section_header("Tracker"));
        match &mr.linked_ticket {
            Some(ticket) => {
                // ── Ticket ID + subject ───────────────────────────────────────
                lines.push(Line::from(vec![
                    Span::raw("Ticket   : "),
                    Span::styled(
                        format!("#{}", ticket.id),
                        Style::default()
                            .fg(Color::LightMagenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(ticket.subject.clone(), Style::default().fg(Color::White)),
                ]));

                // ── Type badge — colour from tracker_colors (user-configurable) ──
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

                // ── Priority badge — colour from tracker_colors (user-configurable) ──
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

                // ── Status ────────────────────────────────────────────────────
                lines.push(Line::from(vec![
                    Span::raw("Status   : "),
                    Span::styled(
                        ticket.status.clone(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));

                // ── Author / Assignee ─────────────────────────────────────────
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

                // ── Target version / sprint ───────────────────────────────────
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

                // ── Start date ────────────────────────────────────────────────
                if let Some(start) = &ticket.start_date {
                    lines.push(Line::from(vec![
                        Span::raw("Start    : "),
                        Span::styled(start.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }

                // ── Progress bar (done_ratio) ─────────────────────────────────
                if let Some(ratio) = ticket.done_ratio {
                    let bar_width: usize = 28;
                    let filled = (ratio as f32 / 100.0 * bar_width as f32).round() as usize;
                    let empty = bar_width.saturating_sub(filled);
                    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
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

                // ── Time tracking ─────────────────────────────────────────────
                let has_estimate = ticket.time_estimate.map(|v| v > 0).unwrap_or(false);
                let has_spent = ticket.time_spent.map(|v| v > 0).unwrap_or(false);
                let has_remaining = ticket.time_remaining.map(|v| v > 0).unwrap_or(false);
                if has_estimate || has_spent || has_remaining {
                    // Colour the spent value relative to the estimate:
                    // green < 80 %, yellow 80–100 %, red when over budget.
                    let spent_color = match (ticket.time_estimate, ticket.time_spent) {
                        (Some(est), Some(spent)) if est > 0 => {
                            let ratio = spent as f32 / est as f32;
                            if ratio >= 1.0 {
                                Color::Red
                            } else if ratio >= 0.8 {
                                Color::Yellow
                            } else {
                                Color::Green
                            }
                        }
                        _ => Color::DarkGray,
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
                                    .fg(Color::DarkGray)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }
                }

                // ── URL ───────────────────────────────────────────────────────
                lines.push(Line::from(vec![
                    Span::raw("URL      : "),
                    Span::styled(ticket.url.clone(), Style::default().fg(Color::DarkGray)),
                    Span::raw("  "),
                    Span::styled(
                        "[T] open",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            None => {
                lines.push(Line::from(vec![Span::styled(
                    "No linked ticket detected.",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }
    }

    // ── Description ──────────────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::raw("")]));
    lines.push(section_header("Description"));
    lines.push(Line::from(vec![Span::raw("")]));

    if mr.description.trim().is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No description text provided.",
            Style::default().dark_gray(),
        )]));
        return Text::from(lines);
    }

    for raw_line in mr.description.lines() {
        let trimmed = raw_line.trim();

        if trimmed.starts_with("# ") || trimmed.starts_with("## ") {
            let text = trimmed.trim_start_matches('#').trim();
            lines.push(Line::from(vec![Span::styled(
                text.to_string(),
                Style::default()
                    .fg(ratatui::style::Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if trimmed.starts_with("### ") {
            let text = trimmed.trim_start_matches("### ");
            lines.push(Line::from(vec![Span::styled(
                text.to_string(),
                Style::default()
                    .fg(ratatui::style::Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if trimmed == "---" {
            lines.push(Line::from(vec![Span::styled(
                "───────────────────────────────────",
                Style::default().fg(ratatui::style::Color::DarkGray),
            )]));
        } else {
            let mut spans = Vec::new();
            let mut line_content = raw_line.to_string();
            if trimmed.starts_with("- ") {
                spans.push(Span::styled(
                    " • ",
                    Style::default().fg(ratatui::style::Color::Yellow),
                ));
                line_content = raw_line.replacen("- ", "", 1);
            } else if trimmed.starts_with("* ") {
                spans.push(Span::styled(
                    " • ",
                    Style::default().fg(ratatui::style::Color::Yellow),
                ));
                line_content = raw_line.replacen("* ", "", 1);
            }

            let bold_parts = line_content.split("**");
            let mut is_bold = false;

            for bold_part in bold_parts {
                let code_parts = bold_part.split('`');
                let mut is_code = false;

                for code_part in code_parts {
                    let mut style = Style::default();
                    if is_bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if is_code {
                        style = style.fg(ratatui::style::Color::Magenta);
                    }

                    spans.push(Span::styled(code_part.to_string(), style));
                    is_code = !is_code;
                }
                is_bold = !is_bold;
            }
            lines.push(Line::from(spans));
        }
    }

    Text::from(lines)
}
