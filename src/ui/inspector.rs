use crate::config::AppConfig;
use crate::models::{GitlabMrState, MergeabilityStatus, Pipeline, PipelineState, TrackedMr};
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

pub fn render_safe_inspector_text(mr: &TrackedMr, config: &AppConfig) -> Text<'static> {
    // Format ISO 8601 timestamp to a more readable form (date + time, drop sub-seconds).
    let updated_at_display = mr
        .updated_at
        .as_deref()
        .map(|s| s.get(..19).unwrap_or(s).replace('T', " "))
        .unwrap_or_else(|| "Unknown".to_string());

    // Compute the activity badge (icon + color) based on configurable thresholds.
    let (badge_icon, badge_color) = config.activity_badge(mr.updated_at.as_deref());

    // Build the state badge using the same colour coding as the table column.
    // All labels are padded to 9 chars to match the table badge width.
    let (state_label, state_fg, state_bg) = match mr.state {
        GitlabMrState::Opened => ("  Open   ", Color::Black, Color::Green),
        GitlabMrState::Merged => (" Merged  ", Color::Black, Color::Magenta),
        GitlabMrState::Closed => (" Closed  ", Color::Black, Color::Red),
    };

    // For open MRs, display the mergeability status directly — no animation
    // needed here since the State field above already shows the MR lifecycle
    // state.
    let mergeability_line: Option<Line<'static>> = if mr.state == GitlabMrState::Opened {
        // Each label is wrapped with a single space on each side via format!(" {text} ").
        let (merge_text, merge_fg, merge_bg) = match mr.mergeability {
            MergeabilityStatus::Mergeable => ("Mergeable", Color::Black, Color::LightGreen),
            MergeabilityStatus::Conflict => ("Conflict", Color::White, Color::Red),
            MergeabilityStatus::NeedsRebase => ("Rebase", Color::Black, Color::Yellow),
            MergeabilityStatus::Unknown => ("Unknown", Color::DarkGray, Color::Black),
        };
        Some(Line::from(vec![
            Span::raw("Merge    : "),
            Span::styled(
                format!(" {merge_text} "),
                Style::default()
                    .fg(merge_fg)
                    .bg(merge_bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
    } else {
        None
    };

    let mut lines = vec![
        Line::from(vec![Span::styled(
            format!("MR ID    : !{}", mr.id),
            Style::default()
                .fg(ratatui::style::Color::Cyan)
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
    ];

    if let Some(line) = mergeability_line {
        lines.push(line);
    }
    lines.push(Line::from(vec![
        Span::raw("Author   : "),
        Span::styled(
            format!("@{}", mr.author),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Assignee : "),
        Span::styled(
            format!("@{}", mr.assignee),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Milestone: "),
        Span::styled(
            mr.milestone.clone(),
            Style::default().fg(ratatui::style::Color::Cyan),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Updated  : "),
        Span::styled(
            updated_at_display,
            Style::default().fg(ratatui::style::Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            badge_icon,
            Style::default()
                .fg(badge_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if !mr.labels.is_empty() {
        let mut label_spans = vec![Span::raw("Labels   : ")];
        for label in &mr.labels {
            label_spans.push(create_chip_span(label, config));
            label_spans.push(Span::raw(" "));
        }
        lines.push(Line::from(label_spans));
    } else {
        lines.push(Line::from(vec![
            Span::raw("Labels   : "),
            Span::styled("None", Style::default().dark_gray()),
        ]));
    }

    lines.push(Line::from(vec![Span::styled(
        "───────────────────────────────────",
        Style::default().fg(ratatui::style::Color::DarkGray),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "Description:",
        Style::default()
            .fg(ratatui::style::Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));
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
