//! The inspector's body, and its review, pipeline and effort cards.

use super::*;

pub(super) fn body(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    people(g, ui, mr);
    planning(ui, &theme, mr);
    history(g, ui, mr);
    anchor(g, ui, InspectorSection::Review);
    review(g, ui, mr);
    if let Some(stats) = &mr.diff_stats {
        let score = stats.difficulty(&g.app.config.complexity_profile);
        let (label, tone) = tones::effort(Effort::from_score(score), &theme.tones);
        let profile = g.app.config.complexity_profile.name.clone();
        section(ui, &theme, tl("Review effort"), |ui| {
            ui.horizontal(|ui| {
                ui.label(muted(&theme, tf("Profile: {name}", &[("name", &profile)])));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.add(Chip::new(label, tone).glyph("●").pill());
                });
            });
            effort_blocks(ui, &theme, score, tone);
            ui.label(muted(
                &theme,
                format!(
                    "{} · +{} −{} · {}",
                    tn(stats.files_changed as u64, "{n} file", "{n} files"),
                    stats.additions,
                    stats.deletions,
                    tn(stats.commits_count as u64, "{n} commit", "{n} commits"),
                ),
            ));
        });
    }
    anchor(g, ui, InspectorSection::Changes);
    section(ui, &theme, tl("Changes"), |ui| {
        super::super::details::changes(
            &mut g.details,
            &g.app,
            ui,
            &theme,
            &mr.id,
            &mr.web_url,
            false,
        );
    });
    anchor(g, ui, InspectorSection::Branches);
    // The deployments come with the MR page's data, read once per version
    // of the MR and shared with the page.
    let data = g.details.page_data(&g.app, mr, ui.ctx()).cloned();
    branches(g, ui, mr, data.as_ref());
    if !mr.labels.is_empty() {
        let mut clicked = None;
        section(ui, &theme, tl("Labels"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                for label in &mr.labels {
                    if label_chip(ui, &g.app, label, &theme).clicked() {
                        clicked = Some(label.clone());
                    }
                }
            });
        });
        if let Some(label) = clicked {
            g.filter_by_label(&label);
        }
    }
    if g.app.tracker.is_some() || mr.linked_ticket.is_some() {
        anchor(g, ui, InspectorSection::Ticket);
        ticket(g, ui, mr);
    }
    anchor(g, ui, InspectorSection::Threads);
    threads(ui, &theme, mr, g.app.config.gitlab_username.as_deref());
    anchor(g, ui, InspectorSection::Pipelines);
    pipelines(g, ui, mr);
    anchor(g, ui, InspectorSection::Description);
    if !mr.description.trim().is_empty() {
        section(ui, &theme, tl("Description"), |ui| {
            g.description.show(ui, &theme, mr, false);
        });
    }
    ui.add_space(16.0);
}

pub(super) fn review(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let tones = &theme.tones;
    let me = g.app.config.gitlab_username.clone();
    section(ui, &theme, tl("Review"), |ui| {
        key_values(ui, &theme, |rows| {
            rows.row(tl("Status"), |ui| {
                let waiting = me.as_deref().filter(|me| {
                    awaiting_review_by(
                        mr.state,
                        mr.mergeability,
                        &mr.reviewers,
                        &mr.approved_by,
                        me,
                    )
                });
                if let Some(username) = waiting {
                    let since = review_waiting_since(
                        mr.review.as_ref(),
                        mr.created_at.as_deref(),
                        username,
                    )
                    .map(|at| {
                        let when = format_relative_date(&at.to_rfc3339());
                        format!(" · {}", tf("requested {when}", &[("when", &when)]))
                    })
                    .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{}{since}", tl("Waiting for you")))
                            .color(tones.amber.fg),
                    );
                } else if !mr.approved_by.is_empty() {
                    ui.label(RichText::new(tl("Approved")).color(tones.green.fg));
                } else if mr.reviewers.is_empty() {
                    ui.label(muted(&theme, tl("No reviewer")));
                } else {
                    let names: Vec<String> = mr.reviewers.iter().map(|u| format!("@{u}")).collect();
                    ui.label(tf(
                        "Requested from {names}",
                        &[("names", &names.join(", "))],
                    ));
                }
            });
            if !mr.approved_by.is_empty() {
                rows.row(tl("Approved by"), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        for login in &mr.approved_by {
                            person(ui, &theme, me.as_deref(), &display_of(mr, None, login));
                        }
                    });
                });
            }
            rows.row(tl("Pipeline"), |ui| match mr.pipelines.first() {
                Some(pipeline) => {
                    let (label, tone) = tones::pipeline(pipeline.status, tones);
                    let detail = pipeline_detail(pipeline);
                    ui.label(
                        RichText::new(format!(
                            "{} {label}{detail}",
                            pipeline_glyph(pipeline.status)
                        ))
                        .color(tone.fg),
                    );
                }
                None => {
                    ui.label(muted(&theme, tl("None")));
                }
            });
            rows.row(tl("Behind"), |ui| {
                match mr.diff_stats.as_ref().and_then(|s| s.commits_behind) {
                    Some(0) => {
                        ui.label(tl("Up to date"));
                    }
                    Some(n) => {
                        let tone = if n >= 10 { tones.red } else { tones.amber };
                        let behind = tn(
                            n as u64,
                            "{n} commit behind {branch}",
                            "{n} commits behind {branch}",
                        );
                        let behind = behind.replace("{branch}", &mr.target_branch);
                        ui.label(RichText::new(behind).color(tone.fg));
                    }
                    None => {
                        ui.label(muted(&theme, "—"));
                    }
                }
            });
            // Only when the project enforces signed commits and one is off.
            if let Some(summary) = g
                .app
                .signature_report(mr)
                .map(|r| r.summary())
                .filter(|s| s.anomalies() > 0)
            {
                rows.row(tl("Signatures"), |ui| {
                    let unsigned = summary.unsigned.len() + summary.system.len();
                    let (text, tone) = if unsigned > 0 {
                        (
                            tn(
                                unsigned as u64,
                                "{n} commit without a signature",
                                "{n} commits without a signature",
                            ),
                            tones.red,
                        )
                    } else {
                        (
                            tn(
                                summary.unverified.len() as u64,
                                "{n} signature not verified",
                                "{n} signatures not verified",
                            ),
                            tones.amber,
                        )
                    };
                    ui.label(RichText::new(text).color(tone.fg));
                });
            }
            rows.row(tl("Threads"), |ui| {
                if mr.user_notes_count == 0 {
                    ui.label(muted(&theme, tl("None")));
                } else {
                    ui.label(
                        RichText::new(tf(
                            "{n} unresolved",
                            &[("n", &mr.user_notes_count.to_string())],
                        ))
                        .color(tones.amber.fg),
                    );
                }
            });
        });
    });
}

pub(super) fn pipeline_glyph(state: PipelineState) -> &'static str {
    match state {
        PipelineState::Success => "●",
        PipelineState::Failed => "✕",
        PipelineState::Running | PipelineState::Pending | PipelineState::Created => "◌",
        _ => "–",
    }
}

/// " · test:integration" for a failed pipeline, " · 4 of 7 jobs" while it runs.
pub(super) fn pipeline_detail(pipeline: &crate::models::Pipeline) -> String {
    match pipeline.status {
        PipelineState::Failed => pipeline
            .jobs
            .iter()
            .find(|j| j.status == "failed")
            .map(|j| format!(" · {}", j.name))
            .unwrap_or_default(),
        PipelineState::Running if !pipeline.jobs.is_empty() => {
            let done = pipeline
                .jobs
                .iter()
                .filter(|j| matches!(j.status.as_str(), "success" | "failed" | "skipped"))
                .count();
            let jobs = tf(
                "{done} of {total} jobs",
                &[
                    ("done", &done.to_string()),
                    ("total", &pipeline.jobs.len().to_string()),
                ],
            );
            format!(" · {jobs}")
        }
        _ => String::new(),
    }
}

/// Ten blocks, lit up to the effort score.
pub(super) fn effort_blocks(ui: &mut Ui, theme: &Theme, score: f64, tone: Tone) {
    let lit = ((score.clamp(0.0, 1.0) * 10.0).ceil() as usize).max(1);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let gap = 3.0;
    let width = (rect.width() - gap * 9.0) / 10.0;
    for i in 0..10 {
        let block = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * (width + gap), rect.top()),
            Vec2::new(width, rect.height()),
        );
        let color = if i < lit {
            tone.fg
        } else {
            theme.palette.border_soft
        };
        ui.painter()
            .rect_filled(block, egui::CornerRadius::same(2), color);
    }
}
