//! The Releases page: what shipped, and what the next release will hold.
//!
//! The releases are the project's GitLab releases, else its tags named like a
//! version ([`ProjectOverview::releases`]). What each one holds is read from
//! git: the commits between it and the one before (`v1.2.0..v1.3.0`), each
//! tied to its MR by its merge commit (`See merge request !N`), its SHA or its
//! `cherry picked from` trailer, the MRs the app does not track too. What
//! waits is the same range from the latest release to the integration branch.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevel_theme::{Theme, Tone};
use chrono::{DateTime, Duration, Local, Utc};
use egui::{Align2, FontId, Ui};

use super::board::{self, Figure, GAP};
use super::details::Fetch;
use super::icons::Icon;
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button, semibold};
use super::GuiApp;
use crate::app::App;
use crate::branch_flow::BranchRole;
use crate::changelog::ApiCommit;
use crate::gitlab_detail::{ProjectOverview, ReleaseRecord};
use crate::i18n::{tf, tl, tn};
use crate::models::GitlabMrState;

mod cards;
mod contents;
mod model;
mod notes;
mod side_panel;

use cards::*;
pub use contents::*;
pub use model::*;
use notes::*;
use side_panel::*;

/// The releases whose content is read, newest first.
const READ_RELEASES: usize = 8;
/// The window of the cadence, in days.
const CADENCE_DAYS: i64 = 90;
/// Rows of the unreleased list before "N more".
const PENDING_ROWS: usize = 14;
/// Rows of the history.
const HISTORY_ROWS: usize = 12;

/// A range's commits, newest first, and whether there were more.
type RangeCommits = (Vec<ApiCommit>, bool);

/// The page's state: the release picked, the ranges read.
#[derive(Default)]
pub struct State {
    selected: Option<String>,
    /// The project the ranges belong to.
    project: String,
    /// The commits of each range read, and when.
    ranges: HashMap<String, (Instant, Fetch<RangeCommits>)>,
    /// Newer reads of ranges already shown; a failed one is dropped.
    next: HashMap<String, Fetch<RangeCommits>>,
}

// ── What an MR is ───────────────────────────────────────────────────────────

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let ctx = ui.ctx().clone();
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_RELEASE=v2.3.0`
    // picks that release, for a screenshot.
    #[cfg(debug_assertions)]
    if g.releases.selected.is_none() {
        if let Ok(tag) = std::env::var("GITLAB_TRACKER_GUI_RELEASE") {
            g.releases.selected = Some(tag);
        }
    }
    let m = model(g, &ctx, now);
    board::screen(
        g,
        ui,
        sizes,
        "releases",
        |g, ui| nav(g, ui, &m, now),
        |g, ui| inspector(g, ui, &m, now),
        |g, ui| main(g, ui, &m, now),
    );
}

fn local_date(at: DateTime<Utc>) -> String {
    crate::utils::day_text(at.with_timezone(&Local).date_naive())
}

fn nav(g: &mut GuiApp, ui: &mut Ui, m: &Model, now: DateTime<Utc>) {
    let theme = g.theme;
    board::nav_header(
        ui,
        &theme,
        tl("Releases"),
        &tn(
            m.recent as u64,
            "{n} release in 90 days",
            "{n} releases in 90 days",
        ),
    );
    if !m.releases.is_empty() {
        let mut picked = None;
        board::nav_group(ui, &theme, tl("Releases"), |ui| {
            for s in m.releases.iter().take(HISTORY_ROWS) {
                let when =
                    s.at.map(|at| board::ago(at, now))
                        .unwrap_or_else(|| board::dash().to_string());
                let selected = g.releases.selected.as_deref() == Some(s.record.tag.as_str());
                if board::nav_item(ui, &theme, &s.record.tag, &when, None, selected).clicked() {
                    picked = Some(s.record.tag.clone());
                }
            }
        });
        if let Some(tag) = picked {
            g.releases.selected =
                (g.releases.selected.as_deref() != Some(tag.as_str())).then_some(tag);
        }
    }
    board::nav_note(
        ui,
        &theme,
        &tf(
            "What a release holds is read from git: the commits since the release before, each tied to its MR. Unreleased: merged into {branch} since the latest release.",
            &[("branch", &m.integration)],
        ),
    );
    if m.denied {
        board::nav_note(
            ui,
            &theme,
            tl("The token cannot read the GitLab releases: the version tags stand in. Give it Release › Read for the releases' names and notes."),
        );
    }
}

fn main(g: &mut GuiApp, ui: &mut Ui, m: &Model, now: DateTime<Utc>) {
    let theme = g.theme;
    let pending: Option<&Contents> = m.pending.as_ref().and_then(Read::ready);
    let draft_title = m
        .next
        .as_ref()
        .map(|(v, _)| v.clone())
        .unwrap_or_else(|| tl("Unreleased").to_string());
    let draft = pending
        .filter(|c| !c.mrs.is_empty())
        .map(|c| notes(&draft_title, &c.mrs, g.app.config.release_note_tickets));
    let cliff = super::changelog::config(g, ui.ctx());
    let mut copy = false;
    let mut changelog = false;
    board::heading(
        ui,
        &theme,
        tl("Releases"),
        tl("What shipped, and what the next release will hold"),
        |ui| {
            if draft.is_some() {
                copy = primary_button(ui, &theme, tl("Copy the release notes")).clicked();
            }
            if cliff.is_some() {
                changelog = button(
                    ui,
                    &theme,
                    Some(Icon::List),
                    tl("Generate changelog"),
                    false,
                )
                .tip(tl(
                    "The changelog since the last release tag, with the project's git-cliff config",
                ))
                .clicked();
            }
        },
    );
    if let (true, Some(text)) = (copy, &draft) {
        ui.ctx().copy_text(board::Links::of(&g.app).apply(text));
        g.app.flash(tl("Release notes copied"), false);
    }
    if let (true, Some(config)) = (changelog, cliff) {
        let version = m.next.as_ref().map(|(v, _)| v.as_str()).unwrap_or_default();
        super::changelog::open(g, ui.ctx(), config, version);
    }
    ui.add_space(14.0);
    if !m.loaded {
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(ui, &theme, tl("Reading the releases…"));
        });
        return;
    }
    if m.releases.is_empty() {
        board::card(ui, &theme, tl("No release yet"), "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("No GitLab release, and no tag named like a version (v1.2.0, 3.8.0). The page fills in with the first one."),
            );
        });
        return;
    }
    figures(ui, &theme, m, pending, now);

    let mut open = None;
    let mut show = None;
    let mut picked = None;
    let mut copied = None;
    let app = &g.app;
    board::split(
        ui,
        0.58,
        360.0,
        280.0,
        |ui| unreleased_card(ui, &theme, app, m, now, &mut open, &mut show),
        |ui| copied = notes_card(ui, &theme, m, draft.as_deref()),
    );
    if let Some(text) = copied {
        ui.ctx().copy_text(board::Links::of(&g.app).apply(&text));
        g.app.flash(tl("Release notes copied"), false);
    }
    ui.add_space(GAP);
    history_card(
        ui,
        &theme,
        m,
        g.releases.selected.as_deref(),
        now,
        &mut picked,
    );
    if let Some(tag) = picked {
        g.releases.selected = (g.releases.selected.as_deref() != Some(tag.as_str())).then_some(tag);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
    if let Some(ids) = show {
        g.focus_on_ids(tl("Unreleased"), ids);
    }
}

#[cfg(test)]
mod tests;
