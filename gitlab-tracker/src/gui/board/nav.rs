//! The pages' left panel: headers, groups, notes, items, checks, segmented controls, switches.

use super::*;

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
    let response = if cut { response.tip(label) } else { response };
    response.clicked()
}
