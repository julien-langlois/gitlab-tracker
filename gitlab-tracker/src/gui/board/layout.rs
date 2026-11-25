//! Laying a page out: sections, title rows, facts, the inspector's title, headings, columns, cards.

use super::*;

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
pub(super) fn title_row(ui: &mut Ui, theme: &Theme, title: &str, note: &str, size: f32) {
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
                    closed = super::super::widgets::icon_button(
                        ui,
                        tl("Close"),
                        super::super::icons::Icon::Close,
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
