//! The first-run window: with no project configured, `gitlab-tracker gui` asks
//! for the first one here (GitLab URL, project, name, branches, token) instead of
//! in the terminal, saves it to `projects.toml` and the token to the OS keyring,
//! then the main window opens on it.

use std::sync::{Arc, Mutex};

use bevel_theme::Theme;
use egui::{Align, Layout, Margin, RichText, Ui};
use zeroize::Zeroizing;

use super::widgets::{button, muted, semibold};
use crate::i18n::tl;
use crate::storage::ProjectEntry;

/// What is typed so far.
#[derive(Default)]
struct Form {
    url: String,
    project_id: String,
    name: String,
    branches: String,
    token: Zeroizing<String>,
    errors: Vec<&'static str>,
    /// Set when the token could not be stored (the project is saved).
    keyring_error: Option<String>,
}

/// A checked form, ready to save.
#[derive(Debug, PartialEq)]
struct Checked {
    url: String,
    project_id: String,
    name: Option<String>,
    branches: Vec<String>,
}

/// Checks the form: an http(s) URL without spaces (its trailing `/` dropped), a
/// project id or path without spaces, branch names without spaces (default
/// `main`), and the name cleaned like the settings do.
fn check(
    url: &str,
    project_id: &str,
    name: &str,
    branches: &str,
) -> Result<Checked, Vec<&'static str>> {
    let mut errors = Vec::new();
    let url = url.trim().trim_end_matches('/').to_string();
    let url_ok = (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() > "https://".len()
        && !url.contains(char::is_whitespace);
    if !url_ok {
        errors.push("The GitLab URL starts with https:// (or http://), without spaces.");
    }
    let project_id = project_id.trim().to_string();
    if project_id.is_empty() || project_id.contains(char::is_whitespace) {
        errors.push("The project is its numeric id or its path (group/project), without spaces.");
    }
    let mut list: Vec<String> = branches
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(str::to_string)
        .collect();
    if list.iter().any(|b| b.contains(char::is_whitespace)) {
        errors.push("Branch names have no spaces; separate them with commas.");
    }
    if list.is_empty() {
        list.push("main".into());
    }
    list.dedup();
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Checked {
        url,
        project_id,
        name: crate::settings_core::clean_project_name(name),
        branches: list,
    })
}

/// Opens the first-run window; `true` once a project is saved, `false` when the
/// window was closed without one.
pub async fn run() -> anyhow::Result<bool> {
    let saved = Arc::new(Mutex::new(false));
    let done = saved.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GitLab Tracker")
            .with_app_id("gitlab-tracker")
            .with_inner_size([560.0, 640.0])
            .with_resizable(false),
        ..Default::default()
    };
    tokio::task::block_in_place(|| {
        eframe::run_native(
            "gitlab-tracker-setup",
            options,
            Box::new(move |cc| {
                bevel_theme::fonts::install(&cc.egui_ctx);
                super::i18n::install();
                super::i18n::set_language(None);
                let theme = Theme::dark();
                theme.apply(&cc.egui_ctx);
                Ok(Box::new(Setup {
                    theme,
                    form: Form {
                        url: "https://gitlab.com".into(),
                        branches: "main".into(),
                        ..Form::default()
                    },
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

struct Setup {
    theme: Theme,
    form: Form,
    saved: Arc<Mutex<bool>>,
}

impl eframe::App for Setup {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        super::screenshot_for_development(ui.ctx());
        let theme = self.theme;
        egui::CentralPanel::default()
            .frame(theme.content_frame().inner_margin(Margin::same(28)))
            .show(ui, |ui| {
                ui.label(semibold(tl("Welcome to GitLab Tracker"), theme.text_size + 6.0));
                ui.add_space(4.0);
                ui.label(muted(
                    &theme,
                    tl("No project is configured yet. Tell it which GitLab project to follow; everything can be changed later in projects.toml."),
                ));
                ui.add_space(18.0);
                let form = &mut self.form;
                field(ui, &theme, tl("GitLab URL"), tl("https://gitlab.com or your own instance"), &mut form.url, false);
                field(ui, &theme, tl("Project"), tl("Its numeric id (Settings › General) or its path, group/project"), &mut form.project_id, false);
                field(ui, &theme, tl("Name (optional)"), tl("Shown in the title bar instead of the project id"), &mut form.name, false);
                field(ui, &theme, tl("Tracked branches"), tl("Comma-separated, in the table's order: main, staging, production"), &mut form.branches, false);
                field(ui, &theme, tl("Personal access token"), tl("Scope read_api; saved in the OS keyring, never in a file. Empty: asked in the terminal."), &mut form.token, true);
                for error in &form.errors {
                    ui.label(RichText::new(format!("⚠ {}", tl(error))).color(theme.tones.red.fg));
                }
                if let Some(error) = &form.keyring_error {
                    ui.label(RichText::new(format!("⚠ {}: {error}", tl("The token could not be saved in the keyring"))).color(theme.tones.amber.fg));
                }
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if button(ui, &theme, None, tl("Save and open"), true).clicked() {
                        self.submit(ui.ctx());
                    }
                    if button(ui, &theme, None, tl("Cancel"), false).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }
}

impl Setup {
    fn submit(&mut self, ctx: &egui::Context) {
        // Already saved, the token's error shown: open the window now.
        if *self
            .saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let form = &mut self.form;
        let checked = match check(&form.url, &form.project_id, &form.name, &form.branches) {
            Ok(checked) => checked,
            Err(errors) => {
                form.errors = errors;
                return;
            }
        };
        form.errors.clear();
        let entry = ProjectEntry {
            name: checked.name,
            gitlab_url: checked.url,
            project_id: checked.project_id,
            tracked_branches: Some(checked.branches),
            ..ProjectEntry::default()
        };
        let token = std::mem::take(&mut form.token);
        let result = futures::executor::block_on(crate::storage::add_first_project(entry, token));
        *self
            .saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        match result {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            // Saved, but say why the token will be asked again.
            Err(e) => form.keyring_error = Some(e),
        }
    }
}

/// A labelled field with its help below; `secret` masks what is typed.
fn field(ui: &mut Ui, theme: &Theme, label: &str, help: &str, text: &mut String, secret: bool) {
    ui.label(RichText::new(label).font(bevel_theme::fonts::medium(theme.text_size)));
    ui.add_space(3.0);
    ui.add(
        egui::TextEdit::singleline(text)
            .password(secret)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(8, 6)),
    );
    ui.add_space(2.0);
    ui.label(muted(theme, help).size(theme.text_size - 1.5));
    ui.add_space(10.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_form_is_cleaned() {
        let checked = check(
            " https://gitlab.example/ ",
            " group/app ",
            "  My   App ",
            "main, staging,,prod ",
        )
        .unwrap();
        assert_eq!(
            checked,
            Checked {
                url: "https://gitlab.example".into(),
                project_id: "group/app".into(),
                name: Some("My App".into()),
                branches: vec!["main".into(), "staging".into(), "prod".into()],
            }
        );
        // No branch: main.
        assert_eq!(
            check("https://g.io", "42", "", " ").unwrap().branches,
            ["main"]
        );
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(check("gitlab.com", "42", "", "main").unwrap_err().len(), 1);
        assert_eq!(check("https://", "42", "", "main").unwrap_err().len(), 1);
        assert_eq!(
            check("https://g.io", "my project", "", "main")
                .unwrap_err()
                .len(),
            1
        );
        assert_eq!(
            check("https://g.io", "", "", "release 1")
                .unwrap_err()
                .len(),
            2
        );
    }
}
