//! The Flow tab: lead time from merge to each branch, time to merge by author, reviewer and milestone.

use super::*;

pub(super) fn flow(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    card(ui, theme, tl("Lead time from merge to branch"), |ui| {
        if report.promotions.is_empty() {
            ui.label(muted(
                theme,
                tl("No promotion to the other tracked branches recorded yet."),
            ));
            return;
        }
        let max = report
            .promotions
            .iter()
            .filter_map(|p| p.p90_hours.or(p.median_hours))
            .fold(0.0, f64::max)
            .max(f64::EPSILON);
        ui.spacing_mut().item_spacing.y = 10.0;
        for p in &report.promotions {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(120.0, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        // Same as `bar_row`: the bars line up whatever the branch's name.
                        ui.set_min_width(120.0);
                        ui.add(egui::Label::new(mono(&p.branch, theme.text_size - 0.5)).truncate());
                    },
                );
                let width = (ui.available_width() - 190.0).max(60.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
                let radius = CornerRadius::same(4);
                ui.painter()
                    .rect_filled(rect, radius, theme.palette.content);
                let mut p90 = rect;
                p90.set_width(rect.width() * (p.p90_hours.unwrap_or(0.0) / max) as f32);
                ui.painter()
                    .rect_filled(p90, radius, theme.palette.accent.gamma_multiply(0.3));
                let mut median = rect;
                median.set_width(rect.width() * (p.median_hours.unwrap_or(0.0) / max) as f32);
                ui.painter()
                    .rect_filled(median, radius, theme.palette.accent);
                ui.label(
                    RichText::new(tf(
                        "median {median} · P90 {p90}",
                        &[
                            ("median", &hours(p.median_hours)),
                            ("p90", &hours(p.p90_hours)),
                        ],
                    ))
                    .color(theme.palette.text_secondary),
                );
            });
        }
        ui.add_space(4.0);
        super::super::widgets::rule(ui, theme);
        egui::Grid::new("promotions")
            .num_columns(6)
            .spacing(Vec2::new(24.0, 8.0))
            .show(ui, |ui| {
                for title in ["Branch", "MRs", "Median", "P90", "Max", "Via copy"] {
                    let title = tl(title);
                    ui.label(muted(theme, title));
                }
                ui.end_row();
                for p in &report.promotions {
                    ui.label(mono(&p.branch, theme.text_size - 1.0));
                    ui.label(p.arrivals.to_string());
                    ui.label(hours(p.median_hours));
                    ui.label(hours(p.p90_hours));
                    ui.label(hours(p.max_hours));
                    ui.label(format!("{:.0}%", p.copied_share * 100.0));
                    ui.end_row();
                }
            });
    });
    let a = &report.aggregated;
    let per_row = fit(ui, 300.0, 3);
    let lists = [
        (
            tl("Time to merge by author"),
            top(&a.cycle_time_by_author, 8),
            theme.palette.accent,
        ),
        (
            tl("Time to merge by reviewer"),
            top(&a.cycle_time_by_reviewer, 8),
            theme.tones.teal.fg,
        ),
        (
            tl("Time to merge by milestone"),
            top(&a.cycle_time_by_milestone, 8),
            theme.tones.purple.fg,
        ),
    ];
    for chunk in lists.chunks(per_row) {
        columns(ui, per_row, |ui, i| {
            if let Some((title, entries, color)) = chunk.get(i) {
                bar_list(ui, theme, title, entries, *color, |h| hours(Some(h)));
            }
        });
    }
    let mut milestones: Vec<(String, f64)> = a
        .throughput_by_milestone
        .iter()
        .map(|(k, v)| (k.clone(), f64::from(*v)))
        .collect();
    milestones.sort_by(|x, y| y.1.total_cmp(&x.1));
    milestones.truncate(10);
    bar_list(
        ui,
        theme,
        tl("Merged MRs by milestone"),
        &milestones,
        theme.tones.green.fg,
        |v| format!("{v:.0}"),
    );
}

// ── Quality ──────────────────────────────────────────────────────────────────
