use crate::app::App;
use crate::models::{GitlabMrState, MergeabilityStatus, MrStatus, PipelineState};
use crate::ui::inspector::create_chip_span;
use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, Table},
};

/// Fixed width (in chars) for all state badges, padding included.
/// "CI STILL RUNNING" is 16 chars — add 2 chars of padding (1 each side) → 18.
pub const BADGE_WIDTH: usize = 18;

/// Fixed width (in chars) for the "Behind" chip, padding included.
/// "1000 behind" is 11 chars — add 1 char each side → 13. Column constraint must match.
pub const BEHIND_WIDTH: usize = 13;

/// Centers `text` inside a field of exactly `BADGE_WIDTH` characters.
/// Excess space is distributed evenly left and right (left-biased on odd remainder).
pub fn badge_label(text: &str) -> String {
    // Use Rust's built-in centering formatter.
    format!("{:^width$}", text, width = BADGE_WIDTH)
}

/// Returns a styled badge cell for the GitLab MR state.
///
/// For open MRs, `tick` (the current `time_left` value) drives a three-phase animation:
///   - tick % 3 == 0 → "OPEN" base badge (green)
///   - tick % 3 == 1 → mergeability badge (colour varies)
///   - tick % 3 == 2 → CI pipeline badge when the latest pipeline is running or pending,
///     otherwise falls back to the mergeability badge
///
/// All badges share the same `BADGE_WIDTH` so the column never shifts.
fn state_badge(
    state: &GitlabMrState,
    mergeability: &MergeabilityStatus,
    pipelines: &[crate::models::Pipeline],
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

    // Detect whether the latest pipeline is actively running or pending.
    let ci_active = pipelines
        .first()
        .is_some_and(|p| matches!(p.status, PipelineState::Running | PipelineState::Pending));

    // tick % 3 == 0: always show the base "OPEN" badge.
    if tick.is_multiple_of(3) {
        return Cell::from(Line::from(Span::styled(
            badge_label("OPEN"),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // tick % 3 == 2 and CI is running: show animated CI badge.
    if tick % 3 == 2 && ci_active {
        // Alternate between two spinner frames to create a pulse effect.
        let label = if tick % 6 < 3 {
            "CI RUNNING"
        } else {
            "CI PENDING"
        };
        return Cell::from(Line::from(Span::styled(
            badge_label(label),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(180, 120, 0))
                .add_modifier(Modifier::BOLD),
        )));
    }

    // tick % 3 == 1, or tick % 3 == 2 with no active CI: show the mergeability badge.
    let (text, fg, bg) = match mergeability {
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
        MergeabilityStatus::Retrying => ("RETRYING", Color::Black, Color::Cyan),
        MergeabilityStatus::SyncFailed => ("SYNC FAILED", Color::White, Color::Red),
        MergeabilityStatus::Unknown => ("OPEN", Color::Black, Color::Green),
    };
    Cell::from(Line::from(Span::styled(
        badge_label(text),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    )))
}

/// Builds the MR table. Cells borrow their text from `app` instead of cloning
/// every title / branch / milestone on each frame.
pub fn render_table(app: &App) -> Table<'_> {
    // Resolve the active palette once — used for header and all row cells.
    let palette = app.theme;

    // Resolve column visibility via the inventory-registered ColumnDef ids.
    // This replaces the old fixed-field `VisibleColumns` struct accesses.
    let col = |id: &str| app.config.visible_columns.is_visible(id);

    // Build the header dynamically based on enabled optional columns.
    let mut header_cells = vec![
        Cell::from("MR ID"),
        Cell::from("Title / API Status"),
        Cell::from("Status").bold(),
    ];
    if col("activity") {
        header_cells.push(Cell::from("Activity").bold());
    }
    if col("target_branch") {
        header_cells.push(Cell::from("Target").bold());
    }
    if col("labels") {
        header_cells.push(Cell::from("Labels").bold());
    }
    if col("milestone") {
        header_cells.push(Cell::from("Milestone").bold());
    }
    if col("notes") {
        header_cells.push(Cell::from("Notes").bold());
    }
    if col("diff_stats") {
        header_cells.push(Cell::from("Effort").bold());
    }
    if col("commits_behind") {
        header_cells.push(Cell::from("Behind").bold());
    }
    if col("tracker_ticket") {
        header_cells.push(Cell::from("Tracker").bold());
    }
    for b in &app.branches {
        header_cells.push(Cell::from(b.as_str()).bold());
    }
    let header = Row::new(header_cells).bottom_margin(1).underlined();

    let rows: Vec<Row> = app
        .visible_mrs()
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
                    let gitlab_color = app
                        .config
                        .gitlab_label_colors
                        .get(&label.to_lowercase())
                        .map(|s| s.as_str());
                    spans.push(create_chip_span(label, &app.config, gitlab_color));
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
            fn maybe_highlight(cell: Cell<'_>, highlight: bool) -> Cell<'_> {
                if highlight {
                    cell.bg(Color::Rgb(0, 90, 40))
                } else {
                    cell
                }
            }

            // Build the title cell — prepend a coloured flag chevron for flagged MRs.
            let title_cell = if mr.flagged {
                let title_color = match mr.status {
                    MrStatus::Error => palette.accent_red,
                    _ => palette.fg,
                };
                Cell::from(Line::from(vec![
                    Span::styled(
                        "★ ",
                        Style::default()
                            .fg(palette.accent_yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(mr.title.as_str(), Style::default().fg(title_color)),
                ]))
            } else {
                Cell::from(mr.title.as_str()).fg(match mr.status {
                    MrStatus::Error => palette.accent_red,
                    _ => palette.fg,
                })
            };

            // Fixed columns — always present.
            let mut cells = vec![
                maybe_highlight(Cell::from(format!("!{}", mr.id)), highlight),
                maybe_highlight(title_cell, highlight),
                maybe_highlight(
                    state_badge(&mr.state, &mr.mergeability, &mr.pipelines, app.time_left),
                    highlight,
                ),
            ];

            // Optional columns — inserted only when enabled in config.
            if col("activity") {
                let (icon, color) = app.config.activity_badge(mr.updated_at.as_deref());
                cells.push(maybe_highlight(Cell::from(icon).fg(color), highlight));
            }
            if col("target_branch") {
                cells.push(maybe_highlight(
                    Cell::from(mr.target_branch.as_str()).fg(Color::LightBlue),
                    highlight,
                ));
            }
            if col("labels") {
                cells.push(maybe_highlight(label_cell, highlight));
            }
            if col("milestone") {
                cells.push(maybe_highlight(
                    Cell::from(mr.milestone.as_str()).fg(Color::Cyan),
                    highlight,
                ));
            }
            if col("notes") {
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

            // Optional complexity column — shows a fixed-width coloured chip badge
            // matching the side-panel style. All labels are uppercased and padded to
            // the width of the longest one ("COMPLEX") so every chip is the same size.
            // Emoji glyphs occupy 2 terminal columns, so padding is computed manually:
            //   🟢 EASY    → 2 + 5 = 7  → 3 trailing spaces to reach 10
            //   🟡 MEDIUM  → 2 + 7 = 9  → 1 trailing space  to reach 10
            //   🔴 COMPLEX → 2 + 8 = 10 → no extra padding needed
            if col("diff_stats") {
                let complexity_cell = match &mr.diff_stats {
                    Some(stats) => {
                        let score = stats.difficulty(&app.config.complexity_profile);

                        let (label, fg, bg) = if score < 0.33 {
                            ("🟢 EASY   ", Color::Black, Color::Green)
                        } else if score < 0.66 {
                            ("🟡 MEDIUM ", Color::Black, Color::Yellow)
                        } else {
                            ("🔴 COMPLEX", Color::White, Color::Red)
                        };

                        Cell::from(Line::from(vec![Span::styled(
                            format!(" {} ", label),
                            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
                        )]))
                    }

                    None => Cell::from("—").fg(Color::DarkGray),
                };

                cells.push(maybe_highlight(complexity_cell, highlight));
            }

            // Optional "commits behind target" column — only meaningful for open MRs
            // that are not Mergeable. Shows "Up to date" for Mergeable, "N behind" otherwise,
            // "—" for merged/closed MRs (not applicable).
            if col("commits_behind") {
                let behind_cell = match &mr.diff_stats {
                    Some(stats) if mr.state == crate::models::GitlabMrState::Opened => {
                        match (stats.commits_behind, &mr.mergeability) {
                            (Some(0), _) | (_, MergeabilityStatus::Mergeable) => {
                                // Centered plain text — no background colour needed.
                                Cell::from(format!(
                                    "{:^width$}",
                                    "✔ Up to date",
                                    width = BEHIND_WIDTH
                                ))
                                .fg(Color::Green)
                            }
                            (Some(n), _) => {
                                let (fg, bg) = if n >= 10 {
                                    (Color::White, Color::Red)
                                } else if n >= 3 {
                                    (Color::Black, Color::Yellow)
                                } else {
                                    (Color::Black, Color::LightYellow)
                                };
                                // The text is padded to BEHIND_WIDTH so the background colour
                                // fills the entire column width, matching the Status chip style.
                                Cell::from(Span::styled(
                                    format!(
                                        "{:^width$}",
                                        format!("{} behind", n),
                                        width = BEHIND_WIDTH
                                    ),
                                    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
                                ))
                            }
                            (None, MergeabilityStatus::Retrying) => {
                                Cell::from("RETRYING").fg(Color::Cyan)
                            }
                            (None, MergeabilityStatus::SyncFailed) => {
                                Cell::from("SYNC FAILED").fg(Color::Red)
                            }
                            // None + non-Mergeable: still loading from the API.
                            (None, _) => Cell::from("…").fg(Color::DarkGray),
                        }
                    }
                    // Merged / closed or no diff_stats yet.
                    _ => Cell::from("—").fg(Color::DarkGray),
                };
                cells.push(maybe_highlight(behind_cell, highlight));
            }

            // Optional tracker ticket column — visible when a provider is configured.
            if col("tracker_ticket") {
                let ticket_cell = match &mr.linked_ticket {
                    Some(t) => {
                        // Format time tracking as "Xh Ym" — reused from inspector logic.
                        let fmt_duration = |secs: u32| -> String {
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
                        };

                        let has_tracking = t.time_estimate.map(|v| v > 0).unwrap_or(false)
                            || t.time_spent.map(|v| v > 0).unwrap_or(false);

                        if has_tracking {
                            let spent = t
                                .time_spent
                                .map(fmt_duration)
                                .unwrap_or_else(|| "—".to_string());
                            let estimate = t
                                .time_estimate
                                .map(fmt_duration)
                                .unwrap_or_else(|| "—".to_string());

                            // Colour the tracking ratio: green < 80 %, yellow 80–100 %, red over budget.
                            let ratio_color = match (t.time_estimate, t.time_spent) {
                                (Some(est), Some(sp)) if est > 0 => {
                                    let ratio = sp as f32 / est as f32;
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

                            let spans = vec![
                                Span::styled(
                                    format!("#{} {} ", t.id, t.status),
                                    Style::default()
                                        .fg(Color::LightMagenta)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    format!("{}/{}", spent, estimate),
                                    Style::default().fg(ratio_color),
                                ),
                            ];
                            Cell::from(Line::from(spans))
                        } else {
                            let text = format!("#{} {}", t.id, t.status);
                            Cell::from(text).fg(Color::LightMagenta)
                        }
                    }
                    None => Cell::from("—").fg(Color::DarkGray),
                };
                cells.push(maybe_highlight(ticket_cell, highlight));
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
    // State badge column width must match BADGE_WIDTH exactly so the text is centred.
    let mut constraints = vec![
        Constraint::Length(8),                      // ID
        Constraint::Fill(3),                        // Title
        Constraint::Length(BADGE_WIDTH as u16 + 2), // State badge
    ];
    if col("activity") {
        constraints.push(Constraint::Length(12)); // Activity badge
    }
    if col("target_branch") {
        constraints.push(Constraint::Fill(2)); // Target branch
    }
    if col("labels") {
        constraints.push(Constraint::Fill(2)); // Labels
    }
    if col("milestone") {
        // Milestone gets a smaller share when the tracker column is also visible,
        // to give more room to the ticket subject which is typically longer.
        let milestone_fill = if col("tracker_ticket") { 1 } else { 2 };
        constraints.push(Constraint::Fill(milestone_fill)); // Milestone
    }
    if col("notes") {
        constraints.push(Constraint::Length(10)); // Notes badge
    }
    if col("diff_stats") {
        constraints.push(Constraint::Length(14)); // Complexity chip badge (e.g. " 🔴 Complex ")
    }
    if col("commits_behind") {
        constraints.push(Constraint::Length(BEHIND_WIDTH as u16)); // "✔ Up to date " or " NNN behind " (13 chars, fixed width)
    }
    if col("tracker_ticket") {
        constraints.push(Constraint::Fill(2)); // Tracker ticket
    }

    for _ in &app.branches {
        constraints.push(Constraint::Fill(1));
    }

    Table::new(rows, constraints)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .block(Block::default().borders(Borders::ALL).title(" Dashboard "))
}
