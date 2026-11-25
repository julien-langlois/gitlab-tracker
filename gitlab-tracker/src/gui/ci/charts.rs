//! The daily median duration of the pipelines, and the stages card.

use super::*;

/// The median duration of each day's pipelines, the period's median dashed.
pub(super) fn durations_card(ui: &mut Ui, theme: &Theme, ci: &Ci) {
    let note = ci
        .median
        .map(|m| tf("median {d}", &[("d", &board::seconds_text(m))]))
        .unwrap_or_default();
    board::card(ui, theme, tl("Pipeline durations"), &note, 210.0, |ui| {
        let max = ci.days.iter().filter_map(|d| d.1).fold(0.0_f64, f64::max);
        if max <= 0.0 {
            board::empty(ui, theme, tl("No finished pipeline in the period."));
            return;
        }
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), Sense::hover());
        let n = ci.days.len().max(1);
        let gap = if n > 20 { 3.0 } else { 5.0 };
        let w = (rect.width() - gap * (n - 1) as f32) / n as f32;
        let top = max * 1.15;
        let median = ci.median.unwrap_or(0.0);
        for (i, (day, value, count)) in ci.days.iter().enumerate() {
            let x = rect.left() + i as f32 * (w + gap);
            let column =
                Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + w, rect.bottom()));
            match value {
                Some(v) => {
                    let h = (rect.height() * (*v / top) as f32).max(3.0);
                    let bar = Rect::from_min_max(
                        egui::pos2(x, rect.bottom() - h),
                        egui::pos2(x + w, rect.bottom()),
                    );
                    let color = if *v > median * 1.2 {
                        theme.tones.amber.fg
                    } else {
                        theme.palette.accent
                    };
                    ui.painter().rect_filled(
                        bar,
                        CornerRadius {
                            nw: 3,
                            ne: 3,
                            sw: 0,
                            se: 0,
                        },
                        color,
                    );
                }
                None => {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            egui::pos2(x, rect.bottom() - 2.0),
                            egui::pos2(x + w, rect.bottom()),
                        ),
                        CornerRadius::same(1),
                        theme.palette.border,
                    );
                }
            }
            let tip = match value {
                Some(v) => format!(
                    "{} · {} · {}",
                    day.format("%d/%m"),
                    board::seconds_text(*v),
                    tn(*count as u64, "{n} pipeline", "{n} pipelines")
                ),
                None => format!("{} · {}", day.format("%d/%m"), tl("no pipeline")),
            };
            ui.interact(column, ui.scope_id().with(("day", i)), Sense::hover())
                .tip(tip);
        }
        if median > 0.0 {
            let y = rect.bottom() - rect.height() * (median / top) as f32;
            let mut x = rect.left();
            while x < rect.right() {
                ui.painter().hline(
                    x..=(x + 5.0).min(rect.right()),
                    y,
                    Stroke::new(1.0, theme.tones.amber.fg),
                );
                x += 9.0;
            }
        }
        let (axis, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        if let (Some(first), Some(last)) = (ci.days.first(), ci.days.last()) {
            let font = FontId::proportional(theme.text_size - 2.5);
            board::paint_text(
                ui,
                egui::pos2(axis.left(), axis.center().y),
                Align2::LEFT_CENTER,
                &first.0.format("%d/%m").to_string(),
                font.clone(),
                theme.palette.text_muted,
                60.0,
            );
            board::paint_text(
                ui,
                egui::pos2(axis.right(), axis.center().y),
                Align2::RIGHT_CENTER,
                &last.0.format("%d/%m").to_string(),
                font,
                theme.palette.text_muted,
                60.0,
            );
        }
    });
}

pub(super) fn stages_card(ui: &mut Ui, theme: &Theme, ci: &Ci) {
    board::card(ui, theme, tl("Failures by stage"), "", 210.0, |ui| {
        if ci.stages.is_empty() {
            board::empty(ui, theme, tl("No failed job in the period."));
            return;
        }
        let mut stages: Vec<(&String, &usize)> = ci.stages.iter().collect();
        stages.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let max = *stages[0].1 as f32;
        for (stage, n) in &stages {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
            let label_w = 70.0;
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                stage,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_secondary,
                label_w - 6.0,
            );
            let count = n.to_string();
            board::paint_text(
                ui,
                egui::pos2(line.right(), line.center().y),
                Align2::RIGHT_CENTER,
                &count,
                fonts::medium(theme.text_size - 1.5),
                theme.palette.text_muted,
                30.0,
            );
            let track = Rect::from_min_size(
                egui::pos2(line.left() + label_w, line.center().y - 5.0),
                Vec2::new(line.width() - label_w - 36.0, 10.0),
            );
            ui.painter()
                .rect_filled(track, CornerRadius::same(5), theme.palette.border_soft);
            let mut fill = track;
            fill.set_width(track.width() * **n as f32 / max);
            ui.painter()
                .rect_filled(fill, CornerRadius::same(5), theme.tones.red.fg);
        }
        // The job that fails the most, and its share of its stage.
        let top = ci
            .jobs
            .iter()
            .max_by_key(|j| j.failures())
            .filter(|j| j.failures() > 0);
        if let Some(job) = top {
            let of_stage = ci.stages.get(&job.stage).copied().unwrap_or(0).max(1);
            ui.add_space(6.0);
            board::caption(
                ui,
                theme,
                &tf(
                    "{job} makes {share} of the failures of the {stage} stage.",
                    &[
                        ("job", &job.name),
                        (
                            "share",
                            &board::percent(job.failures() as f64 / of_stage as f64),
                        ),
                        ("stage", &job.stage),
                    ],
                ),
            );
        }
    });
}
