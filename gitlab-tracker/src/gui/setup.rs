//! The project form, in a window of its own or as a dialog of the main window:
//!
//! * **new project** — GitLab URL, project, name, branches, the GitLab token and,
//!   with Redmine, the tracker's URL and token. A token already in the keyring
//!   (or the environment) for the same instance is said so, and needs not be typed
//!   again. The project and the token are checked against GitLab before anything
//!   is saved (the name and the default branch come from it when left empty);
//! * **tokens** — a project of `projects.toml` whose token is missing: asked here,
//!   since the window has no terminal to prompt in.
//!
//! With no project configured, `gitlab-tracker gui` opens the form in a window of
//! its own ([`run`]); the main window opens it from its `+` and when a project it
//! loads has no token.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use bevel_theme::Theme;
use egui::{Align, Layout, Margin, RichText, Ui};
use zeroize::Zeroizing;

use super::widgets::{button, muted, semibold, spinner};
use crate::i18n::{tf, tl};
use crate::storage::ProjectEntry;

/// What the form asks for.
#[derive(Debug, Clone)]
pub enum Mode {
    /// A project to add to `projects.toml`.
    New,
    /// The tokens `entry` lacks: GitLab's when `gitlab`, Redmine's when `redmine`.
    Tokens {
        entry: Box<ProjectEntry>,
        gitlab: bool,
        redmine: bool,
    },
}

/// Which token a field is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Gitlab,
    #[cfg_attr(not(feature = "redmine"), allow(dead_code))]
    Redmine,
}

/// The instances known to have a token (keyring or environment), by kind and
/// URL; looked up in the background, since the keyring blocks.
#[derive(Default)]
struct Known {
    found: HashMap<(Kind, String), bool>,
    pending: HashSet<(Kind, String)>,
}

/// The project form's state.
pub struct ProjectForm {
    mode: Mode,
    url: String,
    project_id: String,
    name: String,
    branches: String,
    token: Zeroizing<String>,
    redmine_url: String,
    redmine_token: Zeroizing<String>,
    errors: Vec<String>,
    known: Arc<Mutex<Known>>,
    /// The check and save running in the background.
    task: Option<Receiver<Result<ProjectEntry, Vec<String>>>>,
    /// Saved as the `active` project (the first-run window).
    make_active: bool,
    ctx: egui::Context,
}

/// What a frame of the form ended with.
pub enum Outcome {
    Open,
    Cancelled,
    /// Saved: the project to open.
    Saved(Box<ProjectEntry>),
}

impl ProjectForm {
    pub fn new(ctx: &egui::Context, mode: Mode) -> Self {
        let form = Self {
            mode,
            url: "https://gitlab.com".into(),
            project_id: String::new(),
            name: String::new(),
            branches: String::new(),
            token: Zeroizing::default(),
            redmine_url: String::new(),
            redmine_token: Zeroizing::default(),
            errors: Vec::new(),
            known: Arc::default(),
            task: None,
            make_active: false,
            ctx: ctx.clone(),
        };
        // The instances of the projects already configured: their tokens are
        // looked up at once, so the form says which ones need no typing.
        if matches!(form.mode, Mode::New) {
            let known = form.known.clone();
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let projects = crate::storage::load_projects_toml().await.projects;
                let mut urls: Vec<(Kind, String)> = Vec::new();
                for p in &projects {
                    urls.push((Kind::Gitlab, normalize(&p.gitlab_url)));
                    #[cfg(feature = "redmine")]
                    if let Some(t) = p.tracker.as_ref().filter(|t| is_redmine(&t.provider)) {
                        urls.push((Kind::Redmine, normalize(&t.url)));
                    }
                }
                urls.sort_by(|a, b| a.1.cmp(&b.1));
                urls.dedup();
                if let Ok(mut k) = known.lock() {
                    k.pending.extend(urls.iter().cloned());
                }
                for (kind, url) in urls {
                    lookup(&known, kind, url, &ctx).await;
                }
            });
        }
        form
    }

    /// The first-run window's form: the project is saved as the active one.
    fn first_run(ctx: &egui::Context) -> Self {
        Self {
            branches: "main".into(),
            make_active: true,
            ..Self::new(ctx, Mode::New)
        }
    }

    /// Whether it asks for the tokens of a configured project.
    pub fn is_tokens(&self) -> bool {
        matches!(self.mode, Mode::Tokens { .. })
    }

    /// Whether a check is running: the dialog stays open meanwhile.
    pub fn is_busy(&self) -> bool {
        self.task.is_some()
    }

    /// The window title for this form.
    pub fn title(&self) -> &'static str {
        match self.mode {
            Mode::New => tl("Add a project"),
            Mode::Tokens { .. } => tl("Tokens needed"),
        }
    }

    /// Whether the token of `url` is known: `Some(true)` saved, `Some(false)` not,
    /// `None` still looked up (started here when needed and `lookup_now`).
    fn token_known(&self, kind: Kind, url: &str, lookup_now: bool) -> Option<bool> {
        let url = normalize(url);
        if url.is_empty() {
            return Some(false);
        }
        let mut known = self.known.lock().ok()?;
        if let Some(found) = known.found.get(&(kind, url.clone())) {
            return Some(*found);
        }
        let key = (kind, url.clone());
        if known.pending.contains(&key) {
            return None;
        }
        if lookup_now {
            known.pending.insert(key);
            let (known, ctx) = (self.known.clone(), self.ctx.clone());
            tokio::spawn(async move { lookup(&known, kind, url, &ctx).await });
            return None;
        }
        // Being typed: looked up once the field is left.
        Some(false)
    }

    /// Draws the form; its buttons end it.
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme) -> Outcome {
        if let Some(done) = self.task.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.task = None;
            match done {
                Ok(entry) => return Outcome::Saved(Box::new(entry)),
                Err(errors) => self.errors = errors,
            }
        }
        let busy = self.task.is_some();
        ui.add_enabled_ui(!busy, |ui| match self.mode.clone() {
            Mode::New => self.new_fields(ui, theme),
            Mode::Tokens {
                entry,
                gitlab,
                redmine,
            } => self.token_fields(ui, theme, &entry, gitlab, redmine),
        });
        for error in &self.errors {
            ui.label(RichText::new(format!("⚠ {error}")).color(theme.tones.red.fg));
        }
        ui.add_space(12.0);
        let mut outcome = Outcome::Open;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let label = match self.mode {
                Mode::New => tl("Check and add"),
                Mode::Tokens { .. } => tl("Save and open"),
            };
            if busy {
                ui.add_enabled_ui(false, |ui| button(ui, theme, None, label, true));
                ui.label(muted(theme, tl("Checking with GitLab…")));
                spinner(ui, 12.0, theme.palette.accent);
            } else if button(ui, theme, None, label, true).clicked() {
                self.submit();
            }
            if button(ui, theme, None, tl("Cancel"), false).clicked() {
                outcome = Outcome::Cancelled;
            }
        });
        outcome
    }

    fn new_fields(&mut self, ui: &mut Ui, theme: &Theme) {
        let url = field(
            ui,
            theme,
            tl("GitLab URL"),
            tl("https://gitlab.com or your own instance"),
            &mut self.url,
            false,
        );
        field(
            ui,
            theme,
            tl("Project"),
            tl("Its numeric id (Settings › General) or its path, group/project"),
            &mut self.project_id,
            false,
        );
        field(
            ui,
            theme,
            tl("Name (optional)"),
            tl("Shown in the tab; GitLab's name when empty"),
            &mut self.name,
            false,
        );
        field(ui, theme, tl("Tracked branches (optional)"), tl("Comma-separated, in the table's order: main, staging, production. The default branch when empty."), &mut self.branches, false);
        let known = self.token_known(Kind::Gitlab, &self.url, !url.has_focus());
        let mut token = std::mem::take(&mut self.token);
        token_field(
            ui,
            theme,
            tl("Personal access token"),
            tl("Scope read_api; saved in the OS keyring, never in a file."),
            &mut token,
            known,
        );
        self.token = token;

        #[cfg(feature = "redmine")]
        {
            ui.add_space(4.0);
            ui.label(semibold(tl("Redmine (optional)"), theme.text_size + 1.0));
            ui.add_space(6.0);
            let url = field(
                ui,
                theme,
                tl("Redmine URL"),
                tl("Links the MRs to their tickets; empty: no tracker"),
                &mut self.redmine_url,
                false,
            );
            if !self.redmine_url.trim().is_empty() {
                let known = self.token_known(Kind::Redmine, &self.redmine_url, !url.has_focus());
                let mut token = std::mem::take(&mut self.redmine_token);
                token_field(
                    ui,
                    theme,
                    tl("Redmine API key"),
                    tl("My account › API access key; saved in the OS keyring."),
                    &mut token,
                    known,
                );
                self.redmine_token = token;
            }
        }
    }

    fn token_fields(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        entry: &ProjectEntry,
        gitlab: bool,
        redmine: bool,
    ) {
        let name = entry.name.as_deref().unwrap_or(&entry.project_id);
        ui.label(muted(
            theme,
            tf(
                "{project} has no token in the OS keyring for the instances below. Type them once: they are saved in the keyring, and every project on the same instance uses them.",
                &[("project", name)],
            ),
        ));
        ui.add_space(12.0);
        if gitlab {
            let label = tf("GitLab token · {url}", &[("url", &entry.gitlab_url)]);
            let mut token = std::mem::take(&mut self.token);
            token_field(
                ui,
                theme,
                &label,
                tl("Personal access token, scope read_api."),
                &mut token,
                Some(false),
            );
            self.token = token;
        }
        if redmine {
            let url = entry
                .tracker
                .as_ref()
                .map(|t| t.url.as_str())
                .unwrap_or_default();
            let label = tf("Redmine API key · {url}", &[("url", url)]);
            let mut token = std::mem::take(&mut self.redmine_token);
            token_field(
                ui,
                theme,
                &label,
                tl("Empty: the project opens without its tickets."),
                &mut token,
                Some(false),
            );
            self.redmine_token = token;
        }
    }

    /// Checks the fields, then checks and saves in the background.
    fn submit(&mut self) {
        self.errors.clear();
        let (done, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        match self.mode.clone() {
            Mode::New => {
                let checked = match check(&self.url, &self.project_id, &self.name, &self.branches) {
                    Ok(checked) => checked,
                    Err(errors) => {
                        self.errors = errors.into_iter().map(|e| tl(e).to_string()).collect();
                        return;
                    }
                };
                let request = NewProject {
                    checked,
                    token: std::mem::take(&mut self.token),
                    redmine_url: normalize(&self.redmine_url),
                    redmine_token: std::mem::take(&mut self.redmine_token),
                    make_active: self.make_active,
                };
                tokio::spawn(async move {
                    let _ = done.send(add_new(request).await);
                    ctx.request_repaint();
                });
            }
            Mode::Tokens { entry, gitlab, .. } => {
                if gitlab && self.token.trim().is_empty() {
                    self.errors =
                        vec![tl("Type the GitLab token: nothing can load without it.").to_string()];
                    return;
                }
                let token = std::mem::take(&mut self.token);
                let redmine_token = std::mem::take(&mut self.redmine_token);
                tokio::spawn(async move {
                    let _ = done.send(save_tokens(*entry, token, redmine_token).await);
                    ctx.request_repaint();
                });
            }
        }
        self.task = Some(rx);
    }
}

/// Looks up whether `url` has a token of `kind`, off the runtime (the keyring blocks).
async fn lookup(known: &Arc<Mutex<Known>>, kind: Kind, url: String, ctx: &egui::Context) {
    let probe = url.clone();
    let found = tokio::task::spawn_blocking(move || match kind {
        Kind::Gitlab => crate::storage::has_gitlab_token(&probe),
        #[cfg(feature = "redmine")]
        Kind::Redmine => gitlab_tracker_redmine::keyring::has_token(&probe),
        #[cfg(not(feature = "redmine"))]
        Kind::Redmine => false,
    })
    .await
    .unwrap_or(false);
    if let Ok(mut known) = known.lock() {
        known.pending.remove(&(kind, url.clone()));
        known.found.insert((kind, url), found);
    }
    ctx.request_repaint();
}

/// The URL as the keyring keys it: trimmed, without its trailing `/`.
fn normalize(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

#[cfg(feature = "redmine")]
fn is_redmine(provider: &str) -> bool {
    provider.eq_ignore_ascii_case("redmine")
}

/// A checked new project and its tokens.
struct NewProject {
    checked: Checked,
    token: Zeroizing<String>,
    redmine_url: String,
    redmine_token: Zeroizing<String>,
    make_active: bool,
}

/// The token typed, else the one stored for `url`.
async fn gitlab_token(url: &str, typed: Zeroizing<String>) -> Option<Zeroizing<String>> {
    if !typed.trim().is_empty() {
        return Some(Zeroizing::new(typed.trim().to_string()));
    }
    let url = url.to_string();
    tokio::task::spawn_blocking(move || crate::storage::stored_gitlab_token(&url))
        .await
        .ok()
        .flatten()
}

/// Checks the project and the tokens, saves the tokens typed to the keyring and
/// the project to `projects.toml`. The errors are the form's to show.
async fn add_new(request: NewProject) -> Result<ProjectEntry, Vec<String>> {
    let NewProject {
        checked,
        token,
        redmine_url,
        redmine_token,
        make_active,
    } = request;
    let typed = !token.trim().is_empty();
    let Some(token) = gitlab_token(&checked.url, token).await else {
        return Err(vec![tl(
            "No token is saved for this GitLab instance yet: type one.",
        )
        .to_string()]);
    };
    let info = crate::gitlab::check_project(&checked.url, &token, &checked.project_id)
        .await
        .map_err(|e| vec![check_error(&e)])?;

    #[cfg(feature = "redmine")]
    let tracker = if redmine_url.is_empty() {
        None
    } else {
        let url = redmine_url.clone();
        let typed_redmine = !redmine_token.trim().is_empty();
        let key = if typed_redmine {
            Some(Zeroizing::new(redmine_token.trim().to_string()))
        } else {
            let url = url.clone();
            tokio::task::spawn_blocking(move || {
                gitlab_tracker_redmine::keyring::has_token(&url)
                    .then(|| gitlab_tracker_redmine::get_or_prompt_token(&url))
                    .flatten()
            })
            .await
            .ok()
            .flatten()
        };
        let Some(key) = key else {
            return Err(vec![tl("No API key is saved for this Redmine instance yet: type one, or leave the Redmine URL empty.").to_string()]);
        };
        gitlab_tracker_redmine::check_token(&url, &key)
            .await
            .map_err(|e| vec![redmine_error(&e)])?;
        Some((url, typed_redmine.then_some(key)))
    };
    #[cfg(not(feature = "redmine"))]
    let tracker: Option<(String, Option<Zeroizing<String>>)> = {
        let _ = (redmine_url, redmine_token);
        None
    };

    // Saved once both are known good: a refused token never reaches the keyring.
    let url = checked.url.clone();
    let save = typed.then(|| token.clone());
    let redmine_save = tracker
        .as_ref()
        .and_then(|(u, k)| k.clone().map(|k| (u.clone(), k)));
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(token) = save {
            crate::storage::save_gitlab_token(&url, &token)?;
        }
        #[cfg(feature = "redmine")]
        if let Some((url, key)) = redmine_save {
            gitlab_tracker_redmine::keyring::save_token(&url, &key)?;
        }
        #[cfg(not(feature = "redmine"))]
        let _ = redmine_save;
        Ok(())
    })
    .await
    .map_err(|e| vec![e.to_string()])?
    .map_err(|e| {
        vec![tf(
            "The token could not be saved in the keyring: {error}",
            &[("error", &e)],
        )]
    })?;

    let mut entry = entry_for(checked, &info, tracker.map(|(url, _)| url));
    entry.gitlab_username = crate::gitlab::current_username(&entry.gitlab_url, &token).await;
    // Already configured (same instance and project): open it as it is.
    let projects = crate::storage::load_projects_toml().await.projects;
    if let Some(existing) = projects.into_iter().find(|p| {
        p.gitlab_url.trim_end_matches('/') == entry.gitlab_url
            && (p.project_id == entry.project_id
                || p.project_id == info.path_with_namespace
                || p.project_id == info.id.to_string())
    }) {
        return Ok(existing);
    }
    crate::storage::add_project(entry.clone(), make_active).await;
    Ok(entry)
}

/// Why GitLab refused, in the window's language.
fn check_error(e: &crate::gitlab::ProjectCheckError) -> String {
    use crate::gitlab::ProjectCheckError as E;
    match e {
        E::Token => tl("GitLab refused the token: it may be expired, or lack the read_api scope.")
            .to_string(),
        E::NotFound => {
            tl("No such project on this instance, or the token cannot see it.").to_string()
        }
        E::Status(status) => tf(
            "GitLab answered {status}.",
            &[("status", &status.to_string())],
        ),
        E::Network(why) => tf("Could not reach GitLab: {error}", &[("error", why)]),
    }
}

/// Why Redmine refused, in the window's language.
#[cfg(feature = "redmine")]
fn redmine_error(e: &gitlab_tracker_core::TrackerError) -> String {
    match e {
        gitlab_tracker_core::TrackerError::Auth(_) => {
            tl("Redmine refused the API key.").to_string()
        }
        other => tf("Redmine: {error}", &[("error", &other.to_string())]),
    }
}

/// The `projects.toml` entry of a checked project: GitLab's name and default
/// branch fill what was left empty.
fn entry_for(
    checked: Checked,
    info: &crate::gitlab::ProjectInfo,
    redmine: Option<String>,
) -> ProjectEntry {
    let branches = if checked.branches.is_empty() {
        vec![info.default_branch.clone().unwrap_or_else(|| "main".into())]
    } else {
        checked.branches
    };
    ProjectEntry {
        name: checked.name.or_else(|| Some(info.name.clone())),
        gitlab_url: checked.url,
        project_id: checked.project_id,
        tracked_branches: Some(branches),
        tracker: redmine.map(|url| crate::storage::TrackerConfig {
            provider: "redmine".into(),
            url,
            extra: toml::Table::new(),
        }),
        ..ProjectEntry::default()
    }
}

/// Checks and saves the tokens typed for `entry` (an empty Redmine key leaves the
/// tracker out for this session).
async fn save_tokens(
    entry: ProjectEntry,
    token: Zeroizing<String>,
    redmine_token: Zeroizing<String>,
) -> Result<ProjectEntry, Vec<String>> {
    if !token.trim().is_empty() {
        crate::gitlab::check_project(&entry.gitlab_url, token.trim(), &entry.project_id)
            .await
            .map_err(|e| vec![check_error(&e)])?;
        let (url, token) = (entry.gitlab_url.clone(), token.clone());
        tokio::task::spawn_blocking(move || crate::storage::save_gitlab_token(&url, &token))
            .await
            .map_err(|e| vec![e.to_string()])?
            .map_err(|e| {
                vec![tf(
                    "The token could not be saved in the keyring: {error}",
                    &[("error", &e)],
                )]
            })?;
    }
    #[cfg(feature = "redmine")]
    if let Some(tracker) = entry
        .tracker
        .as_ref()
        .filter(|_| !redmine_token.trim().is_empty())
    {
        gitlab_tracker_redmine::check_token(&tracker.url, redmine_token.trim())
            .await
            .map_err(|e| vec![redmine_error(&e)])?;
        let (url, key) = (tracker.url.clone(), redmine_token.clone());
        tokio::task::spawn_blocking(move || {
            gitlab_tracker_redmine::keyring::save_token(&url, &key)
        })
        .await
        .map_err(|e| vec![e.to_string()])?
        .map_err(|e| {
            vec![tf(
                "The token could not be saved in the keyring: {error}",
                &[("error", &e)],
            )]
        })?;
    }
    #[cfg(not(feature = "redmine"))]
    let _ = redmine_token;
    Ok(entry)
}

/// The tokens `project` lacks, as the form's [`Mode::Tokens`]; `None` when it has
/// them all. Blocks on the keyring: off the UI thread.
pub fn missing_tokens(project: &ProjectEntry) -> Option<Mode> {
    let gitlab = !crate::storage::has_gitlab_token(&project.gitlab_url);
    #[cfg(feature = "redmine")]
    let redmine = project
        .tracker
        .as_ref()
        .filter(|t| is_redmine(&t.provider) && !t.url.trim().is_empty())
        .is_some_and(|t| {
            std::env::var("REDMINE_URL").is_err()
                && !gitlab_tracker_redmine::keyring::has_token(&t.url)
        });
    #[cfg(not(feature = "redmine"))]
    let redmine = false;
    (gitlab || redmine).then(|| Mode::Tokens {
        entry: Box::new(project.clone()),
        gitlab,
        redmine,
    })
}

/// A checked form, ready to save.
#[derive(Debug, PartialEq)]
struct Checked {
    url: String,
    project_id: String,
    name: Option<String>,
    /// Empty: the project's default branch.
    branches: Vec<String>,
}

/// Checks the form: an http(s) URL without spaces (its trailing `/` dropped), a
/// project id or path without spaces, branch names without spaces, and the name
/// cleaned like the settings do.
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
    let project_id = project_id.trim().trim_matches('/').to_string();
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
                super::i18n::install();
                let language =
                    futures::executor::block_on(crate::storage::load_projects_toml()).language;
                super::i18n::set_language(language.as_deref());
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

struct Setup {
    theme: Theme,
    form: ProjectForm,
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if matches!(self.form.mode, Mode::New) {
                        ui.label(semibold(tl("Welcome to GitLab Tracker"), theme.text_size + 6.0));
                        ui.add_space(4.0);
                        ui.label(muted(
                            &theme,
                            tl("No project is configured yet. Tell it which GitLab project to follow; everything can be changed later in projects.toml."),
                        ));
                    } else {
                        ui.label(semibold(self.form.title(), theme.text_size + 6.0));
                    }
                    ui.add_space(18.0);
                    match self.form.show(ui, &theme) {
                        Outcome::Open => {}
                        Outcome::Cancelled => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        Outcome::Saved(_) => {
                            *self
                                .saved
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                });
            });
    }
}

/// A labelled field with its help below; `secret` masks what is typed.
fn field(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    help: &str,
    text: &mut String,
    secret: bool,
) -> egui::Response {
    ui.label(RichText::new(label).font(bevel_theme::fonts::medium(theme.text_size)));
    ui.add_space(3.0);
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .password(secret)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(8, 6)),
    );
    ui.add_space(2.0);
    ui.label(muted(theme, help).size(theme.text_size - 1.5));
    ui.add_space(10.0);
    response
}

/// A token field: when the instance already has one (`known`), says so and lets
/// it be left empty; `None` while that is looked up.
fn token_field(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    help: &str,
    text: &mut String,
    known: Option<bool>,
) {
    ui.label(RichText::new(label).font(bevel_theme::fonts::medium(theme.text_size)));
    ui.add_space(3.0);
    let hint = if known == Some(true) {
        tl("Saved — leave empty to keep it")
    } else {
        ""
    };
    ui.add(
        egui::TextEdit::singleline(text)
            .password(true)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(8, 6)),
    );
    ui.add_space(2.0);
    match known {
        Some(true) => {
            ui.label(
                RichText::new(format!(
                    "✔ {}",
                    tl("A token is already saved for this instance: no need to type it again.")
                ))
                .color(theme.tones.green.fg)
                .size(theme.text_size - 1.5),
            );
        }
        None => {
            ui.horizontal(|ui| {
                spinner(ui, 10.0, theme.palette.accent);
                ui.label(
                    muted(theme, tl("Looking for a saved token…")).size(theme.text_size - 1.5),
                );
            });
        }
        Some(false) => {
            ui.label(muted(theme, help).size(theme.text_size - 1.5));
        }
    }
    ui.add_space(10.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_form_is_cleaned() {
        let checked = check(
            " https://gitlab.example/ ",
            " group/app/ ",
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
        // No branch: the project's default branch, read from GitLab.
        assert!(check("https://g.io", "42", "", " ")
            .unwrap()
            .branches
            .is_empty());
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

    #[test]
    fn gitlab_fills_what_was_left_empty() {
        let info = crate::gitlab::ProjectInfo {
            id: 42,
            name: "App".into(),
            path_with_namespace: "group/app".into(),
            default_branch: Some("develop".into()),
        };
        let checked = check("https://g.io", "group/app", "", "").unwrap();
        let entry = entry_for(checked, &info, Some("https://redmine.io".into()));
        assert_eq!(entry.name.as_deref(), Some("App"));
        assert_eq!(entry.tracked_branches, Some(vec!["develop".to_string()]));
        assert_eq!(entry.tracker.unwrap().url, "https://redmine.io");
        assert!(!entry.active);
    }
}
