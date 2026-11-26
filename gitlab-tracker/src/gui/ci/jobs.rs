//! The jobs table: each job's stage, recent runs, failure rate, median duration and trend.

use super::*;

pub(super) struct Cols {
    pub(super) job: (f32, f32),
    pub(super) stage: Option<(f32, f32)>,
    pub(super) strip: Option<(f32, f32)>,
    pub(super) rate: (f32, f32),
    pub(super) median: (f32, f32),
    pub(super) trend: Option<(f32, f32)>,
}

pub(super) fn cols(width: f32) -> Cols {
    let pad = 10.0;
    let gap = 14.0;
    let strip_w = STRIP as f32 * 11.0 - 3.0;
    let fixed = |stage: bool, strip: bool, trend: bool| {
        pad * 2.0
            + 130.0
            + if stage { 72.0 + gap } else { 0.0 }
            + if strip { strip_w + gap } else { 0.0 }
            + 120.0
            + gap
            + 76.0
            + gap
            + if trend { 80.0 + gap } else { 0.0 }
    };
    let (stage, strip, trend) = if fixed(true, true, true) <= width {
        (true, true, true)
    } else if fixed(false, true, true) <= width {
        (false, true, true)
    } else if fixed(false, true, false) <= width {
        (false, true, false)
    } else {
        (false, false, false)
    };
    let job_w = width - fixed(stage, strip, trend) + 130.0;
    let mut x = pad;
    let mut take = |w: f32| {
        let at = x;
        x += w + gap;
        (at, w)
    };
    let job = take(job_w);
    let stage = stage.then(|| take(72.0));
    let strip = strip.then(|| take(strip_w));
    let rate = take(120.0);
    let median = take(76.0);
    let trend = trend.then(|| take(80.0));
    Cols {
        job,
        stage,
        strip,
        rate,
        median,
        trend,
    }
}

pub(super) fn status_color(status: &str, flaky: bool, theme: &Theme) -> Color32 {
    let t = &theme.tones;
    match status {
        "failed" if flaky => t.amber.fg,
        "failed" => t.red.fg,
        "success" => t.green.fg,
        "running" | "pending" | "created" => t.blue.fg,
        _ => theme.palette.border,
    }
}

pub(super) fn jobs_table(
    ui: &mut Ui,
    theme: &Theme,
    jobs: &[&Job],
    selected: Option<&str>,
    picked: &mut Option<String>,
) {
    let c = cols(ui.available_width());
    let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let head = |col: (f32, f32), text: &str| {
        board::paint_text(
            ui,
            egui::pos2(header.left() + col.0, header.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(theme.text_size - 1.5),
            theme.palette.text_muted,
            col.1,
        );
    };
    head(c.job, tl("Job"));
    if let Some(col) = c.stage {
        head(col, tl("Stage"));
    }
    if let Some(col) = c.strip {
        head(col, &tf("{n} latest runs", &[("n", &STRIP.to_string())]));
    }
    head(c.rate, tl("Failures"));
    head(c.median, tl("Median"));
    if let Some(col) = c.trend {
        head(col, tl("Trend"));
    }
    for job in jobs {
        let is_selected = selected == Some(job.name.as_str());
        let (rect, response) = board::row(ui, theme, 40.0, is_selected);
        if !is_selected {
            board::row_rule(ui, theme, rect);
        }
        let cy = rect.center().y;
        let x = |col: (f32, f32)| rect.left() + col.0;
        // Name, and a chip when flaky.
        let chip_w = if job.is_flaky() {
            let galley = ui.painter().layout_no_wrap(
                tl("flaky").to_string(),
                fonts::medium(theme.text_size - 2.5),
                theme.tones.amber.fg,
            );
            galley.size().x + 12.0 + 8.0
        } else {
            0.0
        };
        let w = board::paint_text(
            ui,
            egui::pos2(x(c.job), cy),
            Align2::LEFT_CENTER,
            &job.name,
            egui::FontId::monospace(theme.text_size - 1.0),
            theme.palette.text,
            c.job.1 - chip_w,
        );
        if job.is_flaky() {
            board::paint_tag(
                ui,
                theme,
                egui::pos2(x(c.job) + w + 8.0, cy),
                tl("flaky"),
                theme.tones.amber,
                false,
            );
        }
        if let Some(col) = c.stage {
            board::paint_tag(
                ui,
                theme,
                egui::pos2(x(col), cy),
                &job.stage,
                theme.tones.gray,
                false,
            );
        }
        if let Some(col) = c.strip {
            let runs: Vec<&Run> = job
                .runs
                .iter()
                .rev()
                .take(STRIP)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            // Right-aligned: the latest run last; empty slots before.
            let offset = (STRIP - runs.len()) as f32 * 11.0;
            for k in 0..STRIP {
                let slot = Rect::from_min_size(
                    egui::pos2(x(col) + k as f32 * 11.0, cy - 8.0),
                    Vec2::new(8.0, 16.0),
                );
                if (k as f32 * 11.0) < offset {
                    ui.painter().rect_filled(
                        slot,
                        CornerRadius::same(2),
                        theme.palette.raised.gamma_multiply(0.6),
                    );
                }
            }
            for (k, run) in runs.iter().enumerate() {
                let slot = Rect::from_min_size(
                    egui::pos2(x(col) + offset + k as f32 * 11.0, cy - 8.0),
                    Vec2::new(8.0, 16.0),
                );
                ui.painter().rect_filled(
                    slot,
                    CornerRadius::same(2),
                    status_color(&run.status, run.flaky, theme),
                );
                let tip = Tip::new()
                    .mr(&run.mr, &run.mr_title)
                    .dot(
                        status_color(&run.status, run.flaky, theme),
                        if run.flaky {
                            format!("{} · {}", run.status, tl("passed on a rerun"))
                        } else {
                            run.status.clone()
                        },
                    )
                    .text(crate::utils::moment_text(run.at));
                ui.interact(
                    slot.expand2(Vec2::new(1.5, 4.0)),
                    ui.scope_id().with(("run", job.name.as_str(), k)),
                    Sense::hover(),
                )
                .rich(tip);
            }
        }
        // Failure rate: the figure and a bar.
        let (tone_color, text_color) = match job.failure_rate {
            r if r >= 0.3 => (theme.tones.red.fg, theme.tones.red.fg),
            r if r >= 0.1 => (theme.tones.amber.fg, theme.tones.amber.fg),
            _ => (theme.tones.green.fg, theme.palette.text_secondary),
        };
        let label_w = board::paint_text(
            ui,
            egui::pos2(x(c.rate), cy),
            Align2::LEFT_CENTER,
            &board::percent(job.failure_rate),
            fonts::semibold(theme.text_size - 1.0),
            text_color,
            44.0,
        );
        let track = Rect::from_min_size(
            egui::pos2(x(c.rate) + label_w.max(34.0) + 8.0, cy - 3.0),
            Vec2::new(c.rate.1 - label_w.max(34.0) - 8.0, 6.0),
        );
        ui.painter()
            .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
        let mut fill = track;
        fill.set_width(track.width() * job.failure_rate as f32);
        if fill.width() > 0.5 {
            ui.painter()
                .rect_filled(fill, CornerRadius::same(3), tone_color);
        }
        board::paint_text(
            ui,
            egui::pos2(x(c.median), cy),
            Align2::LEFT_CENTER,
            &job.median
                .map_or_else(|| board::dash().to_string(), board::seconds_text),
            egui::FontId::monospace(theme.text_size - 2.0),
            theme.palette.text_secondary,
            c.median.1,
        );
        if let Some(col) = c.trend {
            let (text, color) = match job.rate_before {
                Some(before) => {
                    let delta = (job.failure_rate - before) * 100.0;
                    if delta >= 3.0 {
                        (format!("↑ +{delta:.0} {}", tl("pts")), theme.tones.red.fg)
                    } else if delta <= -3.0 {
                        (format!("↓ {delta:.0} {}", tl("pts")), theme.tones.green.fg)
                    } else {
                        (format!("→ {}", tl("stable")), theme.palette.text_muted)
                    }
                }
                None => (board::dash().to_string(), theme.palette.text_muted),
            };
            board::paint_text(
                ui,
                egui::pos2(x(col), cy),
                Align2::LEFT_CENTER,
                &text,
                FontId::proportional(theme.text_size - 1.5),
                color,
                col.1,
            );
        }
        if response.clicked() {
            *picked = Some(job.name.clone());
        }
    }
}
