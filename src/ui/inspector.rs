use crate::config::AppConfig;
use crate::models::TrackedMr;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

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

    let mut lines = vec![
        Line::from(vec![Span::styled(
            format!("MR ID    : !{}", mr.id),
            Style::default()
                .fg(ratatui::style::Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![
            Span::raw("Author   : "),
            Span::styled(
                format!("@{}", mr.author),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("Assignee : "),
            Span::styled(
                format!("@{}", mr.assignee),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("Milestone: "),
            Span::styled(
                mr.milestone.clone(),
                Style::default().fg(ratatui::style::Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::raw("Updated  : "),
            Span::styled(
                updated_at_display,
                Style::default().fg(ratatui::style::Color::Yellow),
            ),
        ]),
    ];

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
