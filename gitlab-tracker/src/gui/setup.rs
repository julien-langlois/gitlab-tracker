//! The project form, in a window of its own or as a dialog of the main window:
//!
//! * **new project** — GitLab URL, project, name, branches, the GitLab token and,
//!   with Redmine, the tracker's URL and token, grouped in cards. A token already
//!   in the keyring (or the environment) for the same instance is said so, and
//!   needs not be typed again. **Check** asks GitLab (and Redmine) whether the
//!   project and the tokens work without saving anything, and shows what it found
//!   (GitLab's name and default branch then fill the empty fields' hints);
//!   **Add** checks again and saves;
//! * **tokens** — a project of `projects.toml` whose token is missing: asked here,
//!   since the window has no terminal to prompt in.
//!
//! Both are drawn in the dialogs' shell (header, body, footer): [`ProjectForm::body`]
//! and [`ProjectForm::footer`]. With no project configured, `gitlab-tracker gui`
//! opens the form in a window of its own ([`run`]); the main window opens it from
//! its `+` and when a project it loads has no token.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use bevel_theme::{Theme, Tone};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Stroke, Ui, Vec2};
use zeroize::Zeroizing;

use super::icons::Icon;
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

/// The field a problem is about: its border turns red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Url,
    Project,
    Branches,
    Token,
    #[cfg_attr(not(feature = "redmine"), allow(dead_code))]
    RedmineUrl,
    #[cfg_attr(not(feature = "redmine"), allow(dead_code))]
    RedmineToken,
}

/// Something wrong with the form. A mistake in a field (`remote` false) is said
/// under it; what GitLab or Redmine answered goes in the verdict's banner.
#[derive(Debug, Clone)]
struct Problem {
    slot: Option<Slot>,
    text: String,
    remote: bool,
}

impl Problem {
    fn local(slot: Option<Slot>, text: impl Into<String>) -> Self {
        Self {
            slot,
            text: text.into(),
            remote: false,
        }
    }

    fn remote(slot: Option<Slot>, text: impl Into<String>) -> Self {
        Self {
            slot,
            text: text.into(),
            remote: true,
        }
    }
}

/// What a successful check found.
#[derive(Debug, Clone)]
struct Report {
    name: String,
    path: String,
    default_branch: Option<String>,
    username: Option<String>,
    /// The token used was the one already saved, none was typed.
    saved_token: bool,
    /// Redmine's URL, its key accepted.
    redmine: Option<String>,
}

/// The fields as they were checked: when they change, the verdict and the
/// problems no longer hold and are dropped.
#[derive(PartialEq, Eq)]
struct Inputs {
    url: String,
    project_id: String,
    name: String,
    branches: String,
    token: Zeroizing<String>,
    redmine_url: String,
    redmine_token: Zeroizing<String>,
}

/// What runs in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Checking,
    Saving,
}

/// What a background run ended with.
enum Done {
    Checked(Result<Report, Vec<Problem>>),
    Saved(Result<Box<ProjectEntry>, Vec<Problem>>),
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
    problems: Vec<Problem>,
    /// The last check that went through.
    report: Option<Report>,
    /// The fields the problems and the report are about.
    checked: Option<Inputs>,
    known: Arc<Mutex<Known>>,
    /// The check (or the check and save) running in the background.
    task: Option<(Busy, Receiver<Done>)>,
    /// Saved as the `active` project (the first-run window).
    make_active: bool,
    /// The ticket tracker's card is unfolded: optional, folded until asked.
    #[cfg_attr(not(feature = "redmine"), allow(dead_code))]
    tracker_open: bool,
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
            problems: Vec::new(),
            report: None,
            checked: None,
            known: Arc::default(),
            task: None,
            make_active: false,
            tracker_open: false,
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

    /// The line under the title.
    pub fn subtitle(&self) -> &'static str {
        match self.mode {
            Mode::New => {
                tl("Nothing is saved until GitLab has accepted the project and the token.")
            }
            Mode::Tokens { .. } => tl("Saved in the OS keyring, never in a file."),
        }
    }

    /// The icon on the header's tile.
    pub fn icon(&self) -> Icon {
        match self.mode {
            Mode::New => Icon::Plus,
            Mode::Tokens { .. } => Icon::Shield,
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

    fn inputs(&self) -> Inputs {
        Inputs {
            url: self.url.clone(),
            project_id: self.project_id.clone(),
            name: self.name.clone(),
            branches: self.branches.clone(),
            token: self.token.clone(),
            redmine_url: self.redmine_url.clone(),
            redmine_token: self.redmine_token.clone(),
        }
    }

    /// The problem of `slot`, if any.
    fn problem(&self, slot: Slot) -> Option<&Problem> {
        self.problems.iter().find(|p| p.slot == Some(slot))
    }

    /// Takes what the background run ended with: the project to open once saved.
    pub fn poll(&mut self) -> Outcome {
        let Some(done) = self.task.as_ref().and_then(|(_, rx)| rx.try_recv().ok()) else {
            return Outcome::Open;
        };
        self.task = None;
        match done {
            Done::Saved(Ok(entry)) => return Outcome::Saved(entry),
            Done::Checked(Ok(report)) => self.report = Some(report),
            Done::Checked(Err(problems)) | Done::Saved(Err(problems)) => {
                self.problems = problems;
            }
        }
        Outcome::Open
    }

    /// Draws the fields (the verdict of the last check is [`Self::verdict`]'s,
    /// pinned above the footer so it shows whatever the scroll).
    pub fn body(&mut self, ui: &mut Ui, theme: &Theme) {
        // A field changed since the check: what it said no longer holds.
        if !self.is_busy() && self.checked.as_ref().is_some_and(|c| *c != self.inputs()) {
            self.checked = None;
            self.report = None;
            self.problems.clear();
        }
        let busy = self.is_busy();
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_enabled_ui(!busy, |ui| match self.mode.clone() {
            Mode::New => self.new_fields(ui, theme),
            Mode::Tokens {
                entry,
                gitlab,
                redmine,
            } => self.token_fields(ui, theme, &entry, gitlab, redmine),
        });
    }

    /// Draws the footer's line: what runs, then the buttons. Cancelled by its
    /// button; `esc` adds the Esc hint (the dialog closes on it).
    pub fn footer(&mut self, ui: &mut Ui, theme: &Theme, esc: bool) -> Outcome {
        let mut outcome = Outcome::Open;
        match self.task.as_ref().map(|(busy, _)| *busy) {
            Some(busy) => {
                spinner(ui, 12.0, theme.palette.accent);
                ui.label(muted(
                    theme,
                    match busy {
                        Busy::Checking => tl("Checking with GitLab…"),
                        Busy::Saving => tl("Checking and saving…"),
                    },
                ));
            }
            None if esc => {
                ui.label(muted(theme, tl("Esc to cancel")));
            }
            None => {}
        }
        let busy = self.is_busy();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_enabled_ui(!busy, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                match self.mode {
                    Mode::New => {
                        if button(ui, theme, Some(Icon::Plus), tl("Add"), true).clicked() {
                            self.submit(false);
                        }
                        let check = button(ui, theme, Some(Icon::Shield), tl("Check"), false);
                        if check.clicked() {
                            self.submit(true);
                        }
                    }
                    Mode::Tokens { .. } => {
                        if button(ui, theme, None, tl("Save and open"), true).clicked() {
                            self.submit(false);
                        }
                    }
                }
            });
            if button(ui, theme, None, tl("Cancel"), false).clicked() {
                outcome = Outcome::Cancelled;
            }
        });
        outcome
    }

    fn new_fields(&mut self, ui: &mut Ui, theme: &Theme) {
        let report = self.report.clone();
        group(ui, theme, tl("GitLab"), None, |ui| {
            let url = input(
                ui,
                theme,
                Input {
                    label: tl("GitLab URL"),
                    hint: "https://gitlab.com",
                    help: tl("https://gitlab.com or your own instance"),
                    mono: true,
                    ..Input::new(self.problem(Slot::Url))
                },
                &mut self.url,
            );
            ui.add_space(12.0);
            let known = self.token_known(Kind::Gitlab, &self.url, !url.has_focus());
            let problem = self.problem(Slot::Token).cloned();
            let mut token = std::mem::take(&mut self.token);
            token_input(
                ui,
                theme,
                Input {
                    label: tl("Personal access token"),
                    hint: if known == Some(true) {
                        tl("Saved — leave empty to keep it")
                    } else {
                        "glpat-…"
                    },
                    help: tl("Scope read_api; saved in the OS keyring, never in a file."),
                    ..Input::new(problem.as_ref())
                },
                &mut token,
                known,
            );
            self.token = token;
        });
        ui.add_space(16.0);
        group(ui, theme, tl("Project"), None, |ui| {
            input(
                ui,
                theme,
                Input {
                    label: tl("Project"),
                    hint: tl("group/project or 4217"),
                    help: tl("Its numeric id (Settings › General) or its path, group/project"),
                    mono: true,
                    ..Input::new(self.problem(Slot::Project))
                },
                &mut self.project_id,
            );
            ui.add_space(12.0);
            // GitLab's values once checked: what an empty field stands for.
            let name_hint = report
                .as_ref()
                .map_or(tl("GitLab's name"), |r| r.name.as_str())
                .to_string();
            let branch_hint = report
                .as_ref()
                .and_then(|r| r.default_branch.clone())
                .unwrap_or_else(|| tl("The default branch").to_string());
            let branches_problem = self.problem(Slot::Branches).cloned();
            ui.columns(2, |columns| {
                // Not justified: a wrapped help would be stretched to the edge.
                let top = Layout::top_down(Align::Min);
                let (left, right) = columns.split_at_mut(1);
                left[0].with_layout(top, |ui| {
                    input(
                        ui,
                        theme,
                        Input {
                            label: tl("Name"),
                            optional: true,
                            hint: &name_hint,
                            help: tl("Shown in the tab"),
                            ..Input::new(None)
                        },
                        &mut self.name,
                    );
                });
                right[0].with_layout(top, |ui| {
                    input(
                        ui,
                        theme,
                        Input {
                            label: tl("Tracked branches"),
                            optional: true,
                            hint: &branch_hint,
                            help: tl("Comma-separated, in the table's order"),
                            mono: true,
                            ..Input::new(branches_problem.as_ref())
                        },
                        &mut self.branches,
                    );
                });
            });
        });

        // The ticket tracker is optional (Redmine today, others later): folded
        // behind one row until asked for.
        #[cfg(feature = "redmine")]
        {
            ui.add_space(16.0);
            // Unfolded by a problem of its own: it must show.
            let open = self.tracker_open
                || self.problem(Slot::RedmineUrl).is_some()
                || self.problem(Slot::RedmineToken).is_some();
            if !open {
                // Folded with a URL typed: said on the row, never hidden away.
                let url = normalize(&self.redmine_url);
                let (title, detail, icon) = if url.is_empty() {
                    (
                        tl("Link a ticket tracker").to_string(),
                        tl("Optional · Redmine").to_string(),
                        Icon::Plus,
                    )
                } else {
                    (
                        tl("Ticket tracker").to_string(),
                        format!("Redmine · {url}"),
                        Icon::ArrowDown,
                    )
                };
                if fold_row(ui, theme, &title, &detail, icon) {
                    self.tracker_open = true;
                }
                return;
            }
            let mut fold = false;
            let mut remove = false;
            group_with(
                ui,
                theme,
                |ui| {
                    // The caption folds it back, the values kept; the cross
                    // removes the tracker.
                    fold = caption_toggle(ui, theme, tl("Ticket tracker"), tl("optional"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        remove = super::widgets::icon_button_tip(
                            ui,
                            tl("Remove the ticket tracker"),
                            Icon::Close,
                            &theme.icon_button(),
                            22.0,
                            super::tip::Tip::new().hint(tl(
                                "Remove the ticket tracker: the project is added without one.",
                            )),
                        )
                        .clicked();
                    });
                },
                |ui| {
                    let url = input(
                        ui,
                        theme,
                        Input {
                            label: tl("Redmine URL"),
                            hint: "https://redmine.example.com",
                            help: tl("Links the MRs to their tickets; empty: no tracker"),
                            mono: true,
                            ..Input::new(self.problem(Slot::RedmineUrl))
                        },
                        &mut self.redmine_url,
                    );
                    if !self.redmine_url.trim().is_empty() {
                        ui.add_space(12.0);
                        let known =
                            self.token_known(Kind::Redmine, &self.redmine_url, !url.has_focus());
                        let problem = self.problem(Slot::RedmineToken).cloned();
                        let mut token = std::mem::take(&mut self.redmine_token);
                        token_input(
                            ui,
                            theme,
                            Input {
                                label: tl("Redmine API key"),
                                hint: if known == Some(true) {
                                    tl("Saved — leave empty to keep it")
                                } else {
                                    ""
                                },
                                help: tl("My account › API access key; saved in the OS keyring."),
                                ..Input::new(problem.as_ref())
                            },
                            &mut token,
                            known,
                        );
                        self.redmine_token = token;
                    }
                },
            );
            if remove {
                self.redmine_url.clear();
                self.redmine_token = Zeroizing::default();
            }
            if fold || remove {
                self.tracker_open = false;
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
        ui.add(
            egui::Label::new(muted(
                theme,
                tf(
                    "{project} has no token in the OS keyring for the instances below. Type them once: they are saved in the keyring, and every project on the same instance uses them.",
                    &[("project", name)],
                ),
            ))
            .wrap(),
        );
        ui.add_space(14.0);
        if gitlab {
            group(ui, theme, tl("GitLab"), Some(&entry.gitlab_url), |ui| {
                let problem = self.problem(Slot::Token).cloned();
                let mut token = std::mem::take(&mut self.token);
                token_input(
                    ui,
                    theme,
                    Input {
                        label: tl("Personal access token"),
                        hint: "glpat-…",
                        help: tl("Personal access token, scope read_api."),
                        ..Input::new(problem.as_ref())
                    },
                    &mut token,
                    Some(false),
                );
                self.token = token;
            });
            ui.add_space(16.0);
        }
        if redmine {
            let url = entry
                .tracker
                .as_ref()
                .map(|t| t.url.as_str())
                .unwrap_or_default();
            group(ui, theme, tl("Redmine"), Some(url), |ui| {
                let problem = self.problem(Slot::RedmineToken).cloned();
                let mut token = std::mem::take(&mut self.redmine_token);
                token_input(
                    ui,
                    theme,
                    Input {
                        label: tl("Redmine API key"),
                        hint: "",
                        help: tl("Empty: the project opens without its tickets."),
                        ..Input::new(problem.as_ref())
                    },
                    &mut token,
                    Some(false),
                );
                self.redmine_token = token;
            });
        }
    }

    /// The banner above the footer: what the last check found, or why GitLab or
    /// Redmine refused. Nothing when there is nothing to say.
    pub fn has_verdict(&self) -> bool {
        self.report.is_some() || self.problems.iter().any(|p| p.remote || p.slot.is_none())
    }

    pub fn verdict(&self, ui: &mut Ui, theme: &Theme) {
        let refused: Vec<&Problem> = self
            .problems
            .iter()
            .filter(|p| p.remote || p.slot.is_none())
            .collect();
        if !refused.is_empty() {
            banner(ui, theme, theme.tones.red, "!", tl("Check failed"), |ui| {
                for problem in refused {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&problem.text).color(theme.palette.text_secondary),
                        )
                        .wrap(),
                    );
                }
            });
        } else if let Some(report) = &self.report {
            banner(
                ui,
                theme,
                theme.tones.green,
                "✔",
                tl("Connection works"),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(semibold(&report.name, theme.text_size).color(theme.palette.text));
                        ui.label(
                            RichText::new(&report.path)
                                .font(egui::FontId::monospace(theme.text_size - 1.5))
                                .color(theme.palette.text_muted),
                        );
                    });
                    let mut facts: Vec<String> = Vec::new();
                    if let Some(user) = &report.username {
                        facts.push(tf("Signed in as @{user}", &[("user", user)]));
                    }
                    if let Some(branch) = &report.default_branch {
                        facts.push(tf("default branch {branch}", &[("branch", branch)]));
                    }
                    if report.saved_token {
                        facts.push(tl("with the saved token").to_string());
                    }
                    if !facts.is_empty() {
                        ui.add(egui::Label::new(muted(theme, facts.join(" · "))).wrap());
                    }
                    if let Some(url) = &report.redmine {
                        ui.add(
                            egui::Label::new(muted(
                                theme,
                                tf("Redmine accepted the API key · {url}", &[("url", url)]),
                            ))
                            .wrap(),
                        );
                    }
                },
            );
        }
    }

    /// Checks the fields, then checks with GitLab in the background: only that
    /// when `dry_run`, else saving too.
    fn submit(&mut self, dry_run: bool) {
        self.problems.clear();
        self.report = None;
        self.checked = Some(self.inputs());
        let (done, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        match self.mode.clone() {
            Mode::New => {
                let checked = match check(&self.url, &self.project_id, &self.name, &self.branches) {
                    Ok(checked) => checked,
                    Err(errors) => {
                        self.problems = errors
                            .into_iter()
                            .map(|(slot, e)| Problem::local(Some(slot), tl(e)))
                            .collect();
                        return;
                    }
                };
                let redmine_url = normalize(&self.redmine_url);
                if !redmine_url.is_empty()
                    && !gitlab_tracker_core::secrets::is_secure_url(&redmine_url)
                {
                    self.problems = vec![Problem::local(Some(Slot::RedmineUrl), tl("The Redmine URL starts with https:// (http:// only for this machine): the token must not cross the network unencrypted."))];
                    return;
                }
                // The fields stay filled: Check, then Add, takes them again.
                let request = NewProject {
                    checked,
                    token: self.token.clone(),
                    redmine_url,
                    redmine_token: self.redmine_token.clone(),
                    make_active: self.make_active,
                };
                tokio::spawn(async move {
                    let result = if dry_run {
                        Done::Checked(check_new(request).await)
                    } else {
                        Done::Saved(add_new(request).await.map(Box::new))
                    };
                    let _ = done.send(result);
                    ctx.request_repaint();
                });
            }
            Mode::Tokens { entry, gitlab, .. } => {
                if gitlab && self.token.trim().is_empty() {
                    self.problems = vec![Problem::local(
                        Some(Slot::Token),
                        tl("Type the GitLab token: nothing can load without it."),
                    )];
                    return;
                }
                // A project saved with http:// before this check existed: refuse to
                // send it a token typed now, rather than let it cross in clear.
                let secure = gitlab_tracker_core::secrets::is_secure_url;
                if !self.token.trim().is_empty() && !secure(&entry.gitlab_url) {
                    self.problems = vec![Problem::local(
                        None,
                        tl("The GitLab URL starts with https:// (http:// only for this machine), without spaces."),
                    )];
                    return;
                }
                if !self.redmine_token.trim().is_empty()
                    && entry.tracker.as_ref().is_some_and(|t| !secure(&t.url))
                {
                    self.problems = vec![Problem::local(
                        None,
                        tl("The Redmine URL starts with https:// (http:// only for this machine): the token must not cross the network unencrypted."),
                    )];
                    return;
                }
                let token = self.token.clone();
                let redmine_token = self.redmine_token.clone();
                tokio::spawn(async move {
                    let _ = done.send(Done::Saved(
                        save_tokens(*entry, token, redmine_token)
                            .await
                            .map(Box::new),
                    ));
                    ctx.request_repaint();
                });
            }
        }
        let busy = if dry_run {
            Busy::Checking
        } else {
            Busy::Saving
        };
        self.task = Some((busy, rx));
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

/// A project and its tokens GitLab (and Redmine) accepted.
struct Verified {
    info: crate::gitlab::ProjectInfo,
    token: Zeroizing<String>,
    /// The token was typed (else it is the saved one).
    typed: bool,
    /// Redmine's URL, and its key when typed.
    tracker: Option<(String, Option<Zeroizing<String>>)>,
    username: Option<String>,
}

/// Asks GitLab whether the token sees the project, and Redmine whether it takes
/// the key; saves nothing. The problems are the form's to show.
async fn verify(
    checked: &Checked,
    token: Zeroizing<String>,
    redmine_url: String,
    redmine_token: Zeroizing<String>,
) -> Result<Verified, Vec<Problem>> {
    let typed = !token.trim().is_empty();
    let Some(token) = gitlab_token(&checked.url, token).await else {
        return Err(vec![Problem::remote(
            Some(Slot::Token),
            tl("No token is saved for this GitLab instance yet: type one."),
        )]);
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
            return Err(vec![Problem::remote(Some(Slot::RedmineToken), tl("No API key is saved for this Redmine instance yet: type one, or leave the Redmine URL empty."))]);
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
    let username = crate::gitlab::current_username(&checked.url, &token).await;
    Ok(Verified {
        info,
        token,
        typed,
        tracker,
        username,
    })
}

/// Checks the project and the tokens, without saving: what was found.
async fn check_new(request: NewProject) -> Result<Report, Vec<Problem>> {
    let NewProject {
        checked,
        token,
        redmine_url,
        redmine_token,
        ..
    } = request;
    let verified = verify(&checked, token, redmine_url, redmine_token).await?;
    Ok(Report {
        name: verified.info.name,
        path: verified.info.path_with_namespace,
        default_branch: verified.info.default_branch,
        username: verified.username,
        saved_token: !verified.typed,
        redmine: verified.tracker.map(|(url, _)| url),
    })
}

/// Checks the project and the tokens, saves the tokens typed to the keyring and
/// the project to `projects.toml`. The problems are the form's to show.
async fn add_new(request: NewProject) -> Result<ProjectEntry, Vec<Problem>> {
    let NewProject {
        checked,
        token,
        redmine_url,
        redmine_token,
        make_active,
    } = request;
    let Verified {
        info,
        token,
        typed,
        tracker,
        username,
    } = verify(&checked, token, redmine_url, redmine_token).await?;

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
    .map_err(|e| vec![Problem::remote(None, e.to_string())])?
    .map_err(|e| vec![keyring_error(&e)])?;

    let mut entry = entry_for(checked, &info, tracker.map(|(url, _)| url));
    entry.gitlab_username = username;
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

/// The keyring refused to save a token.
fn keyring_error(e: &str) -> Problem {
    Problem::remote(
        None,
        tf(
            "The token could not be saved in the keyring: {error}",
            &[("error", e)],
        ),
    )
}

/// Why GitLab refused, in the window's language, on the field it is about.
fn check_error(e: &crate::gitlab::ProjectCheckError) -> Problem {
    use crate::gitlab::ProjectCheckError as E;
    match e {
        E::Token => Problem::remote(
            Some(Slot::Token),
            tl("GitLab refused the token: it may be expired, or lack the read_api scope."),
        ),
        E::NotFound => Problem::remote(
            Some(Slot::Project),
            tl("No such project on this instance, or the token cannot see it."),
        ),
        E::Status(status) => Problem::remote(
            Some(Slot::Url),
            tf(
                "GitLab answered {status}.",
                &[("status", &status.to_string())],
            ),
        ),
        E::Network(why) => Problem::remote(
            Some(Slot::Url),
            tf("Could not reach GitLab: {error}", &[("error", why)]),
        ),
    }
}

/// Why Redmine refused, in the window's language.
#[cfg(feature = "redmine")]
fn redmine_error(e: &gitlab_tracker_core::TrackerError) -> Problem {
    match e {
        gitlab_tracker_core::TrackerError::Auth(_) => {
            Problem::remote(Some(Slot::RedmineToken), tl("Redmine refused the API key."))
        }
        other => Problem::remote(
            Some(Slot::RedmineUrl),
            tf("Redmine: {error}", &[("error", &other.to_string())]),
        ),
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
) -> Result<ProjectEntry, Vec<Problem>> {
    if !token.trim().is_empty() {
        crate::gitlab::check_project(&entry.gitlab_url, token.trim(), &entry.project_id)
            .await
            .map_err(|e| vec![check_error(&e)])?;
        let (url, token) = (entry.gitlab_url.clone(), token.clone());
        tokio::task::spawn_blocking(move || crate::storage::save_gitlab_token(&url, &token))
            .await
            .map_err(|e| vec![Problem::remote(None, e.to_string())])?
            .map_err(|e| vec![keyring_error(&e)])?;
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
        .map_err(|e| vec![Problem::remote(None, e.to_string())])?
        .map_err(|e| vec![keyring_error(&e)])?;
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

/// Checks the form: an https URL (http only to this machine) without spaces (its trailing `/` dropped), a
/// project id or path without spaces, branch names without spaces, and the name
/// cleaned like the settings do.
fn check(
    url: &str,
    project_id: &str,
    name: &str,
    branches: &str,
) -> Result<Checked, Vec<(Slot, &'static str)>> {
    let mut errors = Vec::new();
    let url = url.trim().trim_end_matches('/').to_string();
    let url_ok = gitlab_tracker_core::secrets::is_secure_url(&url)
        && url.len() > "https://".len()
        && !url.contains(char::is_whitespace);
    if !url_ok {
        errors.push((
            Slot::Url,
            "The GitLab URL starts with https:// (http:// only for this machine), without spaces.",
        ));
    }
    let project_id = project_id.trim().trim_matches('/').to_string();
    if project_id.is_empty() || project_id.contains(char::is_whitespace) {
        errors.push((
            Slot::Project,
            "The project is its numeric id or its path (group/project), without spaces.",
        ));
    }
    let mut list: Vec<String> = branches
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(str::to_string)
        .collect();
    if list.iter().any(|b| b.contains(char::is_whitespace)) {
        errors.push((
            Slot::Branches,
            "Branch names have no spaces; separate them with commas.",
        ));
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
                super::dialogs::footer(ui, &theme, |ui| {
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
                if super::dialogs::header(ui, &theme, self.form.icon(), title, Some(subtitle)) {
                    outcome = Outcome::Cancelled;
                }
                super::widgets::rule(ui, &theme);
                ui.spacing_mut().item_spacing.y = theme.spacing.item.y;
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| super::dialogs::body(ui, |ui| self.form.body(ui, &theme)));
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

/// A group of fields: its caption over a card (`aside` muted at the caption's
/// right end).
fn group(
    ui: &mut Ui,
    theme: &Theme,
    caption: &str,
    aside: Option<&str>,
    add: impl FnOnce(&mut Ui),
) {
    group_with(
        ui,
        theme,
        |ui| {
            ui.label(
                semibold(caption.to_uppercase(), theme.text_size - 2.5)
                    .color(theme.palette.text_muted),
            );
            if let Some(aside) = aside {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(muted(theme, aside).size(theme.text_size - 2.0))
                            .truncate(),
                    );
                });
            }
        },
        add,
    );
}

/// A group of fields whose caption line `caption` draws, over a card.
fn group_with(
    ui: &mut Ui,
    theme: &Theme,
    caption: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        caption(ui);
    });
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

/// A folded optional group: a dashed row with `icon` on a tile, `title` over
/// `detail`.
/// Whether it was clicked.
#[cfg(feature = "redmine")]
fn fold_row(ui: &mut Ui, theme: &Theme, title: &str, detail: &str, icon: Icon) -> bool {
    let size = Vec2::new(ui.available_width(), 52.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let radius = CornerRadius::same(theme.radii.large);
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, radius, theme.palette.panel);
    }
    // Dashed outline: optional, not there yet.
    let stroke = Stroke::new(
        1.0,
        if hovered {
            theme.palette.accent.gamma_multiply(0.6)
        } else {
            theme.palette.border
        },
    );
    let r = rect.shrink(0.5);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for pair in corners.windows(2) {
        ui.painter().extend(egui::Shape::dashed_line(
            &[pair[0], pair[1]],
            stroke,
            5.0,
            4.0,
        ));
    }
    let tile = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        Vec2::splat(24.0),
    );
    ui.painter().rect_filled(
        tile,
        CornerRadius::same(theme.radii.medium),
        theme.palette.accent.gamma_multiply(0.14),
    );
    super::icons::paint(ui.painter(), tile.shrink(5.0), icon, theme.palette.accent);
    let x = rect.left() + 50.0;
    ui.painter().text(
        egui::pos2(x, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        bevel_theme::fonts::medium(theme.text_size),
        if hovered {
            theme.palette.text
        } else {
            theme.palette.text_secondary
        },
    );
    ui.painter().text(
        egui::pos2(x, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
    );
    super::widgets::focus_ring(ui, theme, &response, theme.radii.large);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A group's caption that folds it: a chevron, the caption, `aside` muted; the
/// whole line lights up on hover. Whether it was clicked.
#[cfg(feature = "redmine")]
fn caption_toggle(ui: &mut Ui, theme: &Theme, caption: &str, aside: &str) -> bool {
    use super::tip::TipExt;
    // Lit from the last frame's hover: the line's size is known only once drawn.
    let id = ui.scope_id().with(("caption_toggle", caption));
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let tint = if hovered {
        theme.palette.text
    } else {
        theme.palette.text_muted
    };
    let rect = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (chevron, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
            super::icons::paint(ui.painter(), chevron, Icon::ArrowUp, tint);
            ui.label(semibold(caption.to_uppercase(), theme.text_size - 2.5).color(tint));
            ui.label(muted(theme, aside).size(theme.text_size - 2.0));
        })
        .response
        .rect
        .expand2(Vec2::new(4.0, 2.0));
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
    response.tip(tl("Fold")).clicked()
}

/// How an [`input`] looks.
struct Input<'a> {
    label: &'a str,
    /// Says "optional" after the label.
    optional: bool,
    /// Shown in the empty field.
    hint: &'a str,
    /// Under the field, when nothing is wrong.
    help: &'a str,
    mono: bool,
    /// The field's mistake, said in its help's place.
    error: Option<String>,
    /// Something is wrong with it: a red border.
    bad: bool,
}

impl Input<'_> {
    fn new(problem: Option<&Problem>) -> Self {
        Self {
            label: "",
            optional: false,
            hint: "",
            help: "",
            mono: false,
            error: problem.filter(|p| !p.remote).map(|p| p.text.clone()),
            bad: problem.is_some(),
        }
    }
}

/// A labelled field: the label (with "optional"), the field, then its help or
/// its mistake; `secret` masks what is typed.
fn input(ui: &mut Ui, theme: &Theme, look: Input<'_>, text: &mut String) -> egui::Response {
    let response = input_only(ui, theme, &look, text, false);
    below(ui, theme, &look, |ui| {
        if !look.help.is_empty() {
            ui.add(egui::Label::new(muted(theme, look.help).size(theme.text_size - 1.5)).wrap());
        }
    });
    response
}

/// A token field: when the instance already has one (`known`), says so and lets
/// it be left empty; `None` while that is looked up.
fn token_input(
    ui: &mut Ui,
    theme: &Theme,
    look: Input<'_>,
    text: &mut String,
    known: Option<bool>,
) {
    input_only(ui, theme, &look, text, true);
    below(ui, theme, &look, |ui| match known {
        Some(true) => {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "✔ {}",
                        tl("A token is already saved for this instance: no need to type it again.")
                    ))
                    .color(theme.tones.green.fg)
                    .size(theme.text_size - 1.5),
                )
                .wrap(),
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
            ui.add(egui::Label::new(muted(theme, look.help).size(theme.text_size - 1.5)).wrap());
        }
    });
}

/// The label and the field, drawn like the window's other inputs: the input
/// fill, a border turning to the accent when focused, red when wrong.
fn input_only(
    ui: &mut Ui,
    theme: &Theme,
    look: &Input<'_>,
    text: &mut String,
    secret: bool,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(look.label).font(bevel_theme::fonts::medium(theme.text_size)));
        if look.optional {
            ui.label(muted(theme, tl("optional")).size(theme.text_size - 2.0));
        }
    });
    ui.add_space(5.0);
    let height = theme.spacing.control_height + 6.0;
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let radius = CornerRadius::same(theme.radii.medium);
    ui.painter().rect_filled(rect, radius, theme.palette.input);
    let font = if look.mono {
        egui::FontId::monospace(theme.text_size - 0.5)
    } else {
        egui::FontId::proportional(theme.text_size)
    };
    let edit = egui::TextEdit::singleline(text)
        .id_salt(look.label)
        .frame(egui::Frame::NONE)
        .margin(Margin::ZERO)
        .password(secret)
        .font(font)
        .hint_text(RichText::new(look.hint).color(theme.palette.text_muted))
        .align(egui::Align2::LEFT_CENTER);
    let response = ui.put(rect.shrink2(Vec2::new(10.0, 0.0)), edit);
    let border = if look.bad {
        theme.tones.red.fg
    } else if response.has_focus() {
        theme.palette.accent
    } else {
        theme.palette.border
    };
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    response
}

/// Under a field: its mistake in red, else what `help` draws.
fn below(ui: &mut Ui, theme: &Theme, look: &Input<'_>, help: impl FnOnce(&mut Ui)) {
    ui.add_space(5.0);
    match &look.error {
        Some(error) => {
            ui.add(
                egui::Label::new(
                    RichText::new(error)
                        .color(theme.tones.red.fg)
                        .size(theme.text_size - 1.5),
                )
                .wrap(),
            );
        }
        None => help(ui),
    }
}

/// A banner in `tone`: a mark on a round tile, the title, then what `lines` adds.
fn banner(
    ui: &mut Ui,
    theme: &Theme,
    tone: Tone,
    mark: &str,
    title: &str,
    lines: impl FnOnce(&mut Ui),
) {
    egui::Frame::new()
        .fill(tone.bg)
        .stroke(Stroke::new(1.0, tone.fg.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(tile.center(), 13.0, tone.fg.gamma_multiply(0.2));
                ui.painter().text(
                    tile.center(),
                    egui::Align2::CENTER_CENTER,
                    mark,
                    bevel_theme::fonts::semibold(theme.text_size),
                    tone.fg,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(semibold(title, theme.text_size).color(tone.fg));
                    lines(ui);
                });
            });
        });
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
