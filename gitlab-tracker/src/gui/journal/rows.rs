//! The journal's rows: day headings, the line of what was already seen, one event per row.

use super::*;

pub(super) fn day_heading(ui: &mut Ui, theme: &Theme, label: &str, first: bool) {
    if !first {
        ui.add_space(6.0);
    }
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let width = board::paint_text(
        ui,
        egui::pos2(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::semibold(theme.text_size - 1.0),
        theme.palette.text_secondary,
        rect.width() * 0.6,
    );
    ui.painter().hline(
        (rect.left() + width + 16.0)..=rect.right(),
        rect.center().y,
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
}

/// The line under which the events were already seen.
pub(super) fn seen_line(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let color = theme.palette.accent;
    let text = tl("Already seen below");
    let galley = board::one_line(
        ui,
        text,
        fonts::medium(theme.text_size - 2.0),
        color,
        rect.width(),
    );
    let w = galley.size().x;
    let cx = rect.center().x;
    let line = egui::Stroke::new(1.0, color.gamma_multiply(0.5));
    ui.painter()
        .hline(rect.left()..=(cx - w / 2.0 - 10.0), rect.center().y, line);
    ui.painter()
        .hline((cx + w / 2.0 + 10.0)..=rect.right(), rect.center().y, line);
    ui.painter().galley(
        egui::pos2(cx - w / 2.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

pub(super) enum RowClick {
    Select,
    Open,
}

pub(super) fn event_row(
    ui: &mut Ui,
    theme: &Theme,
    event: &Event,
    selected: bool,
    unread: bool,
    several: bool,
    now: DateTime<Utc>,
) -> Option<RowClick> {
    let (rect, response) = board::row(ui, theme, 46.0, selected);
    if !ui.is_rect_visible(rect) {
        return response.clicked().then_some(RowClick::Select);
    }
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
            CornerRadius {
                nw: 3,
                sw: 3,
                ne: 0,
                se: 0,
            },
            theme.palette.accent,
        );
    }
    let cy = rect.center().y;
    let left = rect.left() + 12.0;
    // The time.
    let time = event.when.with_timezone(&Local).format("%H:%M").to_string();
    board::paint_text(
        ui,
        egui::pos2(left, cy),
        Align2::LEFT_CENTER,
        &time,
        FontId::monospace(theme.text_size - 2.0),
        if unread {
            theme.palette.text
        } else {
            theme.palette.text_muted
        },
        44.0,
    );
    // The circle.
    let tone = event.tone.unwrap_or_else(|| event.kind.tone(theme));
    let center = egui::pos2(left + 64.0, cy);
    ui.painter().circle_filled(center, 13.0, tone.bg);
    if event.glyph == "▸" {
        // The font's triangle sits low and left in its box: drawn as a shape,
        // centred on its centroid, it sits in the middle of the circle.
        let (w, h) = (6.0, 8.0);
        let left = center.x - w / 3.0;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(left, center.y - h / 2.0),
                egui::pos2(left + w, center.y),
                egui::pos2(left, center.y + h / 2.0),
            ],
            tone.fg,
            egui::Stroke::NONE,
        ));
    } else {
        board::paint_text(
            ui,
            center,
            Align2::CENTER_CENTER,
            event.glyph,
            fonts::semibold(theme.text_size - 1.0),
            tone.fg,
            24.0,
        );
    }
    // On the right: the ago, the project.
    let ago_w = 76.0;
    board::paint_text(
        ui,
        egui::pos2(rect.right() - 12.0, cy),
        Align2::RIGHT_CENTER,
        &board::ago(event.when, now),
        FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
        ago_w,
    );
    let mut right = rect.right() - 12.0 - ago_w - 10.0;
    if several {
        let galley = ui.painter().layout_no_wrap(
            event.project.clone(),
            fonts::medium(theme.text_size - 2.5),
            theme.tones.gray.fg,
        );
        let w = (galley.size().x + 12.0).min(120.0);
        right -= w;
        board::paint_tag(
            ui,
            theme,
            egui::pos2(right, cy),
            &event.project,
            theme.tones.gray,
            false,
        );
        right -= 10.0;
    }
    // The text: id, what, chip, title; the detail below.
    let text_left = left + 92.0;
    let top = cy - 9.0;
    let mut x = text_left;
    x += board::paint_tag(
        ui,
        theme,
        egui::pos2(x, top),
        &format!("!{}", event.id),
        board::id_tone(theme),
        true,
    ) + 8.0;
    x += board::paint_text(
        ui,
        egui::pos2(x, top),
        Align2::LEFT_CENTER,
        &event.what,
        fonts::medium(theme.text_size - 0.5),
        theme.palette.text,
        (right - x).max(0.0),
    ) + 8.0;
    if let Some((label, tone)) = &event.chip {
        if x + 60.0 < right {
            x += board::paint_tag(ui, theme, egui::pos2(x, top), label, *tone, false) + 8.0;
        }
    }
    if x + 40.0 < right {
        board::paint_text(
            ui,
            egui::pos2(x, top),
            Align2::LEFT_CENTER,
            &event.title,
            FontId::proportional(theme.text_size - 1.0),
            theme.palette.text_muted,
            right - x,
        );
    }
    board::paint_text(
        ui,
        egui::pos2(text_left, cy + 11.0),
        Align2::LEFT_CENTER,
        &event.detail,
        FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
        (right - text_left).max(0.0),
    );
    let mut tip = Tip::new().mr(&event.id, &event.title);
    if let Some((label, tone)) = &event.chip {
        tip = tip.chip(label.as_str(), *tone);
    }
    let response = response.rich(
        tip.text(&event.detail)
            .hint(tl("Double click: show the MR")),
    );
    let key = ("journal", event.tab, &event.id, event.when, &event.what);
    if super::super::widgets::double_clicked(ui, &response, key) {
        Some(RowClick::Open)
    } else if response.clicked() {
        Some(RowClick::Select)
    } else {
        None
    }
}
