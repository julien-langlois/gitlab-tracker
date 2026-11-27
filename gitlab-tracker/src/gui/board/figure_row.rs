//! Rows of figures, each with an optional note under its caption (what it was
//! before, say) and an optional line of its past at the bottom, and the empty
//! state.

use super::*;

/// A figure: its name, the value large, a caption under it in `caption_color`.
pub struct Figure {
    pub label: String,
    pub value: String,
    pub value_color: Option<Color32>,
    pub caption: String,
    pub caption_color: Option<Color32>,
}

/// A figure's past, one point a period, oldest first: drawn at the bottom of
/// its card, a gap (no point) bridged by a dashed line.
pub struct Spark {
    pub points: Vec<Option<f64>>,
    /// Under the line's ends: its first and last period.
    pub first: String,
    pub last: String,
    /// What each point says on hover.
    pub tips: Vec<String>,
    pub color: Color32,
}

/// The height a spark adds to its card.
const SPARK_HEIGHT: f32 = 50.0;

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
    figures_full(ui, theme, figures, notes, &[]);
}

/// [`figures_with`], each with its past at the bottom when `sparks` has one.
pub fn figures_full(
    ui: &mut Ui,
    theme: &Theme,
    figures: &[Figure],
    notes: &[Option<(String, Color32)>],
    sparks: &[Option<Spark>],
) {
    let per_row = fit(ui, 170.0, figures.len().max(1));
    let width = ui.available_width();
    let card_w = ((width - GAP * (per_row - 1) as f32) / per_row as f32).floor();
    let noted = notes.iter().any(Option::is_some);
    let base = if noted { 110.0 } else { 90.0 };
    let height = if sparks.iter().any(Option::is_some) {
        base + SPARK_HEIGHT
    } else {
        base
    };
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
            if let Some(Some(spark)) = sparks.get(row * per_row + i) {
                let band = Rect::from_min_size(
                    egui::pos2(inner.left(), rect.top() + base - 2.0),
                    Vec2::new(inner.width(), SPARK_HEIGHT - 12.0),
                );
                paint_spark(ui, theme, band, spark, &f.label);
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

/// `spark` in `band`: the line over its upper part, the first and last
/// periods under its ends; on hover, the nearest point's tip.
fn paint_spark(ui: &mut Ui, theme: &Theme, band: Rect, spark: &Spark, id: &str) {
    let line = Rect::from_min_max(band.min, egui::pos2(band.right(), band.bottom() - 14.0))
        .shrink2(Vec2::new(3.0, 3.0));
    let n = spark.points.len();
    let values: Vec<f64> = spark.points.iter().flatten().copied().collect();
    let (Some(low), Some(high)) = (
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
    ) else {
        return;
    };
    let span = if high - low > f64::EPSILON {
        high - low
    } else {
        1.0
    };
    let x = |i: usize| {
        if n < 2 {
            line.center().x
        } else {
            line.left() + line.width() * i as f32 / (n - 1) as f32
        }
    };
    let y = |v: f64| {
        let share = if high - low > f64::EPSILON {
            ((v - low) / span) as f32
        } else {
            0.5
        };
        line.bottom() - line.height() * share
    };
    let points: Vec<(usize, egui::Pos2)> = spark
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, egui::pos2(x(i), y(v)))))
        .collect();
    let painter = ui.painter();
    let solid = Stroke::new(1.6, spark.color);
    for pair in points.windows(2) {
        let ((a, from), (b, to)) = (pair[0], pair[1]);
        if b == a + 1 {
            painter.line_segment([from, to], solid);
        } else {
            // Months without a point between: dashed across.
            painter.extend(egui::Shape::dashed_line(
                &[from, to],
                Stroke::new(1.0, spark.color.gamma_multiply(0.6)),
                3.0,
                3.0,
            ));
        }
    }
    let last = points.last().map(|(i, _)| *i);
    for (i, at) in &points {
        let radius = if Some(*i) == last { 3.2 } else { 2.0 };
        painter.circle_filled(*at, radius, spark.color);
    }
    let small = FontId::proportional(theme.text_size - 3.0);
    let half = band.width() / 2.0 - 4.0;
    paint_text(
        ui,
        egui::pos2(band.left(), band.bottom() - 5.0),
        Align2::LEFT_CENTER,
        &spark.first,
        small.clone(),
        theme.palette.text_muted,
        half,
    );
    paint_text(
        ui,
        egui::pos2(band.right(), band.bottom() - 5.0),
        Align2::RIGHT_CENTER,
        &spark.last,
        small,
        theme.palette.text_muted,
        half,
    );
    let response = ui.interact(band, ui.scope_id().with(("spark", id)), Sense::hover());
    if let Some(pointer) = response.hover_pos() {
        let nearest = (0..n).min_by(|a, b| {
            (x(*a) - pointer.x)
                .abs()
                .total_cmp(&(x(*b) - pointer.x).abs())
        });
        if let Some(i) = nearest {
            ui.painter().line_segment(
                [
                    egui::pos2(x(i), line.top()),
                    egui::pos2(x(i), line.bottom()),
                ],
                Stroke::new(1.0, theme.palette.border),
            );
            if let Some(tip) = spark.tips.get(i) {
                response.tip(tip);
            }
        }
    }
}

/// What a card or a page says when it has nothing to show.
pub fn empty(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    ui.add(egui::Label::new(muted(theme, text)).wrap());
    ui.add_space(6.0);
}
