//! The tracker page: the tickets linked to the listed MRs, grouped by status, with
//! a summary of the time on top. A click selects the MR, whose ticket the
//! inspector shows.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use gitlab_tracker_core::LinkedTicket;

use super::icons::{self, Icon};
use super::inspector::duration;
use super::widgets::{avatar, bar, dot, mono, muted, page_title, semibold, split_person};
use super::{tones, GuiApp};
use crate::i18n::{tf, tl, tn};

/// Below this list width the progress column goes under the subject.
const WIDE_ROW: f32 = 720.0;
const PROGRESS_WIDTH: f32 = 180.0;
const ID_WIDTH: f32 = 72.0;

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
    egui::Frame::new()
        .inner_margin(Margin::symmetric(24, 20))
        .show(ui, |ui| {
            page_title(
                ui,
                &theme,
                tl("Linked tickets"),
                tf("{provider} · live from the list", &[("provider", provider)]),
            );
            ui.add_space(16.0);
            summary(ui, &theme, &rows, listed);
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
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (status, group) in by_status(&rows) {
                        let tone = tones::ticket_status(status, &theme.tones);
                        group_heading(ui, &theme, status, tone, group.len());
                        for row in group {
                            if ticket_row(ui, &theme, row, selected == Some(row.index))
                                .0
                                .clicked()
                            {
                                picked = Some(row.index);
                            }
                            ui.add_space(6.0);
                        }
                        ui.add_space(14.0);
                    }
                });
        });
    if let Some(index) = picked {
        g.app.table_state.select(Some(index));
        g.app.layout.inspector.reset();
    }
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
fn summary(ui: &mut Ui, theme: &Theme, rows: &[Row], listed: usize) -> Vec<egui::Rect> {
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
        ),
        (
            tl("Without a ticket"),
            (listed - rows.len()).to_string(),
            tl("merge requests").to_owned(),
            (listed > rows.len()).then_some(tones.amber),
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
        ),
        (
            tl("Over estimate"),
            over.to_string(),
            tn(over as u64, "ticket", "tickets"),
            (over > 0).then_some(tones.red),
        ),
    ];
    let gap = 12.0;
    let per_line = if ui.available_width() < 640.0 { 2 } else { 4 };
    let width = (ui.available_width() - gap * (per_line - 1) as f32) / per_line as f32;
    let mut rects = Vec::new();
    for line in tiles.chunks(per_line) {
        // From the top: `horizontal` centers each tile on a line that grows
        // with every tile, a step lower each time.
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (caption, value, detail, tone) in line {
                let response = ui.allocate_ui(Vec2::new(width, 0.0), |ui| {
                    tile(ui, theme, caption, value, detail, *tone);
                });
                rects.push(response.response.rect);
            }
        });
        ui.add_space(gap);
    }
    rects
}

fn tile(ui: &mut Ui, theme: &Theme, caption: &str, value: &str, detail: &str, tone: Option<Tone>) {
    theme
        .card_frame()
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(muted(theme, caption).size(theme.text_size - 1.5));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let color = tone.map_or(theme.palette.text, |t| t.fg);
                ui.label(semibold(value, theme.text_size + 6.0).color(color));
                ui.add(
                    egui::Label::new(muted(theme, detail).size(theme.text_size - 1.5)).truncate(),
                );
            });
        });
}

fn group_heading(ui: &mut Ui, theme: &Theme, status: &str, tone: Tone, count: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        dot(ui, tone.fg, 8.0);
        ui.label(semibold(status, theme.text_size));
        ui.add(Chip::new(&count.to_string(), theme.tones.gray).pill());
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

/// Spent / estimated and the done ratio over a bar, red past the estimate.
fn progress(ui: &mut Ui, theme: &Theme, ticket: &LinkedTicket) {
    let size = theme.text_size - 1.5;
    let fraction = ticket.done_ratio.map(|d| d as f32 / 100.0).or(
        match (ticket.time_spent, ticket.time_estimate) {
            (Some(s), Some(e)) if e > 0 => Some(s as f32 / e as f32),
            _ => None,
        },
    );
    let over = is_over(ticket);
    let color = if over {
        theme.tones.red.fg
    } else {
        theme.palette.accent
    };
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
                        .color(if over {
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
        let mut output = ctx.run_ui(input, |ui| rects = summary(ui, &theme, &[], 5));
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
                }
            }
        }
    }
}
