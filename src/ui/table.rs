use crate::app::{App, SortColumn, SortOrder};
use crate::models::{GitlabMrState, MergeabilityStatus, MrStatus};
use crate::ui::inspector::create_chip_span;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Cell, Row, Table},
};

/// Fixed width (in chars) for all state badges, padding included.
/// "Mergeable" is 9 chars — add 2 chars of padding (1 each side) → 11.
const BADGE_WIDTH: usize = 11;

/// Centers `text` inside a field of exactly `BADGE_WIDTH` characters.
/// Excess space is distributed evenly left and right (left-biased on odd remainder).
fn badge_label(text: &str) -> String {
    // Use Rust's built-in centering formatter.
    format!("{:^width$}", text, width = BADGE_WIDTH)
}

/// Returns a styled badge cell for the GitLab MR state.
///
/// For open MRs, `tick` (the current `time_left` value) drives an alternating animation
/// between the base "Open" badge and a contextual mergeability badge:
///   - even tick  → "Open" centred in a fixed-width green badge
///   - odd tick   → mergeability-specific badge (color varies), same fixed width
///
/// All badges share the same `BADGE_WIDTH` so the column never shifts.
fn state_badge(
    state: &GitlabMrState,
    mergeability: &MergeabilityStatus,
    tick: u64,
) -> Cell<'static> {
    // For non-open states the badge is static — mergeability is not meaningful.
    if *state != GitlabMrState::Opened {
        let (text, fg, bg) = match state {
            GitlabMrState::Merged => ("Merged", Color::Black, Color::Magenta),
            GitlabMrState::Closed => ("Closed", Color::Black, Color::Red),
            GitlabMrState::Opened => unreachable!(),
        };
        return Cell::from(Line::from(Span::styled(
            badge_label(text),
            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
        )));
    }

    // Even tick: always show the base "Open" badge.
    if tick.is_multiple_of(2) {
        return Cell::from(Line::from(Span::styled(
            badge_label("Open"),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Odd tick: show the mergeability-specific badge.
    let (text, fg, bg) = match mergeability {
        MergeabilityStatus::Mergeable => ("Mergeable", Color::Black, Color::LightGreen),
        MergeabilityStatus::Conflict => ("Conflict", Color::White, Color::Red),
        MergeabilityStatus::NeedsRebase => ("Rebase", Color::Black, Color::Yellow),
        MergeabilityStatus::Unknown => ("Open", Color::Black, Color::Green),
    };
    Cell::from(Line::from(Span::styled(
        badge_label(text),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    )))
}

pub fn render_table(app: &App, area: Rect) -> Table<'static> {
    let _ = area; // Reserved for future use (e.g. dynamic column width)
    let cols = &app.config.visible_columns;

    // Build the header dynamically based on enabled optional columns.
    let mut header_cells = vec![
        Cell::from("MR ID"),
        Cell::from("Title / API Status"),
        Cell::from("Status").bold(),
    ];
    if cols.target_branch {
        header_cells.push(Cell::from("Target").bold());
    }
    if cols.labels {
        header_cells.push(Cell::from("Labels").bold());
    }
    if cols.milestone {
        header_cells.push(Cell::from("Milestone").bold());
    }
    for b in &app.branches {
        header_cells.push(Cell::from(b.clone()).bold());
    }
    let header = Row::new(header_cells).bottom_margin(1).underlined();

    let rows: Vec<Row> = app
        .mrs
        .iter()
        .map(|mr| {
            // Always compute the label cell — used only when the column is enabled.
            let filtered_labels: Vec<&String> = mr
                .labels
                .iter()
                .filter(|l| app.config.is_table_label(l))
                .collect();

            let label_cell = if filtered_labels.is_empty() {
                Cell::from("-").dark_gray()
            } else {
                let mut spans = Vec::new();
                for label in filtered_labels {
                    spans.push(create_chip_span(label, &app.config));
                    spans.push(ratatui::text::Span::raw(" "));
                }
                Cell::from(Line::from(spans))
            };

            // Fixed columns — always present.
            let mut cells = vec![
                Cell::from(format!("!{}", mr.id)),
                Cell::from(mr.title.clone()).fg(match mr.status {
                    MrStatus::Error => Color::Red,
                    _ => Color::White,
                }),
                state_badge(&mr.state, &mr.mergeability, app.time_left),
            ];

            // Optional columns — inserted only when enabled in config.
            if cols.target_branch {
                cells.push(Cell::from(mr.target_branch.clone()).fg(Color::LightBlue));
            }
            if cols.labels {
                cells.push(label_cell);
            }
            if cols.milestone {
                cells.push(Cell::from(mr.milestone.clone()).fg(Color::Cyan));
            }

            for b in &app.branches {
                let cell = match &mr.status {
                    MrStatus::Loading => Cell::from("⏳ Loading...").yellow(),
                    MrStatus::Error => Cell::from("❌ Failed").red(),
                    MrStatus::MergedIn(set) => {
                        if set.contains(b) {
                            Cell::from("🟢 Present").green()
                        } else {
                            Cell::from("🔴 Absent").red()
                        }
                    }
                };
                cells.push(cell);
            }
            Row::new(cells)
        })
        .collect();

    // Build constraints in lockstep with the header/row cells.
    let mut constraints = vec![
        Constraint::Length(8),  // ID
        Constraint::Fill(3),    // Title
        Constraint::Length(13), // State badge
    ];
    if cols.target_branch {
        constraints.push(Constraint::Fill(2)); // Target branch
    }
    if cols.labels {
        constraints.push(Constraint::Fill(2)); // Labels
    }
    if cols.milestone {
        constraints.push(Constraint::Fill(2)); // Milestone
    }

    for _ in &app.branches {
        constraints.push(Constraint::Fill(1));
    }

    let mins = app.time_left / 60;
    let secs = app.time_left % 60;

    let sort_label = match app.sort_column {
        SortColumn::UpdatedAt => "Updated ↕",
        SortColumn::Id => "ID ↕",
        SortColumn::Milestone => "Milestone ↕",
        SortColumn::Title => "Title ↕",
    };
    let order_label = match app.sort_order {
        SortOrder::Ascending => "↑",
        SortOrder::Descending => "↓",
    };

    let title_text = format!(
        " GitLab MR Tracker ({}) │ 🔄 Next refresh: {:02}:{:02} │ Sort: {} {} ",
        app.base_url, mins, secs, sort_label, order_label
    );

    Table::new(rows, constraints)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(title_text),
        )
}
