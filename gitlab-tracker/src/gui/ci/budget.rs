//! The CI budget card: the runner time the pipelines took, what failed ones spent, the wait for a runner.

use super::*;

/// The runner time the pipelines took: in all and against the period before,
/// what failed pipelines spent, the wait for a runner, by day and by job.
pub(super) fn budget_card(ui: &mut Ui, theme: &Theme, ci: &Ci) {
    let b = &ci.budget;
    let note = b
        .per_pipeline
        .map(|s| tf("{d} a pipeline", &[("d", &board::seconds_text(s))]))
        .unwrap_or_default();
    board::card(ui, theme, tl("CI budget"), &note, 0.0, |ui| {
        if b.runner_secs <= 0.0 {
            board::empty(ui, theme, tl("No job ran in the period."));
            return;
        }
        let t = &theme.tones;
        // Four figures on a line, painted in exact rects.
        let delta = (b.runner_before > 0.0).then(|| b.runner_secs / b.runner_before - 1.0);
        let failed_share = b.failed_secs / b.runner_secs;
        let items: [(String, String, String, Option<Color32>); 4] = [
            (
                tl("Runner time").to_string(),
                hours_text(b.runner_secs),
                match delta {
                    Some(d) if d.abs() >= 0.05 => tf(
                        "{n} on the period before",
                        &[("n", &format!("{:+.0} %", d * 100.0))],
                    ),
                    Some(_) => tl("as before").to_string(),
                    None => tl("nothing to compare with").to_string(),
                },
                delta.filter(|d| d.abs() >= 0.05).map(|d| {
                    if d > 0.0 {
                        t.amber.fg
                    } else {
                        t.green.fg
                    }
                }),
            ),
            (
                tl("Spent on failed pipelines").to_string(),
                hours_text(b.failed_secs),
                tf(
                    "{share} of the runner time",
                    &[("share", &board::percent(failed_share))],
                ),
                (failed_share >= 0.2).then_some(t.red.fg),
            ),
            (
                tl("Wait for a runner").to_string(),
                b.queued
                    .map_or_else(|| board::dash().to_string(), board::seconds_text),
                b.queued_p90
                    .map(|p| tf("P90 {d}", &[("d", &board::seconds_text(p))]))
                    .unwrap_or_else(|| tl("not given by GitLab").to_string()),
                b.queued_p90.filter(|p| *p >= 120.0).map(|_| t.amber.fg),
            ),
            (
                tl("Costliest job").to_string(),
                b.by_job
                    .first()
                    .map_or_else(|| board::dash().to_string(), |j| j.0.clone()),
                b.by_job
                    .first()
                    .map(|j| {
                        tf(
                            "{share} of the runner time",
                            &[("share", &board::percent(j.1 / b.runner_secs))],
                        )
                    })
                    .unwrap_or_default(),
                None,
            ),
        ];
        let width = ui.available_width();
        let per_row = board::fit(ui, 150.0, 4);
        let cell_w = (width - GAP * (per_row - 1) as f32) / per_row as f32;
        for chunk in items.chunks(per_row) {
            let (line, _) = ui.allocate_exact_size(Vec2::new(width, 64.0), Sense::hover());
            for (i, (label, value, caption, color)) in chunk.iter().enumerate() {
                let x = line.left() + i as f32 * (cell_w + GAP);
                board::paint_text(
                    ui,
                    egui::pos2(x, line.top() + 9.0),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(theme.text_size - 1.5),
                    theme.palette.text_muted,
                    cell_w,
                );
                board::paint_text(
                    ui,
                    egui::pos2(x, line.top() + 31.0),
                    Align2::LEFT_CENTER,
                    value,
                    fonts::semibold(19.0),
                    theme.palette.text,
                    cell_w,
                );
                board::paint_text(
                    ui,
                    egui::pos2(x, line.top() + 53.0),
                    Align2::LEFT_CENTER,
                    caption,
                    FontId::proportional(theme.text_size - 2.0),
                    color.unwrap_or(theme.palette.text_muted),
                    cell_w,
                );
            }
        }
        ui.add_space(10.0);
        board::split(
            ui,
            0.55,
            260.0,
            220.0,
            |ui| {
                // Runner time by day.
                board::caption(ui, theme, tl("Runner time by day"));
                ui.add_space(4.0);
                let (area, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 96.0), Sense::hover());
                let most = b.by_day.iter().map(|d| d.1).fold(1.0_f64, f64::max);
                let n = b.by_day.len().max(1) as f32;
                let slot = area.width() / n;
                for (i, (day, secs)) in b.by_day.iter().enumerate() {
                    let h = ((secs / most) as f32 * (area.height() - 16.0)).max(if *secs > 0.0 {
                        2.0
                    } else {
                        0.0
                    });
                    let bar = Rect::from_min_max(
                        egui::pos2(
                            area.left() + i as f32 * slot + slot * 0.15,
                            area.bottom() - 14.0 - h,
                        ),
                        egui::pos2(
                            area.left() + (i as f32 + 1.0) * slot - slot * 0.15,
                            area.bottom() - 14.0,
                        ),
                    );
                    if h > 0.0 {
                        ui.painter().rect_filled(
                            bar,
                            CornerRadius::same(2),
                            theme.palette.accent.gamma_multiply(0.8),
                        );
                    }
                    let hit = Rect::from_min_max(
                        egui::pos2(bar.left(), area.top()),
                        egui::pos2(bar.right(), area.bottom()),
                    );
                    ui.interact(hit, ui.scope_id().with(("budget_day", i)), Sense::hover())
                        .tip(format!(
                            "{} · {}",
                            crate::utils::day_text(*day),
                            hours_text(*secs)
                        ));
                    let every = (b.by_day.len() / 6).max(1);
                    if i % every == 0 {
                        board::paint_text(
                            ui,
                            egui::pos2(bar.center().x, area.bottom() - 5.0),
                            Align2::CENTER_CENTER,
                            &day.format("%d/%m").to_string(),
                            FontId::proportional(theme.text_size - 3.0),
                            theme.palette.text_muted,
                            slot * every as f32,
                        );
                    }
                }
            },
            |ui| {
                // The costliest jobs.
                board::caption(ui, theme, tl("The costliest jobs"));
                ui.add_space(4.0);
                let most = b.by_job.first().map_or(1.0, |j| j.1).max(1.0);
                for (name, secs) in b.by_job.iter().take(5) {
                    let (line, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
                    let name_w = (line.width() * 0.42).min(170.0);
                    board::paint_text(
                        ui,
                        egui::pos2(line.left(), line.center().y),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::monospace(theme.text_size - 2.0),
                        theme.palette.text_secondary,
                        name_w - 6.0,
                    );
                    let value_w = 58.0;
                    board::paint_text(
                        ui,
                        egui::pos2(line.right(), line.center().y),
                        Align2::RIGHT_CENTER,
                        &hours_text(*secs),
                        FontId::proportional(theme.text_size - 2.0),
                        theme.palette.text,
                        value_w,
                    );
                    let track = Rect::from_min_max(
                        egui::pos2(line.left() + name_w, line.center().y - 3.0),
                        egui::pos2(line.right() - value_w - 6.0, line.center().y + 3.0),
                    );
                    if track.width() > 10.0 {
                        ui.painter().rect_filled(
                            track,
                            CornerRadius::same(3),
                            theme.palette.border_soft,
                        );
                        let mut fill = track;
                        fill.set_width((track.width() * (secs / most) as f32).max(2.0));
                        ui.painter()
                            .rect_filled(fill, CornerRadius::same(3), theme.palette.accent);
                    }
                }
            },
        );
    });
}
