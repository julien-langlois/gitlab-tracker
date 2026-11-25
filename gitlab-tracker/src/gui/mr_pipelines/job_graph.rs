//! The MR page's pipeline graph: the stages side by side, their jobs as boxes, a click showing a job's log.

use super::*;

/// The stages side by side, their jobs as boxes; a click on a job shows its log
/// under the graph (the first failed job's at first).
pub(super) fn graph(
    g: &mut GuiApp,
    ui: &mut Ui,
    theme: &Theme,
    mr: &TrackedMr,
    pipeline: &Pipeline,
    state: &mut State,
    project_url: Option<&str>,
) {
    let demo = g.app.read_only;
    // In the order they run (GitLab lists the jobs newest first).
    let stages = crate::critical_path::stage_order(&pipeline.jobs);
    // The job picked in this pipeline, else its first failed one.
    let picked = match state.picked {
        Some((pid, index)) if pid == pipeline.id => Some(index),
        _ => pipeline.jobs.iter().position(|j| j.status == "failed"),
    }
    .filter(|&i| pipeline.jobs.get(i).is_some_and(|j| has_log(j, demo)));

    let mut clicked = None;
    ui.allocate_ui(Vec2::new(240.0, 28.0), |ui| {
        let labels = [tl("Stages"), tl("Timeline")];
        if let Some(i) = super::super::board::segmented(
            ui,
            theme,
            &format!("pipeline-view-{}", pipeline.id),
            &labels,
            usize::from(state.timeline),
        ) {
            state.timeline = i == 1;
        }
    });
    ui.add_space(12.0);
    if state.timeline {
        let usual = g.memo.usual_jobs.get(&g.app.project_id, (), || {
            crate::critical_path::usual(g.app.mrs.iter().flat_map(|m| m.pipelines.iter()))
        });
        if let Some(i) = super::super::pipeline_timeline::view(ui, theme, pipeline, &usual, picked)
        {
            if pipeline.jobs.get(i).is_some_and(|j| has_log(j, demo)) {
                clicked = Some(i);
            }
        }
    } else {
        egui::ScrollArea::horizontal()
            .id_salt(("mr pipeline graph", &mr.id, pipeline.id))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(STAGE_GAP, 0.0);
                    let mut previous: Option<f32> = None;
                    for stage in stages.iter().map(String::as_str) {
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
                                    muted(theme, jobs.len().to_string())
                                        .size(theme.text_size - 2.5),
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
    }
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
pub(super) fn has_log(job: &PipelineJob, demo: bool) -> bool {
    // The demo's jobs have no id: one canned log for all of them.
    (job.id != 0 || demo)
        && !matches!(
            job.status.as_str(),
            "skipped" | "created" | "pending" | "manual" | "scheduled"
        )
}

/// A job in the graph: its state's glyph, its name, its duration.
pub(super) fn job_box(
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

    let mut hover = Tip::new()
        .title(&job.name)
        .dot(color, tl(status_label(&job.status)));
    if sense == Sense::click() {
        hover = hover.hint(tl("Click to read its log"));
    }
    let response = response.rich(hover);
    if sense == Sense::click() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(super) fn status_label(status: &str) -> &'static str {
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

pub(super) fn job_color(theme: &Theme, status: &str) -> Color32 {
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
pub(super) fn status_glyph(
    painter: &egui::Painter,
    rect: egui::Rect,
    status: &str,
    color: Color32,
) {
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
