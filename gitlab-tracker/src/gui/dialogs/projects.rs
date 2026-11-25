//! The project picker, the project form in the dialogs' shell, and Log time.

use super::*;

/// The projects of `projects.toml`: a click shows its tab, opening it when
/// needed. "New project…" opens the project form.
pub(super) fn projects(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let mut picked = None;
    body(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(380.0)
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                    for (i, project) in g.app.project_picker.iter().enumerate() {
                        let current = crate::storage::is_same_project(
                            project,
                            &g.app.base_url,
                            &g.app.project_id,
                        );
                        let open = g.tab_of(&project.gitlab_url, &project.project_id).is_some();
                        let name = project.name.as_deref().unwrap_or(&project.project_id);
                        let response = egui::Frame::new()
                            .fill(if current {
                                theme.palette.selection
                            } else {
                                theme.palette.panel
                            })
                            .stroke(Stroke::new(
                                1.0,
                                if current {
                                    theme.palette.accent.gamma_multiply(0.6)
                                } else {
                                    theme.palette.border
                                },
                            ))
                            .corner_radius(CornerRadius::same(theme.radii.large))
                            .inner_margin(Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        ui.label(semibold(name, theme.text_size));
                                        ui.label(muted(
                                            &theme,
                                            format!(
                                                "{} · {}",
                                                project.gitlab_url, project.project_id
                                            ),
                                        ));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let state = if current {
                                            Some(tl("Shown"))
                                        } else if open {
                                            Some(tl("In a tab"))
                                        } else {
                                            None
                                        };
                                        if let Some(state) = state {
                                            super::super::widgets::badge(
                                                ui,
                                                &theme,
                                                state,
                                                theme.tones.green,
                                            );
                                        }
                                    });
                                });
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() && !current {
                            picked = Some(i);
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        if g.app.project_picker.is_empty() {
            ui.label(muted(&theme, tl("No project in projects.toml.")));
        }
    });
    if let Some(entry) = picked.and_then(|i| g.app.project_picker.get(i).cloned()) {
        g.pending.push(Action::SwitchProject(Box::new(entry)));
        g.app.input_mode = InputMode::Normal;
    }
    footer(ui, &theme, |ui| {
        ui.label(muted(&theme, tl("Esc to cancel")));
        buttons(ui, |ui| {
            if button(ui, &theme, Some(Icon::Plus), tl("New project…"), true).clicked() {
                g.app.input_mode = InputMode::Normal;
                g.wizard = Some(super::super::setup::ProjectForm::new(
                    &g.ctx,
                    super::super::setup::Mode::New,
                ));
            }
            if button(ui, &theme, None, tl("Close"), false).clicked() {
                close(g);
            }
        });
    });
}

/// The project form, as a dialog in the shell of the others: a new project,
/// or the tokens of one.
pub(super) fn wizard(g: &mut GuiApp, ctx: &egui::Context) {
    let theme = g.theme;
    let Some(form) = g.wizard.as_mut() else {
        return;
    };
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(1));
    let mut outcome = form.poll();
    let mut closed = false;
    let width = (ctx.content_rect().width() - 96.0).clamp(480.0, 620.0);
    let response = egui::Modal::new(egui::Id::unique("project_form"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            closed = header(ui, &theme, form.icon(), form.title(), Some(form.subtitle()));
            rule(ui, &theme);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing.item.y;
                // The new project's form is long: it scrolls in a low window.
                let height = (ctx.content_rect().height() - 220.0).clamp(220.0, 640.0);
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .show(ui, |ui| body(ui, |ui| form.body(ui, &theme)));
            });
            // The check's verdict stays in view, whatever the scroll.
            if form.has_verdict() {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 20,
                        right: 20,
                        top: 0,
                        bottom: 14,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        form.verdict(ui, &theme);
                    });
            }
            footer(ui, &theme, |ui| {
                if let super::super::setup::Outcome::Cancelled = form.footer(ui, &theme, true) {
                    outcome = super::super::setup::Outcome::Cancelled;
                }
            });
        });
    let tokens = form.is_tokens();
    if (closed || response.should_close()) && !form.is_busy() {
        outcome = super::super::setup::Outcome::Cancelled;
    }
    match outcome {
        super::super::setup::Outcome::Open => {}
        super::super::setup::Outcome::Cancelled => g.wizard = None,
        super::super::setup::Outcome::Saved(entry) => {
            g.wizard = None;
            if tokens {
                // Asked once: a key left empty opens the project without tickets.
                if let Some(t) = &entry.tracker {
                    g.skip_redmine
                        .insert(t.url.trim_end_matches('/').to_string());
                }
            }
            g.open_project(*entry);
        }
    }
}

// ── Log time ─────────────────────────────────────────────────────────────────

pub(super) fn log_time(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let activities = g.app.activities.clone();
    let error = g.app.activities_error.clone();
    body(ui, |ui| {
        let form = &mut g.app.log_time_form;
        egui::Grid::new("log_time")
            .num_columns(2)
            .spacing([14.0, 12.0])
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tl("Duration")).font(bevel_theme::fonts::medium(theme.text_size)),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut form.duration_input)
                        .hint_text("1h30, 45m, 1.5"),
                );
                ui.end_row();

                ui.label(
                    RichText::new(tl("Activity")).font(bevel_theme::fonts::medium(theme.text_size)),
                );
                if activities.is_empty() {
                    match &error {
                        Some(e) => ui.label(RichText::new(e).color(theme.tones.red.fg)),
                        None => ui.label(muted(&theme, tl("Loading…"))),
                    };
                } else {
                    let selected = activities
                        .get(form.selected_activity_idx)
                        .map(|a| a.name.as_str())
                        .unwrap_or("—");
                    egui::ComboBox::from_id_salt("activity")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (i, activity) in activities.iter().enumerate() {
                                ui.selectable_value(
                                    &mut form.selected_activity_idx,
                                    i,
                                    &activity.name,
                                );
                            }
                        });
                }
                ui.end_row();

                ui.label(
                    RichText::new(tl("Comment")).font(bevel_theme::fonts::medium(theme.text_size)),
                );
                ui.add(egui::TextEdit::multiline(&mut form.comment_input).desired_rows(3));
                ui.end_row();
            });
        if let Some(error) = &form.error {
            ui.add_space(6.0);
            ui.label(RichText::new(error).color(theme.tones.red.fg));
        }
    });
    let submitting = g.app.log_time_form.submitting;
    footer(ui, &theme, |ui| {
        ui.label(muted(&theme, tl("Esc to cancel")));
        buttons(ui, |ui| {
            let label = tl(if submitting { "Sending…" } else { "Log time" });
            if ui
                .add_enabled_ui(!submitting, |ui| primary_button(ui, &theme, label))
                .inner
                .clicked()
            {
                g.pending.push(Action::SubmitLogTime);
            }
            if button(ui, &theme, None, tl("Cancel"), false).clicked() {
                close(g);
            }
        });
    });
}

// ── Help ─────────────────────────────────────────────────────────────────────
