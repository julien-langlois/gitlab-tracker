//! Rows of figures, each with an optional note under its caption (what it was before, say), and the empty state.

use super::*;

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
    figures_with(ui, theme, figures, &[]);
}

/// [`figures`], each with a line under its caption when `notes` has one for
/// it (text and colour): what it was before, say.
pub fn figures_with(
    ui: &mut Ui,
    theme: &Theme,
    figures: &[Figure],
    notes: &[Option<(String, Color32)>],
) {
    let per_row = fit(ui, 170.0, figures.len().max(1));
    let width = ui.available_width();
    let card_w = ((width - GAP * (per_row - 1) as f32) / per_row as f32).floor();
    let noted = notes.iter().any(Option::is_some);
    let height = if noted { 110.0 } else { 90.0 };
    for (row, chunk) in figures.chunks(per_row).enumerate() {
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
            if let Some(Some((note, color))) = notes.get(row * per_row + i) {
                paint_text(
                    ui,
                    egui::pos2(inner.left(), rect.top() + 95.0),
                    Align2::LEFT_CENTER,
                    note,
                    FontId::proportional(theme.text_size - 1.5),
                    *color,
                    inner.width(),
                );
            }
            // The whole caption on hover when it was cut.
            if cut {
                ui.interact(
                    rect,
                    ui.scope_id().with(("figure", f.label.as_str())),
                    Sense::hover(),
                )
                .tip(&f.caption);
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
