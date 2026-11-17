//! Overlaps' hotspots: the folders (or files) where the stuck open MRs meet.
//! A path scores one per open MR changing it that is stuck — in conflict,
//! failing its pipeline, or without activity for the project's stale days —,
//! among at least two open MRs changing it. The path picked unfolds: its MRs,
//! the last merged change to it, and which one to merge first.

use bevel_theme::{Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, CornerRadius, FontId, Rect, Ui, Vec2};

use super::board;
use super::widgets::{button, muted};
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState, TrackedMr};
use gitlab_tracker_core::MergeabilityStatus;

/// Paths listed.
const SHOWN: usize = 8;

/// Why an open MR is stuck.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stuck {
    pub conflict: bool,
    pub ci: bool,
    pub stale: bool,
}

impl Stuck {
    pub fn of(mr: &TrackedMr, stale_days: u64, now: DateTime<Utc>) -> Self {
        let quiet_since = mr.updated_at.as_deref().and_then(board::parse_time);
        Self {
            conflict: matches!(
                mr.mergeability,
                MergeabilityStatus::Conflict | MergeabilityStatus::NeedsRebase
            ),
            ci: mr
                .pipelines
                .first()
                .is_some_and(|p| p.status == PipelineState::Failed),
            stale: quiet_since.is_some_and(|t| now - t >= Duration::days(stale_days.max(1) as i64)),
        }
    }

    pub fn any(self) -> bool {
        self.conflict || self.ci || self.stale
    }
}

/// A folder or file and the open MRs changing it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub path: String,
    /// The open MRs changing it, by id, with why each is stuck.
    pub mrs: Vec<(String, Stuck)>,
    pub conflicts: usize,
    pub ci: usize,
    pub stale: usize,
}

impl Spot {
    pub fn stuck(&self) -> usize {
        self.mrs.iter().filter(|(_, s)| s.any()).count()
    }
}

/// The folder of `path`, two levels deep at most: `src/db/`.
pub fn folder(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 | 1 => "/".into(),
        2 => format!("{}/", parts[0]),
        _ => format!("{}/{}/", parts[0], parts[1]),
    }
}

/// The hotspots of `app`'s open MRs, the most stuck MRs first (then the most
/// MRs, then the path). `by_file`: files rather than folders; `lockfiles`:
/// keep the lock files.
pub fn analyze(app: &App, by_file: bool, lockfiles: bool, now: DateTime<Utc>) -> Vec<Spot> {
    let stale_days = app.config.activity_stale_days;
    let mut spots: Vec<Spot> = Vec::new();
    for mr in &app.mrs {
        if mr.state != GitlabMrState::Opened || mr.title.is_empty() {
            continue;
        }
        let Some(stats) = mr.diff_stats.as_ref().filter(|s| s.files_listed) else {
            continue;
        };
        let stuck = Stuck::of(mr, stale_days, now);
        let mut paths: Vec<String> = stats
            .files
            .iter()
            .filter(|f| lockfiles || !super::overlaps::is_lock_file(&f.path))
            .map(|f| {
                if by_file {
                    f.path.clone()
                } else {
                    folder(&f.path)
                }
            })
            .collect();
        paths.sort();
        paths.dedup();
        for path in paths {
            let spot = match spots.iter_mut().position(|s| s.path == path) {
                Some(i) => &mut spots[i],
                None => {
                    spots.push(Spot {
                        path,
                        mrs: Vec::new(),
                        conflicts: 0,
                        ci: 0,
                        stale: 0,
                    });
                    spots.last_mut().expect("just pushed")
                }
            };
            spot.mrs.push((mr.id.clone(), stuck));
            spot.conflicts += usize::from(stuck.conflict);
            spot.ci += usize::from(stuck.ci);
            spot.stale += usize::from(stuck.stale);
        }
    }
    spots.retain(|s| s.mrs.len() >= 2 && s.stuck() > 0);
    spots.sort_by(|a, b| {
        b.stuck()
            .cmp(&a.stuck())
            .then(b.mrs.len().cmp(&a.mrs.len()))
            .then(a.path.cmp(&b.path))
    });
    spots
}

/// Whether `path` (a folder ending in `/`, or a file) covers `file`.
fn covers(path: &str, file: &str, by_file: bool) -> bool {
    if by_file {
        file == path
    } else {
        folder(file) == path
    }
}

/// The merged MR that last changed `path`, and when.
fn last_merged<'a>(
    app: &'a App,
    path: &str,
    by_file: bool,
) -> Option<(&'a TrackedMr, DateTime<Utc>)> {
    app.mrs
        .iter()
        .filter(|m| m.state == GitlabMrState::Merged)
        .filter(|m| {
            m.diff_stats
                .as_ref()
                .is_some_and(|d| d.files.iter().any(|f| covers(path, &f.path, by_file)))
        })
        .filter_map(|m| Some((m, m.merged_at.as_deref().and_then(board::parse_time)?)))
        .max_by_key(|(_, at)| *at)
}

/// Ready to merge: approved and mergeable.
fn ready(mr: &TrackedMr) -> bool {
    mr.mergeability == MergeabilityStatus::Mergeable && !mr.approved_by.is_empty()
}

/// The MR to merge first among `spot`'s: one ready and stuck nowhere, the
/// smallest change first (the others then rebase onto little).
pub fn merge_first<'a>(app: &'a App, spot: &Spot) -> Option<&'a TrackedMr> {
    spot.mrs
        .iter()
        .filter(|(_, stuck)| !stuck.any())
        .filter_map(|(id, _)| app.mrs.iter().find(|m| &m.id == id))
        .filter(|m| ready(m))
        .min_by_key(|m| {
            m.diff_stats
                .as_ref()
                .map_or(u32::MAX, |d| d.additions + d.deletions)
        })
}

/// What a click on the card asked for.
pub enum Click {
    /// Unfold this path (or fold it, when it is the one unfolded).
    Pick(String),
    ByFile(bool),
    /// Show this MR.
    Mr(String),
    /// List these MRs in the table, under this name.
    Table(String, Vec<String>),
}

fn reason_tags(stuck: Stuck, theme: &Theme) -> Vec<(String, Tone)> {
    let t = &theme.tones;
    let mut tags = Vec::new();
    if stuck.conflict {
        tags.push((tl("conflict").to_string(), t.red));
    }
    if stuck.ci {
        tags.push((tl("CI failing").to_string(), t.amber));
    }
    if stuck.stale {
        tags.push((tl("stale").to_string(), t.gray));
    }
    tags
}

/// The card, full width under the matrix. `picked`: the path unfolded.
pub fn card(
    ui: &mut Ui,
    theme: &Theme,
    app: &App,
    by_file: bool,
    lockfiles: bool,
    picked: Option<&str>,
) -> Option<Click> {
    let now = Utc::now();
    let spots = analyze(app, by_file, lockfiles, now);
    let mut click = None;
    theme
        .card_frame()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(super::widgets::semibold(tl("Bottlenecks"), theme.text_size));
                ui.label(
                    muted(theme, tl("where the stuck MRs meet")).size(theme.text_size - 1.5),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.allocate_ui(Vec2::new(200.0, 28.0), |ui| {
                        let labels = [tl("Folders"), tl("Files")];
                        if let Some(i) =
                            board::segmented(ui, theme, "hotspots_by", &labels, usize::from(by_file))
                        {
                            click = Some(Click::ByFile(i == 1));
                        }
                    });
                });
            });
            ui.label(
                muted(
                    theme,
                    tf(
                        "A path scores one per open MR changing it that is in conflict, failing its CI, or quiet for {days} days or more (the project's stale days).",
                        &[("days", &app.config.activity_stale_days.to_string())],
                    ),
                )
                .size(theme.text_size - 1.5),
            );
            ui.add_space(8.0);
            if spots.is_empty() {
                board::empty(
                    ui,
                    theme,
                    tl("No path where several open MRs meet with one of them stuck."),
                );
                return;
            }
            let most = spots.first().map_or(1, Spot::stuck).max(1);
            for spot in spots.iter().take(SHOWN) {
                let open = picked == Some(spot.path.as_str());
                let (rect, response) = board::row(ui, theme, 34.0, open);
                let y = rect.center().y;
                let count_w = 90.0;
                let tags_w = 250.0;
                let right = rect.right() - 8.0;
                // The heat: a gauge as on the hot spots card, its share of
                // the worst path's stuck MRs, before the reasons.
                let gauge_w = 110.0;
                let track = Rect::from_min_size(
                    egui::pos2(right - count_w - tags_w - gauge_w - 16.0, y - 3.0),
                    Vec2::new(gauge_w, 6.0),
                );
                ui.painter()
                    .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
                let mut fill = track;
                fill.set_width(track.width() * spot.stuck() as f32 / most as f32);
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(3), theme.tones.orange.fg);
                super::overlaps::path_text(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 8.0, y),
                    &spot.path,
                    (track.left() - rect.left() - 24.0).max(40.0),
                );
                // Why: a tag per reason, with its count.
                let mut x = right - count_w - tags_w;
                for (n, label, tone) in [
                    (spot.conflicts, tl("conflict"), theme.tones.red),
                    (spot.ci, tl("CI failing"), theme.tones.amber),
                    (spot.stale, tl("stale"), theme.tones.gray),
                ] {
                    if n > 0 {
                        x += board::paint_tag(ui, theme, egui::pos2(x, y), &format!("{label} {n}"), tone, false) + 6.0;
                    }
                }
                board::paint_text(
                    ui,
                    egui::pos2(right, y),
                    Align2::RIGHT_CENTER,
                    &tf(
                        "{stuck} of {n} MRs",
                        &[("stuck", &spot.stuck().to_string()), ("n", &spot.mrs.len().to_string())],
                    ),
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text_secondary,
                    count_w,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    click = Some(Click::Pick(spot.path.clone()));
                }
                if open {
                    if let Some(c) = detail(ui, theme, app, spot, by_file, now) {
                        click = Some(c);
                    }
                }
            }
            if spots.len() > SHOWN {
                ui.add_space(4.0);
                ui.label(
                    muted(
                        theme,
                        tn(
                            (spots.len() - SHOWN) as u64,
                            "{n} more path",
                            "{n} more paths",
                        ),
                    )
                    .size(theme.text_size - 1.5),
                );
            }
        });
    click
}

/// The path picked, unfolded: its MRs, its last merged change, which to merge first.
fn detail(
    ui: &mut Ui,
    theme: &Theme,
    app: &App,
    spot: &Spot,
    by_file: bool,
    now: DateTime<Utc>,
) -> Option<Click> {
    let mut click = None;
    egui::Frame::new()
        .fill(theme.palette.input)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 12,
        })
        .outer_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 2,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            if let Some((last, at)) = last_merged(app, &spot.path, by_file) {
                ui.label(
                    muted(
                        theme,
                        tf(
                            "Last change merged {time} ago: !{id} {title}",
                            &[
                                ("time", &board::duration_text(board::days_between(at, now).max(0.0))),
                                ("id", &last.id),
                                ("title", &last.title),
                            ],
                        ),
                    )
                    .size(theme.text_size - 1.5),
                );
                ui.add_space(4.0);
            }
            for (id, stuck) in &spot.mrs {
                let Some(mr) = app.mrs.iter().find(|m| &m.id == id) else {
                    continue;
                };
                let (rect, response) = board::row(ui, theme, 28.0, false);
                let y = rect.center().y;
                let mut x = rect.left() + 6.0;
                x += board::paint_tag(ui, theme, egui::pos2(x, y), &format!("!{id}"), board::id_tone(theme), true) + 10.0;
                let tags = reason_tags(*stuck, theme);
                let (status, status_tone) = if tags.is_empty() {
                    if ready(mr) {
                        (tl("ready to merge").to_string(), theme.tones.green)
                    } else {
                        let (label, tone) =
                            super::tones::status(mr.state, &mr.mergeability, &theme.tones);
                        (label.to_lowercase(), tone)
                    }
                } else {
                    (String::new(), theme.tones.gray)
                };
                // Right to left: its reasons (or its status).
                let mut right = rect.right() - 6.0;
                let mut shown: Vec<(String, Tone)> = tags;
                if !status.is_empty() {
                    shown.push((status, status_tone));
                }
                for (text, tone) in shown.iter().rev() {
                    let w = board::one_line(ui, text, FontId::proportional(theme.text_size - 2.5), tone.fg, f32::INFINITY).size().x + 12.0;
                    board::paint_tag(ui, theme, egui::pos2(right - w, y), text, *tone, false);
                    right -= w + 6.0;
                }
                board::paint_text(
                    ui,
                    egui::pos2(x, y),
                    Align2::LEFT_CENTER,
                    &mr.title,
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text,
                    (right - x - 8.0).max(0.0),
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tl("Open its page"))
                    .clicked()
                {
                    click = Some(Click::Mr(id.clone()));
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let advice = match merge_first(app, spot) {
                    Some(first) => (
                        tf(
                            "Merge !{id} first: it is ready, and the others then rebase once onto it.",
                            &[("id", &first.id)],
                        ),
                        theme.tones.green,
                    ),
                    None => (
                        tl("None of them is ready to merge: unblock the stuck ones first.").to_string(),
                        theme.tones.gray,
                    ),
                };
                egui::Frame::new()
                    .fill(advice.1.bg)
                    .corner_radius(CornerRadius::same(theme.radii.small))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(advice.0)
                                .size(theme.text_size - 1.0)
                                .color(advice.1.fg),
                        );
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let text = tn(
                        spot.mrs.len() as u64,
                        "Show the {n} MR in the table",
                        "Show these {n} MRs in the table",
                    );
                    if button(ui, theme, None, &text, false).clicked() {
                        click = Some(Click::Table(
                            spot.path.clone(),
                            spot.mrs.iter().map(|(id, _)| id.clone()).collect(),
                        ));
                    }
                });
            });
        });
    click
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChangedFile, DiffStats};

    fn mr(id: &str, files: &[&str], mergeability: MergeabilityStatus) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
        mr.data.author = "Alex <@alex>".into();
        mr.data.state = GitlabMrState::Opened;
        mr.mergeability = mergeability;
        mr.data.updated_at = Some(Utc::now().to_rfc3339());
        mr.data.diff_stats = Some(DiffStats {
            files: files
                .iter()
                .map(|p| ChangedFile {
                    path: (*p).into(),
                    ..ChangedFile::default()
                })
                .collect(),
            files_listed: true,
            additions: 10,
            ..DiffStats::default()
        });
        mr
    }

    async fn app_with(mrs: Vec<TrackedMr>) -> App {
        let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        app.mrs = mrs;
        app
    }

    #[tokio::test]
    async fn a_path_scores_its_stuck_mrs_among_several() {
        let mut ready = mr("3", &["src/db/pool.rs"], MergeabilityStatus::Mergeable);
        ready.data.approved_by = vec!["Sam".into()];
        let mut quiet = mr(
            "4",
            &["src/db/migrations/1.sql", "README.md"],
            MergeabilityStatus::NotApproved,
        );
        quiet.data.updated_at = Some((Utc::now() - Duration::days(30)).to_rfc3339());
        let app = app_with(vec![
            mr(
                "1",
                &["src/db/pool.rs", "Cargo.lock"],
                MergeabilityStatus::Conflict,
            ),
            mr(
                "2",
                &["src/api/a.rs", "Cargo.lock"],
                MergeabilityStatus::Conflict,
            ),
            ready,
            quiet,
        ])
        .await;
        let spots = analyze(&app, false, false, Utc::now());
        // src/db/: !1 (conflict), !3 (ready), !4 (stale) — two stuck of three.
        assert_eq!(spots[0].path, "src/db/");
        assert_eq!((spots[0].stuck(), spots[0].mrs.len()), (2, 3));
        assert_eq!((spots[0].conflicts, spots[0].stale), (1, 1));
        // src/api/ has one MR; the lock file is left out; README.md alone.
        assert_eq!(spots.len(), 1);
        // With the lock files, Cargo.lock joins two conflicts.
        let with_locks = analyze(&app, true, true, Utc::now());
        assert!(with_locks
            .iter()
            .any(|s| s.path == "Cargo.lock" && s.stuck() == 2));
        // Merge first: the ready one.
        assert_eq!(
            merge_first(&app, &spots[0]).map(|m| m.id.as_str()),
            Some("3")
        );
    }

    #[test]
    fn folders_are_two_levels_deep() {
        assert_eq!(folder("src/db/migrations/1.sql"), "src/db/");
        assert_eq!(folder("src/main.rs"), "src/");
        assert_eq!(folder("README.md"), "/");
    }

    #[tokio::test]
    async fn the_demo_has_hotspots() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let spots = analyze(&app, false, false, Utc::now());
        assert!(!spots.is_empty());
        for s in &spots {
            assert!(s.mrs.len() >= 2 && s.stuck() >= 1, "{}", s.path);
        }
    }
}
