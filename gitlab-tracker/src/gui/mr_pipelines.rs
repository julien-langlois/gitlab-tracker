//! The MR page's Pipelines tab: each pipeline as a card (the latest unfolded),
//! its jobs as a graph of stages as in GitLab, and the log of the job picked
//! (the first failed one at first) under it, read from GitLab on demand.

use bevel_theme::{fonts, Chip, Theme};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::details;
use super::inspector::duration;
use super::tip::{Tip, TipExt};
use super::widgets::{button, icon_button, mono, muted};
use super::{icons::Icon, tones, GuiApp};
use crate::i18n::{tf, tl, tn};
use crate::models::{Pipeline, PipelineJob, TrackedMr};
use crate::utils::format_relative_date;

mod compact_view;
mod job_graph;
mod job_log;

pub use compact_view::*;
use job_graph::*;
use job_log::*;

/// Pipelines listed; older ones are in GitLab.
const PIPELINES_SHOWN: usize = 10;
/// A job's box in the graph.
const JOB_WIDTH: f32 = 210.0;
const JOB_HEIGHT: f32 = 34.0;
/// Room between two stages, where their connector goes.
const STAGE_GAP: f32 = 28.0;

/// What the tab remembers between frames, per MR: the pipelines unfolded or
/// folded by hand, and the job picked (pipeline id, job index).
#[derive(Clone, Default)]
struct State {
    toggled: std::collections::HashSet<u64>,
    picked: Option<(u64, usize)>,
    /// The jobs on a timeline, the critical path lit, rather than as stages.
    timeline: bool,
}

pub fn tab(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    if mr.pipelines.is_empty() {
        ui.label(muted(&theme, tl("No pipeline yet.")));
        return;
    }
    let id = ui.make_persistent_id(("mr pipelines", &mr.id));
    let mut state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let project_url = project_url(&mr.web_url);

    for (i, pipeline) in mr.pipelines.iter().take(PIPELINES_SHOWN).enumerate() {
        // The latest unfolded, the others folded; a click on the header swaps it.
        let open = (i == 0) != state.toggled.contains(&pipeline.id);
        let header_clicked = card(ui, &theme, pipeline, open, project_url.as_deref(), |ui| {
            graph(
                g,
                ui,
                &theme,
                mr,
                pipeline,
                &mut state,
                project_url.as_deref(),
            );
        });
        if header_clicked && !state.toggled.remove(&pipeline.id) {
            state.toggled.insert(pipeline.id);
        }
        ui.add_space(10.0);
    }
    if mr.pipelines.len() > PIPELINES_SHOWN {
        ui.hyperlink_to(
            tn(
                (mr.pipelines.len() - PIPELINES_SHOWN) as u64,
                "{n} older pipeline in GitLab",
                "{n} older pipelines in GitLab",
            ),
            format!("{}/pipelines", mr.web_url),
        );
    }
    ui.data_mut(|d| d.insert_temp(id, state));
}

/// "https://host/group/project" out of an MR's web URL.
fn project_url(mr_url: &str) -> Option<String> {
    mr_url
        .split_once("/-/merge_requests")
        .map(|(project, _)| project.to_string())
}

/// A pipeline's card: a header (state, id, when, the jobs' tally), and `body`
/// under it when `open`. Returns whether the header was clicked.
fn card(
    ui: &mut Ui,
    theme: &Theme,
    pipeline: &Pipeline,
    open: bool,
    project_url: Option<&str>,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let (label, tone) = tones::pipeline(pipeline.status, &theme.tones);
    let mut clicked = false;
    egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The header, the whole row clickable.
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.border_soft.gamma_multiply(0.5),
                );
            }
            // The state down the left edge.
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    rect.min + Vec2::new(0.0, 10.0),
                    Vec2::new(3.0, rect.height() - 20.0),
                ),
                CornerRadius::same(2),
                tone.fg,
            );
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(Vec2::new(14.0, 0.0)))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            // The header takes the click: selectable texts (the id, the date)
            // would take it over the header, and a click on them folded nothing.
            row.style_mut().interaction.selectable_labels = false;
            row.spacing_mut().item_spacing.x = 10.0;
            row.label(muted(theme, if open { "▾" } else { "▸" }));
            row.add(Chip::new(label, tone).strong());
            row.label(
                mono(format!("#{}", pipeline.id), theme.text_size - 0.5).color(theme.palette.text),
            );
            if let Some(at) = &pipeline.created_at {
                row.label(muted(theme, format_relative_date(at)));
            }
            row.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if let Some(url) = project_url {
                    let link = ui
                        .add(
                            egui::Label::new(
                                RichText::new(tl("Open in GitLab"))
                                    .size(theme.text_size - 1.5)
                                    .color(theme.palette.accent),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if link.clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                            "{url}/-/pipelines/{}",
                            pipeline.id
                        )));
                    }
                }
                tally(ui, theme, &pipeline.jobs);
            });
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                clicked = true;
            }
            if open && !pipeline.jobs.is_empty() {
                let rule = ui.cursor().min.y;
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    rule,
                    Stroke::new(1.0, theme.palette.border_soft),
                );
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 16,
                        right: 16,
                        top: 14,
                        bottom: 14,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        body(ui);
                    });
            } else if open {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(muted(theme, tl("GitLab listed no job for it.")));
                    });
            }
        });
    clicked
}

/// "4 jobs · 1 failed · 2m 10s" on the right of a pipeline's header.
fn tally(ui: &mut Ui, theme: &Theme, jobs: &[PipelineJob]) {
    if jobs.is_empty() {
        return;
    }
    let total: f64 = jobs.iter().filter_map(|j| j.duration).sum();
    if total > 0.0 {
        ui.label(muted(theme, long_duration(total)));
        ui.label(muted(theme, "·"));
    }
    let failed = jobs.iter().filter(|j| j.status == "failed").count();
    if failed > 0 {
        ui.label(
            RichText::new(tn(failed as u64, "{n} failed", "{n} failed"))
                .size(theme.text_size - 1.0)
                .color(theme.tones.red.fg),
        );
        ui.label(muted(theme, "·"));
    }
    ui.label(muted(theme, tn(jobs.len() as u64, "{n} job", "{n} jobs")));
}

/// "45s", "3m 05s", "1h 2m".
fn long_duration(seconds: f64) -> String {
    let s = seconds.round() as u32;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        duration(s)
    }
}

#[cfg(test)]
mod tests;
