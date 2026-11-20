//! A pipeline as a timeline, in the MR page's Pipelines tab: each job where
//! it ran, its wait for a runner before it, and the critical path that set
//! the pipeline's duration lit. Under it, the path's jobs against their usual
//! time and what would give minutes back. Worked out in
//! [`crate::critical_path`].

use std::collections::HashMap;

use bevel_theme::{fonts, Theme};
use egui::{Color32, CornerRadius, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::board;
use super::tip::TipExt;
use super::widgets::{mono, muted, semibold};
use crate::critical_path::{Change, Plan};
use crate::i18n::{tf, tl, tn};
use crate::models::Pipeline;

/// The job names' column.
const NAME_W: f32 = 170.0;
const ROW_H: f32 = 22.0;
const BAR_H: f32 = 12.0;
/// A job is slow when it ran this much longer than usual.
const SLOW: f64 = 1.2;

/// "45s", "3m 05s", "1h 02m".
pub fn span(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    }
}

/// The timeline of `pipeline`; the job clicked (its index), to show its log.
pub fn view(
    ui: &mut Ui,
    theme: &Theme,
    pipeline: &Pipeline,
    usual: &HashMap<String, f64>,
    picked: Option<usize>,
) -> Option<usize> {
    let Some(plan) = crate::critical_path::plan(pipeline) else {
        let text = if pipeline
            .jobs
            .iter()
            .any(|j| matches!(j.status.as_str(), "running" | "pending" | "preparing"))
        {
            tl("The timeline shows once the pipeline is over.")
        } else {
            tl("No job ran in this pipeline.")
        };
        ui.label(muted(theme, text));
        return None;
    };
    summary(ui, theme, pipeline, &plan);
    ui.add_space(12.0);
    let clicked = gantt(ui, theme, pipeline, &plan, picked);
    ui.add_space(8.0);
    legend(ui, theme);
    ui.add_space(12.0);
    board::split(
        ui,
        0.5,
        260.0,
        260.0,
        |ui| on_the_path(ui, theme, pipeline, &plan, usual),
        |ui| gains(ui, theme, pipeline, &plan),
    );
    clicked
}

/// What set the duration, in a sentence, with the path's jobs.
fn summary(ui: &mut Ui, theme: &Theme, pipeline: &Pipeline, plan: &Plan) {
    let accent = theme.palette.accent;
    egui::Frame::new()
        .fill(accent.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0, accent.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(semibold(
                    tf("What set the {total}:", &[("total", &span(plan.total))]),
                    theme.text_size,
                ));
                for (i, &job) in plan.path.iter().enumerate() {
                    if i > 0 {
                        ui.label(muted(theme, "→"));
                    }
                    ui.label(
                        mono(&pipeline.jobs[job].name, theme.text_size - 1.0)
                            .color(theme.tones.blue.fg),
                    );
                }
            });
            let (queued, ran) = plan.path_times();
            let others = plan.slots.len() - plan.path.len();
            let mut text = tf(
                "{ran} running on that path, {queued} waiting for a runner.",
                &[("ran", &span(ran)), ("queued", &span(queued))],
            );
            if others > 0 {
                text.push(' ');
                text.push_str(&tn(
                    others as u64,
                    "The {n} other job ran meanwhile: making it faster gives nothing back.",
                    "The {n} other jobs ran meanwhile: making them faster gives nothing back.",
                ));
            }
            ui.label(RichText::new(text).color(theme.palette.text_secondary));
            if !plan.exact {
                ui.label(
                    muted(
                        theme,
                        tl("Worked out from the stage order: GitLab's `needs:` were not read (no GraphQL), so a job using them may have started earlier."),
                    )
                    .size(theme.text_size - 1.5),
                );
            }
        });
}

/// A tick every this many seconds, so a few fit.
fn tick_step(total: f64) -> f64 {
    const STEPS: [f64; 10] = [
        10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0, 7200.0,
    ];
    STEPS
        .iter()
        .copied()
        .find(|s| total / s <= 8.0)
        .unwrap_or(7200.0)
}

/// The jobs on the timeline, stage by stage; the job clicked.
fn gantt(
    ui: &mut Ui,
    theme: &Theme,
    pipeline: &Pipeline,
    plan: &Plan,
    picked: Option<usize>,
) -> Option<usize> {
    let width = ui.available_width();
    let track_w = (width - NAME_W - 12.0).max(120.0);
    let scale = track_w as f64 / plan.total.max(1.0);
    let step = tick_step(plan.total);
    let ticks: Vec<f64> = (0..)
        .map(|k| k as f64 * step)
        .take_while(|t| *t <= plan.total)
        .collect();
    let mut clicked = None;
    // The axis.
    let (axis, _) = ui.allocate_exact_size(Vec2::new(width, 16.0), Sense::hover());
    let left = axis.left() + NAME_W + 12.0;
    for t in &ticks {
        let x = left + (*t * scale) as f32;
        let text = if *t == 0.0 { "0".to_string() } else { span(*t) };
        ui.painter().text(
            egui::pos2(x, axis.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(theme.text_size - 2.5),
            theme.palette.text_muted,
        );
    }
    let t = &theme.tones;
    for stage in &plan.stages {
        let slots: Vec<_> = plan
            .slots
            .iter()
            .filter(|s| pipeline.jobs[s.job].stage == *stage)
            .collect();
        if slots.is_empty() {
            continue;
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(stage.to_uppercase())
                .font(fonts::medium(theme.text_size - 2.5))
                .color(theme.palette.text_muted),
        );
        for slot in slots {
            let job = &pipeline.jobs[slot.job];
            let (row, response) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
            let painter = ui.painter();
            if picked == Some(slot.job) {
                painter.rect_filled(row, CornerRadius::same(4), theme.palette.selection);
            } else if response.hovered() {
                painter.rect_filled(row, CornerRadius::same(4), theme.palette.border_soft);
            }
            let failed = job.status == "failed";
            let name_color = if failed {
                t.red.fg
            } else if slot.critical {
                t.blue.fg
            } else {
                theme.palette.text_secondary
            };
            let name = painter.layout_job({
                let mut job_text = egui::text::LayoutJob::single_section(
                    job.name.clone(),
                    egui::TextFormat::simple(
                        egui::FontId::monospace(theme.text_size - 1.5),
                        name_color,
                    ),
                );
                job_text.wrap = egui::text::TextWrapping::truncate_at_width(NAME_W - 4.0);
                job_text
            });
            painter.galley(
                egui::pos2(row.left() + 2.0, row.center().y - name.size().y / 2.0),
                name,
                name_color,
            );
            let x = |at: f64| left + (at * scale) as f32;
            // The grid behind.
            for tick in &ticks {
                painter.vline(
                    x(*tick),
                    row.y_range(),
                    Stroke::new(1.0, theme.palette.border_soft),
                );
            }
            // The wait for a runner, dashed.
            let y = row.center().y;
            let mut at = slot.ready;
            while at < slot.start {
                let to = (at + 3.0 / scale).min(slot.start);
                painter.line_segment(
                    [egui::pos2(x(at), y), egui::pos2(x(to), y)],
                    Stroke::new(3.0, t.amber.fg),
                );
                at += 6.0 / scale;
            }
            let bar = egui::Rect::from_min_max(
                egui::pos2(x(slot.start), y - BAR_H / 2.0),
                egui::pos2(x(slot.end).max(x(slot.start) + 2.0), y + BAR_H / 2.0),
            );
            let fill = if failed {
                t.red.fg
            } else if slot.critical {
                theme.palette.accent
            } else {
                meanwhile(theme)
            };
            painter.rect_filled(bar, CornerRadius::same(3), fill);
            let mut tip = super::tip::Tip::new().title(&job.name).text(tf(
                "Ran {ran}, after {queued} waiting for a runner",
                &[("ran", &span(slot.ran())), ("queued", &span(slot.queued()))],
            ));
            if slot.critical {
                tip = tip.dot(theme.palette.accent, tl("On the critical path"));
            }
            let response = response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .rich(tip.hint(tl("Click to read its log")));
            if response.clicked() {
                clicked = Some(slot.job);
            }
        }
    }
    clicked
}

/// The bars of the jobs off the path: seen, yet behind the path's.
fn meanwhile(theme: &Theme) -> Color32 {
    theme.palette.text_muted.gamma_multiply(0.55)
}

fn legend(ui: &mut Ui, theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let swatch = |ui: &mut Ui, color: Color32| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 8.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(2), color);
        };
        let size = theme.text_size - 1.5;
        swatch(ui, theme.palette.accent);
        ui.label(muted(theme, tl("on the critical path")).size(size));
        ui.add_space(10.0);
        swatch(ui, meanwhile(theme));
        ui.label(muted(theme, tl("ran meanwhile")).size(size));
        ui.add_space(10.0);
        let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 8.0), Sense::hover());
        for k in 0..3 {
            let x = r.left() + k as f32 * 6.0;
            ui.painter().line_segment(
                [
                    egui::pos2(x, r.center().y),
                    egui::pos2(x + 3.0, r.center().y),
                ],
                Stroke::new(3.0, theme.tones.amber.fg),
            );
        }
        ui.label(muted(theme, tl("waiting for a runner")).size(size));
    });
}

/// A small card with a title.
fn panel(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(semibold(title, theme.text_size));
            ui.add_space(8.0);
            body(ui);
        });
}

/// The path's jobs: this run against their usual time.
fn on_the_path(
    ui: &mut Ui,
    theme: &Theme,
    pipeline: &Pipeline,
    plan: &Plan,
    usual: &HashMap<String, f64>,
) {
    panel(ui, theme, tl("On the path"), |ui| {
        let size = theme.text_size - 1.0;
        let mut slowest: Option<(&str, f64)> = None;
        egui::Grid::new(("path-grid", pipeline.id))
            .num_columns(3)
            .spacing(Vec2::new(16.0, 6.0))
            .show(ui, |ui| {
                ui.label(muted(theme, tl("Job")).size(size - 1.0));
                ui.label(muted(theme, tl("This run")).size(size - 1.0));
                ui.label(muted(theme, tl("Usually")).size(size - 1.0))
                    .tip(tl(
                        "The median of this job's passed runs in the tracked MRs' pipelines.",
                    ));
                ui.end_row();
                for &i in &plan.path {
                    let job = &pipeline.jobs[i];
                    let ran = job.duration.unwrap_or(0.0);
                    let typical = usual.get(&job.name).copied();
                    let slow = typical.is_some_and(|u| u > 0.0 && ran > u * SLOW);
                    if let Some(u) = typical.filter(|_| slow) {
                        let ratio = ran / u;
                        if slowest.is_none_or(|(_, r)| ratio > r) {
                            slowest = Some((&job.name, ratio));
                        }
                    }
                    ui.label(mono(&job.name, size - 0.5));
                    ui.label(if slow {
                        RichText::new(span(ran))
                            .size(size)
                            .color(theme.tones.orange.fg)
                            .strong()
                    } else {
                        RichText::new(span(ran)).size(size)
                    });
                    ui.label(muted(theme, typical.map_or("—".to_string(), span)).size(size));
                    ui.end_row();
                }
            });
        if let Some((name, ratio)) = slowest {
            ui.add_space(6.0);
            ui.label(
                RichText::new(tf(
                    "{job} took {pct} % longer than usual.",
                    &[
                        ("job", name),
                        ("pct", &format!("{:.0}", (ratio - 1.0) * 100.0)),
                    ],
                ))
                .size(size)
                .color(theme.palette.text_secondary),
            );
        }
    });
}

/// What would give minutes back: each path job twice as fast, no wait for a
/// runner, and the longest job off the path (nothing).
fn gains(ui: &mut Ui, theme: &Theme, pipeline: &Pipeline, plan: &Plan) {
    panel(ui, theme, tl("Where minutes would come back"), |ui| {
        let size = theme.text_size - 1.0;
        let mut rows: Vec<(String, f64)> = plan
            .path
            .iter()
            .map(|&i| {
                (
                    tf("{job} twice as fast", &[("job", &pipeline.jobs[i].name)]),
                    plan.without(pipeline, Change::Halve(i)),
                )
            })
            .collect();
        let queue = plan.without(pipeline, Change::NoQueue);
        if queue > 0.0 {
            rows.push((tl("no wait for runners").to_string(), queue));
        }
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        if let Some(off) = plan.longest_off_path() {
            rows.push((
                tf(
                    "{job} twice as fast",
                    &[("job", &pipeline.jobs[off.job].name)],
                ),
                0.0,
            ));
        }
        egui::Grid::new(("gains-grid", pipeline.id))
            .num_columns(2)
            .spacing(Vec2::new(16.0, 6.0))
            .show(ui, |ui| {
                for (what, back) in rows {
                    ui.label(RichText::new(what).size(size));
                    if back > 0.0 {
                        ui.label(
                            RichText::new(format!("−{}", span(back)))
                                .size(size)
                                .color(theme.tones.green.fg)
                                .strong(),
                        );
                    } else {
                        ui.label(muted(theme, tl("no gain")).size(size));
                    }
                    ui.end_row();
                }
            });
    });
}
