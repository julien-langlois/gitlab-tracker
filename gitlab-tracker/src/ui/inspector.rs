use crate::config::AppConfig;
use crate::models::{GitlabMrState, MergeabilityStatus, Pipeline, PipelineState, TrackedMr};
use crate::ui::table::badge_label;
use crate::ui::theme;
use crate::utils::format_relative_date;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

/// Renders the pipeline list view for the Inspector panel.
///
/// Shows the last fetched pipelines for the selected MR with their jobs,
/// grouped by pipeline run. Displayed when the user presses [P].
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
            Style::default().fg(theme::MUTED),
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

    // Format the creation timestamp as "YYYY-MM-DD HH:MM  (relative label)".
    let date_display = pipeline
        .created_at
        .as_deref()
        .map(|s| {
            let absolute = s.get(..16).unwrap_or(s).replace('T', " ");
            let relative = format_relative_date(s);
            format!("{}  ({})", absolute, relative)
        })
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
            Style::default().fg(theme::MUTED_DIM),
        ),
    ])];

    if pipeline.jobs.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No jobs.",
            Style::default().fg(theme::MUTED),
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
        PipelineState::Canceled => ("⊘ canceled", theme::MUTED_INACTIVE),
        PipelineState::Skipped => ("⊝ skipped", theme::MUTED_INACTIVE),
        PipelineState::Created => ("○ created", Color::White),
        PipelineState::Unknown => ("? unknown", theme::MUTED_INACTIVE),
    }
}

/// Maps a job status string (as returned by the GitLab API) to icon + colour.
fn job_status_style(status: &str) -> (&'static str, Color) {
    match status {
        "success" => ("✔", Color::Green),
        "failed" => ("✘", Color::Red),
        "running" => ("⟳", Color::Cyan),
        "pending" => ("◔", Color::Yellow),
        "canceled" => ("⊘", theme::MUTED_INACTIVE),
        "skipped" => ("⊝", theme::MUTED_INACTIVE),
        "created" => ("○", Color::White),
        _ => ("?", theme::MUTED_INACTIVE),
    }
}

/// Builds a coloured chip `Span` for a label.
///
/// Colour resolution order:
/// 1. Exact or wildcard override from `config.json`
/// 2. GitLab-side hex colour (`gitlab_color`) with auto-computed foreground
/// 3. Generic dark-gray fallback
pub fn create_chip_span(
    label: &str,
    config: &AppConfig,
    gitlab_color: Option<&str>,
) -> Span<'static> {
    let (bg, fg) = config.get_label_style(label, gitlab_color);
    Span::styled(
        format!(" {} ", label),
        Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
    )
}

/// Renders a section header separator with a coloured title.
fn section_header(title: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled("── ", Style::default().fg(theme::MUTED_DIM)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ──────────────────────────────",
            Style::default().fg(theme::MUTED_DIM),
        ),
    ])
}

pub fn render_safe_inspector_text(mr: &TrackedMr, config: &AppConfig) -> Text<'static> {
    // Format the updated_at timestamp as "YYYY-MM-DD HH:MM  (relative label)".
    let updated_at_display = mr
        .updated_at
        .as_deref()
        .map(|s| {
            let absolute = s.get(..16).unwrap_or(s).replace('T', " ");
            let relative = format_relative_date(s);
            format!("{}  ({})", absolute, relative)
        })
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
            Span::styled(mr.target_branch.clone(), Style::default().fg(theme::MUTED)),
        ]),
        Line::from(vec![
            Span::raw("Clone    : "),
            Span::styled(git_clone_cmd.clone(), Style::default().fg(theme::MUTED)),
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
            Span::styled(mr.web_url.clone(), Style::default().fg(theme::MUTED)),
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
            Span::styled("None", Style::default().fg(theme::MUTED)),
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
        ("  ✔ No comments  ".to_string(), theme::MUTED, Color::Black)
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

    // Milestone due date — show absolute date + relative label, with urgency colouring.
    // The due_date field is YYYY-MM-DD (date only), so we append T00:00:00Z for parsing.
    let (due_text, due_color) = match mr.milestone_due_date.as_deref() {
        None | Some("") => ("Not set".to_string(), Color::DarkGray),
        Some(date) => {
            // Colour the date based on proximity: red if past, yellow if within 7 days,
            // green otherwise. Lexicographic comparison works for YYYY-MM-DD format.
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
            // Append T00:00:00Z so format_relative_date can parse it as a full timestamp.
            let iso_for_relative = format!("{}T00:00:00Z", date);
            let relative = format_relative_date(&iso_for_relative);
            (format!("{}  ({})", date, relative), color)
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
        // Format the merged_at timestamp as "YYYY-MM-DD HH:MM  (relative label)".
        let merged_at_display = mr
            .merged_at
            .as_deref()
            .map(|s| {
                let absolute = s.get(..16).unwrap_or(s).replace('T', " ");
                let relative = format_relative_date(s);
                format!("{}  ({})", absolute, relative)
            })
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
            // Look up the GitLab-side hex colour for this label (passed via `gitlab_label_colors`
            // stored in App). Falls back to config overrides then generic gray if absent.
            let gitlab_color = config
                .gitlab_label_colors
                .get(&label.to_lowercase())
                .map(|s| s.as_str());
            label_spans.push(create_chip_span(label, config, gitlab_color));
            label_spans.push(Span::raw(" "));
        }
        lines.push(Line::from(label_spans));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "None",
            Style::default().fg(theme::MUTED),
        )]));
    }

    // ── Description ──────────────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::raw("")]));
    lines.push(section_header("Description"));
    lines.push(Line::from(vec![Span::raw("")]));

    if mr.description.trim().is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No description text provided.",
            Style::default().fg(theme::MUTED),
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
