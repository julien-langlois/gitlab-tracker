//! The personas' pieces, shared by Settings › Pages and › Startup and by the
//! first launch: the persona cards and chips, the rail's preview, a page's
//! switch, the start page's radio cards and pills.

use bevel_theme::{fonts, Theme};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use super::{page_about, page_label, Persona, Rail, PAGES, TRACKER};
use crate::gui::icons::{self, Icon};
use crate::gui::tip::TipExt;
use crate::gui::widgets::{focus_ring, switch_track};
use crate::i18n::{tf, tl};

/// `text` with `strong` in it set in the primary color and semibold, the rest
/// in `color`: "Tech lead opens on **Today**."
pub fn emphasised(theme: &Theme, text: &str, strong: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let plain = TextFormat::simple(egui::FontId::proportional(size), color);
    let bold = TextFormat::simple(fonts::semibold(size), theme.palette.text);
    match text.find(strong).filter(|_| !strong.is_empty()) {
        Some(at) => {
            job.append(&text[..at], 0.0, plain.clone());
            job.append(strong, 0.0, bold);
            job.append(&text[at + strong.len()..], 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job
}

/// The height a persona card needs at `width`: its tile, name and blurb.
pub fn persona_card_height(ui: &Ui, theme: &Theme, persona: Persona, width: f32) -> f32 {
    let inner = width - 32.0;
    let name = ui.painter().layout(
        persona.label().to_string(),
        fonts::semibold(theme.text_size),
        Color32::WHITE,
        inner,
    );
    let blurb = ui.painter().layout(
        persona.blurb().to_string(),
        egui::FontId::proportional(theme.text_size - 1.5),
        Color32::WHITE,
        inner,
    );
    16.0 + 32.0 + 12.0 + name.size().y + 6.0 + blurb.size().y + 16.0
}

/// A persona on the first launch: its icon on a tile, its name, what it looks
/// at; `size` large (see [`persona_card_height`]).
pub fn persona_card(
    ui: &mut Ui,
    theme: &Theme,
    persona: Persona,
    on: bool,
    size: Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, persona.label())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let p = &theme.palette;
    let radius = CornerRadius::same(theme.radii.large);
    let (fill, stroke) = if on {
        (p.selection, p.accent)
    } else if response.hovered() {
        (p.raised, p.text_muted)
    } else {
        (p.panel, p.border)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(if on { 1.5 } else { 1.0 }, stroke),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(16.0);
    let tile = Rect::from_min_size(inner.min, Vec2::splat(32.0));
    painter.rect_filled(
        tile,
        CornerRadius::same(theme.radii.medium + 2),
        if on { theme.tones.blue.bg } else { p.raised },
    );
    icons::paint(
        painter,
        tile.shrink(7.0),
        Icon::People,
        if on { p.accent } else { p.text_secondary },
    );
    // The check, top right, on the one picked.
    if on {
        let center = egui::pos2(inner.right() - 8.0, inner.top() + 8.0);
        painter.circle_filled(center, 9.0, p.accent_fill);
        painter.line_segment(
            [center + Vec2::new(-4.0, 0.0), center + Vec2::new(-1.0, 3.0)],
            Stroke::new(1.8, Color32::WHITE),
        );
        painter.line_segment(
            [center + Vec2::new(-1.0, 3.0), center + Vec2::new(4.5, -3.0)],
            Stroke::new(1.8, Color32::WHITE),
        );
    }
    let name = painter.layout(
        persona.label().to_string(),
        fonts::semibold(theme.text_size),
        p.text,
        inner.width(),
    );
    let name_top = tile.bottom() + 12.0;
    let name_height = name.size().y;
    painter.galley(egui::pos2(inner.left(), name_top), name, p.text);
    let blurb = painter.layout(
        persona.blurb().to_string(),
        egui::FontId::proportional(theme.text_size - 1.5),
        p.text_secondary,
        inner.width(),
    );
    painter.galley(
        egui::pos2(inner.left(), name_top + name_height + 6.0),
        blurb,
        p.text_secondary,
    );
    focus_ring(ui, theme, &response, theme.radii.large);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A persona in Settings › Pages: a chip of a radio group.
pub fn choice_chip(ui: &mut Ui, theme: &Theme, text: &str, on: bool) -> egui::Response {
    let p = &theme.palette;
    let font = if on {
        fonts::semibold(theme.text_size - 0.5)
    } else {
        fonts::medium(theme.text_size - 0.5)
    };
    let color = if on { p.text } else { p.text_secondary };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = Vec2::new(galley.size().x + 26.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, text));
    if ui.is_rect_visible(rect) {
        let radius = CornerRadius::same(theme.radii.medium);
        let (fill, stroke) = if on {
            (p.selection, p.accent)
        } else if response.hovered() {
            (p.raised, p.text_muted)
        } else {
            (Color32::TRANSPARENT, p.border)
        };
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0, stroke), StrokeKind::Inside);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    focus_ring(ui, theme, &response, theme.radii.medium);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A start page among the rail's: a pill, filled with the accent when picked.
pub fn pick_pill(ui: &mut Ui, theme: &Theme, text: &str, on: bool) -> egui::Response {
    let p = &theme.palette;
    let color = if on { Color32::WHITE } else { p.text_secondary };
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), fonts::medium(theme.text_size - 1.0), color);
    let size = Vec2::new(galley.size().x + 24.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, text));
    if ui.is_rect_visible(rect) {
        let radius = CornerRadius::same(15);
        let (fill, stroke) = if on {
            (p.accent_fill, p.accent_fill)
        } else if response.hovered() {
            (p.raised, p.text_muted)
        } else {
            (Color32::TRANSPARENT, p.border)
        };
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0, stroke), StrokeKind::Inside);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    focus_ring(ui, theme, &response, 15);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// What the rail will show, under the persona cards: the pages in it, the
/// others struck through, and the page the window opens on.
pub fn rail_preview(ui: &mut Ui, theme: &Theme, rail: &Rail) {
    let p = &theme.palette;
    let pages: Vec<&str> = PAGES
        .iter()
        .copied()
        .filter(|k| *k != TRACKER && (cfg!(feature = "stats") || *k != "stats"))
        .collect();
    let shown = pages.iter().filter(|k| rail.shows(k)).count();
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                ui.label(crate::gui::widgets::semibold(
                    tf(
                        "In your rail · {n} of {total} pages",
                        &[("n", &shown.to_string()), ("total", &pages.len().to_string())],
                    ),
                    theme.text_size - 0.5,
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let opens = page_label(rail.opening_page(None));
                    let text = tf("Opens on {page}", &[("page", opens)]);
                    ui.label(emphasised(
                        theme,
                        &text,
                        opens,
                        theme.text_size - 1.5,
                        p.text_secondary,
                    ));
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for key in &pages {
                    rail_chip(ui, theme, page_label(key), rail.shows(key));
                }
            });
            ui.label(
                crate::gui::widgets::muted(
                    theme,
                    tl("Greyed pages stay reachable with Ctrl+K. Tracker shows whenever a ticket tracker is linked."),
                )
                .size(theme.text_size - 1.5),
            );
        });
}

/// A page of the preview: in the rail (blue), or not (struck through, dashed).
fn rail_chip(ui: &mut Ui, theme: &Theme, text: &str, on: bool) {
    let p = &theme.palette;
    let tone = theme.tones.blue;
    let color = if on { tone.fg } else { p.text_muted };
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), fonts::medium(theme.text_size - 1.5), color);
    let size = Vec2::new(galley.size().x + 18.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Label,
            true,
            if on {
                tf("{page}: in your rail", &[("page", text)])
            } else {
                tf("{page}: hidden", &[("page", text)])
            },
        )
    });
    let radius = CornerRadius::same(12);
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, radius, tone.bg);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tone.fg.gamma_multiply(0.45)),
            StrokeKind::Inside,
        );
    } else {
        // Dashed: a page left out, still one Ctrl+K away.
        let points = [
            rect.left_top() + Vec2::new(12.0, 0.5),
            rect.right_top() + Vec2::new(-12.0, 0.5),
        ];
        painter.add(egui::Shape::dashed_line(
            &points,
            Stroke::new(1.0, p.border),
            4.0,
            3.0,
        ));
        let bottom = [
            rect.left_bottom() + Vec2::new(12.0, -0.5),
            rect.right_bottom() + Vec2::new(-12.0, -0.5),
        ];
        painter.add(egui::Shape::dashed_line(
            &bottom,
            Stroke::new(1.0, p.border),
            4.0,
            3.0,
        ));
        for x in [rect.left() + 0.5, rect.right() - 0.5] {
            let side = [
                egui::pos2(x, rect.top() + 6.0),
                egui::pos2(x, rect.bottom() - 6.0),
            ];
            painter.add(egui::Shape::dashed_line(
                &side,
                Stroke::new(1.0, p.border),
                3.0,
                3.0,
            ));
        }
    }
    let at = rect.center() - galley.size() / 2.0;
    let width = galley.size().x;
    let middle = at.y + galley.size().y / 2.0;
    painter.galley(at, galley, color);
    if !on {
        painter.line_segment(
            [egui::pos2(at.x, middle), egui::pos2(at.x + width, middle)],
            Stroke::new(1.0, p.text_muted),
        );
    }
}

/// A page's switch in Settings › Pages: the switch, its name and what it is
/// for. The start page's is greyed, the tracker's follows the configuration.
pub fn page_switch(ui: &mut Ui, theme: &Theme, rail: &Rail, key: &str) -> egui::Response {
    let p = &theme.palette;
    let locked = rail.locked(key);
    let fixed = locked || key == TRACKER;
    let on = rail.shows(key);
    let about = if locked {
        tl("Your start page · change it in Startup")
    } else {
        page_about(key)
    };
    let width = ui.available_width();
    let name_font = fonts::medium(theme.text_size - 0.5);
    let about_font = egui::FontId::proportional(theme.text_size - 1.5);
    let text_width = width - 30.0 - 12.0 - 20.0;
    let name = ui
        .painter()
        .layout(page_label(key).to_string(), name_font, p.text, text_width);
    let about_galley = ui.painter().layout(
        about.to_string(),
        about_font,
        if locked {
            p.text_secondary
        } else {
            p.text_muted
        },
        text_width,
    );
    let height = (name.size().y + about_galley.size().y + 2.0 + 16.0).max(48.0);
    let sense = if fixed {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, !fixed, on, page_label(key))
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && !fixed {
            painter.rect_filled(rect, CornerRadius::same(theme.radii.medium + 2), p.raised);
        }
        let how_on = ui.ctx().animate_bool(response.id, on);
        let left = egui::pos2(
            rect.left() + 10.0,
            rect.top() + 8.0 + name.size().y / 2.0 + 1.0,
        );
        let track = switch_track(ui, theme, left, how_on);
        if locked {
            // Greyed: on, and held there while it is the start page.
            ui.painter()
                .rect_filled(track, CornerRadius::same(9), p.raised.gamma_multiply(0.55));
        }
        let text_left = track.right() + 12.0;
        let name_height = name.size().y;
        ui.painter()
            .galley(egui::pos2(text_left, rect.top() + 8.0), name, p.text);
        ui.painter().galley(
            egui::pos2(text_left, rect.top() + 8.0 + name_height + 2.0),
            about_galley,
            p.text_muted,
        );
    }
    focus_ring(ui, theme, &response, theme.radii.medium + 2);
    if locked {
        response
            .on_hover_cursor(egui::CursorIcon::NotAllowed)
            .tip(tl(
                "The start page stays in the rail. Pick another one in Startup to hide it.",
            ))
    } else if fixed {
        response.tip(tl(
            "Shown whenever a ticket tracker is linked to the project.",
        ))
    } else {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

/// A start option in Settings › Startup: its radio, its title and what it does.
pub fn radio_card(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    detail: LayoutJob,
    on: bool,
) -> egui::Response {
    let p = &theme.palette;
    let width = ui.available_width();
    let mut detail = detail;
    detail.wrap.max_width = width - 16.0 - 18.0 - 12.0 - 16.0;
    let detail = ui.painter().layout_job(detail);
    let title_galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), fonts::semibold(theme.text_size), p.text);
    let height = 14.0 + title_galley.size().y + 4.0 + detail.size().y + 14.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, title));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme.radii.large);
        let (fill, stroke) = if on {
            (p.selection, p.accent)
        } else if response.hovered() {
            (p.raised, p.text_muted)
        } else {
            (p.panel, p.border)
        };
        painter.rect_filled(rect, radius, fill);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(if on { 1.5 } else { 1.0 }, stroke),
            StrokeKind::Inside,
        );
        let dot = egui::pos2(
            rect.left() + 16.0 + 8.0,
            rect.top() + 14.0 + title_galley.size().y / 2.0,
        );
        if on {
            painter.circle_filled(dot, 8.0, p.accent_fill);
            painter.circle_filled(dot, 3.5, Color32::WHITE);
        } else {
            painter.circle_stroke(dot, 7.0, Stroke::new(1.5, p.text_muted));
        }
        let left = rect.left() + 16.0 + 18.0 + 12.0;
        let title_height = title_galley.size().y;
        painter.galley(egui::pos2(left, rect.top() + 14.0), title_galley, p.text);
        painter.galley(
            egui::pos2(left, rect.top() + 14.0 + title_height + 4.0),
            detail,
            p.text_secondary,
        );
    }
    focus_ring(ui, theme, &response, theme.radii.large);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
