//! What the project pages (team, journal, environments, CI, milestones,
//! overlaps) are built from: the three-column screen — views on the left, the
//! page in the middle, the inspector on the right —, the side panel's header,
//! views and segmented controls, the inspector's sections, and small painted
//! pieces (pills, chips, bars, rows) laid out in exact rects so that nothing
//! overlaps whatever the width.

use std::sync::Arc;

use bevel_theme::{fonts, Chip, Theme, Tone};
use chrono::{DateTime, Utc};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Galley, Layout, Margin, Rect, Sense, Stroke, Ui,
    Vec2,
};

use super::shell::Sizes;
use super::widgets::{focus_ring, muted, rule, semibold};
use super::GuiApp;
use crate::i18n::{tf, tl};

/// The gap between cards.
pub const GAP: f32 = 12.0;

/// Draws a project page: `nav` in the left panel and `inspector` in the right
/// one when the window is wide enough for them (each scrolls on its own), and
/// `main` in the middle, scrolling.
pub fn screen(
    g: &mut GuiApp,
    ui: &mut Ui,
    sizes: &Sizes,
    id: &'static str,
    nav: impl FnOnce(&mut GuiApp, &mut Ui),
    inspector: impl FnOnce(&mut GuiApp, &mut Ui),
    main: impl FnOnce(&mut GuiApp, &mut Ui),
) {
    let theme = g.theme;
    if let Some(width) = sizes.views {
        egui::Panel::left(egui::Id::unique((id, "nav")))
            .exact_size(width)
            .resizable(false)
            .frame(theme.panel_frame())
            .show(ui, |ui| {
                ui.set_max_width(width);
                let edge = ui.max_rect();
                ui.painter().vline(
                    edge.right() - 0.5,
                    edge.y_range(),
                    Stroke::new(1.0, theme.palette.border),
                );
                egui::ScrollArea::vertical()
                    .id_salt((id, "nav_scroll"))
                    .max_width(ui.available_width())
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let room = ui.available_width();
                        ui.set_width(room);
                        ui.set_max_width(room);
                        nav(g, ui);
                    });
            });
    }
    if let Some(width) = sizes.inspector {
        egui::Panel::right(egui::Id::unique((id, "inspector")))
            .exact_size(width)
            .resizable(false)
            .frame(theme.panel_frame())
            .show(ui, |ui| {
                ui.set_max_width(width);
                let edge = ui.max_rect();
                ui.painter().vline(
                    edge.left() + 0.5,
                    edge.y_range(),
                    Stroke::new(1.0, theme.palette.border),
                );
                egui::ScrollArea::vertical()
                    .id_salt((id, "inspector_scroll"))
                    .max_width(ui.available_width())
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let room = ui.available_width();
                        ui.set_width(room);
                        ui.set_max_width(room);
                        inspector(g, ui);
                    });
            });
    }
    egui::CentralPanel::default()
        .frame(theme.content_frame().inner_margin(Margin {
            left: 24,
            right: 14,
            top: 20,
            bottom: 20,
        }))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt((id, "main_scroll"))
                .max_width(ui.available_width())
                .auto_shrink(false)
                .show(ui, |ui| {
                    // Room on the right for the scroll bar, over nothing.
                    let room = ui.available_width() - 10.0;
                    ui.set_width(room);
                    ui.set_max_width(room);
                    main(g, ui);
                    ui.add_space(8.0);
                });
        });
}

// ── Side panel ───────────────────────────────────────────────────────────────

/// The side panel's heading: the page's name over a line of context.
pub fn nav_header(ui: &mut Ui, theme: &Theme, title: &str, subtitle: &str) {
    egui::Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(semibold(title, theme.text_size + 1.0));
            ui.add(egui::Label::new(muted(theme, subtitle)).wrap());
        });
    rule(ui, theme);
}

/// A group of the side panel, under a small title.
pub fn nav_group(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 12,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        semibold(title, theme.text_size - 1.5).color(theme.palette.text_secondary),
                    );
                });
                ui.add_space(4.0);
            }
            body(ui);
        });
}

/// A note at the foot of the side panel, wrapped.
pub fn nav_note(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(muted(theme, text).size(theme.text_size - 1.5)).wrap());
        });
}

/// A view of the side panel: its label, a dot before it when given, a count
/// (or any short text) on the right; highlighted when `selected`.
pub fn nav_item(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    trailing: &str,
    dot: Option<Color32>,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let fill = if selected {
        theme.palette.selection
    } else if response.hovered() {
        theme.palette.raised
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    let mut x = rect.left() + 10.0;
    if let Some(color) = dot {
        ui.painter()
            .circle_filled(egui::pos2(x + 3.5, rect.center().y), 3.5, color);
        x += 15.0;
    }
    let trailing_width = if trailing.is_empty() {
        0.0
    } else {
        let galley = ui.painter().layout_no_wrap(
            trailing.to_owned(),
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_muted,
        );
        let width = galley.size().x;
        ui.painter().galley(
            egui::pos2(
                rect.right() - 10.0 - width,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme.palette.text_muted,
        );
        width + 12.0
    };
    let (font, color) = if selected {
        (fonts::medium(theme.text_size), theme.palette.text)
    } else {
        (
            FontId::proportional(theme.text_size),
            theme.palette.text_secondary,
        )
    };
    let galley = one_line(
        ui,
        label,
        font,
        color,
        rect.right() - 10.0 - trailing_width - x,
    );
    ui.painter().galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    focus_ring(ui, theme, &response, theme.radii.medium);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A filter of the side panel that is on or off: a box in `color`, checked
/// when `on`, its label (dimmed when off) and a count on the right.
pub fn nav_check(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    trailing: &str,
    color: Color32,
    on: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let boxed = Rect::from_center_size(
        egui::pos2(rect.left() + 17.0, rect.center().y),
        Vec2::splat(14.0),
    );
    if on {
        ui.painter()
            .rect_filled(boxed, CornerRadius::same(4), color);
        let mark = theme.palette.content;
        ui.painter().add(egui::Shape::line(
            vec![
                boxed.left_top() + Vec2::new(3.2, 7.2),
                boxed.left_top() + Vec2::new(6.0, 10.0),
                boxed.left_top() + Vec2::new(11.0, 4.2),
            ],
            Stroke::new(2.0, mark),
        ));
    } else {
        ui.painter().rect_stroke(
            boxed,
            CornerRadius::same(4),
            Stroke::new(1.5, color.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
    }
    let trailing_width = if trailing.is_empty() {
        0.0
    } else {
        12.0 + paint_text(
            ui,
            egui::pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            trailing,
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_muted,
            60.0,
        )
    };
    let color = if on {
        theme.palette.text
    } else {
        theme.palette.text_muted
    };
    let left = boxed.right() + 10.0;
    paint_text(
        ui,
        egui::pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.text_size),
        color,
        rect.right() - 10.0 - trailing_width - left,
    );
    focus_ring(ui, theme, &response, theme.radii.medium);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A segmented control as wide as the panel; the segment clicked, if any.
pub fn segmented(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    labels: &[&str],
    selected: usize,
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    let radius = CornerRadius::same(theme.radii.medium);
    ui.painter().rect_filled(rect, radius, theme.palette.input);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme.palette.border),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(2.0);
    let width = inner.width() / labels.len().max(1) as f32;
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let segment = Rect::from_min_size(
            egui::pos2(inner.left() + i as f32 * width, inner.top()),
            Vec2::new(width, inner.height()),
        );
        let response = ui.interact(segment, ui.scope_id().with((id, i)), Sense::click());
        let on = i == selected;
        if on {
            ui.painter()
                .rect_filled(segment, CornerRadius::same(4), theme.palette.raised);
        }
        let color = if on || response.hovered() {
            theme.palette.text
        } else {
            theme.palette.text_muted
        };
        let galley = one_line(
            ui,
            label,
            FontId::proportional(theme.text_size - 1.0),
            color,
            segment.width() - 6.0,
        );
        ui.painter()
            .galley(segment.center() - galley.size() / 2.0, galley, color);
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
            && !on
        {
            clicked = Some(i);
        }
    }
    clicked
}

/// A switch on its own line of the side panel, its label cut to the panel's
/// width (the whole label on hover when cut); whether it was clicked.
pub fn nav_switch(ui: &mut Ui, theme: &Theme, on: bool, label: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let how_on = ui.ctx().animate_bool(response.id, on);
    let track = Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.center().y - 8.5),
        Vec2::new(30.0, 17.0),
    );
    let idle = theme.palette.border;
    let fill = Color32::from_rgb(
        egui::lerp(
            f32::from(idle.r())..=f32::from(theme.palette.accent_fill.r()),
            how_on,
        ) as u8,
        egui::lerp(
            f32::from(idle.g())..=f32::from(theme.palette.accent_fill.g()),
            how_on,
        ) as u8,
        egui::lerp(
            f32::from(idle.b())..=f32::from(theme.palette.accent_fill.b()),
            how_on,
        ) as u8,
    );
    ui.painter().rect_filled(track, CornerRadius::same(9), fill);
    let x = egui::lerp(track.left() + 8.5..=track.right() - 8.5, how_on);
    ui.painter()
        .circle_filled(egui::pos2(x, track.center().y), 6.5, Color32::WHITE);
    let left = track.right() + 10.0;
    let galley = one_line(
        ui,
        label,
        FontId::proportional(theme.text_size),
        theme.palette.text,
        rect.right() - 6.0 - left,
    );
    let cut = galley.elided;
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        theme.palette.text,
    );
    focus_ring(ui, theme, &response, 9);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let response = if cut {
        response.on_hover_text(label)
    } else {
        response
    };
    response.clicked()
}

// ── Inspector ────────────────────────────────────────────────────────────────

/// A section of the inspector: a title (and a muted note on its right), the
/// body, a rule below.
pub fn section(ui: &mut Ui, theme: &Theme, title: &str, note: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                title_row(ui, theme, title, note, theme.text_size - 1.0);
                ui.add_space(8.0);
            }
            body(ui);
        });
    rule(ui, theme);
}

/// A title on the left, a muted note on the right, on one line. The title
/// keeps its room; the note takes what is left, cut short.
fn title_row(ui: &mut Ui, theme: &Theme, title: &str, note: &str, size: f32) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), size + 6.0), Sense::hover());
    let title_galley = one_line(
        ui,
        title,
        fonts::semibold(size),
        theme.palette.text_secondary,
        rect.width(),
    );
    let title_width = title_galley.size().x;
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - title_galley.size().y / 2.0),
        title_galley,
        theme.palette.text_secondary,
    );
    let room = rect.width() - title_width - 16.0;
    if !note.is_empty() && room > 30.0 {
        paint_text(
            ui,
            egui::pos2(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            note,
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_muted,
            room,
        );
    }
}

/// Caption / value lines, the captions in a column as wide as the longest
/// (half the width at most), the values cut to the rest.
pub fn facts(ui: &mut Ui, theme: &Theme, rows: &[(&str, String)]) {
    let width = ui.available_width();
    let font = FontId::proportional(theme.text_size);
    let caption_w = rows
        .iter()
        .map(|(c, _)| {
            ui.painter()
                .layout_no_wrap((*c).to_owned(), font.clone(), theme.palette.text_muted)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        .min(width * 0.5)
        + 14.0;
    for (caption, value) in rows {
        let (line, _) = ui.allocate_exact_size(Vec2::new(width, 23.0), Sense::hover());
        paint_text(
            ui,
            egui::pos2(line.left(), line.center().y),
            Align2::LEFT_CENTER,
            caption,
            font.clone(),
            theme.palette.text_muted,
            caption_w - 14.0,
        );
        paint_text(
            ui,
            egui::pos2(line.left() + caption_w, line.center().y),
            Align2::LEFT_CENTER,
            value,
            font.clone(),
            theme.palette.text,
            width - caption_w,
        );
    }
}

/// The inspector's top line for what was picked: `title` on the left, a cross
/// on the right to close it and get back to what the inspector shows by
/// default; whether the cross was clicked.
pub fn inspector_title(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    let mut closed = false;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 10,
            top: 10,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    semibold(title, theme.text_size - 1.0).color(theme.palette.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    closed = super::widgets::icon_button(
                        ui,
                        tl("Close"),
                        super::icons::Icon::Close,
                        &theme.icon_button(),
                        24.0,
                    )
                    .clicked();
                });
            });
        });
    closed
}

/// What the inspector says when nothing is picked.
pub fn inspector_empty(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(muted(theme, text)).wrap());
        });
}

// ── Main area ────────────────────────────────────────────────────────────────

/// The page's heading: title and subtitle on the left, `actions` on the right
/// (laid out right to left). The subtitle is cut to the room left.
pub fn heading(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut Ui),
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::hover());
    // The actions first, from the right, to know what is left for the title.
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 8.0;
    actions(&mut right);
    let used = right.min_rect().width();
    let room = (rect.width() - used - 16.0).max(80.0);
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                rect.min,
                Vec2::new(room, rect.height()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    left.style_mut().interaction.selectable_labels = false;
    left.spacing_mut().item_spacing.y = 4.0;
    left.add(egui::Label::new(semibold(title, theme.text_size + 5.0)).truncate());
    left.add(egui::Label::new(muted(theme, subtitle)).truncate());
}

/// `count` columns as wide as the room allows, `GAP` apart; `cell` draws column `i`.
pub fn columns(ui: &mut Ui, count: usize, mut cell: impl FnMut(&mut Ui, usize)) {
    let count = count.max(1);
    let width = ((ui.available_width() - GAP * (count - 1) as f32) / count as f32).floor();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for i in 0..count {
            ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                cell(ui, i);
            });
        }
    });
}

/// How many columns of at least `min` points fit, up to `most`.
pub fn fit(ui: &Ui, min: f32, most: usize) -> usize {
    (((ui.available_width() + GAP) / (min + GAP)).floor() as usize).clamp(1, most)
}

/// Two columns side by side when the room allows (the left one `ratio` of the
/// width), else one above the other.
pub fn split(
    ui: &mut Ui,
    ratio: f32,
    min_left: f32,
    min_right: f32,
    left: impl FnOnce(&mut Ui),
    right: impl FnOnce(&mut Ui),
) {
    let width = ui.available_width();
    let left_width = ((width - GAP) * ratio).floor();
    let right_width = width - GAP - left_width;
    if left_width >= min_left && right_width >= min_right {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(
                Vec2::new(left_width, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(left_width);
                    left(ui);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(right_width, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(right_width);
                    right(ui);
                },
            );
        });
    } else {
        left(ui);
        ui.add_space(GAP);
        right(ui);
    }
}

/// A card: a title with a muted note on its right, then the body. `min_height`
/// keeps cards of a row level.
pub fn card(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    note: &str,
    min_height: f32,
    body: impl FnOnce(&mut Ui),
) {
    theme
        .card_frame()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height((min_height - 28.0).max(0.0));
            if !title.is_empty() {
                title_row(ui, theme, title, note, theme.text_size);
                ui.add_space(10.0);
            }
            body(ui);
        });
}

/// A figure: its name, the value large, a caption under it in `caption_color`.
pub struct Figure {
    pub label: String,
    pub value: String,
    pub value_color: Option<Color32>,
    pub caption: String,
    pub caption_color: Option<Color32>,
}

/// A row of figures, as many per line as fit (each at least 170 points wide),
/// all the same height. Painted in exact rects: nothing in a figure can widen
/// its card.
pub fn figures(ui: &mut Ui, theme: &Theme, figures: &[Figure]) {
    let per_row = fit(ui, 170.0, figures.len().max(1));
    let width = ui.available_width();
    let card_w = ((width - GAP * (per_row - 1) as f32) / per_row as f32).floor();
    let height = 90.0;
    for chunk in figures.chunks(per_row) {
        let (line, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        for (i, f) in chunk.iter().enumerate() {
            let rect = Rect::from_min_size(
                egui::pos2(line.left() + i as f32 * (card_w + GAP), line.top()),
                Vec2::new(card_w, height),
            );
            ui.painter().rect(
                rect,
                CornerRadius::same(theme.radii.large),
                theme.palette.panel,
                Stroke::new(1.0, theme.palette.border_soft),
                egui::StrokeKind::Inside,
            );
            let inner = rect.shrink2(Vec2::new(16.0, 0.0));
            paint_text(
                ui,
                egui::pos2(inner.left(), rect.top() + 22.0),
                Align2::LEFT_CENTER,
                &f.label,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_muted,
                inner.width(),
            );
            paint_text(
                ui,
                egui::pos2(inner.left(), rect.top() + 49.0),
                Align2::LEFT_CENTER,
                &f.value,
                fonts::semibold(24.0),
                f.value_color.unwrap_or(theme.palette.text),
                inner.width(),
            );
            let caption = one_line(
                ui,
                &f.caption,
                FontId::proportional(theme.text_size - 1.5),
                f.caption_color.unwrap_or(theme.palette.text_muted),
                inner.width(),
            );
            let cut = caption.elided;
            let pos = egui::pos2(inner.left(), rect.top() + 75.0 - caption.size().y / 2.0);
            ui.painter().galley(
                pos,
                caption,
                f.caption_color.unwrap_or(theme.palette.text_muted),
            );
            // The whole caption on hover when it was cut.
            if cut {
                ui.interact(
                    rect,
                    ui.scope_id().with(("figure", f.label.as_str())),
                    Sense::hover(),
                )
                .on_hover_text(&f.caption);
            }
        }
        ui.add_space(GAP);
    }
}

/// What a card or a page says when it has nothing to show.
pub fn empty(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    ui.add(egui::Label::new(muted(theme, text)).wrap());
    ui.add_space(6.0);
}

// ── Painted pieces ───────────────────────────────────────────────────────────

/// `text` on one line, cut with an ellipsis to `max_width`.
pub fn one_line(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: max_width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Paints `text` cut to `max_width`, anchored at `pos` by `align`; its width.
pub fn paint_text(
    ui: &Ui,
    pos: egui::Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> f32 {
    let galley = one_line(ui, text, font, color, max_width);
    let size = galley.size();
    let rect = align.anchor_size(pos, size);
    let width = size.x;
    ui.painter().galley(rect.min, galley, color);
    width
}

/// A small rounded tag of `text` in `tone`, painted at `pos` (its left, its
/// vertical center); its width. Monospace for MR ids.
pub fn paint_tag(
    ui: &Ui,
    theme: &Theme,
    pos: egui::Pos2,
    text: &str,
    tone: Tone,
    mono: bool,
) -> f32 {
    paint_tag_within(ui, theme, pos, text, tone, mono, f32::INFINITY)
}

/// [`paint_tag`] no wider than `max_width`: its text cut short.
pub fn paint_tag_within(
    ui: &Ui,
    theme: &Theme,
    pos: egui::Pos2,
    text: &str,
    tone: Tone,
    mono: bool,
    max_width: f32,
) -> f32 {
    let font = if mono {
        FontId::monospace(theme.text_size - 2.0)
    } else {
        fonts::medium(theme.text_size - 2.5)
    };
    let galley = one_line(ui, text, font, tone.fg, (max_width - 12.0).max(10.0));
    let size = Vec2::new(galley.size().x + 12.0, 20.0);
    let rect = Rect::from_min_size(egui::pos2(pos.x, pos.y - size.y / 2.0), size);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.small), tone.bg);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, tone.fg);
    size.x
}

/// The tone of an MR id pill: the raised ground, the text color.
pub fn id_tone(theme: &Theme) -> Tone {
    Tone {
        bg: theme.palette.raised,
        fg: theme.palette.text,
    }
}

/// An MR id as a clickable pill, `!123`; with `title` on hover.
pub fn mr_pill(ui: &mut Ui, theme: &Theme, id: &str, title: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        format!("!{id}"),
        FontId::monospace(theme.text_size - 2.0),
        theme.palette.text,
    );
    let size = Vec2::new(galley.size().x + 12.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let stroke = if response.hovered() {
        Stroke::new(1.0, theme.palette.text_muted)
    } else {
        Stroke::NONE
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(theme.radii.small),
        theme.palette.raised,
        stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme.palette.text,
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if title.is_empty() {
        response
    } else {
        response.on_hover_text(title)
    }
}

/// A chip of `text` in `tone`, the size of the table's chips.
pub fn chip(ui: &mut Ui, theme: &Theme, text: &str, tone: Tone) -> egui::Response {
    ui.add(Chip::new(text, tone).font(fonts::medium(theme.text_size - 2.5)))
}

/// A bar split in `parts` (value, color), each segment as wide as its share,
/// `gap` points apart; the empty track when every value is zero.
pub fn stacked_bar(ui: &mut Ui, theme: &Theme, parts: &[(f32, Color32)], height: f32, gap: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    paint_stacked_bar(ui, theme, rect, parts, gap);
}

/// [`stacked_bar`] in a given rect.
pub fn paint_stacked_bar(ui: &Ui, theme: &Theme, rect: Rect, parts: &[(f32, Color32)], gap: f32) {
    let radius = CornerRadius::same((rect.height() / 2.0).min(255.0) as u8);
    let total: f32 = parts.iter().map(|p| p.0.max(0.0)).sum();
    if total <= 0.0 {
        ui.painter()
            .rect_filled(rect, radius, theme.palette.border_soft);
        return;
    }
    let shown: Vec<&(f32, Color32)> = parts.iter().filter(|p| p.0 > 0.0).collect();
    let room = rect.width() - gap * (shown.len() as f32 - 1.0).max(0.0);
    let mut x = rect.left();
    for (i, (value, color)) in shown.iter().enumerate() {
        let w = if i + 1 == shown.len() {
            rect.right() - x
        } else {
            (room * value / total).max(3.0)
        };
        let seg = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w, rect.height()));
        ui.painter().rect_filled(seg, radius, *color);
        x += w + gap;
    }
}

/// A line of colored dots and their captions, wrapping.
pub fn legend(ui: &mut Ui, theme: &Theme, items: &[(Color32, String)]) {
    // Measured and placed by hand: nested horizontal groups in a wrapping
    // layout would widen the panel instead of wrapping.
    let width = ui.available_width();
    let font = FontId::proportional(theme.text_size - 1.5);
    let line_height = 18.0;
    let galleys: Vec<(Color32, Arc<Galley>)> = items
        .iter()
        .map(|(color, text)| {
            (
                *color,
                one_line(
                    ui,
                    text,
                    font.clone(),
                    theme.palette.text_secondary,
                    width - 14.0,
                ),
            )
        })
        .collect();
    // Lines first, to allocate the height.
    let mut lines = 1;
    let mut x = 0.0;
    for (_, galley) in &galleys {
        let w = 14.0 + galley.size().x;
        if x > 0.0 && x + w > width {
            lines += 1;
            x = 0.0;
        }
        x += w + 14.0;
    }
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, lines as f32 * line_height + (lines - 1) as f32 * 4.0),
        Sense::hover(),
    );
    let (mut x, mut y) = (rect.left(), rect.top());
    for (color, galley) in galleys {
        let w = 14.0 + galley.size().x;
        if x > rect.left() && x + w > rect.right() {
            x = rect.left();
            y += line_height + 4.0;
        }
        let cy = y + line_height / 2.0;
        ui.painter()
            .circle_filled(egui::pos2(x + 4.0, cy), 4.0, color);
        ui.painter().galley(
            egui::pos2(x + 14.0, cy - galley.size().y / 2.0),
            galley,
            theme.palette.text_secondary,
        );
        x += w + 14.0;
    }
}

/// A row of a list: the full width, `height` tall, filled when hovered or
/// `selected`; the rect and the response.
pub fn row(ui: &mut Ui, theme: &Theme, height: f32, selected: bool) -> (Rect, egui::Response) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if selected {
        ui.painter().rect(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.selection,
            Stroke::new(1.0, theme.palette.accent.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    (
        rect,
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

/// A soft line across `rect`'s top.
pub fn row_rule(ui: &Ui, theme: &Theme, rect: Rect) {
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        Stroke::new(1.0, theme.palette.border_soft),
    );
}

/// A child ui laid out left to right inside `rect`, its labels not selectable:
/// to put widgets (pills, chips) in a painted row.
pub fn inside(ui: &mut Ui, rect: Rect) -> Ui {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.style_mut().interaction.selectable_labels = false;
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}

// ── Links in the exports ─────────────────────────────────────────────────────

/// Where the copied texts point: each MR to its GitLab page, each ticket to
/// the tracker's, so that `!123` and `#4567` can be clicked once pasted (a chat
/// that does not render Markdown still turns the URL in it into a link).
pub struct Links {
    mrs: std::collections::HashMap<String, String>,
    /// `https://gitlab/group/project/-/merge_requests/`, from any tracked MR:
    /// for the ids that are not tracked.
    prefix: Option<String>,
    tickets: std::collections::HashMap<String, String>,
    /// Each tracked MR's ticket, for the texts that name it next to the MR.
    mr_tickets: std::collections::HashMap<String, String>,
}

impl Links {
    pub fn of(app: &crate::app::App) -> Self {
        let mut mrs = std::collections::HashMap::new();
        let mut prefix = None;
        let mut tickets = std::collections::HashMap::new();
        let mut mr_tickets = std::collections::HashMap::new();
        for mr in app.mrs.iter().filter(|mr| !mr.web_url.is_empty()) {
            mrs.insert(mr.id.clone(), mr.web_url.clone());
            if prefix.is_none() {
                if let Some((base, _)) = mr.web_url.split_once("/-/merge_requests/") {
                    prefix = Some(format!("{base}/-/merge_requests/"));
                }
            }
            // The tickets the MRs name (only those: a `#` number in a title
            // is not always a ticket).
            if let Some(tracker) = &app.tracker {
                let id = mr
                    .linked_ticket
                    .as_ref()
                    .map(|t| t.id.clone())
                    .or_else(|| tracker.detect_ticket_id(&mr.title, &mr.description));
                if let Some(id) = id {
                    let url = mr
                        .linked_ticket
                        .as_ref()
                        .filter(|t| t.id == id && !t.url.is_empty())
                        .map(|t| t.url.clone())
                        .unwrap_or_else(|| tracker.ticket_url(&id));
                    mr_tickets.insert(mr.id.clone(), id.clone());
                    tickets.insert(id, url);
                }
            }
        }
        Self {
            mrs,
            prefix,
            tickets,
            mr_tickets,
        }
    }

    fn url(&self, sigil: char, id: &str) -> Option<String> {
        match sigil {
            '!' => self
                .mrs
                .get(id)
                .cloned()
                .or_else(|| self.prefix.as_ref().map(|p| format!("{p}{id}"))),
            _ => self.tickets.get(id).cloned(),
        }
    }

    /// `text` with each `!123` as `[!123](its MR)` and each known `#4567` as
    /// `[#4567](its ticket)`. The first time a line names an MR, its ticket follows
    /// it, `[!123](…) ([#4567](…))`, unless its line already names that
    /// ticket. What is already a link, or part of a word or a path, is left as
    /// it is.
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 64);
        for (n, line) in text.split('\n').enumerate() {
            if n > 0 {
                out.push('\n');
            }
            self.apply_line(line, &mut out);
        }
        out
    }

    fn apply_line(&self, line: &str, out: &mut String) {
        let mut named = std::collections::HashSet::new();
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let before = i.checked_sub(1).map(|j| chars[j]);
            let starts = (c == '!' || c == '#')
                && !before.is_some_and(|b| {
                    b.is_alphanumeric() || matches!(b, '[' | '/' | '&' | '_' | '!' | '#')
                });
            if starts {
                let digits: String = chars[i + 1..]
                    .iter()
                    .take_while(|d| d.is_ascii_digit())
                    .collect();
                let end = i + 1 + digits.len();
                let after = chars.get(end);
                let whole = !after.is_some_and(|a| a.is_alphanumeric() || matches!(a, '_' | ']'));
                if !digits.is_empty() && whole {
                    if let Some(url) = self.url(c, &digits) {
                        out.push_str(&format!("[{c}{digits}]({url})"));
                        if c == '!' && named.insert(digits.clone()) {
                            if let Some(ticket) = self.mr_tickets.get(&digits) {
                                let mentioned = mentions(line, ticket);
                                if let (false, Some(url)) = (mentioned, self.tickets.get(ticket)) {
                                    out.push_str(&format!(" ([#{ticket}]({url}))"));
                                }
                            }
                        }
                        i = end;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        }
    }
}

/// Whether `line` names ticket `#id` as a whole number.
fn mentions(line: &str, id: &str) -> bool {
    let reference = format!("#{id}");
    line.match_indices(&reference).any(|(at, _)| {
        !line[at + reference.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

// ── Time ─────────────────────────────────────────────────────────────────────

/// A GitLab timestamp (RFC 3339) as UTC.
pub fn parse_time(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// [`parse_time`] of an optional timestamp.
pub fn time_of(at: Option<&str>) -> Option<DateTime<Utc>> {
    at.and_then(parse_time)
}

/// Days (fractional) from `from` to `to`.
pub fn days_between(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    (to - from).num_minutes() as f64 / 1440.0
}

/// A short duration: "12 min", "3 h", "1.5 d", "12 d".
pub fn duration_text(days: f64) -> String {
    if days < 1.0 / 24.0 {
        let minutes = (days * 1440.0).round().max(1.0) as u64;
        tf("{n} min", &[("n", &minutes.to_string())])
    } else if days >= 365.0 {
        tf("{n} y", &[("n", &format!("{:.0}", (days / 365.0).floor()))])
    } else if days >= 60.0 {
        tf("{n} mo", &[("n", &format!("{:.0}", (days / 30.0).floor()))])
    } else {
        crate::next_step::days_text(days)
    }
}

/// "3 h ago"-like: [`duration_text`] of the time since `at`.
pub fn ago(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    tf(
        "{time} ago",
        &[("time", &duration_text(days_between(at, now).max(0.0)))],
    )
}

/// A whole number with a `%` sign.
pub fn percent(fraction: f64) -> String {
    format!("{:.0} %", fraction * 100.0)
}

/// A duration in seconds as "7 min 40" or "48 s".
pub fn seconds_text(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s < 60 {
        tf("{n} s", &[("n", &s.to_string())])
    } else {
        tf(
            "{m} min {s}",
            &[
                ("m", &(s / 60).to_string()),
                ("s", &format!("{:02}", s % 60)),
            ],
        )
    }
}

/// "Open in GitLab" for the URL, else nothing.
pub fn open_url(ui: &Ui, url: &str) {
    if !url.is_empty() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

/// "a person" → their first name: "Thomas Dubosc (@thomas_db)" → "Thomas".
pub fn first_name(display: &str) -> &str {
    crate::next_step::first_name(display)
}

/// Whether a person's display string or login looks like a bot account.
pub fn is_bot(display: &str) -> bool {
    let lower = display.to_lowercase();
    ["bot", "renovate", "dependabot", "gitlab-ci", "project_"]
        .iter()
        .any(|word| lower.contains(word))
}

/// A muted caption under a figure or a list, small.
pub fn caption(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add(egui::Label::new(muted(theme, text).size(theme.text_size - 1.5)).wrap());
}

/// Translated "—" placeholder for a missing value.
pub fn dash() -> &'static str {
    tl("—")
}

#[cfg(test)]
mod links_tests {
    use super::Links;

    fn links() -> Links {
        Links {
            mrs: [(
                "12".to_string(),
                "https://gl/p/-/merge_requests/12".to_string(),
            )]
            .into(),
            prefix: Some("https://gl/p/-/merge_requests/".into()),
            tickets: [("4567".to_string(), "https://rm/issues/4567".to_string())].into(),
            mr_tickets: [("12".to_string(), "4567".to_string())].into(),
        }
    }

    #[test]
    fn mr_and_ticket_numbers_become_links() {
        let l = links();
        assert_eq!(
            l.apply("- !12 Fix #4567 (2 d), !99 × !12"),
            "- [!12](https://gl/p/-/merge_requests/12) Fix [#4567](https://rm/issues/4567) (2 d), \
             [!99](https://gl/p/-/merge_requests/99) × [!12](https://gl/p/-/merge_requests/12)"
        );
    }

    #[test]
    fn an_mr_brings_its_ticket_on_each_line_that_names_it() {
        let l = links();
        assert_eq!(
            l.apply("- !12 Fix the login, !12 again\n- !12"),
            "- [!12](https://gl/p/-/merge_requests/12) ([#4567](https://rm/issues/4567)) \
             Fix the login, [!12](https://gl/p/-/merge_requests/12) again\n\
             - [!12](https://gl/p/-/merge_requests/12) ([#4567](https://rm/issues/4567))"
        );
        // Not when its line names the ticket already.
        assert_eq!(
            l.apply("- Fix (!12) · #4567"),
            "- Fix ([!12](https://gl/p/-/merge_requests/12)) · [#4567](https://rm/issues/4567)"
        );
    }

    #[test]
    fn what_is_not_a_reference_is_left_alone() {
        let l = links();
        for text in [
            "[!12](https://gl/p/-/merge_requests/12)", // already a link
            "#0000 unknown ticket",                    // not a ticket the MRs name
            "page#4567 and a/!12 path",                // part of a word, a path
            "!12a, Hi!, ##4567, !!12",                 // not whole numbers
            "### Heading",                             // Markdown, no number
        ] {
            assert_eq!(l.apply(text), text);
        }
        // A sentence's end is fine.
        assert_eq!(
            l.apply("merge !99."),
            "merge [!99](https://gl/p/-/merge_requests/99)."
        );
    }
}
