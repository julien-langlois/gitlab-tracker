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
const LOG_LINES: usize = 60;
/// Lines of a file's patch drawn; the rest is one click away in GitLab.
const PATCH_LINES: usize = 400;

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
    /// Files unfolded, by MR id and path; logs shown, by job id.
    open_files: HashSet<(String, String)>,
    open_logs: HashSet<u64>,
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

/// The "Changes" section's body: a button until loaded, then the files, each
/// unfolding to its patch.
pub fn changes(
    details: &mut Details,
    app: &App,
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    web_url: &str,
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
            for file in files {
                let key = (mr_id.to_string(), file.new_path.clone());
                let open = details.open_files.contains(&key);
                if file_row(ui, theme, file, open) {
                    toggle = Some(key.clone());
                }
                if open {
                    patch(ui, theme, &file.diff, web_url);
                }
            }
            if let Some(key) = toggle {
                if !details.open_files.remove(&key) {
                    details.open_files.insert(key);
                }
            }
        }
    }
}

/// A file: its state, its path and its counts; `true` when clicked.
fn file_row(ui: &mut Ui, theme: &Theme, file: &FileDiff, open: bool) -> bool {
    let (added, deleted) = count(&file.diff);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
    }
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(4.0, 0.0)))
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
            // The file name stands out from its folder.
            let (dir, name) = match path.rsplit_once('/') {
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

/// Lines added and removed by a patch.
fn count(diff: &str) -> (usize, usize) {
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
fn is_failure(line: &str) -> bool {
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
