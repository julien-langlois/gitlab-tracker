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
/// "MERGEABLE" is 9 chars — add 2 chars of padding (1 each side) → 11.
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
            GitlabMrState::Merged => ("MERGED", Color::Black, Color::Magenta),
            GitlabMrState::Closed => ("CLOSED", Color::Black, Color::Red),
            GitlabMrState::Opened => unreachable!(),
        };
        return Cell::from(Line::from(Span::styled(
            badge_label(text),
            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
        )));
    }

    // Even tick: always show the base "OPEN" badge.
    if tick.is_multiple_of(2) {
        return Cell::from(Line::from(Span::styled(
            badge_label("OPEN"),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Odd tick: show the mergeability-specific badge.
    let (text, fg, bg) = match mergeability {
        MergeabilityStatus::Mergeable => ("MERGEABLE", Color::Black, Color::LightGreen),
        MergeabilityStatus::Conflict => ("CONFLICT", Color::White, Color::Red),
        MergeabilityStatus::NeedsRebase => ("REBASE", Color::Black, Color::Yellow),
        MergeabilityStatus::Unknown => ("OPEN", Color::Black, Color::Green),
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
    if cols.activity {
        header_cells.push(Cell::from("Activity").bold());
    }
    if cols.target_branch {
        header_cells.push(Cell::from("Target").bold());
    }
    if cols.labels {
        header_cells.push(Cell::from("Labels").bold());
    }
    if cols.milestone {
        header_cells.push(Cell::from("Milestone").bold());
    }
    if cols.notes {
        header_cells.push(Cell::from("Notes").bold());
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

            // Whether this row should receive the "recently updated" highlight.
            // Cells must be individually coloured because a Row-level bg is overridden
            // by any Cell that sets its own fg/bg style.
            let highlight = mr.recently_updated && app.update_highlight_ticks > 0;

            /// Applies the update-highlight background to a cell when active.
            /// The foreground is left untouched so each cell keeps its own colour.
            fn maybe_highlight(cell: Cell<'static>, highlight: bool) -> Cell<'static> {
                if highlight {
                    cell.bg(Color::Rgb(0, 90, 40))
                } else {
                    cell
                }
            }

            // Fixed columns — always present.
            let mut cells = vec![
                maybe_highlight(Cell::from(format!("!{}", mr.id)), highlight),
                maybe_highlight(
                    Cell::from(mr.title.clone()).fg(match mr.status {
                        MrStatus::Error => Color::Red,
                        _ => Color::White,
                    }),
                    highlight,
                ),
                maybe_highlight(
                    state_badge(&mr.state, &mr.mergeability, app.time_left),
                    highlight,
                ),
            ];

            // Optional columns — inserted only when enabled in config.
            if cols.activity {
                let (icon, color) = app.config.activity_badge(mr.updated_at.as_deref());
                cells.push(maybe_highlight(Cell::from(icon).fg(color), highlight));
            }
            if cols.target_branch {
                cells.push(maybe_highlight(
                    Cell::from(mr.target_branch.clone()).fg(Color::LightBlue),
                    highlight,
                ));
            }
            if cols.labels {
                cells.push(maybe_highlight(label_cell, highlight));
            }
            if cols.milestone {
                cells.push(maybe_highlight(
                    Cell::from(mr.milestone.clone()).fg(Color::Cyan),
                    highlight,
                ));
            }
            if cols.notes {
                let (notes_text, notes_color) = if mr.user_notes_count == 0 {
                    ("✔ 0".to_string(), Color::DarkGray)
                } else {
                    (format!("💬 {}", mr.user_notes_count), Color::Yellow)
                };
                cells.push(maybe_highlight(
                    Cell::from(notes_text).fg(notes_color),
                    highlight,
                ));
            }

            for b in &app.branches {
                let cell = match &mr.status {
                    MrStatus::Loading => Cell::from("⏳ LOADING...").yellow(),
                    MrStatus::Error => Cell::from("❌ FAILED").red(),
                    MrStatus::MergedIn(set) => {
                        if set.contains(b) {
                            Cell::from("🟢 PRESENT").green()
                        } else {
                            Cell::from("🔴 ABSENT").red()
                        }
                    }
                };
                cells.push(maybe_highlight(cell, highlight));
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
    if cols.activity {
        constraints.push(Constraint::Length(12)); // Activity badge
    }
    if cols.target_branch {
        constraints.push(Constraint::Fill(2)); // Target branch
    }
    if cols.labels {
        constraints.push(Constraint::Fill(2)); // Labels
    }
    if cols.milestone {
        constraints.push(Constraint::Fill(2)); // Milestone
    }
    if cols.notes {
        constraints.push(Constraint::Length(10)); // Notes badge
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
