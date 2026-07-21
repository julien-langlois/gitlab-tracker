use crate::app::App;
use crate::models::MrStatus;
use crate::ui::inspector::create_chip_span;
use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style, Stylize},
    text::Line,
    widgets::{Cell, Row, Table},
};

pub fn render_table(app: &App) -> Table<'static> {
    let mut header_cells = vec![
        Cell::from("MR ID"),
        Cell::from("Title / API Status"),
        Cell::from("Labels").bold(),
        Cell::from("Milestone").bold(),
    ];
    for b in &app.branches {
        header_cells.push(Cell::from(b.clone()).bold());
    }
    let header = Row::new(header_cells).bottom_margin(1).underlined();

    let rows: Vec<Row> = app
        .mrs
        .iter()
        .map(|mr| {
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

            let mut cells = vec![
                Cell::from(format!("!{}", mr.id)),
                Cell::from(mr.title.clone()).fg(match mr.status {
                    MrStatus::Error => Color::Red,
                    _ => Color::White,
                }),
                label_cell,
                Cell::from(mr.milestone.clone()).fg(Color::Cyan),
            ];
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

    let mut constraints = vec![
        Constraint::Length(8), // ID
        Constraint::Fill(3),   // Title
        Constraint::Fill(2),   // Labels
        Constraint::Fill(2),   // Milestone
    ];

    for _ in &app.branches {
        constraints.push(Constraint::Fill(1));
    }

    let mins = app.time_left / 60;
    let secs = app.time_left % 60;
    let title_text = format!(
        " GitLab MR Tracker ({}) │ 🔄 Next refresh: {:02}:{:02} ",
        app.base_url, mins, secs
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
