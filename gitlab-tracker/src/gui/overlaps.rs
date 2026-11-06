//! The overlaps page: the open MRs that change the same files — on the same
//! lines, close to each other, or only in the same file —, read from the files
//! and hunks each MR's changes already carry (no fetch of its own); a matrix of
//! the pairs, the files most changed of late, and the pair picked with where
//! the two MRs meet and which one to merge first.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::widgets::{button, muted, primary_button, split_person};
use super::GuiApp;
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{ChangedFile, Effort, GitlabMrState};
use gitlab_tracker_core::MergeabilityStatus;

/// Lines apart under which two hunks are "close": git may well stop on them.
const NEAR_LINES: u32 = 8;

/// The lock files, rewritten by tools: left out by default.
const LOCK_FILES: [&str; 10] = [
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "composer.lock",
    "Gemfile.lock",
    "poetry.lock",
    "go.sum",
    "Pipfile.lock",
    "mix.lock",
];

/// How two MRs meet in a file, from the worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    SameLines,
    Near,
    SameFile,
}

impl Level {
    fn label(self) -> &'static str {
        tl(match self {
            Level::SameLines => "Same lines",
            Level::Near => "Close lines",
            Level::SameFile => "Same file",
        })
    }

    fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Level::SameLines => t.red,
            Level::Near => t.orange,
            Level::SameFile => t.amber,
        }
    }
}

/// The side panel's views: which list the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Files,
    Pairs,
    Mine,
}

/// The overlaps page's choices.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub view: View,
    pub folder: Option<String>,
    pub lockfiles: bool,
    pub drafts: bool,
    pub pair: Option<(String, String)>,
}

/// An open MR the page looks at.
#[derive(Debug, Clone)]
pub struct Mr {
    pub id: String,
    pub title: String,
    pub author: String,
    pub effort: Option<Effort>,
    pub lines: u32,
    pub approved: bool,
    pub mergeable: bool,
    pub files: Vec<ChangedFile>,
}

/// A file changed by several open MRs.
#[derive(Debug, Clone)]
pub struct Shared {
    pub path: String,
    /// The MRs changing it, by id, with their hunks.
    pub mrs: Vec<(String, Vec<(u32, u32)>)>,
    pub level: Level,
    pub added: u32,
    pub deleted: u32,
}

/// Two MRs and the files they share, the worst level first.
#[derive(Debug, Clone)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub files: Vec<(String, Level)>,
}

impl Pair {
    fn level(&self) -> Level {
        self.files
            .iter()
            .map(|f| f.1)
            .min()
            .unwrap_or(Level::SameFile)
    }
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Overlaps {
    pub mrs: Vec<Mr>,
    pub files: Vec<Shared>,
    pub pairs: Vec<Pair>,
    /// All the files the open MRs change.
    pub total_files: usize,
    /// Open MRs whose files are not known yet.
    pub unknown: usize,
}

fn is_lock_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    LOCK_FILES.contains(&name)
}

/// How two MRs' hunks in the same file meet.
fn meet(a: &ChangedFile, b: &ChangedFile) -> Level {
    // Both add the file, or one deletes it: git stops.
    if (a.new_file && b.new_file) || a.deleted_file || b.deleted_file {
        return Level::SameLines;
    }
    let mut level = Level::SameFile;
    for (a0, a1) in &a.hunks {
        for (b0, b1) in &b.hunks {
            if a0 <= b1 && b0 <= a1 {
                return Level::SameLines;
            }
            let gap = if a1 < b0 {
                b0 - a1
            } else {
                a0.saturating_sub(*b1)
            };
            if gap <= NEAR_LINES {
                level = Level::Near;
            }
        }
    }
    level
}

/// The overlaps between `app`'s open MRs.
pub fn analyze(app: &App, lockfiles: bool, drafts: bool) -> Overlaps {
    let profile = &app.config.complexity_profile;
    let mut out = Overlaps::default();
    for mr in &app.mrs {
        if mr.title.is_empty() || mr.state != GitlabMrState::Opened {
            continue;
        }
        if !drafts && mr.mergeability == MergeabilityStatus::Draft {
            continue;
        }
        let Some(stats) = mr.diff_stats.as_ref().filter(|s| s.files_listed) else {
            out.unknown += 1;
            continue;
        };
        out.mrs.push(Mr {
            id: mr.id.clone(),
            title: mr.title.clone(),
            author: mr.author.clone(),
            effort: Some(Effort::from_score(stats.difficulty(profile))),
            lines: stats.additions + stats.deletions,
            approved: !mr.approved_by.is_empty(),
            mergeable: mr.mergeability == MergeabilityStatus::Mergeable,
            files: stats
                .files
                .iter()
                .filter(|f| lockfiles || !is_lock_file(&f.path))
                .cloned()
                .collect(),
        });
    }
    // By the file's path on the target.
    let mut by_path: BTreeMap<String, Vec<(usize, &ChangedFile)>> = BTreeMap::new();
    for (index, mr) in out.mrs.iter().enumerate() {
        for f in &mr.files {
            let key = f.old_path.clone().unwrap_or_else(|| f.path.clone());
            by_path.entry(key).or_default().push((index, f));
        }
    }
    out.total_files = by_path.len();
    let mut pairs: BTreeMap<(usize, usize), Vec<(String, Level)>> = BTreeMap::new();
    for (path, list) in &by_path {
        if list.len() < 2 {
            continue;
        }
        let mut worst = Level::SameFile;
        for (i, (ia, fa)) in list.iter().enumerate() {
            for (ib, fb) in &list[i + 1..] {
                let level = meet(fa, fb);
                worst = worst.min(level);
                let key = ((*ia).min(*ib), (*ia).max(*ib));
                pairs.entry(key).or_default().push((path.clone(), level));
            }
        }
        out.files.push(Shared {
            path: path.clone(),
            mrs: list
                .iter()
                .map(|(i, f)| (out.mrs[*i].id.clone(), f.hunks.clone()))
                .collect(),
            level: worst,
            added: list.iter().map(|(_, f)| f.added).sum(),
            deleted: list.iter().map(|(_, f)| f.deleted).sum(),
        });
    }
    out.files.sort_by(|a, b| {
        a.level
            .cmp(&b.level)
            .then(b.mrs.len().cmp(&a.mrs.len()))
            .then(a.path.cmp(&b.path))
    });
    out.pairs = pairs
        .into_iter()
        .map(|((a, b), mut files)| {
            files.sort_by(|x, y| x.1.cmp(&y.1).then(x.0.cmp(&y.0)));
            Pair {
                a: out.mrs[a].id.clone(),
                b: out.mrs[b].id.clone(),
                files,
            }
        })
        .collect();
    out.pairs.sort_by(|x, y| {
        x.level()
            .cmp(&y.level())
            .then(y.files.len().cmp(&x.files.len()))
            .then(x.a.cmp(&y.a))
    });
    out
}

impl Overlaps {
    fn mr(&self, id: &str) -> Option<&Mr> {
        self.mrs.iter().find(|m| m.id == id)
    }

    fn pair(&self, a: &str, b: &str) -> Option<&Pair> {
        self.pairs
            .iter()
            .find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a))
    }

    fn file(&self, path: &str) -> Option<&Shared> {
        self.files.iter().find(|f| f.path == path)
    }
}

/// The folder of a path, two levels deep: `src/api/`.
fn folder(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 | 1 => "/".into(),
        2 => format!("{}/", parts[0]),
        _ => format!("{}/{}/", parts[0], parts[1]),
    }
}

/// The files changed by the most tracked MRs created in the last 30 days.
fn hot_spots(app: &App, now: DateTime<Utc>) -> Vec<(String, usize)> {
    let since = now - Duration::days(30);
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for mr in &app.mrs {
        let recent = board::time_of(mr.created_at.as_deref())
            .or_else(|| board::time_of(mr.updated_at.as_deref()))
            .is_some_and(|t| t >= since);
        let Some(stats) = mr.diff_stats.as_ref().filter(|_| recent) else {
            continue;
        };
        for f in stats.files.iter().filter(|f| !is_lock_file(&f.path)) {
            *counts.entry(f.path.as_str()).or_default() += 1;
        }
    }
    let mut list: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .map(|(p, n)| (p.to_string(), n))
        .collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    list.truncate(5);
    list
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let overlaps = analyze(&g.app, g.overlaps.lockfiles, g.overlaps.drafts);
    let hot = hot_spots(&g.app, now);
    let pair = g
        .overlaps
        .pair
        .clone()
        .filter(|(a, b)| overlaps.pair(a, b).is_some())
        .or_else(|| overlaps.pairs.first().map(|p| (p.a.clone(), p.b.clone())));
    board::screen(
        g,
        ui,
        sizes,
        "overlaps",
        |g, ui| nav(g, ui, &overlaps),
        |g, ui| inspector(g, ui, &overlaps, pair.as_ref()),
        |g, ui| main(g, ui, &overlaps, &hot, pair.as_ref()),
    );
}

fn me(app: &App) -> Option<String> {
    app.config.gitlab_username.clone()
}

fn mine(app: &App, overlaps: &Overlaps, pair: &Pair) -> bool {
    let Some(me) = me(app) else {
        return false;
    };
    [&pair.a, &pair.b].iter().any(|id| {
        overlaps
            .mr(id)
            .is_some_and(|m| crate::next_step::login(&m.author) == me)
    })
}

fn nav(g: &mut GuiApp, ui: &mut Ui, overlaps: &Overlaps) {
    let theme = g.theme;
    let subtitle = format!(
        "{} · {}",
        tn(overlaps.mrs.len() as u64, "{n} open MR", "{n} open MRs"),
        tn(overlaps.total_files as u64, "{n} file", "{n} files")
    );
    board::nav_header(ui, &theme, tl("Overlaps"), &subtitle);
    let my_pairs = overlaps
        .pairs
        .iter()
        .filter(|p| mine(&g.app, overlaps, p))
        .count();
    board::nav_group(ui, &theme, tl("Views"), |ui| {
        for (view, label, n) in [
            (View::Files, tl("Shared files"), overlaps.files.len()),
            (View::Pairs, tl("Pairs at risk"), overlaps.pairs.len()),
            (View::Mine, tl("My MRs"), my_pairs),
        ] {
            if board::nav_item(
                ui,
                &theme,
                label,
                &n.to_string(),
                None,
                g.overlaps.view == view,
            )
            .clicked()
            {
                g.overlaps.view = view;
            }
        }
    });
    let mut folders: BTreeMap<String, usize> = BTreeMap::new();
    for f in &overlaps.files {
        *folders.entry(folder(&f.path)).or_default() += 1;
    }
    if !folders.is_empty() {
        board::nav_group(ui, &theme, tl("Folders"), |ui| {
            if board::nav_item(
                ui,
                &theme,
                tl("Every folder"),
                &overlaps.files.len().to_string(),
                None,
                g.overlaps.folder.is_none(),
            )
            .clicked()
            {
                g.overlaps.folder = None;
            }
            let mut picked = None;
            for (name, n) in &folders {
                let selected = g.overlaps.folder.as_deref() == Some(name.as_str());
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                let fill = if selected {
                    theme.palette.selection
                } else if response.hovered() {
                    theme.palette.raised
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
                let count = n.to_string();
                let w = board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &count,
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    40.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 10.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    egui::FontId::monospace(theme.text_size - 1.5),
                    if selected {
                        theme.palette.text
                    } else {
                        theme.palette.text_secondary
                    },
                    rect.width() - w - 30.0,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    picked = Some(name.clone());
                }
            }
            if let Some(name) = picked {
                g.overlaps.folder = Some(name);
            }
        });
    }
    board::nav_group(ui, &theme, "", |ui| {
        if board::nav_switch(
            ui,
            &theme,
            !g.overlaps.lockfiles,
            tl("Leave out lock files"),
        ) {
            g.overlaps.lockfiles = !g.overlaps.lockfiles;
        }
        if board::nav_switch(ui, &theme, g.overlaps.drafts, tl("Include drafts")) {
            g.overlaps.drafts = !g.overlaps.drafts;
        }
    });
    let note = if overlaps.unknown > 0 {
        tn(overlaps.unknown as u64, "Read from the changes the app already fetches. {n} open MR is not read yet: its files show after the next refresh.", "Read from the changes the app already fetches. {n} open MRs are not read yet: their files show after the next refresh.")
    } else {
        tl("Read from the changes the app already fetches for the review effort: no request of its own.").to_string()
    };
    board::nav_note(ui, &theme, &note);
}

// ── Main area ────────────────────────────────────────────────────────────────

fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    overlaps: &Overlaps,
    hot: &[(String, usize)],
    pair: Option<&(String, String)>,
) {
    let theme = g.theme;
    let mut copy = false;
    board::heading(
        ui,
        &theme,
        tl("Overlaps"),
        tl("Open MRs that change the same files, before they step on each other"),
        |ui| {
            copy = button(
                ui,
                &theme,
                Some(super::icons::Icon::Copy),
                tl("Copy the report"),
                false,
            )
            .clicked();
        },
    );
    if copy {
        ui.ctx()
            .copy_text(board::Links::of(&g.app).apply(&report(overlaps)));
        g.app.flash(tl("Report copied"), false);
    }
    ui.add_space(14.0);
    let same = overlaps
        .pairs
        .iter()
        .filter(|p| p.level() == Level::SameLines)
        .count();
    let other = overlaps.pairs.len() - same;
    let t = &theme.tones;
    board::figures(
        ui,
        &theme,
        &[
            Figure {
                label: tl("Pairs on the same lines").into(),
                value: same.to_string(),
                value_color: (same > 0).then_some(t.red.fg),
                caption: tl("git will stop on them").into(),
                caption_color: None,
            },
            Figure {
                label: tl("Pairs in the same files").into(),
                value: other.to_string(),
                value_color: (other > 0).then_some(t.amber.fg),
                caption: tl("close lines, or apart").into(),
                caption_color: None,
            },
            Figure {
                label: tl("Shared files").into(),
                value: overlaps.files.len().to_string(),
                value_color: None,
                caption: tf(
                    "of {n} changed",
                    &[("n", &overlaps.total_files.to_string())],
                ),
                caption_color: None,
            },
        ],
    );
    if overlaps.mrs.len() < 2 {
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("Fewer than two open MRs with their files known: nothing can overlap."),
            );
        });
        return;
    }
    let mut picked: Option<(String, String)> = None;
    let mut picked_right: Option<(String, String)> = None;
    let mut open = None;
    let view = g.overlaps.view;
    let folder_filter = g.overlaps.folder.clone();
    let my_login = me(&g.app);
    let app = &g.app;
    board::split(
        ui,
        0.42,
        290.0,
        340.0,
        |ui| {
            matrix_card(ui, &theme, overlaps, pair, &mut picked);
            ui.add_space(GAP);
            hot_card(ui, &theme, hot);
        },
        |ui| match view {
            View::Files => files_card(
                ui,
                &theme,
                overlaps,
                folder_filter.as_deref(),
                pair,
                &mut picked_right,
                &mut open,
            ),
            View::Pairs | View::Mine => {
                let list: Vec<&Pair> = overlaps
                    .pairs
                    .iter()
                    .filter(|p| view == View::Pairs || my_login.is_some() && mine(app, overlaps, p))
                    .collect();
                pairs_card(ui, &theme, overlaps, &list, pair, &mut picked_right);
            }
        },
    );
    if let Some(p) = picked.or(picked_right) {
        g.overlaps.pair = Some(p);
    }
    if let Some(id) = open {
        g.show_mr(&id);
    }
}

/// The open MRs that share a file, in a lower triangle: the count of files
/// each pair shares, in the tone of how they meet.
fn matrix_card(
    ui: &mut Ui,
    theme: &Theme,
    overlaps: &Overlaps,
    pair: Option<&(String, String)>,
    picked: &mut Option<(String, String)>,
) {
    board::card(ui, theme, tl("MR × MR"), tl("files in common"), 0.0, |ui| {
        let mut ids: BTreeSet<&str> = BTreeSet::new();
        for p in &overlaps.pairs {
            ids.insert(&p.a);
            ids.insert(&p.b);
        }
        let mut ids: Vec<&str> = ids.into_iter().collect();
        ids.sort_by_key(|id| id.parse::<u64>().unwrap_or(u64::MAX));
        ids.truncate(10);
        if ids.len() < 2 {
            board::empty(ui, theme, tl("No two open MRs change the same file."));
            return;
        }
        let label_w = 46.0;
        let n = ids.len() - 1;
        let cell = ((ui.available_width() - label_w - 4.0 * n as f32) / n as f32).clamp(22.0, 48.0);
        let font = egui::FontId::monospace(theme.text_size - 3.0);
        let (head, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        for (j, id) in ids.iter().enumerate().take(n) {
            let cx = head.left() + label_w + j as f32 * (cell + 4.0) + cell / 2.0;
            board::paint_text(
                ui,
                egui::pos2(cx, head.center().y),
                Align2::CENTER_CENTER,
                &format!("!{id}"),
                font.clone(),
                theme.palette.text_secondary,
                cell + 4.0,
            );
        }
        // A lower triangle: no first row, no last column.
        for (i, a) in ids.iter().enumerate().skip(1) {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), cell + 4.0), Sense::hover());
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &format!("!{a}"),
                font.clone(),
                theme.palette.text_secondary,
                label_w - 4.0,
            );
            for (j, b) in ids.iter().enumerate().take(i) {
                let r = Rect::from_min_size(
                    egui::pos2(
                        line.left() + label_w + j as f32 * (cell + 4.0),
                        line.top() + 2.0,
                    ),
                    Vec2::splat(cell),
                );
                match overlaps.pair(a, b) {
                    Some(p) => {
                        let tone = p.level().tone(theme);
                        let response = ui
                            .interact(r, ui.scope_id().with(("pair", *a, *b)), Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(format!(
                                "!{a} × !{b} · {}",
                                tn(p.files.len() as u64, "{n} file", "{n} files")
                            ));
                        let fill = if response.hovered() {
                            tone.bg.gamma_multiply(1.7)
                        } else {
                            tone.bg
                        };
                        ui.painter().rect_filled(r, CornerRadius::same(5), fill);
                        if pair.is_some_and(|(x, y)| (x == a && y == b) || (x == b && y == a)) {
                            ui.painter().rect_stroke(
                                r,
                                CornerRadius::same(5),
                                Stroke::new(2.0, theme.palette.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        board::paint_text(
                            ui,
                            r.center(),
                            Align2::CENTER_CENTER,
                            &p.files.len().to_string(),
                            fonts::semibold(theme.text_size - 1.0),
                            tone.fg,
                            cell,
                        );
                        if response.clicked() {
                            *picked = Some((p.a.clone(), p.b.clone()));
                        }
                    }
                    None => {
                        ui.painter().rect_filled(
                            r,
                            CornerRadius::same(5),
                            theme.palette.raised.gamma_multiply(0.55),
                        );
                        board::paint_text(
                            ui,
                            r.center(),
                            Align2::CENTER_CENTER,
                            "·",
                            FontId::proportional(theme.text_size - 1.0),
                            theme.palette.text_muted.gamma_multiply(0.6),
                            cell,
                        );
                    }
                }
            }
        }
        ui.add_space(8.0);
        board::legend(
            ui,
            theme,
            &[
                (theme.tones.red.fg, Level::SameLines.label().to_string()),
                (theme.tones.orange.fg, Level::Near.label().to_string()),
                (theme.tones.amber.fg, Level::SameFile.label().to_string()),
            ],
        );
    });
}

fn hot_card(ui: &mut Ui, theme: &Theme, hot: &[(String, usize)]) {
    board::card(
        ui,
        theme,
        tl("Hot spots"),
        tl("MRs in 30 days"),
        0.0,
        |ui| {
            if hot.is_empty() {
                board::empty(ui, theme, tl("No file changed by several recent MRs."));
                return;
            }
            let max = hot.first().map_or(1, |h| h.1).max(1);
            for (path, n) in hot {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                let count_w = 26.0;
                let bar_w = (line.width() * 0.28).clamp(50.0, 110.0);
                board::paint_text(
                    ui,
                    egui::pos2(line.right(), line.center().y),
                    Align2::RIGHT_CENTER,
                    &n.to_string(),
                    fonts::medium(theme.text_size - 1.5),
                    theme.palette.text_muted,
                    count_w,
                );
                let track = Rect::from_min_size(
                    egui::pos2(line.right() - count_w - bar_w - 6.0, line.center().y - 3.0),
                    Vec2::new(bar_w, 6.0),
                );
                ui.painter()
                    .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
                let mut fill = track;
                fill.set_width(track.width() * *n as f32 / max as f32);
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(3), theme.tones.orange.fg);
                path_text(
                    ui,
                    theme,
                    egui::pos2(line.left(), line.center().y),
                    path,
                    track.left() - 10.0 - line.left(),
                );
            }
        },
    );
}

/// A path: its folder muted, its name bright, cut from the left when long.
fn path_text(ui: &Ui, theme: &Theme, pos: egui::Pos2, path: &str, max_width: f32) -> f32 {
    let (dir, name) = match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    };
    let font = egui::FontId::monospace(theme.text_size - 1.5);
    let name_galley = board::one_line(ui, name, font.clone(), theme.palette.text, max_width);
    let name_w = name_galley.size().x;
    let room = max_width - name_w;
    let mut x = pos.x;
    if !dir.is_empty() && room > 20.0 {
        // The folder, cut from its start: "…api/graphql/".
        let mut shown = dir.to_string();
        let mut galley = board::one_line(
            ui,
            &shown,
            font.clone(),
            theme.palette.text_muted,
            f32::INFINITY,
        );
        while galley.size().x > room && shown.chars().count() > 2 {
            shown = format!("…{}", shown.chars().skip(2).collect::<String>());
            galley = board::one_line(
                ui,
                &shown,
                font.clone(),
                theme.palette.text_muted,
                f32::INFINITY,
            );
        }
        if galley.size().x <= room {
            let w = galley.size().x;
            ui.painter().galley(
                egui::pos2(x, pos.y - galley.size().y / 2.0),
                galley,
                theme.palette.text_muted,
            );
            x += w;
        }
    }
    ui.painter().galley(
        egui::pos2(x, pos.y - name_galley.size().y / 2.0),
        name_galley,
        theme.palette.text,
    );
    x + name_w - pos.x
}

#[allow(clippy::too_many_arguments)]
fn files_card(
    ui: &mut Ui,
    theme: &Theme,
    overlaps: &Overlaps,
    folder_filter: Option<&str>,
    pair: Option<&(String, String)>,
    picked: &mut Option<(String, String)>,
    open: &mut Option<String>,
) {
    board::card(
        ui,
        theme,
        tl("Shared files"),
        tl("the riskiest first"),
        0.0,
        |ui| {
            let files: Vec<&Shared> = overlaps
                .files
                .iter()
                .filter(|f| folder_filter.is_none_or(|d| folder(&f.path) == d))
                .collect();
            if files.is_empty() {
                board::empty(ui, theme, tl("No file changed by two open MRs here."));
                return;
            }
            let width = ui.available_width();
            let level_w = 92.0;
            let lines_w = 76.0;
            let mrs_w = (width * 0.28).clamp(96.0, 170.0);
            let (head, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
            let small = FontId::proportional(theme.text_size - 1.5);
            board::paint_text(
                ui,
                egui::pos2(head.left() + 8.0, head.center().y),
                Align2::LEFT_CENTER,
                tl("File"),
                small.clone(),
                theme.palette.text_muted,
                80.0,
            );
            let mrs_x = head.right() - 8.0 - lines_w - 12.0 - level_w - 12.0 - mrs_w;
            board::paint_text(
                ui,
                egui::pos2(mrs_x, head.center().y),
                Align2::LEFT_CENTER,
                tl("MRs"),
                small.clone(),
                theme.palette.text_muted,
                mrs_w,
            );
            board::paint_text(
                ui,
                egui::pos2(
                    head.right() - 8.0 - lines_w - 12.0 - level_w,
                    head.center().y,
                ),
                Align2::LEFT_CENTER,
                tl("Risk"),
                small.clone(),
                theme.palette.text_muted,
                level_w,
            );
            board::paint_text(
                ui,
                egui::pos2(head.right() - 8.0, head.center().y),
                Align2::RIGHT_CENTER,
                tl("Lines"),
                small,
                theme.palette.text_muted,
                lines_w,
            );
            for f in files {
                let selected = pair.is_some_and(|(a, b)| {
                    f.mrs.iter().any(|m| &m.0 == a) && f.mrs.iter().any(|m| &m.0 == b)
                });
                let (rect, response) = board::row(ui, theme, 38.0, selected);
                if !selected {
                    board::row_rule(ui, theme, rect);
                }
                let cy = rect.center().y;
                let mrs_x = rect.right() - 8.0 - lines_w - 12.0 - level_w - 12.0 - mrs_w;
                path_text(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 8.0, cy),
                    &f.path,
                    mrs_x - 12.0 - rect.left() - 8.0,
                );
                // The MRs, as many as fit.
                let mut x = mrs_x;
                for (k, (id, _)) in f.mrs.iter().enumerate() {
                    let text = format!("!{id}");
                    let galley = ui.painter().layout_no_wrap(
                        text.clone(),
                        egui::FontId::monospace(theme.text_size - 2.0),
                        theme.palette.text,
                    );
                    let w = galley.size().x + 12.0;
                    if x + w > mrs_x + mrs_w - if k + 1 < f.mrs.len() { 24.0 } else { 0.0 } {
                        board::paint_text(
                            ui,
                            egui::pos2(x, cy),
                            Align2::LEFT_CENTER,
                            &format!("+{}", f.mrs.len() - k),
                            fonts::medium(theme.text_size - 2.0),
                            theme.palette.text_muted,
                            30.0,
                        );
                        break;
                    }
                    let r = Rect::from_min_size(egui::pos2(x, cy - 10.0), Vec2::new(w, 20.0));
                    let hit = ui
                        .interact(
                            r,
                            ui.scope_id().with(("file_mr", f.path.as_str(), k)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(overlaps.mr(id).map_or(String::new(), |m| m.title.clone()));
                    let fill = if hit.hovered() {
                        theme.palette.border
                    } else {
                        theme.palette.raised
                    };
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(theme.radii.small), fill);
                    ui.painter().galley(
                        r.center() - galley.size() / 2.0,
                        galley,
                        theme.palette.text,
                    );
                    if hit.clicked() {
                        *open = Some(id.clone());
                    }
                    x += w + 5.0;
                }
                let level_x = rect.right() - 8.0 - lines_w - 12.0 - level_w;
                board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(level_x, cy),
                    f.level.label(),
                    f.level.tone(theme),
                    false,
                );
                let added = format!("+{}", f.added);
                let deleted = format!("−{}", f.deleted);
                let font = egui::FontId::monospace(theme.text_size - 2.0);
                let dw = board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 8.0, cy),
                    Align2::RIGHT_CENTER,
                    &deleted,
                    font.clone(),
                    theme.tones.red.fg,
                    lines_w / 2.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 8.0 - dw - 6.0, cy),
                    Align2::RIGHT_CENTER,
                    &added,
                    font,
                    theme.tones.green.fg,
                    lines_w / 2.0,
                );
                if response.clicked() {
                    // The worst pair on this file.
                    let best = overlaps
                        .pairs
                        .iter()
                        .filter(|p| p.files.iter().any(|(path, _)| path == &f.path))
                        .min_by_key(|p| {
                            p.files
                                .iter()
                                .find(|(path, _)| path == &f.path)
                                .map(|x| x.1)
                        });
                    if let Some(p) = best {
                        *picked = Some((p.a.clone(), p.b.clone()));
                    }
                }
            }
        },
    );
}

fn pairs_card(
    ui: &mut Ui,
    theme: &Theme,
    overlaps: &Overlaps,
    list: &[&Pair],
    pair: Option<&(String, String)>,
    picked: &mut Option<(String, String)>,
) {
    board::card(
        ui,
        theme,
        tl("Pairs at risk"),
        tl("the riskiest first"),
        0.0,
        |ui| {
            if list.is_empty() {
                board::empty(ui, theme, tl("No pair of open MRs in this view."));
                return;
            }
            for p in list {
                let selected =
                    pair.is_some_and(|(a, b)| (a == &p.a && b == &p.b) || (a == &p.b && b == &p.a));
                let (rect, response) = board::row(ui, theme, 46.0, selected);
                if !selected {
                    board::row_rule(ui, theme, rect);
                }
                let top = rect.top() + 14.0;
                let mut x = rect.left() + 8.0;
                x += board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(x, top),
                    &format!("!{}", p.a),
                    board::id_tone(theme),
                    true,
                ) + 6.0;
                x += board::paint_text(
                    ui,
                    egui::pos2(x, top),
                    Align2::LEFT_CENTER,
                    "×",
                    FontId::proportional(theme.text_size),
                    theme.palette.text_muted,
                    14.0,
                ) + 6.0;
                x += board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(x, top),
                    &format!("!{}", p.b),
                    board::id_tone(theme),
                    true,
                ) + 10.0;
                let level = p.level();
                let galley = ui.painter().layout_no_wrap(
                    level.label().to_string(),
                    fonts::medium(theme.text_size - 2.5),
                    level.tone(theme).fg,
                );
                let tag_w = galley.size().x + 12.0;
                board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.right() - 8.0 - tag_w, top),
                    level.label(),
                    level.tone(theme),
                    false,
                );
                let titles = format!(
                    "{} · {}",
                    overlaps.mr(&p.a).map_or("", |m| m.title.as_str()),
                    overlaps.mr(&p.b).map_or("", |m| m.title.as_str())
                );
                board::paint_text(
                    ui,
                    egui::pos2(x, top),
                    Align2::LEFT_CENTER,
                    &titles,
                    FontId::proportional(theme.text_size - 1.5),
                    theme.palette.text_muted,
                    rect.right() - 16.0 - tag_w - x,
                );
                let files = p
                    .files
                    .iter()
                    .map(|(f, _)| f.rsplit('/').next().unwrap_or(f))
                    .collect::<Vec<_>>()
                    .join(", ");
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 8.0, rect.top() + 33.0),
                    Align2::LEFT_CENTER,
                    &format!(
                        "{} · {files}",
                        tn(p.files.len() as u64, "{n} file", "{n} files")
                    ),
                    egui::FontId::monospace(theme.text_size - 2.5),
                    theme.palette.text_secondary,
                    rect.width() - 16.0,
                );
                if response.clicked() {
                    *picked = Some((p.a.clone(), p.b.clone()));
                }
            }
        },
    );
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(g: &mut GuiApp, ui: &mut Ui, overlaps: &Overlaps, pair: Option<&(String, String)>) {
    let theme = g.theme;
    let Some((a, b)) = pair else {
        board::inspector_empty(
            ui,
            &theme,
            tl("No two open MRs change the same file: nothing to pick."),
        );
        return;
    };
    let (Some(ma), Some(mb), Some(p)) = (overlaps.mr(a), overlaps.mr(b), overlaps.pair(a, b))
    else {
        return;
    };
    let ca = theme.tones.blue.fg;
    let cb = theme.tones.purple.fg;
    let mut open = None;
    board::section(ui, &theme, tl("Pair picked"), p.level().label(), |ui| {
        for (m, color) in [(ma, ca), (mb, cb)] {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect.expand2(Vec2::new(6.0, 0.0)),
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            ui.painter().rect_filled(
                Rect::from_min_size(
                    egui::pos2(rect.left(), rect.top() + 4.0),
                    Vec2::new(3.0, 34.0),
                ),
                CornerRadius::same(2),
                color,
            );
            let w = board::paint_tag(
                ui,
                &theme,
                egui::pos2(rect.left() + 10.0, rect.top() + 12.0),
                &format!("!{}", m.id),
                board::id_tone(&theme),
                true,
            );
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 18.0 + w, rect.top() + 12.0),
                Align2::LEFT_CENTER,
                &m.title,
                fonts::medium(theme.text_size - 0.5),
                theme.palette.text,
                rect.width() - w - 18.0,
            );
            let effort = m
                .effort
                .map(|e| super::tones::effort(e, &theme.tones).0)
                .unwrap_or_default();
            let state = if m.approved {
                tl("approved")
            } else {
                tl("not approved")
            };
            let detail = format!("{} · {effort} · {state}", split_person(&m.author).0);
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 10.0, rect.top() + 31.0),
                Align2::LEFT_CENTER,
                &detail,
                FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
                rect.width() - 10.0,
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some(m.id.clone());
            }
        }
    });
    board::section(
        ui,
        &theme,
        tl("Where they meet"),
        &tn(p.files.len() as u64, "{n} file", "{n} files"),
        |ui| {
            // How many files of each kind, before the detail.
            let count = |level: Level| p.files.iter().filter(|f| f.1 == level).count();
            let summary: Vec<(Level, String)> = [
                (
                    Level::SameLines,
                    tn(
                        count(Level::SameLines) as u64,
                        "{n} on the same lines",
                        "{n} on the same lines",
                    ),
                ),
                (
                    Level::Near,
                    tn(
                        count(Level::Near) as u64,
                        "{n} on close lines",
                        "{n} on close lines",
                    ),
                ),
                (
                    Level::SameFile,
                    tn(
                        count(Level::SameFile) as u64,
                        "{n} only in the same file",
                        "{n} only in the same file",
                    ),
                ),
            ]
            .into_iter()
            .filter(|(level, _)| count(*level) > 0)
            .collect();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for (level, text) in &summary {
                    board::chip(ui, &theme, text, level.tone(&theme));
                }
            });
            ui.add_space(4.0);
            board::caption(
                ui,
                &theme,
                tl("The files where they change the most lines in common first."),
            );
            ui.add_space(10.0);
            // The files, the largest overlap first.
            let mut files: Vec<Meeting> = p
                .files
                .iter()
                .filter_map(|(path, level)| {
                    let shared = overlaps.file(path)?;
                    let hunks = |id: &str| {
                        shared
                            .mrs
                            .iter()
                            .find(|m| m.0 == id)
                            .map(|m| m.1.clone())
                            .unwrap_or_default()
                    };
                    let (ha, hb) = (hunks(a), hunks(b));
                    let both = common_lines(&ha, &hb);
                    Some((path, *level, ha, hb, both))
                })
                .collect();
            let size = |r: &[(u32, u32)]| r.iter().map(|(s, e)| e - s + 1).sum::<u32>();
            files.sort_by(|x, y| {
                x.1.cmp(&y.1)
                    .then(size(&y.4).cmp(&size(&x.4)))
                    .then(x.0.cmp(y.0))
            });
            for (path, level, hunks_a, hunks_b, both) in files.iter().take(4) {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
                let galley = ui.painter().layout_no_wrap(
                    level.label().to_string(),
                    fonts::medium(theme.text_size - 2.5),
                    level.tone(&theme).fg,
                );
                let tag_w = galley.size().x + 12.0;
                board::paint_tag(
                    ui,
                    &theme,
                    egui::pos2(line.right() - tag_w, line.center().y),
                    level.label(),
                    level.tone(&theme),
                    false,
                );
                path_text(
                    ui,
                    &theme,
                    egui::pos2(line.left(), line.center().y),
                    path,
                    line.width() - tag_w - 10.0,
                );
                ui.add_space(6.0);
                file_lanes(ui, &theme, (a, hunks_a, ca), (b, hunks_b, cb), both);
                let caption = if both.is_empty() {
                    if *level == Level::Near {
                        tl("A few lines apart: git may stop.").to_string()
                    } else {
                        tl("Far apart: it most likely merges on its own.").to_string()
                    }
                } else {
                    let ranges = both
                        .iter()
                        .take(3)
                        .map(|(s, e)| {
                            if s == e {
                                tf("line {n}", &[("n", &s.to_string())])
                            } else {
                                tf(
                                    "lines {from}–{to}",
                                    &[("from", &s.to_string()), ("to", &e.to_string())],
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let more = if both.len() > 3 {
                        format!(
                            " {}",
                            tf("and {n} more", &[("n", &(both.len() - 3).to_string())])
                        )
                    } else {
                        String::new()
                    };
                    format!(
                        "{}{more}",
                        tf("Changed by both: {ranges}", &[("ranges", &ranges)])
                    )
                };
                ui.add_space(4.0);
                board::caption(ui, &theme, &caption);
                ui.add_space(12.0);
            }
            if files.len() > 4 {
                ui.label(muted(
                    &theme,
                    tf("and {n} more", &[("n", &(files.len() - 4).to_string())]),
                ));
            }
        },
    );
    // Which one first: approved and mergeable, then the smaller.
    let score = |m: &Mr| (!(m.approved && m.mergeable), !m.approved, m.lines);
    let (first, second) = if score(ma) <= score(mb) {
        (ma, mb)
    } else {
        (mb, ma)
    };
    board::section(ui, &theme, tl("Suggestion"), "", |ui| {
        let why = if first.approved && !second.approved {
            tl("already approved")
        } else if first.lines < second.lines {
            tl("the smaller one")
        } else {
            tl("the closest to merging")
        };
        ui.add(
            egui::Label::new(egui::RichText::new(tf(
                "Merge !{first} first: {why}. !{second} will then need a rebase on its target, where the files meet.",
                &[("first", &first.id), ("why", why), ("second", &second.id)],
            )).size(theme.text_size - 0.5))
            .wrap(),
        );
    });
    let mut diff = false;
    let mut message = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                diff = primary_button(ui, &theme, tl("See the changes")).clicked();
                message = button(
                    ui,
                    &theme,
                    Some(super::icons::Icon::Copy),
                    tl("Copy a message"),
                    false,
                )
                .clicked();
            });
        });
    if message {
        let files = p
            .files
            .iter()
            .map(|(f, l)| format!("- `{f}` ({})", l.label()))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!(
            "{}\n{files}\n{}",
            tf(
                "!{a} and !{b} change the same files:",
                &[("a", &ma.id), ("b", &mb.id)]
            ),
            tf(
                "Suggestion: merge !{first} first, then rebase !{second}.",
                &[("first", &first.id), ("second", &second.id)],
            ),
        );
        ui.ctx().copy_text(board::Links::of(&g.app).apply(&text));
        g.app.flash(tl("Message copied"), false);
    }
    if diff {
        g.open_mr(&first.id);
        g.mr_tab = super::mr_page::Tab::Changes;
        return;
    }
    if let Some(id) = open {
        g.show_mr(&id);
    }
}

/// A file two MRs share: its path, how they meet, the hunks of each, and the
/// lines both change.
type Meeting<'a> = (
    &'a String,
    Level,
    Vec<(u32, u32)>,
    Vec<(u32, u32)>,
    Vec<(u32, u32)>,
);

/// The lines (first, last) both hunk lists change, merged and in order.
fn common_lines(a: &[(u32, u32)], b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut both: Vec<(u32, u32)> = a
        .iter()
        .flat_map(|x| {
            b.iter().filter_map(move |y| {
                let (s, e) = (x.0.max(y.0), x.1.min(y.1));
                (s <= e).then_some((s, e))
            })
        })
        .collect();
    both.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (s, e) in both {
        match merged.last_mut() {
            Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// The part of a file the two MRs change: one lane per MR, its id on the
/// left in its color and its hunks as bars; the lines both change shaded red
/// across the two lanes; the first and last line under it.
fn file_lanes(
    ui: &mut Ui,
    theme: &Theme,
    (a_id, a, ca): (&str, &[(u32, u32)], Color32),
    (b_id, b, cb): (&str, &[(u32, u32)], Color32),
    both: &[(u32, u32)],
) {
    let all: Vec<&(u32, u32)> = a.iter().chain(b).collect();
    let lo = all.iter().map(|h| h.0).min().unwrap_or(1);
    let hi = all.iter().map(|h| h.1).max().unwrap_or(1).max(lo);
    let width = ui.available_width();
    let id_font = egui::FontId::monospace(theme.text_size - 2.5);
    let label_w = [a_id, b_id]
        .iter()
        .map(|id| {
            ui.painter()
                .layout_no_wrap(format!("!{id}"), id_font.clone(), theme.palette.text)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        + 12.0;
    let lane_h = 14.0;
    let gap = 6.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, lane_h * 2.0 + gap + 12.0), Sense::hover());
    let track = Rect::from_min_max(egui::pos2(rect.left() + label_w, rect.top()), rect.max);
    ui.painter().rect_filled(
        track,
        CornerRadius::same(theme.radii.medium),
        theme.palette.input,
    );
    let inner = track.shrink2(Vec2::new(8.0, 6.0));
    // Zoomed on the changes; a single line still gets some width.
    let (from, to) = if hi == lo {
        (lo as f32 - 3.0, hi as f32 + 2.0)
    } else {
        let pad = ((hi - lo) as f32 * 0.06).max(1.0);
        ((lo as f32 - 1.0 - pad).max(0.0), hi as f32 + pad)
    };
    let x_of =
        |line: f32| inner.left() + inner.width() * ((line - from) / (to - from)).clamp(0.0, 1.0);
    let span = |s: u32, e: u32| {
        let x0 = x_of(s as f32 - 1.0);
        (x0, x_of(e as f32).max(x0 + 4.0))
    };
    let top_lane = inner.top();
    let bottom_lane = inner.bottom() - lane_h;
    // The lines both change, behind the bars, over the two lanes.
    let red = theme.tones.red.fg;
    for (s, e) in both {
        let (x0, x1) = span(*s, *e);
        let band = Rect::from_min_max(
            egui::pos2(x0 - 2.0, inner.top() - 3.0),
            egui::pos2(x1 + 2.0, inner.bottom() + 3.0),
        );
        ui.painter()
            .rect_filled(band, CornerRadius::same(3), red.gamma_multiply(0.22));
        ui.painter().rect_stroke(
            band,
            CornerRadius::same(3),
            Stroke::new(1.5, red),
            egui::StrokeKind::Inside,
        );
    }
    for (id, hunks, lane_top, color) in [(a_id, a, top_lane, ca), (b_id, b, bottom_lane, cb)] {
        board::paint_text(
            ui,
            egui::pos2(rect.left(), lane_top + lane_h / 2.0),
            Align2::LEFT_CENTER,
            &format!("!{id}"),
            id_font.clone(),
            color,
            label_w - 6.0,
        );
        for (s, e) in hunks {
            let (x0, x1) = span(*s, *e);
            ui.painter().rect_filled(
                Rect::from_min_max(egui::pos2(x0, lane_top), egui::pos2(x1, lane_top + lane_h)),
                CornerRadius::same(3),
                color,
            );
        }
    }
    let hover = format!("!{a_id}: {}\n!{b_id}: {}", ranges_text(a), ranges_text(b));
    ui.interact(
        track,
        ui.scope_id()
            .with(("lanes", a_id, b_id, lo, hi, a.len(), b.len())),
        Sense::hover(),
    )
    .on_hover_text(hover);
    // The line numbers: both ends, or the one line.
    let (axis, _) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::hover());
    let font = egui::FontId::monospace(theme.text_size - 3.5);
    let axis_left = axis.left() + label_w + 8.0;
    let axis_right = axis.right() - 8.0;
    if hi == lo {
        board::paint_text(
            ui,
            egui::pos2(x_of(lo as f32 - 0.5), axis.center().y),
            Align2::CENTER_CENTER,
            &tf("line {n}", &[("n", &lo.to_string())]),
            font,
            theme.palette.text_muted,
            axis.width() / 2.0,
        );
    } else {
        board::paint_text(
            ui,
            egui::pos2(axis_left, axis.center().y),
            Align2::LEFT_CENTER,
            &tf("line {n}", &[("n", &lo.to_string())]),
            font.clone(),
            theme.palette.text_muted,
            (axis_right - axis_left) / 2.0,
        );
        board::paint_text(
            ui,
            egui::pos2(axis_right, axis.center().y),
            Align2::RIGHT_CENTER,
            &tf("line {n}", &[("n", &hi.to_string())]),
            font,
            theme.palette.text_muted,
            (axis_right - axis_left) / 2.0,
        );
    }
}

/// "lines 1–5, 12" for a tooltip.
fn ranges_text(hunks: &[(u32, u32)]) -> String {
    hunks
        .iter()
        .map(|(s, e)| {
            if s == e {
                s.to_string()
            } else {
                format!("{s}–{e}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The overlaps as Markdown, to paste.
fn report(overlaps: &Overlaps) -> String {
    let mut out = format!("### {}\n\n", tl("Overlaps between open MRs"));
    if overlaps.pairs.is_empty() {
        out.push_str(&format!(
            "{}\n",
            tl("No two open MRs change the same file.")
        ));
    }
    for p in &overlaps.pairs {
        let files = p
            .files
            .iter()
            .map(|(f, l)| format!("`{f}` ({})", l.label()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("- !{} × !{} · {files}\n", p.a, p.b));
    }
    out
}

/// The rail's badge: the pairs of open MRs changing the same files (only those
/// with one of mine when the username is known), in the tone of the worst: the
/// same lines, close lines, the same file.
pub fn badge(g: &GuiApp) -> Option<Badge> {
    let overlaps = analyze(&g.app, g.overlaps.lockfiles, g.overlaps.drafts);
    let known = me(&g.app).is_some();
    let pairs: Vec<Level> = overlaps
        .pairs
        .iter()
        .filter(|p| !known || mine(&g.app, &overlaps, p))
        .map(Pair::level)
        .collect();
    let worst = pairs.iter().min()?;
    let mut hint = if known {
        tn(
            pairs.len() as u64,
            "{n} pair of open MRs, one of them yours, change the same files",
            "{n} pairs of open MRs, one of them yours, change the same files",
        )
    } else {
        tn(
            pairs.len() as u64,
            "{n} pair of open MRs change the same files",
            "{n} pairs of open MRs change the same files",
        )
    };
    for level in [Level::SameLines, Level::Near, Level::SameFile] {
        let n = pairs.iter().filter(|l| **l == level).count();
        if n > 0 {
            hint.push('\n');
            hint.push_str(&rail_badges::listed(level.label(), n.to_string()));
        }
    }
    Some(Badge {
        n: pairs.len(),
        tone: worst.tone(&g.theme),
        hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DiffStats, MrData, TrackedMr};

    fn mr(id: &str, files: Vec<ChangedFile>) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
        mr.data = MrData {
            title: format!("MR {id}"),
            author: format!("Dev {id} (@dev{id})"),
            state: GitlabMrState::Opened,
            diff_stats: Some(DiffStats {
                files_changed: files.len() as u32,
                additions: 10,
                commits_count: 1,
                files,
                files_listed: true,
                ..DiffStats::default()
            }),
            ..MrData::default()
        };
        mr.mergeability = MergeabilityStatus::Mergeable;
        mr
    }

    fn file(path: &str, hunks: &[(u32, u32)]) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            hunks: hunks.to_vec(),
            ..ChangedFile::default()
        }
    }

    #[test]
    fn pairs_by_how_they_meet() {
        let mut app = crate::app::tests::test_app();
        app.mrs = vec![
            mr(
                "1",
                vec![
                    file("src/a.rs", &[(10, 20)]),
                    file("src/b.rs", &[(1, 5)]),
                    file("Cargo.lock", &[(1, 9)]),
                ],
            ),
            mr(
                "2",
                vec![file("src/a.rs", &[(15, 30)]), file("Cargo.lock", &[(1, 9)])],
            ),
            mr(
                "3",
                vec![
                    file("src/b.rs", &[(11, 12)]),
                    file("src/a.rs", &[(200, 210)]),
                ],
            ),
        ];
        let o = analyze(&app, false, false);
        assert_eq!(o.mrs.len(), 3);
        // The lock file is left out.
        assert!(o.files.iter().all(|f| f.path != "Cargo.lock"));
        assert_eq!(o.pair("1", "2").unwrap().level(), Level::SameLines);
        // b.rs lines 5 and 11 are close; a.rs far apart.
        assert_eq!(o.pair("1", "3").unwrap().level(), Level::Near);
        assert_eq!(o.pair("2", "3").unwrap().level(), Level::SameFile);
        assert_eq!(o.pairs[0].level(), Level::SameLines);
        // With the lock files, !1 and !2 share one more file.
        let with = analyze(&app, true, false);
        assert_eq!(with.pair("1", "2").unwrap().files.len(), 2);
    }

    #[test]
    fn common_lines_are_merged() {
        assert_eq!(common_lines(&[(1, 10)], &[(5, 20), (8, 12)]), vec![(5, 10)]);
        assert_eq!(
            common_lines(&[(1, 3), (10, 12)], &[(3, 10)]),
            vec![(3, 3), (10, 10)]
        );
        assert!(common_lines(&[(1, 3)], &[(5, 9)]).is_empty());
    }

    #[test]
    fn folders_two_levels_deep() {
        assert_eq!(folder("src/api/graphql/schema.rs"), "src/api/");
        assert_eq!(folder("Cargo.toml"), "/");
        assert_eq!(folder("config/default.toml"), "config/");
    }
}
