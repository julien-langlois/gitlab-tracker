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

mod fields;
mod first_run;
mod form;
mod submit;
mod verify;

pub use first_run::*;
use form::*;
pub use verify::*;

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
}

#[cfg(test)]
mod tests;
