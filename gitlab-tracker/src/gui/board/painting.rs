//! Painted text, tags, MR pills, chips, stacked bars, legends, rows.

use super::*;

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
    response.rich(Tip::new().mr(id, title))
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
