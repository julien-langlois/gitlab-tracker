//! The form in a window of its own, before the main one opens: for the first project, or the tokens a project lacks.

use super::*;

/// Opens the form in a window of its own, for the first project or the tokens
/// `mode` asks for; `true` once saved, `false` when closed without.
pub async fn run(mode: Option<Mode>) -> anyhow::Result<bool> {
    let saved = Arc::new(Mutex::new(false));
    let done = saved.clone();
    let height = if mode.is_some() { 420.0 } else { 760.0 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GitLab Tracker")
            .with_app_id("gitlab-tracker")
            .with_inner_size([560.0, height])
            .with_resizable(false),
        ..Default::default()
    };
    tokio::task::block_in_place(|| {
        eframe::run_native(
            "gitlab-tracker-setup",
            options,
            Box::new(move |cc| {
                bevel_theme::fonts::install(&cc.egui_ctx);
                super::super::i18n::install();
                let language =
                    futures::executor::block_on(crate::storage::load_projects_toml()).language;
                super::super::i18n::set_language(language.as_deref());
                let theme = Theme::dark();
                theme.apply(&cc.egui_ctx);
                let form = match mode {
                    Some(mode) => ProjectForm::new(&cc.egui_ctx, mode),
                    None => ProjectForm::first_run(&cc.egui_ctx),
                };
                Ok(Box::new(Setup {
                    theme,
                    form,
                    saved: done,
                }))
            }),
        )
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let saved = *saved
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(saved)
}

pub(super) struct Setup {
    pub(super) theme: Theme,
    pub(super) form: ProjectForm,
    pub(super) saved: Arc<Mutex<bool>>,
}

impl eframe::App for Setup {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        super::super::screenshot_for_development(ui.ctx());
        let theme = self.theme;
        let mut outcome = self.form.poll();
        // The dialogs' shell, on the whole window: header, the fields scrolling,
        // the footer and its buttons at the bottom.
        egui::Panel::bottom("setup_footer")
            .frame(egui::Frame::new().fill(theme.palette.raised))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // The check's verdict stays in view, whatever the scroll.
                if self.form.has_verdict() {
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 20,
                            right: 20,
                            top: 12,
                            bottom: 14,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.form.verdict(ui, &theme);
                        });
                }
                super::super::dialogs::footer(ui, &theme, |ui| {
                    if let Outcome::Cancelled = self.form.footer(ui, &theme, false) {
                        outcome = Outcome::Cancelled;
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme.palette.raised))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (title, subtitle) = if matches!(self.form.mode, Mode::New) {
                    (
                        tl("Welcome to GitLab Tracker"),
                        tl("No project is configured yet. Tell it which GitLab project to follow; everything can be changed later in projects.toml."),
                    )
                } else {
                    (self.form.title(), self.form.subtitle())
                };
                if super::super::dialogs::header(ui, &theme, self.form.icon(), title, Some(subtitle)) {
                    outcome = Outcome::Cancelled;
                }
                super::super::widgets::rule(ui, &theme);
                ui.spacing_mut().item_spacing.y = theme.spacing.item.y;
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| super::super::dialogs::body(ui, |ui| self.form.body(ui, &theme)));
            });
        match outcome {
            Outcome::Open => {}
            Outcome::Cancelled => {
                if !self.form.is_busy() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Outcome::Saved(_) => {
                *self
                    .saved
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
