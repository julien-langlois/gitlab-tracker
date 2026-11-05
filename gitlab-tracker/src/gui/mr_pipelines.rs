//! The MR page's Pipelines tab: each pipeline as a card (the latest unfolded),
//! its jobs as a graph of stages as in GitLab, and the log of the job picked
//! (the first failed one at first) under it, read from GitLab on demand.

use bevel_theme::{fonts, Chip, Theme};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::details;
use super::inspector::duration;
use super::widgets::{button, icon_button, mono, muted};
use super::{icons::Icon, tones, GuiApp};
use crate::i18n::{tf, tl, tn};
use crate::models::{Pipeline, PipelineJob, TrackedMr};
use crate::utils::format_relative_date;

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

/// The stages side by side, their jobs as boxes; a click on a job shows its log
/// under the graph (the first failed job's at first).
fn graph(
    g: &mut GuiApp,
    ui: &mut Ui,
    theme: &Theme,
    mr: &TrackedMr,
    pipeline: &Pipeline,
    state: &mut State,
    project_url: Option<&str>,
) {
    let demo = g.app.read_only;
    let mut stages: Vec<&str> = Vec::new();
    for job in &pipeline.jobs {
        if !stages.contains(&job.stage.as_str()) {
            stages.push(&job.stage);
        }
    }
    // The job picked in this pipeline, else its first failed one.
    let picked = match state.picked {
        Some((pid, index)) if pid == pipeline.id => Some(index),
        _ => pipeline.jobs.iter().position(|j| j.status == "failed"),
    }
    .filter(|&i| pipeline.jobs.get(i).is_some_and(|j| has_log(j, demo)));

    let mut clicked = None;
    egui::ScrollArea::horizontal()
        .id_salt(("mr pipeline graph", &mr.id, pipeline.id))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(STAGE_GAP, 0.0);
                let mut previous: Option<f32> = None;
                for stage in &stages {
                    let column = ui.vertical(|ui| {
                        ui.set_width(JOB_WIDTH);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        let jobs: Vec<(usize, &PipelineJob)> = pipeline
                            .jobs
                            .iter()
                            .enumerate()
                            .filter(|(_, j)| j.stage == *stage)
                            .collect();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                RichText::new(stage.to_uppercase())
                                    .font(fonts::medium(theme.text_size - 2.5))
                                    .color(theme.palette.text_muted),
                            );
                            ui.label(
                                muted(theme, jobs.len().to_string()).size(theme.text_size - 2.5),
                            );
                        });
                        let mut first_center = None;
                        for (index, job) in jobs {
                            let response =
                                job_box(ui, theme, job, JOB_WIDTH, picked == Some(index), demo);
                            first_center.get_or_insert(response.rect.center().y);
                            if response.clicked() && has_log(job, demo) {
                                clicked = Some(index);
                            }
                        }
                        first_center
                    });
                    // The connector from the previous stage's first job.
                    let rect = column.response.rect;
                    if let (Some(left), Some(y)) = (previous, column.inner) {
                        ui.painter().line_segment(
                            [egui::pos2(left + 4.0, y), egui::pos2(rect.left() - 4.0, y)],
                            Stroke::new(1.5, theme.palette.border),
                        );
                    }
                    previous = Some(rect.right());
                }
            });
        });
    if let Some(index) = clicked {
        // A second click on the job shown folds its log.
        state.picked = if picked == Some(index) {
            Some((pipeline.id, usize::MAX))
        } else {
            Some((pipeline.id, index))
        };
    }
    let picked = match state.picked {
        Some((pid, index)) if pid == pipeline.id => pipeline.jobs.get(index).map(|j| (index, j)),
        _ => picked.and_then(|i| pipeline.jobs.get(i).map(|j| (i, j))),
    };
    if let Some((_, job)) = picked {
        ui.add_space(14.0);
        let job_url = project_url
            .filter(|_| job.id != 0)
            .map(|url| format!("{url}/-/jobs/{}", job.id));
        log_panel(g, ui, theme, job, job_url.as_deref(), 420.0);
    }
}

/// Whether GitLab keeps a log for the job: it started.
fn has_log(job: &PipelineJob, demo: bool) -> bool {
    // The demo's jobs have no id: one canned log for all of them.
    (job.id != 0 || demo)
        && !matches!(
            job.status.as_str(),
            "skipped" | "created" | "pending" | "manual" | "scheduled"
        )
}

/// A job in the graph: its state's glyph, its name, its duration.
fn job_box(
    ui: &mut Ui,
    theme: &Theme,
    job: &PipelineJob,
    width: f32,
    picked: bool,
    demo: bool,
) -> egui::Response {
    let sense = if has_log(job, demo) {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, JOB_HEIGHT), sense);
    let color = job_color(theme, &job.status);
    let failed = job.status == "failed";
    let fill = if picked {
        theme.palette.selection
    } else if response.hovered() && sense == Sense::click() {
        theme.palette.border_soft
    } else {
        theme.palette.input
    };
    let stroke = if picked {
        Stroke::new(1.5, theme.palette.accent)
    } else if failed {
        Stroke::new(1.0, theme.tones.red.fg.gamma_multiply(0.6))
    } else {
        Stroke::new(1.0, theme.palette.border_soft)
    };
    let radius = CornerRadius::same((JOB_HEIGHT / 2.0) as u8);
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter()
        .rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 18.0, rect.center().y),
        Vec2::splat(16.0),
    );
    status_glyph(ui.painter(), glyph, &job.status, color);

    let time = match job.duration {
        Some(secs) => long_duration(secs),
        None => tl(status_label(&job.status)).to_string(),
    };
    let time_galley = ui.painter().layout_no_wrap(
        time,
        egui::FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
    );
    let time_pos = egui::pos2(
        rect.right() - 14.0 - time_galley.size().x,
        rect.center().y - time_galley.size().y / 2.0,
    );
    let name_left = glyph.right() + 8.0;
    let mut job_text = egui::text::LayoutJob::single_section(
        job.name.clone(),
        egui::TextFormat::simple(
            fonts::medium(theme.text_size - 1.0),
            if failed {
                theme.tones.red.fg
            } else {
                theme.palette.text
            },
        ),
    );
    job_text.wrap = egui::text::TextWrapping::truncate_at_width(time_pos.x - 8.0 - name_left);
    let name = ui.painter().layout_job(job_text);
    ui.painter().galley(
        egui::pos2(name_left, rect.center().y - name.size().y / 2.0),
        name,
        theme.palette.text,
    );
    ui.painter()
        .galley(time_pos, time_galley, theme.palette.text_muted);

    let hover = format!("{} · {}", job.name, tl(status_label(&job.status)));
    let response = response.on_hover_text(if sense == Sense::click() {
        format!("{hover}\n{}", tl("Click to read its log"))
    } else {
        hover
    });
    if sense == Sense::click() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "success" => "Passed",
        "failed" => "Failed",
        "running" => "Running",
        "pending" => "Pending",
        "created" => "Created",
        "manual" => "Manual",
        "scheduled" => "Scheduled",
        "skipped" => "Skipped",
        "canceled" | "canceling" => "Canceled",
        _ => "Unknown",
    }
}

fn job_color(theme: &Theme, status: &str) -> Color32 {
    let t = &theme.tones;
    match status {
        "success" => t.green.fg,
        "failed" => t.red.fg,
        "running" => t.blue.fg,
        "pending" | "created" | "scheduled" => t.amber.fg,
        _ => theme.palette.text_muted,
    }
}

/// GitLab's state glyphs, drawn: a tick, a cross, a running ring, a slash for
/// skipped, a ring for the rest.
fn status_glyph(painter: &egui::Painter, rect: egui::Rect, status: &str, color: Color32) {
    let c = rect.center();
    let r = rect.width() / 2.0;
    let line = Stroke::new(1.6, color);
    match status {
        "success" | "failed" => {
            painter.circle_filled(c, r, color);
            let ink = Stroke::new(1.8, Color32::from_black_alpha(200));
            if status == "success" {
                painter.line_segment([c + Vec2::new(-3.6, 0.2), c + Vec2::new(-1.0, 2.8)], ink);
                painter.line_segment([c + Vec2::new(-1.0, 2.8), c + Vec2::new(3.8, -2.6)], ink);
            } else {
                let d = 3.0;
                painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], ink);
                painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], ink);
            }
        }
        "running" => {
            painter.circle_stroke(c, r - 1.0, line);
            painter.circle_filled(c, r - 4.5, color);
        }
        "skipped" | "canceled" | "canceling" => {
            painter.circle_stroke(c, r - 1.0, line);
            let d = (r - 1.0) * 0.7;
            painter.line_segment([c + Vec2::new(d, -d), c + Vec2::new(-d, d)], line);
        }
        _ => {
            painter.circle_stroke(c, r - 1.0, line);
        }
    }
}

/// The log of `job`: a title bar (its name, state, copy, GitLab) over the end
/// of the log as in a terminal, numbered, the lines that tell what failed in red.
fn log_panel(
    g: &mut GuiApp,
    ui: &mut Ui,
    theme: &Theme,
    job: &PipelineJob,
    url: Option<&str>,
    max_height: f32,
) {
    g.details.ensure_log(&g.app, job.id, ui.ctx());
    let log = g.details.job_log(job.id);
    let color = job_color(theme, &job.status);
    let narrow = ui.available_width() < 480.0;
    egui::Frame::new()
        .fill(theme.palette.chrome)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::new()
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    // The buttons first, on the right, then the job in what is
                    // left: a long name is cut, not the panel widened.
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            log_buttons(ui, theme, &log, url, narrow);
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let (glyph, _) =
                                    ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                                status_glyph(ui.painter(), glyph, &job.status, color);
                                let mut details = String::new();
                                if !narrow {
                                    details = format!(" · {}", job.stage);
                                    if let Some(secs) = job.duration {
                                        details += &format!(" · {}", long_duration(secs));
                                    }
                                }
                                let mut text = egui::text::LayoutJob::default();
                                text.append(
                                    &job.name,
                                    0.0,
                                    egui::TextFormat::simple(
                                        fonts::medium(theme.text_size - 0.5),
                                        theme.palette.text,
                                    ),
                                );
                                text.append(
                                    &details,
                                    0.0,
                                    egui::TextFormat::simple(
                                        egui::FontId::proportional(theme.text_size - 1.0),
                                        theme.palette.text_muted,
                                    ),
                                );
                                ui.add(egui::Label::new(text).truncate())
                                    .on_hover_text(&job.name);
                            });
                        });
                    });
                });
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().min.y,
                Stroke::new(1.0, theme.palette.border_soft),
            );
            egui::Frame::new()
                .inner_margin(Margin::symmetric(0, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match &log {
                        details::LogView::Loading => {
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                super::widgets::spinner(ui, 12.0, theme.palette.accent);
                                ui.label(muted(theme, tl("Reading the log…")));
                            });
                        }
                        details::LogView::Failed(error) => {
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("⚠ {error}"))
                                            .color(theme.tones.red.fg),
                                    )
                                    .wrap(),
                                );
                            });
                        }
                        details::LogView::Ready(text) => {
                            terminal(ui, theme, job.id, text, max_height)
                        }
                    }
                });
        });
    if let details::LogView::Ready(text) = &log {
        ui.add_space(4.0);
        ui.label(muted(
            theme,
            tf(
                "The last {n} lines, read once from GitLab for this session",
                &[("n", &text.lines().count().to_string())],
            ),
        ));
    }
}

/// The log's buttons, right to left: GitLab's whole log, a copy of it; icons
/// only when `narrow`.
fn log_buttons(
    ui: &mut Ui,
    theme: &Theme,
    log: &details::LogView<'_>,
    url: Option<&str>,
    narrow: bool,
) {
    if narrow {
        let style = theme.outlined_icon_button();
        if let Some(url) = url {
            if icon_button(
                ui,
                tl("The whole log, in GitLab"),
                Icon::ExternalLink,
                &style,
                24.0,
            )
            .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        if let details::LogView::Ready(text) = log {
            if icon_button(ui, tl("Copy"), Icon::Copy, &style, 24.0).clicked() {
                ui.ctx().copy_text(text.to_string());
            }
        }
        return;
    }
    if let Some(url) = url {
        if button(ui, theme, Some(Icon::ExternalLink), tl("Full log"), false)
            .on_hover_text(tl("The whole log, in GitLab"))
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    if let details::LogView::Ready(text) = log {
        if button(ui, theme, Some(Icon::Copy), tl("Copy"), false).clicked() {
            ui.ctx().copy_text(text.to_string());
        }
    }
}

/// The log's lines, numbered, scrolled to the end; the failures on a red band.
fn terminal(ui: &mut Ui, theme: &Theme, job_id: u64, text: &str, max_height: f32) {
    let font = egui::FontId::monospace(theme.text_size - 2.0);
    let lines: Vec<&str> = text.lines().collect();
    let digits = ui
        .painter()
        .layout_no_wrap(lines.len().to_string(), font.clone(), Color32::WHITE)
        .size();
    let line_height = digits.y + 3.0;
    let gutter = digits.x + 28.0;
    let number_color = theme.palette.text_muted.gamma_multiply(0.7);
    egui::ScrollArea::both()
        .id_salt(("mr page job log", job_id))
        .max_height(max_height)
        // Inside the inspector's own scroll, it would shrink to egui's minimum.
        .min_scrolled_height(max_height)
        .stick_to_bottom(true)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, line) in lines.iter().enumerate() {
                let failure = details::is_failure(line);
                let color = if failure {
                    theme.tones.red.fg
                } else {
                    theme.palette.text_secondary
                };
                let galley =
                    ui.painter()
                        .layout_no_wrap(line.replace('\t', "    "), font.clone(), color);
                let width = (gutter + galley.size().x + 16.0).max(ui.available_width());
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(width, line_height), Sense::hover());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let painter = ui.painter();
                if failure {
                    painter.rect_filled(rect, 0.0, theme.tones.red.bg);
                    painter.rect_filled(
                        egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
                        0.0,
                        theme.tones.red.fg,
                    );
                }
                painter.text(
                    egui::pos2(rect.left() + gutter - 14.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    font.clone(),
                    number_color,
                );
                painter.galley(
                    egui::pos2(
                        rect.left() + gutter,
                        rect.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    color,
                );
            }
        });
}

/// Older pipelines listed under the latest one in the inspector.
const OLDER_SHOWN: usize = 4;

/// The pipelines in the inspector, as narrow as it is: the latest as a card,
/// its stages one under the other and their jobs as in the MR page's graph, the
/// log of the job picked (the first failed one at first) under it; the older
/// ones as rows, a click opening them in GitLab.
pub fn compact(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let Some(latest) = mr.pipelines.first() else {
        ui.label(muted(&theme, tl("No pipeline yet.")));
        return;
    };
    let id = ui.make_persistent_id(("inspector pipelines", &mr.id));
    let mut state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let project_url = project_url(&mr.web_url);
    let demo = g.app.read_only;
    let (label, tone) = tones::pipeline(latest.status, &theme.tones);

    egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The inspector spaces its rows widely, and a row is at least the
            // theme's control height: the card spaces its own.
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().interact_size.y = 18.0;
            // The header: the state down the left edge, the chip, the id, when;
            // the jobs' tally under them.
            let header = egui::Frame::new()
                .inner_margin(Margin {
                    left: 14,
                    right: 10,
                    top: 9,
                    bottom: 9,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add(Chip::new(label, tone).strong());
                        ui.label(
                            mono(format!("#{}", latest.id), theme.text_size - 0.5)
                                .color(theme.palette.text),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(url) = project_url.as_deref() {
                                let style = theme.icon_button();
                                if icon_button(
                                    ui,
                                    tl("Open in GitLab"),
                                    Icon::ExternalLink,
                                    &style,
                                    22.0,
                                )
                                .clicked()
                                {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                                        "{url}/-/pipelines/{}",
                                        latest.id
                                    )));
                                }
                            }
                            if let Some(at) = &latest.created_at {
                                ui.label(muted(&theme, format_relative_date(at)));
                            }
                        });
                    });
                    if !latest.jobs.is_empty() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            tally_left(ui, &theme, &latest.jobs);
                        });
                    }
                });
            let edge = header.response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    edge.min + Vec2::new(0.0, 10.0),
                    Vec2::new(3.0, edge.height() - 20.0),
                ),
                CornerRadius::same(2),
                tone.fg,
            );
            if latest.jobs.is_empty() {
                return;
            }
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().min.y,
                Stroke::new(1.0, theme.palette.border),
            );
            egui::Frame::new()
                .inner_margin(Margin::symmetric(10, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    stages(g, ui, &theme, latest, &mut state, project_url.as_deref(), demo);
                });
        });

    let older: Vec<&Pipeline> = mr.pipelines.iter().skip(1).take(OLDER_SHOWN).collect();
    if !older.is_empty() {
        ui.add_space(6.0);
        for pipeline in older {
            older_row(ui, &theme, pipeline, project_url.as_deref());
        }
    }
    let hidden = mr.pipelines.len().saturating_sub(1 + OLDER_SHOWN);
    if hidden > 0 {
        ui.add_space(2.0);
        ui.hyperlink_to(
            RichText::new(tn(
                hidden as u64,
                "{n} older pipeline in GitLab",
                "{n} older pipelines in GitLab",
            ))
            .size(theme.text_size - 1.5),
            format!("{}/pipelines", mr.web_url),
        );
    }
    ui.data_mut(|d| d.insert_temp(id, state));
}

/// "1 failed · 4 jobs · 2m 10s", left to right.
fn tally_left(ui: &mut Ui, theme: &Theme, jobs: &[PipelineJob]) {
    let size = theme.text_size - 1.5;
    ui.label(
        RichText::new(tn(jobs.len() as u64, "{n} job", "{n} jobs"))
            .size(size)
            .color(theme.palette.text_muted),
    );
    let failed = jobs.iter().filter(|j| j.status == "failed").count();
    if failed > 0 {
        ui.label(RichText::new("·").size(size).color(theme.palette.text_muted));
        ui.label(
            RichText::new(tn(failed as u64, "{n} failed", "{n} failed"))
                .size(size)
                .color(theme.tones.red.fg),
        );
    }
    let total: f64 = jobs.iter().filter_map(|j| j.duration).sum();
    if total > 0.0 {
        ui.label(RichText::new("·").size(size).color(theme.palette.text_muted));
        ui.label(
            RichText::new(long_duration(total))
                .size(size)
                .color(theme.palette.text_muted),
        );
    }
}

/// The stages one under the other, a rail joining them; the log of the job
/// picked opens under it.
fn stages(
    g: &mut GuiApp,
    ui: &mut Ui,
    theme: &Theme,
    pipeline: &Pipeline,
    state: &mut State,
    project_url: Option<&str>,
    demo: bool,
) {
    let mut names: Vec<&str> = Vec::new();
    for job in &pipeline.jobs {
        if !names.contains(&job.stage.as_str()) {
            names.push(&job.stage);
        }
    }
    let picked = match state.picked {
        Some((pid, index)) if pid == pipeline.id => Some(index),
        _ => pipeline.jobs.iter().position(|j| j.status == "failed"),
    }
    .filter(|&i| pipeline.jobs.get(i).is_some_and(|j| has_log(j, demo)));

    let width = ui.available_width();
    let mut clicked = None;
    let mut previous: Option<egui::Pos2> = None;
    for (n, stage) in names.iter().enumerate() {
        if n > 0 {
            ui.add_space(8.0);
        }
        let jobs: Vec<(usize, &PipelineJob)> = pipeline
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| j.stage == *stage)
            .collect();
        // The stage's state: its worst job's.
        let worst = stage_status(jobs.iter().map(|(_, j)| j.status.as_str()));
        let title = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (glyph, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter()
                .circle_filled(glyph.center(), 3.5, job_color(theme, worst));
            // The name and the count as one text, cut when the name is long.
            let size = theme.text_size - 2.5;
            let mut text = egui::text::LayoutJob::default();
            text.append(
                &stage.to_uppercase(),
                0.0,
                egui::TextFormat::simple(fonts::medium(size), theme.palette.text_muted),
            );
            text.append(
                &jobs.len().to_string(),
                6.0,
                egui::TextFormat::simple(
                    egui::FontId::proportional(size),
                    theme.palette.text_muted,
                ),
            );
            ui.add(egui::Label::new(text).truncate());
            glyph.center()
        });
        let anchor = title.inner;
        // The rail from the previous stage's dot to this one's.
        if let Some(from) = previous {
            ui.painter().line_segment(
                [from + Vec2::new(0.0, 6.0), anchor - Vec2::new(0.0, 6.0)],
                Stroke::new(1.5, theme.palette.border),
            );
        }
        let mut last_bottom = anchor.y;
        for (index, job) in jobs {
            ui.add_space(4.0);
            let response = ui
                .horizontal(|ui| {
                    ui.add_space(14.0);
                    job_box(ui, theme, job, width - 14.0, picked == Some(index), demo)
                })
                .inner;
            last_bottom = response.rect.center().y;
            // A tick from the rail to the job.
            ui.painter().line_segment(
                [
                    egui::pos2(anchor.x, last_bottom),
                    egui::pos2(response.rect.left() - 2.0, last_bottom),
                ],
                Stroke::new(1.5, theme.palette.border),
            );
            if response.clicked() && has_log(job, demo) {
                clicked = Some(index);
            }
            if picked == Some(index) && clicked.is_none() {
                ui.add_space(6.0);
                let job_url = project_url
                    .filter(|_| job.id != 0)
                    .map(|url| format!("{url}/-/jobs/{}", job.id));
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(width - 14.0);
                        log_panel(g, ui, theme, job, job_url.as_deref(), 280.0);
                    });
                });
                last_bottom = ui.cursor().min.y;
            }
        }
        // The rail down the stage, along its jobs.
        ui.painter().line_segment(
            [anchor + Vec2::new(0.0, 6.0), egui::pos2(anchor.x, last_bottom)],
            Stroke::new(1.5, theme.palette.border),
        );
        previous = Some(egui::pos2(anchor.x, last_bottom));
    }
    if let Some(index) = clicked {
        // A second click on the job shown folds its log.
        state.picked = if picked == Some(index) {
            Some((pipeline.id, usize::MAX))
        } else {
            Some((pipeline.id, index))
        };
    }
}

/// The state a stage reads as: failed if a job failed, else running, else
/// waiting, else passed, else its first job's.
fn stage_status<'a>(statuses: impl Iterator<Item = &'a str> + Clone) -> &'a str {
    for wanted in ["failed", "running", "pending", "created", "success"] {
        if let Some(s) = statuses.clone().find(|s| *s == wanted) {
            return s;
        }
    }
    statuses.clone().next().unwrap_or("skipped")
}

/// An older pipeline: its glyph, id, state and when; a click opens it in GitLab.
fn older_row(ui: &mut Ui, theme: &Theme, pipeline: &Pipeline, project_url: Option<&str>) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 28.0),
        if project_url.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if response.hovered() && project_url.is_some() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.small),
            theme.palette.border_soft.gamma_multiply(0.5),
        );
    }
    let (label, tone) = tones::pipeline(pipeline.status, &theme.tones);
    let status = match pipeline.status {
        crate::models::PipelineState::Success => "success",
        crate::models::PipelineState::Failed => "failed",
        crate::models::PipelineState::Running => "running",
        crate::models::PipelineState::Pending | crate::models::PipelineState::Created => "pending",
        crate::models::PipelineState::Canceled => "canceled",
        crate::models::PipelineState::Skipped => "skipped",
        crate::models::PipelineState::Unknown => "unknown",
    };
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Vec2::splat(13.0),
    );
    status_glyph(ui.painter(), glyph, status, tone.fg);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.with_min_x(glyph.right() + 8.0).shrink2(Vec2::new(0.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 8.0;
    row.label(
        mono(format!("#{}", pipeline.id), theme.text_size - 1.5)
            .color(theme.palette.text_secondary),
    );
    row.label(
        RichText::new(label)
            .size(theme.text_size - 1.5)
            .color(tone.fg),
    );
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(8.0);
        if response.hovered() && project_url.is_some() {
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
            super::icons::paint(ui.painter(), icon, Icon::ExternalLink, theme.palette.text_muted);
        }
        if let Some(at) = &pipeline.created_at {
            ui.label(muted(theme, format_relative_date(at)).size(theme.text_size - 1.5));
        }
    });
    if let Some(url) = project_url {
        if response
            .on_hover_text(tl("Open in GitLab"))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                "{url}/-/pipelines/{}",
                pipeline.id
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_is_read_off_the_mr_url() {
        assert_eq!(
            project_url("https://gitlab.com/shop/api/-/merge_requests/104").as_deref(),
            Some("https://gitlab.com/shop/api")
        );
        assert_eq!(project_url("not a url"), None);
    }

    #[test]
    fn durations_read_as_in_gitlab() {
        assert_eq!(long_duration(42.4), "42s");
        assert_eq!(long_duration(185.0), "3m 05s");
        assert_eq!(long_duration(3720.0), "1h 2m");
    }
}
