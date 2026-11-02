//! The tracker page: the tickets linked to the listed MRs, grouped by status (a
//! list, or a board of columns), with a summary of the time on top. A click
//! selects the MR, whose ticket the inspector shows.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use gitlab_tracker_core::LinkedTicket;

use super::icons::{self, Icon};
use super::inspector::duration;
use super::widgets::{
    avatar, bar, dot, icon_toggle, kpi, mono, muted, page_title, semibold, split_person,
};
use super::{tones, GuiApp};
use crate::actions::Action;
use crate::i18n::{tf, tl, tn};

/// Below this list width the progress column goes under the subject.
const WIDE_ROW: f32 = 720.0;
const PROGRESS_WIDTH: f32 = 180.0;
const ID_WIDTH: f32 = 72.0;
/// A board column is never narrower than this; past it the board scrolls sideways.
const BOARD_COLUMN_WIDTH: f32 = 260.0;

/// A listed MR with its ticket.
struct Row {
    index: usize,
    mr_id: String,
    mr_title: String,
    ticket: LinkedTicket,
}

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let selected = g.app.table_state.selected();
    let rows: Vec<Row> = g
        .app
        .visible_mrs()
        .enumerate()
        .filter_map(|(index, mr)| {
            mr.linked_ticket.clone().map(|ticket| Row {
                index,
                mr_id: mr.id.clone(),
                mr_title: mr.title.clone(),
                ticket,
            })
        })
        .collect();
    let listed = g.app.visible_mrs().count();
    let provider = g.app.tracker.as_ref().map_or("Tracker", |t| t.name());

    let mut picked = None;
    let mut log_time = None;
    let mut moved: Option<Move> = None;
    let mut tile = None;
    let board = g.tracker_board;
    let mut set_board = None;
    egui::Frame::new()
        .inner_margin(Margin::symmetric(24, 20))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                page_title(
                    ui,
                    &theme,
                    tl("Linked tickets"),
                    tf("{provider} · live from the list", &[("provider", provider)]),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let style = theme.icon_button();
                    if icon_toggle(ui, tl("Board"), Icon::Board, &style, 30.0, board).clicked() {
                        set_board = Some(true);
                    }
                    if icon_toggle(ui, tl("List"), Icon::List, &style, 30.0, !board).clicked() {
                        set_board = Some(false);
                    }
                });
            });
            ui.add_space(16.0);
            summary(ui, &theme, &rows, listed, &mut tile);
            ui.add_space(20.0);
            if rows.is_empty() {
                ui.add_space(32.0);
                ui.vertical_centered(|ui| {
                    ui.label(muted(
                        &theme,
                        tl("No listed merge request references a ticket."),
                    ));
                });
                return;
            }
            if board {
                let columns = board_columns(&rows, g.statuses.as_deref());
                let movable = g.app.ticket_transitioner.is_some();
                board_view(
                    ui,
                    &theme,
                    &rows,
                    &columns,
                    movable,
                    selected,
                    (&mut picked, &mut log_time, &mut moved),
                );
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (status, group) in by_status(&rows) {
                        let tone = tones::ticket_status(status, &theme.tones);
                        group_heading(ui, &theme, status, tone, group.len(), time_total(&group));
                        for row in group {
                            let (card, avatar) =
                                ticket_row(ui, &theme, row, selected == Some(row.index));
                            if card.clicked() {
                                picked = Some(row.index);
                            }
                            let right =
                                avatar.map_or(card.rect.right() - 14.0, |a| a.left() - 10.0);
                            if quick_log(ui, &theme, &card, right) {
                                log_time = Some(row.index);
                            }
                            ui.add_space(6.0);
                        }
                        ui.add_space(14.0);
                    }
                });
        });
    if let Some(board) = set_board {
        g.tracker_board = board;
    }
    if let Some((key, label)) = tile {
        g.focus_on(key, tl(label));
    }
    if let Some(index) = picked.or(log_time) {
        g.app.table_state.select(Some(index));
        g.app.layout.inspector.reset();
    }
    if log_time.is_some() {
        g.pending.push(Action::OpenLogTime);
    }
    if let Some(m) = moved {
        g.move_ticket(m.ticket, m.target, m.label);
    }
    g.ensure_statuses();
}

/// A ticket dropped on another column: its id, the status's id and name.
pub struct Move {
    pub ticket: String,
    pub target: String,
    pub label: String,
}

/// What a card carries while dragged.
struct Dragged {
    ticket: String,
    status: String,
}

/// The board's columns: the tracker's statuses in its order (with their id, to
/// move a ticket there), then the statuses of the tickets it did not list.
fn board_columns(
    rows: &[Row],
    statuses: Option<&[gitlab_tracker_core::TicketTransitionTarget]>,
) -> Vec<(String, Option<String>)> {
    let mut columns: Vec<(String, Option<String>)> = statuses
        .unwrap_or_default()
        .iter()
        .map(|t| (t.label.clone(), Some(t.id.clone())))
        .collect();
    for row in rows {
        let known = columns
            .iter()
            .any(|(label, _)| label.eq_ignore_ascii_case(&row.ticket.status));
        if !known {
            columns.push((row.ticket.status.clone(), None));
        }
    }
    columns
}

/// Spent / estimated over the tickets, once each: "5h / 8h".
fn time_total(rows: &[&Row]) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let tickets: Vec<&LinkedTicket> = rows
        .iter()
        .map(|r| &r.ticket)
        .filter(|t| seen.insert(t.id.clone()))
        .collect();
    let spent: u32 = tickets.iter().filter_map(|t| t.time_spent).sum();
    let estimated: u32 = tickets.iter().filter_map(|t| t.time_estimate).sum();
    (spent > 0 || estimated > 0).then(|| {
        format!(
            "{} / {}",
            duration(spent),
            if estimated > 0 {
                duration(estimated)
            } else {
                "—".into()
            }
        )
    })
}

/// A "+ Log time" button over the right of `card` while the pointer is on it,
/// its right edge at `right`. Returns whether it was clicked.
fn quick_log(ui: &mut Ui, theme: &Theme, card: &egui::Response, right: f32) -> bool {
    if !ui.rect_contains_pointer(card.rect) {
        return false;
    }
    let size = Vec2::new(28.0, 28.0);
    let rect = egui::Rect::from_min_size(egui::pos2(right - size.x, card.rect.top() + 10.0), size);
    let response = ui
        .interact(rect, card.id.with("log_time"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Log time… (L)"));
    let fill = if response.hovered() {
        theme.palette.accent_fill
    } else {
        theme.palette.raised
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(theme.radii.medium),
        Stroke::new(1.0, theme.palette.border),
        egui::StrokeKind::Inside,
    );
    let tint = if response.hovered() {
        egui::Color32::WHITE
    } else {
        theme.palette.text_secondary
    };
    icons::paint(ui.painter(), rect.shrink(7.0), Icon::Plus, tint);
    response.clicked()
}

/// The tickets as a board: a column per status, compact cards in each, the
/// columns side by side (scrolling sideways when they do not fit). With a
/// tracker that can move tickets, a card dropped on a column moves it there.
fn board_view(
    ui: &mut Ui,
    theme: &Theme,
    rows: &[Row],
    columns: &[(String, Option<String>)],
    movable: bool,
    selected: Option<usize>,
    (picked, log_time, moved): (&mut Option<usize>, &mut Option<usize>, &mut Option<Move>),
) {
    let gap = 12.0;
    let count = columns.len().max(1) as f32;
    let width = ((ui.available_width() - gap * (count - 1.0)) / count).max(BOARD_COLUMN_WIDTH);
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (status, target) in columns {
                let group: Vec<&Row> = rows
                    .iter()
                    .filter(|r| r.ticket.status.eq_ignore_ascii_case(status))
                    .collect();
                let tone = tones::ticket_status(status, &theme.tones);
                ui.allocate_ui_with_layout(
                    Vec2::new(width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(width);
                        let frame = egui::Frame::new()
                            .fill(theme.palette.panel.gamma_multiply(0.6))
                            .corner_radius(CornerRadius::same(theme.radii.large))
                            .inner_margin(Margin::same(10));
                        let (column, dropped) = ui.dnd_drop_zone::<Dragged, _>(frame, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add_space(2.0);
                            group_heading(ui, theme, status, tone, group.len(), time_total(&group));
                            ui.spacing_mut().item_spacing.y = 8.0;
                            if group.is_empty() {
                                ui.add_space(4.0);
                                let hint = if movable && target.is_some() {
                                    tl("Drop a ticket here to move it")
                                } else {
                                    tl("No ticket")
                                };
                                ui.vertical_centered(|ui| ui.label(muted(theme, hint)));
                                ui.add_space(8.0);
                            }
                            for row in &group {
                                let selected = selected == Some(row.index);
                                let card = if movable {
                                    let id =
                                        egui::Id::unique(("board_card", &row.ticket.id, row.index));
                                    let payload = Dragged {
                                        ticket: row.ticket.id.clone(),
                                        status: row.ticket.status.clone(),
                                    };
                                    ui.dnd_drag_source(id, payload, |ui| {
                                        board_card(ui, theme, row, selected)
                                    })
                                    .inner
                                } else {
                                    board_card(ui, theme, row, selected)
                                };
                                if card.clicked() {
                                    *picked = Some(row.index);
                                }
                                if quick_log(ui, theme, &card, card.rect.right() - 10.0) {
                                    *log_time = Some(row.index);
                                }
                            }
                        });
                        if let (Some(dragged), Some(target)) = (dropped, target) {
                            if !dragged.status.eq_ignore_ascii_case(status) {
                                *moved = Some(Move {
                                    ticket: dragged.ticket.clone(),
                                    target: target.clone(),
                                    label: status.clone(),
                                });
                            }
                        }
                        // The status's tone along the top of the column.
                        let top = column.response.rect;
                        let radius = theme.radii.large;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(top.min, Vec2::new(top.width(), 3.0)),
                            CornerRadius {
                                nw: radius,
                                ne: radius,
                                sw: 0,
                                se: 0,
                            },
                            tone.fg,
                        );
                    },
                );
            }
        });
    });
}

/// A ticket on the board: id and type, subject, MR, progress, assignee.
fn board_card(ui: &mut Ui, theme: &Theme, row: &Row, selected: bool) -> egui::Response {
    let ticket = &row.ticket;
    let (fill, stroke) = if selected {
        (theme.palette.selection, theme.palette.accent)
    } else {
        (theme.palette.raised, theme.palette.border_soft)
    };
    let response = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    mono(format!("#{}", ticket.id), theme.text_size - 1.0)
                        .color(theme.palette.accent),
                );
                if let Some(kind) = &ticket.tracker_type {
                    ui.label(muted(theme, kind).size(theme.text_size - 2.0));
                }
                if let Some(assignee) = &ticket.assignee {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (name, _) = split_person(assignee);
                        avatar(ui, theme, name, 20.0)
                            .on_hover_text(tf("Assigned to {name}", &[("name", assignee)]));
                    });
                }
            });
            ui.add(
                egui::Label::new(
                    RichText::new(&ticket.subject).font(fonts::medium(theme.text_size)),
                )
                .wrap(),
            );
            mr_line(ui, theme, row);
            ui.add_space(2.0);
            progress(ui, theme, ticket);
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() && !selected {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(theme.radii.medium),
            Stroke::new(1.0, theme.palette.border),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// The rows by ticket status, statuses in the order they first show up.
fn by_status(rows: &[Row]) -> Vec<(&str, Vec<&Row>)> {
    let mut groups: Vec<(&str, Vec<&Row>)> = Vec::new();
    for row in rows {
        match groups.iter_mut().find(|(s, _)| *s == row.ticket.status) {
            Some((_, group)) => group.push(row),
            None => groups.push((&row.ticket.status, vec![row])),
        }
    }
    groups
}

/// Four tiles: linked tickets, MRs without one, time spent against estimated,
/// tickets over their estimate. Two per line when the page is narrow. Returns
/// where each tile went.
///
/// The tiles about MRs are links: a click lists them (`clicked`: the cockpit
/// counter's key and label).
fn summary(
    ui: &mut Ui,
    theme: &Theme,
    rows: &[Row],
    listed: usize,
    clicked: &mut Option<(&'static str, &'static str)>,
) -> Vec<egui::Rect> {
    // A ticket linked from two MRs counts once in the totals.
    let mut seen = std::collections::HashSet::new();
    let tickets: Vec<&LinkedTicket> = rows
        .iter()
        .map(|r| &r.ticket)
        .filter(|t| seen.insert(t.id.clone()))
        .collect();
    let spent: u32 = tickets.iter().filter_map(|t| t.time_spent).sum();
    let estimated: u32 = tickets.iter().filter_map(|t| t.time_estimate).sum();
    let over = tickets.iter().filter(|t| is_over(t)).count();
    let tones = &theme.tones;
    let tiles = [
        (
            tl("Linked tickets"),
            tickets.len().to_string(),
            tf(
                "from {n} of {total} MRs",
                &[
                    ("n", &rows.len().to_string()),
                    ("total", &listed.to_string()),
                ],
            ),
            None,
            Some(("With ticket", "With ticket")),
        ),
        (
            tl("Without a ticket"),
            (listed - rows.len()).to_string(),
            tl("merge requests").to_owned(),
            (listed > rows.len()).then_some(tones.amber),
            (listed > rows.len()).then_some(("No ticket", "No ticket")),
        ),
        (
            tl("Time spent"),
            duration(spent),
            if estimated > 0 {
                tf("of {time} estimated", &[("time", &duration(estimated))])
            } else {
                tl("no estimate").to_owned()
            },
            (estimated > 0 && spent > estimated).then_some(tones.red),
            None,
        ),
        (
            tl("Over estimate"),
            over.to_string(),
            tn(over as u64, "ticket", "tickets"),
            (over > 0).then_some(tones.red),
            (over > 0).then_some(("Over estimate", "Over estimate")),
        ),
    ];
    let gap = 12.0;
    let per_line = if ui.available_width() < WIDE_ROW {
        2
    } else {
        4
    };
    let width = (ui.available_width() - gap * (per_line - 1) as f32) / per_line as f32;
    let mut rects = Vec::new();
    for line in tiles.chunks(per_line) {
        // From the top: `horizontal` centers each tile on a line that grows
        // with every tile, a step lower each time.
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (caption, value, detail, tone, link) in line {
                let size = Vec2::new(width, 0.0);
                let response =
                    ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                        let color = tone.map_or(theme.palette.text, |t| t.fg);
                        kpi(
                            ui,
                            theme,
                            caption,
                            value,
                            color,
                            detail,
                            theme.palette.text_muted,
                        );
                    });
                let rect = response.response.rect;
                rects.push(rect);
                let Some(link) = link else { continue };
                let response = ui
                    .interact(rect, ui.scope_id().with(("tile", link.0)), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tl("Click to list these MRs"));
                if response.hovered() {
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(theme.radii.large),
                        Stroke::new(1.0, theme.palette.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if response.clicked() {
                    *clicked = Some(*link);
                }
            }
        });
        ui.add_space(gap);
    }
    rects
}

/// A status's heading: its dot, name and count, and the time of its tickets.
fn group_heading(
    ui: &mut Ui,
    theme: &Theme,
    status: &str,
    tone: Tone,
    count: usize,
    time: Option<String>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        dot(ui, tone.fg, 8.0);
        ui.label(semibold(status, theme.text_size));
        ui.add(Chip::new(&count.to_string(), theme.tones.gray).pill());
        if let Some(time) = time {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (clock, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                icons::paint(ui.painter(), clock, Icon::Clock, theme.palette.text_muted);
                ui.label(muted(theme, time))
                    .on_hover_text(tl("Time spent / estimated on these tickets"));
            });
        }
    });
    ui.add_space(8.0);
}

fn is_over(ticket: &LinkedTicket) -> bool {
    matches!(
        (ticket.time_spent, ticket.time_estimate),
        (Some(s), Some(e)) if e > 0 && s > e
    )
}

/// One ticket as a card: id and type, subject and MR, progress, assignee.
/// Returns the card's response and where the avatar went.
fn ticket_row(
    ui: &mut Ui,
    theme: &Theme,
    row: &Row,
    selected: bool,
) -> (egui::Response, Option<egui::Rect>) {
    let ticket = &row.ticket;
    let mut avatar_rect = None;
    let wide = ui.available_width() >= WIDE_ROW;
    let stroke = if selected {
        theme.palette.accent
    } else {
        theme.palette.border_soft
    };
    let fill = if selected {
        theme.palette.selection
    } else {
        theme.palette.panel
    };
    let response = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(ID_WIDTH, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(ID_WIDTH);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(
                            mono(format!("#{}", ticket.id), theme.text_size - 0.5)
                                .color(theme.palette.accent),
                        );
                        if let Some(kind) = &ticket.tracker_type {
                            ui.add(
                                egui::Label::new(muted(theme, kind).size(theme.text_size - 2.0))
                                    .truncate(),
                            );
                        }
                    },
                );
                let assignee_width = if ticket.assignee.is_some() {
                    24.0 + 14.0
                } else {
                    0.0
                };
                let progress_width = if wide { PROGRESS_WIDTH + 14.0 } else { 0.0 };
                let main = (ui.available_width() - assignee_width - progress_width).max(120.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(main, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(main);
                        ui.spacing_mut().item_spacing.y = 5.0;
                        ui.add(
                            egui::Label::new(
                                RichText::new(&ticket.subject).font(fonts::medium(theme.text_size)),
                            )
                            .truncate(),
                        );
                        mr_line(ui, theme, row);
                        if !wide {
                            ui.add_space(4.0);
                            progress(ui, theme, ticket);
                        }
                    },
                );
                if wide {
                    ui.allocate_ui_with_layout(
                        Vec2::new(PROGRESS_WIDTH, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(PROGRESS_WIDTH);
                            ui.add_space(2.0);
                            progress(ui, theme, ticket);
                        },
                    );
                }
                if let Some(assignee) = &ticket.assignee {
                    // Last, once the other columns set the row's height: centered
                    // on it.
                    let height = ui.min_rect().height();
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), height),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            let (name, _) = split_person(assignee);
                            let response = avatar(ui, theme, name, 24.0);
                            avatar_rect = Some(response.rect);
                            response.on_hover_text(tf("Assigned to {name}", &[("name", assignee)]));
                        },
                    );
                }
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() && !selected {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(theme.radii.large),
            Stroke::new(1.0, theme.palette.border),
            egui::StrokeKind::Inside,
        );
    }
    (response, avatar_rect)
}

/// The MR that links the ticket: its icon, id and title.
fn mr_line(ui: &mut Ui, theme: &Theme, row: &Row) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        icons::paint(
            ui.painter(),
            rect,
            Icon::MergeRequests,
            theme.palette.text_muted,
        );
        ui.label(
            mono(format!("!{}", row.mr_id), theme.text_size - 1.5)
                .color(theme.palette.text_secondary),
        );
        ui.add(egui::Label::new(muted(theme, &row.mr_title)).truncate());
    });
}

/// The bar's color for the time used: the accent, amber from 80 % of the
/// estimate, red past it.
fn consumption_color(theme: &Theme, ticket: &LinkedTicket) -> egui::Color32 {
    match (ticket.time_spent, ticket.time_estimate) {
        (Some(s), Some(e)) if e > 0 && s > e => theme.tones.red.fg,
        (Some(s), Some(e)) if e > 0 && s as f32 >= e as f32 * 0.8 => theme.tones.amber.fg,
        _ => theme.palette.accent,
    }
}

/// Spent / estimated and the done ratio over a bar, amber near the estimate and
/// red past it.
fn progress(ui: &mut Ui, theme: &Theme, ticket: &LinkedTicket) {
    let size = theme.text_size - 1.5;
    let fraction = ticket.done_ratio.map(|d| d as f32 / 100.0).or(
        match (ticket.time_spent, ticket.time_estimate) {
            (Some(s), Some(e)) if e > 0 => Some(s as f32 / e as f32),
            _ => None,
        },
    );
    let color = consumption_color(theme, ticket);
    let warn = color != theme.palette.accent;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let time = match (ticket.time_spent, ticket.time_estimate) {
            (None, None) => None,
            (spent, estimate) => Some(format!(
                "{} / {}",
                spent.map_or("—".into(), duration),
                estimate.map_or("—".into(), duration)
            )),
        };
        match &time {
            Some(time) => {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                icons::paint(ui.painter(), rect, Icon::Clock, theme.palette.text_muted);
                ui.label(
                    RichText::new(time)
                        .font(fonts::medium(size))
                        .color(if warn {
                            color
                        } else {
                            theme.palette.text_secondary
                        }),
                );
            }
            None => {
                ui.label(muted(theme, tl("No time logged")).size(size));
            }
        }
        if let Some(fraction) = fraction {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", fraction * 100.0))
                        .size(size)
                        .color(theme.palette.text_muted),
                );
            });
        }
    });
    ui.add_space(6.0);
    bar(ui, theme, fraction.unwrap_or(0.0), color, 4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the summary tiles go in a themed window `width` points wide.
    fn tiles(width: f32) -> Vec<egui::Rect> {
        let theme = Theme::dark();
        let ctx = egui::Context::default();
        ctx.global_style_mut(|style| theme.apply_to_style(style));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 900.0),
            )),
            ..Default::default()
        };
        let mut rects = Vec::new();
        let mut output = ctx.run_ui(input, |ui| rects = summary(ui, &theme, &[], 5, &mut None));
        output.textures_delta.clear();
        rects
    }

    fn ticket() -> LinkedTicket {
        LinkedTicket {
            schema_version: gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION,
            id: "4271".into(),
            subject: "Expose MR metadata".into(),
            status: "In Progress".into(),
            url: "https://tracker.example/issues/4271".into(),
            author: None,
            assignee: Some("Julien Langlois".into()),
            time_estimate: Some(28_800),
            time_spent: Some(18_000),
            time_remaining: None,
            tracker_type: Some("Evolution".into()),
            priority: None,
            version: None,
            start_date: None,
            done_ratio: Some(65),
        }
    }

    #[test]
    fn the_assignee_sits_in_the_middle_of_the_row() {
        let theme = Theme::dark();
        let ctx = egui::Context::default();
        ctx.global_style_mut(|style| theme.apply_to_style(style));
        let row = Row {
            index: 0,
            mr_id: "104".into(),
            mr_title: "feat: GraphQL".into(),
            ticket: ticket(),
        };
        // Wide (progress in its column) and narrow (progress under the subject).
        for width in [1200.0, 600.0] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 900.0),
                )),
                ..Default::default()
            };
            let mut out = None;
            let mut output =
                ctx.run_ui(input, |ui| out = Some(ticket_row(ui, &theme, &row, false)));
            output.textures_delta.clear();
            let (card, avatar) = out.unwrap();
            let avatar = avatar.expect("an assignee");
            let offset = (avatar.center().y - card.rect.center().y).abs();
            assert!(
                offset < 1.0,
                "{width}: card {:?}, avatar {avatar:?}",
                card.rect
            );
        }
    }

    #[test]
    fn summary_tiles_line_up() {
        // Four on a line, then two lines of two on a narrow page.
        for (width, per_line) in [(1200.0, 4), (500.0, 2)] {
            let rects = tiles(width);
            for line in rects.chunks(per_line) {
                for rect in line {
                    assert_eq!(rect.top(), line[0].top(), "{width}: {rects:?}");
                    assert_eq!(rect.height(), line[0].height(), "{width}: {rects:?}");
                    assert!(rect.right() <= width, "{width}: {rects:?}");
                    // Name, value and caption stacked, not side by side.
                    assert!(rect.height() > 80.0, "{width}: {rects:?}");
                }
            }
        }
    }
}
