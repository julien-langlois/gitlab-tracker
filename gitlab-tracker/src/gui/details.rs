//! What the inspector reads from GitLab only when asked: an MR's changed files
//! with their patches, and the end of a failed job's log. Kept per tab for the
//! session, fetched in the background; the read-only demo shows canned ones.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver};

use bevel_theme::{fonts, Theme};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};

use super::tip::{Tip, TipExt};
use super::widgets::{button, mono, muted, spinner};
use crate::app::App;
use crate::gitlab::FileDiff;
use crate::i18n::{tl, tn};

mod changes_section;
mod file_tree;
mod patch_lines;
mod side_by_side;

pub use changes_section::*;
use file_tree::*;
pub use patch_lines::*;
use side_by_side::*;

/// Lines of a job's log kept.
const LOG_LINES: usize = 200;
/// Lines of a file's patch drawn; the rest is one click away in GitLab.
const PATCH_LINES: usize = 400;

/// Files as a tree (the conflicts', the changes'): sub-folders, then files, each sorted.
#[derive(Default)]
pub struct Folder<'a> {
    pub folders: std::collections::BTreeMap<&'a str, Folder<'a>>,
    /// Full paths.
    pub files: Vec<&'a str>,
}

impl<'a> Folder<'a> {
    pub fn of(paths: impl Iterator<Item = &'a str>) -> Self {
        let mut root = Folder::default();
        for path in paths {
            let mut folder = &mut root;
            let mut parts: Vec<&str> = path.split('/').collect();
            parts.pop();
            for part in parts {
                folder = folder.folders.entry(part).or_default();
            }
            folder.files.push(path);
        }
        root
    }
}

/// A fetch: running, done, or failed with its message.
pub enum Fetch<T> {
    Loading(Receiver<Result<T, String>>),
    Ready(T),
    Failed(String),
}

impl<T: Send + 'static> Fetch<T> {
    /// Runs `work` on the runtime; the window repaints once it is done.
    pub fn spawn(
        ctx: &egui::Context,
        work: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    ) -> Self {
        let (done, rx) = channel();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let _ = done.send(work.await);
            ctx.request_repaint();
        });
        Fetch::Loading(rx)
    }
}

impl<T> Fetch<T> {
    pub fn poll(&mut self) {
        if let Fetch::Loading(rx) = self {
            if let Ok(result) = rx.try_recv() {
                *self = match result {
                    Ok(value) => Fetch::Ready(value),
                    Err(e) => Fetch::Failed(e),
                };
            }
        }
    }
}

/// A tab's fetched details.
#[derive(Default)]
pub struct Details {
    /// By MR id.
    diffs: HashMap<String, Fetch<Vec<FileDiff>>>,
    /// By job id.
    logs: HashMap<u64, Fetch<String>>,
    /// What the MR page read, by MR id and `updated_at` (read again once the MR
    /// changes).
    pages: HashMap<(String, String), Fetch<crate::gitlab_detail::PageData>>,
    /// Files unfolded, by MR id and path.
    open_files: HashSet<(String, String)>,
    /// The file to bring into view once the Changes tab shows it.
    scroll_to: Option<(String, String)>,
    /// The file whose patch the MR page shows beside its tree, by MR id.
    selected: HashMap<String, String>,
    /// The MR page's patches with the old file beside the new one, not unified.
    side_by_side: bool,
    /// The project's deployments and branch pipelines, and when they were asked.
    project: Option<(
        std::time::Instant,
        Fetch<crate::gitlab_detail::ProjectOverview>,
    )>,
    /// A newer overview on its way, while the last one shows.
    project_next: Option<Receiver<Result<crate::gitlab_detail::ProjectOverview, String>>>,
    /// The merged MRs the reviewer suggestions read, and when they were asked.
    owners: Option<(std::time::Instant, Fetch<crate::owners::History>)>,
    /// A newer read on its way, while the last one serves.
    owners_next: Option<Receiver<Result<crate::owners::History, String>>>,
    /// The `CODEOWNERS` of each target branch, and when it was asked.
    declared: HashMap<String, (std::time::Instant, Fetch<crate::owners::Declared>)>,
    /// A newer read of a branch's `CODEOWNERS` on its way.
    declared_next: HashMap<String, Receiver<Result<crate::owners::Declared, String>>>,
}

impl Details {
    fn fetch<T: Send + 'static>(
        ctx: &egui::Context,
        work: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    ) -> Fetch<T> {
        Fetch::spawn(ctx, work)
    }

    /// The MR page's data for `mr`: read once per version of the MR, in the
    /// background (`None` while it loads). Its diff is read along, for the threads'
    /// code and the Changes tab.
    pub fn page(
        &mut self,
        app: &App,
        mr: &crate::models::TrackedMr,
        ctx: &egui::Context,
    ) -> Option<&crate::gitlab_detail::PageData> {
        if !self.diffs.contains_key(&mr.id) {
            self.load_diff(app, &mr.id, ctx);
        }
        self.page_data(app, mr, ctx)
    }

    /// [`Details::page`] without the diff: what a popup of the list needs (the
    /// commits an MR lacks), shared with the page once it opens.
    pub fn page_data(
        &mut self,
        app: &App,
        mr: &crate::models::TrackedMr,
        ctx: &egui::Context,
    ) -> Option<&crate::gitlab_detail::PageData> {
        let key = (mr.id.clone(), mr.updated_at.clone().unwrap_or_default());
        if !self.pages.contains_key(&key) {
            self.pages.retain(|(id, _), _| id != &mr.id);
            let fetch = if app.read_only {
                Fetch::Ready(crate::demo::demo_page_data(mr))
            } else {
                let target = crate::gitlab_detail::Target {
                    base_url: app.base_url.clone(),
                    token: app.token.clone(),
                    project_id: app.project_id.clone(),
                    mr_id: mr.id.clone(),
                    source_branch: mr.source_branch.clone(),
                    target_branch: mr.target_branch.clone(),
                    sha: mr.sha.clone(),
                };
                let branches = app.branches.clone();
                Self::fetch(ctx, async move {
                    Ok(crate::gitlab_detail::fetch(target, branches).await)
                })
            };
            self.pages.insert(key.clone(), fetch);
        }
        let fetch = self.pages.get_mut(&key)?;
        fetch.poll();
        match fetch {
            Fetch::Ready(data) => Some(data),
            _ => None,
        }
    }

    /// The project's deployments and the latest pipeline of each tracked
    /// branch: read in the background when first asked, again once older than
    /// the refresh interval; the last result shows meanwhile. `None` until the
    /// first read is done.
    pub fn project(
        &mut self,
        app: &App,
        ctx: &egui::Context,
    ) -> Option<&crate::gitlab_detail::ProjectOverview> {
        let max_age = std::time::Duration::from_secs(app.refresh_interval_secs.max(60));
        let stale = match &self.project {
            None => true,
            Some((at, Fetch::Ready(_) | Fetch::Failed(_))) => at.elapsed() >= max_age,
            Some((_, Fetch::Loading(_))) => false,
        };
        if stale {
            let fetch = if app.read_only {
                Fetch::Ready(crate::demo::demo_project_overview(app))
            } else {
                let target = crate::gitlab_detail::ProjectTarget {
                    base_url: app.base_url.clone(),
                    token: app.token.clone(),
                    project_id: app.project_id.clone(),
                };
                let branches = app.branches.clone();
                // The merges already known: only what changed since is asked for.
                let merges = self
                    .project_ready()
                    .map(|o| o.merges.clone())
                    .unwrap_or_default();
                let cache = crate::storage::project_store(&app.base_url, &app.project_id);
                Self::fetch(ctx, async move {
                    Ok(crate::gitlab_detail::fetch_project(target, branches, merges, cache).await)
                })
            };
            // Keep showing the previous overview while the new one loads.
            match (&mut self.project, fetch) {
                (Some((at, Fetch::Ready(_))), Fetch::Loading(rx)) => {
                    *at = std::time::Instant::now();
                    self.project_next = Some(rx);
                }
                (slot, fetch) => *slot = Some((std::time::Instant::now(), fetch)),
            }
        }
        if let Some(rx) = &self.project_next {
            if let Ok(result) = rx.try_recv() {
                self.project_next = None;
                if let Ok(value) = result {
                    self.project = Some((std::time::Instant::now(), Fetch::Ready(value)));
                }
            }
        }
        let (_, fetch) = self.project.as_mut()?;
        fetch.poll();
        match fetch {
            Fetch::Ready(data) => Some(data),
            _ => None,
        }
    }

    /// The project's overview as last read, without asking for a newer one.
    pub fn project_ready(&self) -> Option<&crate::gitlab_detail::ProjectOverview> {
        match self.project.as_ref()? {
            (_, Fetch::Ready(data)) => Some(data),
            _ => None,
        }
    }

    /// The project's merged MRs of the period, for the reviewer suggestions:
    /// read in the background, again once older than the refresh interval
    /// (only what merged since), the last read serving meanwhile.
    pub fn owner_history(
        &mut self,
        app: &App,
        ctx: &egui::Context,
    ) -> Option<&crate::owners::History> {
        let max_age = std::time::Duration::from_secs(app.refresh_interval_secs.max(60));
        let stale = match &self.owners {
            None => true,
            Some((at, Fetch::Ready(_) | Fetch::Failed(_))) => at.elapsed() >= max_age,
            Some((_, Fetch::Loading(_))) => false,
        };
        if stale {
            let fetch = if app.read_only {
                Fetch::Ready(crate::demo::demo_owner_history(app))
            } else {
                let target = crate::gitlab_detail::ProjectTarget {
                    base_url: app.base_url.clone(),
                    token: app.token.clone(),
                    project_id: app.project_id.clone(),
                };
                let known = match &self.owners {
                    Some((_, Fetch::Ready(h))) => h.clone(),
                    _ => crate::owners::History::default(),
                };
                let cache = crate::storage::project_store(&app.base_url, &app.project_id);
                Self::fetch(ctx, async move {
                    Ok(crate::owners::read_history(target, known, cache).await)
                })
            };
            match (&mut self.owners, fetch) {
                (Some((at, Fetch::Ready(_))), Fetch::Loading(rx)) => {
                    *at = std::time::Instant::now();
                    self.owners_next = Some(rx);
                }
                (slot, fetch) => *slot = Some((std::time::Instant::now(), fetch)),
            }
        }
        if let Some(rx) = &self.owners_next {
            if let Ok(result) = rx.try_recv() {
                self.owners_next = None;
                if let Ok(value) = result {
                    self.owners = Some((std::time::Instant::now(), Fetch::Ready(value)));
                }
            }
        }
        let (_, fetch) = self.owners.as_mut()?;
        fetch.poll();
        match fetch {
            Fetch::Ready(data) => Some(data),
            _ => None,
        }
    }

    /// The `CODEOWNERS` GitLab uses for MRs into `branch`, read in the
    /// background and again after an hour. `Err` when it could not be read.
    pub fn declared(
        &mut self,
        app: &App,
        branch: &str,
        ctx: &egui::Context,
    ) -> Option<Result<&crate::owners::Declared, &str>> {
        const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(3600);
        let stale = match self.declared.get(branch) {
            None => true,
            Some((at, Fetch::Ready(_) | Fetch::Failed(_))) => at.elapsed() >= MAX_AGE,
            Some((_, Fetch::Loading(_))) => false,
        };
        if stale {
            let fetch = if app.read_only {
                Fetch::Ready(crate::demo::demo_codeowners(app))
            } else {
                let target = crate::gitlab_detail::ProjectTarget {
                    base_url: app.base_url.clone(),
                    token: app.token.clone(),
                    project_id: app.project_id.clone(),
                };
                let branch = branch.to_string();
                Self::fetch(ctx, async move {
                    crate::owners::read_declared(&target, &branch).await
                })
            };
            // A file read before keeps serving while it is read again.
            match (self.declared.get_mut(branch), fetch) {
                (Some((at, Fetch::Ready(_))), Fetch::Loading(rx)) => {
                    *at = std::time::Instant::now();
                    self.declared_next.insert(branch.to_string(), rx);
                }
                (_, fetch) => {
                    self.declared
                        .insert(branch.to_string(), (std::time::Instant::now(), fetch));
                }
            }
        }
        if let Some(rx) = self.declared_next.get(branch) {
            if let Ok(result) = rx.try_recv() {
                self.declared_next.remove(branch);
                if let Ok(value) = result {
                    self.declared.insert(
                        branch.to_string(),
                        (std::time::Instant::now(), Fetch::Ready(value)),
                    );
                }
            }
        }
        let (_, fetch) = self.declared.get_mut(branch)?;
        fetch.poll();
        match fetch {
            Fetch::Ready(data) => Some(Ok(data)),
            Fetch::Failed(e) => Some(Err(e.as_str())),
            Fetch::Loading(_) => None,
        }
    }

    /// Unfolds the first file of the MR's diff when none is (the MR page shows
    /// a patch at once).
    pub fn open_first_file(&mut self, mr_id: &str) {
        if self.open_files.iter().any(|(id, _)| id == mr_id) {
            return;
        }
        let first = self
            .diff(mr_id)
            .and_then(|files| files.first())
            .map(|f| f.new_path.clone());
        if let Some(path) = first {
            self.open_files.insert((mr_id.to_string(), path));
        }
    }

    /// Unfolds `path` in the MR's Changes tab and scrolls to it.
    pub fn open_file(&mut self, mr_id: &str, path: &str) {
        let key = (mr_id.to_string(), path.to_string());
        self.open_files.insert(key.clone());
        self.selected.insert(mr_id.to_string(), path.to_string());
        self.scroll_to = Some(key);
    }

    /// The MR's diff once read.
    pub fn diff(&mut self, mr_id: &str) -> Option<&[FileDiff]> {
        let fetch = self.diffs.get_mut(mr_id)?;
        fetch.poll();
        match fetch {
            Fetch::Ready(files) => Some(files),
            _ => None,
        }
    }

    fn load_diff(&mut self, app: &App, mr_id: &str, ctx: &egui::Context) {
        let fetch = if app.read_only {
            Fetch::Ready(crate::demo::demo_file_diffs(app, mr_id))
        } else {
            let (url, token, project, mr) = (
                app.base_url.clone(),
                app.token.clone(),
                app.project_id.clone(),
                mr_id.to_string(),
            );
            Self::fetch(ctx, async move {
                crate::gitlab::fetch_file_diffs(&url, &token, &project, &mr).await
            })
        };
        self.diffs.insert(mr_id.to_string(), fetch);
    }

    fn load_log(&mut self, app: &App, job_id: u64, ctx: &egui::Context) {
        let fetch = if app.read_only {
            Fetch::Ready(crate::demo::demo_job_log(app, job_id))
        } else {
            let (url, token, project) = (
                app.base_url.clone(),
                app.token.clone(),
                app.project_id.clone(),
            );
            Self::fetch(ctx, async move {
                crate::gitlab::fetch_job_log(&url, &token, &project, job_id, LOG_LINES).await
            })
        };
        self.logs.insert(job_id, fetch);
    }
}

/// A job's log as [`Details::job_log`] gives it.
pub enum LogView<'a> {
    Loading,
    Ready(&'a str),
    Failed(&'a str),
}

impl Details {
    /// Reads the job's log once (again after a failure, at most once a frame).
    pub fn ensure_log(&mut self, app: &App, job_id: u64, ctx: &egui::Context) {
        if !self.logs.contains_key(&job_id) {
            self.load_log(app, job_id, ctx);
        }
    }

    /// The job's log, once [`Details::ensure_log`] asked for it.
    pub fn job_log(&mut self, job_id: u64) -> LogView<'_> {
        let Some(fetch) = self.logs.get_mut(&job_id) else {
            return LogView::Loading;
        };
        fetch.poll();
        match fetch {
            Fetch::Loading(_) => LogView::Loading,
            Fetch::Ready(text) => LogView::Ready(text),
            Fetch::Failed(error) => LogView::Failed(error),
        }
    }
}

impl Details {
    /// Development aid (debug builds): the demo MR's diff, unfolded, and the
    /// log of its last pipeline's first job.
    #[cfg(debug_assertions)]
    pub fn show_demo(&mut self, app: &App, mr_id: &str) {
        let files = crate::demo::demo_file_diffs(app, mr_id);
        if let Some(first) = files.first() {
            self.open_files
                .insert((mr_id.to_string(), first.new_path.clone()));
        }
        self.diffs.insert(mr_id.to_string(), Fetch::Ready(files));
        let failed = app
            .mrs
            .iter()
            .find(|mr| mr.id == mr_id)
            .and_then(|mr| mr.pipelines.first())
            .and_then(|p| p.jobs.iter().find(|j| j.status == "failed"))
            .map(|j| j.id);
        if let Some(job) = failed {
            self.logs
                .insert(job, Fetch::Ready(crate::demo::demo_job_log(app, job)));
        }
    }
}

#[cfg(test)]
mod tests;
