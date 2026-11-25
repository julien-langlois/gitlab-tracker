//! The pipelines in the inspector, as narrow as it is: the latest as a card, its stages one under the other, the older ones as rows.

use super::*;

/// Older pipelines listed under the latest one in the inspector.
pub(super) const OLDER_SHOWN: usize = 4;

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
                    stages(
                        g,
                        ui,
                        &theme,
                        latest,
                        &mut state,
                        project_url.as_deref(),
                        demo,
                    );
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
pub(super) fn tally_left(ui: &mut Ui, theme: &Theme, jobs: &[PipelineJob]) {
    let size = theme.text_size - 1.5;
    ui.label(
        RichText::new(tn(jobs.len() as u64, "{n} job", "{n} jobs"))
            .size(size)
            .color(theme.palette.text_muted),
    );
    let failed = jobs.iter().filter(|j| j.status == "failed").count();
    if failed > 0 {
        ui.label(
            RichText::new("·")
                .size(size)
                .color(theme.palette.text_muted),
        );
        ui.label(
            RichText::new(tn(failed as u64, "{n} failed", "{n} failed"))
                .size(size)
                .color(theme.tones.red.fg),
        );
    }
    let total: f64 = jobs.iter().filter_map(|j| j.duration).sum();
    if total > 0.0 {
        ui.label(
            RichText::new("·")
                .size(size)
                .color(theme.palette.text_muted),
        );
        ui.label(
            RichText::new(long_duration(total))
                .size(size)
                .color(theme.palette.text_muted),
        );
    }
}

/// The stages one under the other, a rail joining them; the log of the job
/// picked opens under it.
pub(super) fn stages(
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
            [
                anchor + Vec2::new(0.0, 6.0),
                egui::pos2(anchor.x, last_bottom),
            ],
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
pub(super) fn stage_status<'a>(statuses: impl Iterator<Item = &'a str> + Clone) -> &'a str {
    for wanted in ["failed", "running", "pending", "created", "success"] {
        if let Some(s) = statuses.clone().find(|s| *s == wanted) {
            return s;
        }
    }
    statuses.clone().next().unwrap_or("skipped")
}

/// An older pipeline: its glyph, id, state and when; a click opens it in GitLab.
pub(super) fn older_row(
    ui: &mut Ui,
    theme: &Theme,
    pipeline: &Pipeline,
    project_url: Option<&str>,
) {
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
            .max_rect(
                rect.with_min_x(glyph.right() + 8.0)
                    .shrink2(Vec2::new(0.0, 0.0)),
            )
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
            super::super::icons::paint(
                ui.painter(),
                icon,
                Icon::ExternalLink,
                theme.palette.text_muted,
            );
        }
        if let Some(at) = &pipeline.created_at {
            ui.label(muted(theme, format_relative_date(at)).size(theme.text_size - 1.5));
        }
    });
    if let Some(url) = project_url {
        if response
            .tip(tl("Open in GitLab"))
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
