//! A job's log: a title bar (name, state, copy, GitLab) over the end of the log, numbered, failures in red.

use super::*;

/// The log of `job`: a title bar (its name, state, copy, GitLab) over the end
/// of the log as in a terminal, numbered, the lines that tell what failed in red.
pub(super) fn log_panel(
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
                                ui.add(
                                    egui::Label::new(text)
                                        .truncate()
                                        .show_tooltip_when_elided(false),
                                )
                                .tip(&job.name);
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
                                super::super::widgets::spinner(ui, 12.0, theme.palette.accent);
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
pub(super) fn log_buttons(
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
            .tip(tl("The whole log, in GitLab"))
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
pub(super) fn terminal(ui: &mut Ui, theme: &Theme, job_id: u64, text: &str, max_height: f32) {
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
