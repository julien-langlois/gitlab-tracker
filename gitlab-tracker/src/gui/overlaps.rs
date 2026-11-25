//! The overlaps page: the open MRs that change the same files — on the same
//! lines, close to each other, or only in the same file —, read from the files
//! and hunks each MR's changes already carry (no fetch of its own); a matrix of
//! the pairs, the files most changed of late, and the pair picked with where
//! the two MRs meet and which one to merge first.

use std::collections::{BTreeMap, HashMap};

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button, split_person};
use super::GuiApp;
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{ChangedFile, Effort, GitlabMrState};
use gitlab_tracker_core::MergeabilityStatus;

/// The folder of a path, two levels deep: `src/api/`.
use super::hotspots::folder;

mod cards;
mod model;
mod pairs;
mod side_panel;

pub(in crate::gui) use cards::*;
pub use model::*;
use pairs::*;
pub use side_panel::*;

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

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let key = (g.overlaps.lockfiles, g.overlaps.drafts);
    let overlaps = g
        .memo
        .overlaps
        .get(&g.app.project_id, key, || analyze(&g.app, key.0, key.1));
    let hot = g
        .memo
        .hot
        .get(&g.app.project_id, (), || hot_spots(&g.app, now));
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
    let Some(me) = app.config.gitlab_username.as_deref() else {
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
    let my_pairs = overlaps.my_pairs;
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
    pair_band(g, ui, overlaps, pair);
    let mut picked: Option<(String, String)> = None;
    let mut picked_right: Option<(String, String)> = None;
    let mut open = None;
    let view = g.overlaps.view;
    let folder_filter = g.overlaps.folder.clone();
    let my_login = me(&g.app);
    let app = &g.app;
    let spots_memo = &g.memo.spots;
    let (spots_by_file, lockfiles) = (g.overlaps.spots_by_file, g.overlaps.lockfiles);
    let spot = g.overlaps.spot.clone();
    let mut spot_click = None;
    // The hot spots and the bottlenecks, then the matrix and the list.
    board::split(
        ui,
        0.34,
        260.0,
        420.0,
        |ui| hot_card(ui, &theme, hot),
        |ui| {
            spot_click = super::hotspots::card(
                ui,
                &theme,
                app,
                spots_memo,
                spots_by_file,
                lockfiles,
                spot.as_deref(),
            );
        },
    );
    ui.add_space(GAP);
    board::split(
        ui,
        0.34,
        290.0,
        340.0,
        |ui| matrix_card(ui, &theme, overlaps, pair, &mut picked),
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
    match spot_click {
        Some(super::hotspots::Click::Pick(path)) => {
            g.overlaps.spot = (g.overlaps.spot.as_deref() != Some(path.as_str())).then_some(path);
        }
        Some(super::hotspots::Click::ByFile(on)) => {
            g.overlaps.spots_by_file = on;
            g.overlaps.spot = None;
        }
        Some(super::hotspots::Click::Mr(id)) => open = Some(id),
        Some(super::hotspots::Click::Table(path, ids)) => g.focus_on_ids(path, ids),
        None => {}
    }
    if let Some(p) = picked.or(picked_right) {
        g.overlaps.pair = Some(p);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

#[cfg(test)]
mod tests;
