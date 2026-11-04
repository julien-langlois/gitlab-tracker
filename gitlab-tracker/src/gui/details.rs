//! What the inspector reads from GitLab only when asked: an MR's changed files
//! with their patches, and the end of a failed job's log. Kept per tab for the
//! session, fetched in the background; the read-only demo shows canned ones.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver};

use bevel_theme::{fonts, Theme};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};

use super::widgets::{button, mono, muted, spinner};
use crate::app::App;
use crate::gitlab::FileDiff;
use crate::i18n::{tf, tl, tn};

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

impl<T> Fetch<T> {
    fn poll(&mut self) {
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
    /// Files unfolded, by MR id and path; logs shown, by job id.
    open_files: HashSet<(String, String)>,
    open_logs: HashSet<u64>,
    /// The file to bring into view once the Changes tab shows it.
    scroll_to: Option<(String, String)>,
}

impl Details {
    fn fetch<T: Send + 'static>(
        ctx: &egui::Context,
        work: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    ) -> Fetch<T> {
        let (done, rx) = channel();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let _ = done.send(work.await);
            ctx.request_repaint();
        });
        Fetch::Loading(rx)
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
            if !self.diffs.contains_key(&mr.id) {
                self.load_diff(app, &mr.id, ctx);
            }
        }
        let fetch = self.pages.get_mut(&key)?;
        fetch.poll();
        match fetch {
            Fetch::Ready(data) => Some(data),
            _ => None,
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
            Fetch::Ready(crate::demo::demo_file_diffs())
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
            Fetch::Ready(crate::demo::demo_job_log())
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
    /// Development aid (debug builds): the demo's diff and log, unfolded.
    #[cfg(debug_assertions)]
    pub fn show_demo(&mut self, mr_id: &str) {
        let files = crate::demo::demo_file_diffs();
        if let Some(first) = files.first() {
            self.open_files
                .insert((mr_id.to_string(), first.new_path.clone()));
        }
        self.diffs.insert(mr_id.to_string(), Fetch::Ready(files));
        self.logs
            .insert(0, Fetch::Ready(crate::demo::demo_job_log()));
        self.open_logs.insert(0);
    }
}

/// The "Changes" section's body: a button until loaded, then the files (as a
/// tree of folders when `tree`, as in GitLab), each unfolding to its patch.
pub fn changes(
    details: &mut Details,
    app: &App,
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    web_url: &str,
    tree: bool,
) {
    if let Some(fetch) = details.diffs.get_mut(mr_id) {
        fetch.poll();
    }
    match details.diffs.get(mr_id) {
        None => {
            if button(ui, theme, None, tl("Show the changed files"), false).clicked() {
                details.load_diff(app, mr_id, ui.ctx());
            }
        }
        Some(Fetch::Loading(_)) => {
            ui.horizontal(|ui| {
                spinner(ui, 12.0, theme.palette.accent);
                ui.label(muted(theme, tl("Reading the diff…")));
            });
        }
        Some(Fetch::Failed(error)) => {
            ui.label(RichText::new(format!("⚠ {error}")).color(theme.tones.red.fg));
            if button(ui, theme, None, tl("Retry"), false).clicked() {
                details.load_diff(app, mr_id, ui.ctx());
            }
        }
        Some(Fetch::Ready(files)) => {
            if files.is_empty() {
                ui.label(muted(theme, tl("No file changed.")));
                return;
            }
            let mut toggle = None;
            let mut target = None;
            let scroll_to = details
                .scroll_to
                .take_if(|(id, _)| id == mr_id)
                .map(|(_, path)| path);
            let mut list = FileList {
                mr_id,
                web_url,
                open_files: &details.open_files,
                scroll_to: scroll_to.as_deref(),
                toggle: &mut toggle,
                target: &mut target,
            };
            if tree {
                let by_path: HashMap<&str, &FileDiff> =
                    files.iter().map(|f| (f.new_path.as_str(), f)).collect();
                let root = Folder::of(files.iter().map(|f| f.new_path.as_str()));
                ui.spacing_mut().item_spacing.y = 2.0;
                list.folder(ui, theme, &root, &by_path, "", 0);
            } else {
                for file in files {
                    list.file(ui, theme, file, 0, false);
                }
            }
            // The file's row at the top of the page (as far as the page goes).
            if let Some(row) = target {
                ui.scroll_to_rect(row, Some(Align::TOP));
            }
            if let Some(key) = toggle {
                if !details.open_files.remove(&key) {
                    details.open_files.insert(key);
                }
            }
        }
    }
}

/// Indentation of one level of the Changes tree.
const TREE_INDENT: f32 = 18.0;

/// What drawing the Changes files needs and gives back: the files unfolded, the
/// one to scroll to; the one clicked and where the scrolled-to one is.
struct FileList<'a> {
    mr_id: &'a str,
    web_url: &'a str,
    open_files: &'a HashSet<(String, String)>,
    scroll_to: Option<&'a str>,
    toggle: &'a mut Option<(String, String)>,
    target: &'a mut Option<egui::Rect>,
}

impl FileList<'_> {
    /// A file's row, `depth` deep, and its patch when unfolded.
    fn file(&mut self, ui: &mut Ui, theme: &Theme, file: &FileDiff, depth: usize, name_only: bool) {
        let key = (self.mr_id.to_string(), file.new_path.clone());
        let open = self.open_files.contains(&key);
        let top = ui.cursor().min;
        if file_row(ui, theme, file, open, depth, name_only) {
            *self.toggle = Some(key);
        }
        if self.scroll_to == Some(file.new_path.as_str()) {
            *self.target = Some(egui::Rect::from_min_max(
                top,
                egui::pos2(ui.max_rect().right(), ui.cursor().min.y),
            ));
        }
        if open {
            let indent = depth as f32 * TREE_INDENT;
            let top = ui.cursor().min.y;
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                ui.vertical(|ui| patch(ui, theme, &file.diff, self.web_url));
            });
            guides(ui, theme, depth, top, ui.cursor().min.y);
        }
    }

    /// The rows of `folder` (at `path`, `depth` deep): each sub-folder (a chain
    /// of folders holding only one folder on one row, as `src/api/graphql`),
    /// foldable, its content under it; then its files.
    fn folder(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        folder: &Folder,
        files: &HashMap<&str, &FileDiff>,
        path: &str,
        depth: usize,
    ) {
        for (name, mut sub) in &folder.folders {
            let mut label = (*name).to_string();
            while sub.files.is_empty() && sub.folders.len() == 1 {
                let (next, deeper) = sub.folders.iter().next().expect("one folder");
                label = format!("{label}/{next}");
                sub = deeper;
            }
            let full = format!("{path}{label}/");
            let id = egui::Id::unique(("changes folder", self.mr_id, &full));
            let folded = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
            if folder_row(ui, theme, &label, folded, depth, sub, files) {
                ui.data_mut(|d| d.insert_temp(id, !folded));
            }
            if !folded {
                self.folder(ui, theme, sub, files, &full, depth + 1);
            }
        }
        for path in &folder.files {
            if let Some(file) = files.get(path) {
                self.file(ui, theme, file, depth, true);
            }
        }
    }
}

/// The tree's guides, one per level above `depth`, from `top` to `bottom`.
fn guides(ui: &Ui, theme: &Theme, depth: usize, top: f32, bottom: f32) {
    let left = ui.max_rect().left();
    for level in 0..depth {
        let x = left + level as f32 * TREE_INDENT + 10.0;
        ui.painter().vline(
            x,
            egui::Rangef::new(top, bottom),
            egui::Stroke::new(1.0, theme.palette.text_muted.gamma_multiply(0.45)),
        );
    }
}

/// A folder of the tree: its chevron, its name, what changed under it; `true`
/// when clicked.
fn folder_row(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    folded: bool,
    depth: usize,
    folder: &Folder,
    files: &HashMap<&str, &FileDiff>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
    }
    guides(ui, theme, depth, rect.top() - 1.0, rect.bottom() + 1.0);
    let indent = depth as f32 * TREE_INDENT;
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(
                rect.shrink2(Vec2::new(4.0, 0.0))
                    .with_min_x(rect.left() + 4.0 + indent),
            )
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 6.0;
    row.label(muted(theme, if folded { "▸" } else { "▾" }));
    let (inside, added, deleted) = folder_totals(folder, files);
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if folded {
            ui.label(mono(format!("−{deleted}"), theme.text_size - 2.0).color(theme.tones.red.fg));
            ui.label(mono(format!("+{added}"), theme.text_size - 2.0).color(theme.tones.green.fg));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{label}/"))
                        .font(fonts::medium(theme.text_size - 1.0))
                        .color(theme.palette.text_secondary),
                )
                .truncate(),
            );
            if folded {
                ui.label(muted(theme, tn(inside as u64, "{n} file", "{n} files")));
            }
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl(if folded { "Unfold" } else { "Fold" }))
        .clicked()
}

/// Files under `folder`, at any depth, and the lines they add and remove.
fn folder_totals(folder: &Folder, files: &HashMap<&str, &FileDiff>) -> (usize, usize, usize) {
    let mut totals = (0, 0, 0);
    for path in &folder.files {
        if let Some(file) = files.get(path) {
            let (a, d) = count(&file.diff);
            totals = (totals.0 + 1, totals.1 + a, totals.2 + d);
        }
    }
    for sub in folder.folders.values() {
        let (n, a, d) = folder_totals(sub, files);
        totals = (totals.0 + n, totals.1 + a, totals.2 + d);
    }
    totals
}

/// A file: its state, its path (its name alone in the tree) and its counts;
/// `true` when clicked.
fn file_row(
    ui: &mut Ui,
    theme: &Theme,
    file: &FileDiff,
    open: bool,
    depth: usize,
    name_only: bool,
) -> bool {
    let (added, deleted) = count(&file.diff);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
    }
    guides(ui, theme, depth, rect.top() - 1.0, rect.bottom() + 1.0);
    let indent = depth as f32 * TREE_INDENT;
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(
                rect.shrink2(Vec2::new(4.0, 0.0))
                    .with_min_x(rect.left() + 4.0 + indent),
            )
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 6.0;
    row.label(muted(theme, if open { "▾" } else { "▸" }));
    let (mark, color) = if file.new_file {
        ("A", theme.tones.green.fg)
    } else if file.deleted_file {
        ("D", theme.tones.red.fg)
    } else if file.renamed_file {
        ("R", theme.tones.blue.fg)
    } else {
        ("M", theme.tones.amber.fg)
    };
    row.label(mono(mark, theme.text_size - 1.5).color(color));
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(mono(format!("−{deleted}"), theme.text_size - 2.0).color(theme.tones.red.fg));
        ui.label(mono(format!("+{added}"), theme.text_size - 2.0).color(theme.tones.green.fg));
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let path = if file.renamed_file {
                format!("{} → {}", file.old_path, file.new_path)
            } else {
                file.new_path.clone()
            };
            // The file name stands out from its folder (given by the tree).
            let (dir, name) = match path.rsplit_once('/') {
                Some(_) if name_only => {
                    let name = file.new_path.rsplit('/').next().unwrap_or(&file.new_path);
                    (String::new(), name.to_string())
                }
                Some((dir, name)) => (format!("{dir}/"), name.to_string()),
                None => (String::new(), path.clone()),
            };
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &dir,
                0.0,
                egui::TextFormat::simple(
                    egui::FontId::proportional(theme.text_size - 1.5),
                    theme.palette.text_secondary,
                ),
            );
            job.append(
                &name,
                0.0,
                egui::TextFormat::simple(fonts::medium(theme.text_size - 1.0), theme.palette.text),
            );
            ui.add(egui::Label::new(job).truncate()).on_hover_text(path);
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A patch, its lines coloured as in GitLab.
fn patch(ui: &mut Ui, theme: &Theme, diff: &str, web_url: &str) {
    if diff.trim().is_empty() {
        ui.label(muted(
            theme,
            tl("No text diff: a binary file, or too large for GitLab to show."),
        ));
        return;
    }
    let size = theme.text_size - 2.5;
    let green = theme.tones.green;
    let red = theme.tones.red;
    egui::Frame::new()
        .fill(theme.palette.chrome)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(diff.len())
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for line in diff.lines().take(PATCH_LINES) {
                        let (fill, color) = match line.chars().next() {
                            Some('+') => (green.bg, green.fg),
                            Some('-') => (red.bg, red.fg),
                            Some('@') => (Color32::TRANSPARENT, theme.tones.blue.fg),
                            _ => (Color32::TRANSPARENT, theme.palette.text_secondary),
                        };
                        egui::Frame::new()
                            .fill(fill)
                            .inner_margin(Margin::symmetric(8, 0))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        mono(line.replace('\t', "    "), size).color(color),
                                    )
                                    .extend(),
                                );
                            });
                    }
                });
        });
    let total = diff.lines().count();
    if total > PATCH_LINES {
        ui.horizontal(|ui| {
            ui.label(muted(
                theme,
                tn(
                    (total - PATCH_LINES) as u64,
                    "{n} more line",
                    "{n} more lines",
                ),
            ));
            ui.hyperlink_to(tl("in GitLab"), format!("{web_url}/diffs"));
        });
    }
    ui.add_space(4.0);
}

/// The lines of `diff` up to the new file's line `line`, at most `before` of them
/// before it: the code a thread on that line is about. Empty when the line is not
/// in the patch.
pub fn snippet(diff: &str, line: u32, before: usize) -> Vec<&str> {
    let mut numbered: Vec<(Option<u32>, &str)> = Vec::new();
    let mut new = 0u32;
    for text in diff.lines() {
        if let Some(rest) = text.strip_prefix("@@") {
            // `@@ -a,b +c,d @@`: the hunk's new lines start at c.
            new = rest
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            numbered.push((None, text));
        } else if text.starts_with('-') {
            numbered.push((None, text));
        } else {
            numbered.push((Some(new), text));
            new += 1;
        }
    }
    let Some(at) = numbered.iter().position(|(n, _)| *n == Some(line)) else {
        return Vec::new();
    };
    numbered[at.saturating_sub(before)..=at]
        .iter()
        .map(|(_, text)| *text)
        .collect()
}

/// Code lines coloured as in GitLab, the last one marked when `mark_last`.
pub fn code(ui: &mut Ui, theme: &Theme, lines: &[&str], mark_last: bool) {
    let size = theme.text_size - 2.5;
    egui::Frame::new()
        .fill(theme.palette.chrome)
        .inner_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, line) in lines.iter().enumerate() {
                let (fill, color) = match line.chars().next() {
                    Some('+') => (theme.tones.green.bg, theme.tones.green.fg),
                    Some('-') => (theme.tones.red.bg, theme.tones.red.fg),
                    Some('@') => (Color32::TRANSPARENT, theme.tones.blue.fg),
                    _ => (Color32::TRANSPARENT, theme.palette.text_secondary),
                };
                let marked = mark_last && i + 1 == lines.len();
                let response = egui::Frame::new()
                    .fill(fill)
                    .inner_margin(Margin::symmetric(10, 0))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add(
                            egui::Label::new(mono(line.replace('\t', "    "), size).color(color))
                                .truncate(),
                        );
                    })
                    .response;
                if marked {
                    let r = response.rect;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(r.min, Vec2::new(3.0, r.height())),
                        0.0,
                        theme.palette.accent,
                    );
                }
            }
        });
}

/// Lines added and removed by a patch.
pub fn count(diff: &str) -> (usize, usize) {
    diff.lines().fold((0, 0), |(a, d), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (a + 1, d)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (a, d + 1)
        } else {
            (a, d)
        }
    })
}

/// The "Log" button of a failed job; `true` when its log shows below.
pub fn log_button(
    details: &mut Details,
    app: &App,
    ui: &mut Ui,
    theme: &Theme,
    job_id: u64,
) -> bool {
    let open = details.open_logs.contains(&job_id);
    let label = if open { tl("Hide the log") } else { tl("Log") };
    let response = ui
        .add(egui::Button::new(RichText::new(label).size(theme.text_size - 2.0)).small())
        .on_hover_text(tl("The end of this job's log"));
    if response.clicked() {
        if open {
            details.open_logs.remove(&job_id);
        } else {
            details.open_logs.insert(job_id);
            if !matches!(
                details.logs.get(&job_id),
                Some(Fetch::Ready(_) | Fetch::Loading(_))
            ) {
                details.load_log(app, job_id, ui.ctx());
            }
        }
    }
    details.open_logs.contains(&job_id)
}

/// The end of a job's log, the lines that tell what failed in red.
pub fn log(details: &mut Details, ui: &mut Ui, theme: &Theme, job_id: u64) {
    let Some(fetch) = details.logs.get_mut(&job_id) else {
        return;
    };
    fetch.poll();
    match fetch {
        Fetch::Loading(_) => {
            ui.horizontal(|ui| {
                spinner(ui, 12.0, theme.palette.accent);
                ui.label(muted(theme, tl("Reading the log…")));
            });
        }
        Fetch::Failed(error) => {
            ui.label(RichText::new(format!("⚠ {error}")).color(theme.tones.red.fg));
        }
        Fetch::Ready(text) => {
            let size = theme.text_size - 2.5;
            egui::Frame::new()
                .fill(theme.palette.chrome)
                .corner_radius(CornerRadius::same(theme.radii.medium))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    egui::ScrollArea::both()
                        .id_salt(("job_log", job_id))
                        .max_height(260.0)
                        .min_scrolled_height(220.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            for line in text.lines() {
                                let color = if is_failure(line) {
                                    theme.tones.red.fg
                                } else {
                                    theme.palette.text_secondary
                                };
                                ui.add(egui::Label::new(mono(line, size).color(color)).extend());
                            }
                        });
                });
            ui.horizontal(|ui| {
                ui.label(muted(
                    theme,
                    tf("The last {n} lines", &[("n", &LOG_LINES.to_string())]),
                ));
                if ui.small_button(tl("Copy")).clicked() {
                    ui.ctx().copy_text(text.clone());
                }
            });
        }
    }
}

/// Whether a log line tells what failed.
pub fn is_failure(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "failed",
        "failure",
        "panicked",
        "fatal",
        "exception",
        "exit code",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_count_their_lines_not_their_headers() {
        let diff = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-old\n+new\n+more\n same";
        assert_eq!(count(diff), (2, 1));
    }

    #[test]
    fn failures_stand_out() {
        assert!(is_failure("error[E0308]: mismatched types"));
        assert!(is_failure("test result: FAILED. 1 passed; 2 failed"));
        assert!(!is_failure("Compiling gitlab-tracker v0.1.0"));
    }
}

#[cfg(test)]
mod snippet_tests {
    use super::snippet;

    #[test]
    fn a_thread_finds_its_line_in_the_patch() {
        let diff = "@@ -10,3 +10,4 @@ fn a()\n ctx\n-old\n+new\n+more\n tail";
        // New lines: 10 ctx, 11 new, 12 more, 13 tail.
        assert_eq!(snippet(diff, 12, 2), ["-old", "+new", "+more"]);
        assert_eq!(snippet(diff, 10, 3), ["@@ -10,3 +10,4 @@ fn a()", " ctx"]);
        assert!(snippet(diff, 40, 2).is_empty());
    }
}
