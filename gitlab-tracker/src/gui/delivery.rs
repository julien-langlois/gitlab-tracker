//! The delivery page: how fast changes reach production and where the time
//! goes on the way. The four DORA measures (deployment frequency, lead time
//! for changes, change failure rate, time to restore), each merged MR's path
//! cut into its steps (in development, waiting for a review, in review, waiting
//! for the merge, waiting for production), the rounds a review sends back to
//! the author, and the open MRs placed by step and age against the usual time
//! to merge. Everything is read from what the tracked MRs already carry, plus
//! the project's deployments (the environments page's fetch). The weekly
//! report copies it all as Markdown.

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::jump::{Entry, Jumps};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button, split_person};
use super::GuiApp;
use crate::app::App;
use crate::gitlab_detail::{DeploymentRecord, ProjectOverview};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, TrackedMr};
use gitlab_tracker_core::MergeabilityStatus;

mod aging;
mod analysis;
mod dora;
mod model;
mod report;
mod steps;

use aging::*;
pub use analysis::*;
use dora::*;
pub use model::*;
pub use report::*;
use steps::*;

/// The periods offered, in days.
const PERIODS: [(i64, &str); 3] = [(7, "7 d"), (30, "30 d"), (90, "90 d")];

/// The delivery page's choices.
#[derive(Debug, Clone)]
pub struct State {
    /// Index in [`PERIODS`].
    pub period: usize,
    /// The MR picked, by id.
    pub selected: Option<String>,
    /// The shortcuts to the page's sections.
    pub jumps: Jumps,
    /// Each figure against the period before, of the same length.
    pub compare: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            period: 1,
            selected: None,
            jumps: Jumps::default(),
            compare: true,
        }
    }
}

// ── The steps of an MR's path ────────────────────────────────────────────────

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.delivery.period.min(PERIODS.len() - 1)].0;
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let known = overview.is_some();
    let d = g.memo.delivery.get(&g.app.project_id, (days, known), || {
        analyze(&g.app, overview.as_ref(), now, days)
    });
    let before = g.delivery.compare.then(|| {
        g.memo
            .delivery_before
            .get(&g.app.project_id, (days, known), || {
                analyze(&g.app, overview.as_ref(), now - Duration::days(days), days)
            })
    });
    let before = before.as_deref();
    let period = Period { days, now };
    let entries = vec![
        Entry::new("undos", tl("Reverts and hotfixes")).count(d.undos.len()),
        Entry::new("steps", tl("Where the time goes")),
        Entry::new("rounds", tl("Review rounds")),
        Entry::new("aging", tl("Aging work in progress")),
        Entry::new("paths", tl("Merged in the period")).count(d.merged.len()),
    ];
    board::screen_with_jumps(
        g,
        ui,
        sizes,
        "delivery",
        Some((|g| &mut g.delivery.jumps, entries)),
        |g, ui| nav(g, ui, &d, before, period),
        |g, ui| inspector(g, ui, &d, now),
        |g, ui, jumps| main(g, ui, jumps, &d, before, period, overview.as_ref()),
    );
}

/// The period shown: its length, and its end.
#[derive(Debug, Clone, Copy)]
struct Period {
    days: i64,
    now: DateTime<Utc>,
}

impl Period {
    /// "the 30 d before".
    fn before_label(self) -> String {
        tf(
            "the {period} before",
            &[("period", &tf("{n} d", &[("n", &self.days.to_string())]))],
        )
    }
}

/// "▲ from 1.2 / week · better", in green, orange, or grey when too few or
/// the same.
fn versus_note(theme: &Theme, v: Versus, before: &str, period: Period) -> (String, Color32) {
    let arrow = |up: bool| if up { "▲" } else { "▼" };
    let from = tf("from {before}", &[("before", before)]);
    match v {
        Versus::Same => (
            tf("= as {period}", &[("period", &period.before_label())]),
            theme.palette.text_muted,
        ),
        Versus::Moved { up, better } => (
            format!(
                "{} {from} · {}",
                arrow(up),
                if better { tl("better") } else { tl("worse") }
            ),
            if better {
                theme.tones.green.fg
            } else {
                theme.tones.orange.fg
            },
        ),
        Versus::TooFew { up } => (
            format!("{} {from} · {}", arrow(up), tl("too few to tell")),
            theme.palette.text_muted,
        ),
    }
}

fn nav(g: &mut GuiApp, ui: &mut Ui, d: &Delivery, before: Option<&Delivery>, period: Period) {
    let theme = g.theme;
    board::nav_header(
        ui,
        &theme,
        tl("Delivery"),
        &tn(
            d.merged.len() as u64,
            "{n} MR merged in the period",
            "{n} MRs merged in the period",
        ),
    );
    board::nav_group(ui, &theme, tl("Period"), |ui| {
        let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
        if let Some(i) = board::segmented(ui, &theme, "delivery_period", &labels, g.delivery.period)
        {
            g.delivery.period = i;
        }
    });
    board::nav_group(ui, &theme, tl("Compare with"), |ui| {
        let labels = [period.before_label(), tl("Off").to_string()];
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        if let Some(i) = board::segmented(
            ui,
            &theme,
            "delivery_compare",
            &labels,
            usize::from(!g.delivery.compare),
        ) {
            g.delivery.compare = i == 0;
        }
        if let Some(before) = before {
            let day =
                |t: DateTime<Utc>| t.with_timezone(&chrono::Local).format("%d/%m").to_string();
            let start = period.now - Duration::days(period.days);
            let earlier = start - Duration::days(period.days);
            ui.add_space(6.0);
            ui.label(
                muted(
                    &theme,
                    tf(
                        "{from} – {to} against {from_before} – {to_before}",
                        &[
                            ("from", &day(start)),
                            ("to", &day(period.now)),
                            ("from_before", &day(earlier)),
                            ("to_before", &day(start)),
                        ],
                    ),
                )
                .size(theme.text_size - 1.5),
            );
            ui.label(
                muted(
                    &theme,
                    tn(
                        before.merged.len() as u64,
                        "{n} MR merged then",
                        "{n} MRs merged then",
                    ),
                )
                .size(theme.text_size - 1.5),
            );
            ui.add_space(6.0);
            let size = theme.text_size - 1.5;
            ui.label(muted(&theme, tl("▲ ▼ the way the figure went")).size(size));
            ui.label(
                RichText::new(tl("green: better"))
                    .size(size)
                    .color(theme.tones.green.fg),
            );
            ui.label(
                RichText::new(tl("orange: worse"))
                    .size(size)
                    .color(theme.tones.orange.fg),
            );
            ui.label(
                muted(
                    &theme,
                    tf(
                        "grey: too few to tell (under {n} on either side)",
                        &[("n", &MIN_SAMPLE.to_string())],
                    ),
                )
                .size(size),
            );
        }
    });
    // The steps' medians are the "Where the time goes" card's: not repeated
    // here.
    board::nav_note(
        ui,
        &theme,
        &tf(
            "Production is {branch}, the last tracked branch. The steps come from the review timeline the discussions give; the deployments from the project's deployments to it.",
            &[("branch", &d.production)],
        ),
    );
}

fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    jumps: &mut Jumps,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
    overview: Option<&ProjectOverview>,
) {
    let now = period.now;
    let theme = g.theme;
    let mut copy = false;
    board::heading(
        ui,
        &theme,
        tl("Delivery"),
        tl("How fast changes reach production, and where the time goes"),
        |ui| {
            copy = primary_button(ui, &theme, tl("Copy the weekly report")).clicked();
        },
    );
    if copy {
        let ci = super::ci::analyze(&g.app, now, 7);
        ui.ctx()
            .copy_text(board::Links::of(&g.app).apply(&weekly_report(&g.app, overview, &ci, now)));
        g.app.flash(
            tl("Weekly report copied: paste it in a channel or a wiki page"),
            false,
        );
    }
    ui.add_space(14.0);
    dora_figures(ui, &theme, d, before, period);

    let mut picked = None;
    let selected = g.delivery.selected.as_deref();
    ui.add_space(GAP);
    jumps.section(ui, "undos", |ui| {
        undo_card(
            ui,
            &theme,
            d,
            &g.app.config.hotfix_branches,
            selected,
            &mut picked,
        );
    });
    ui.add_space(GAP);
    // Side by side, or one under the other: each its own section.
    let cell = std::cell::RefCell::new(&mut *jumps);
    board::split(
        ui,
        0.6,
        360.0,
        260.0,
        |ui| {
            cell.borrow_mut()
                .section(ui, "steps", |ui| steps_card(ui, &theme, d, before, period));
        },
        |ui| {
            cell.borrow_mut().section(ui, "rounds", |ui| {
                rounds_card(ui, &theme, d, before, period, &mut picked)
            });
        },
    );
    ui.add_space(GAP);
    jumps.section(ui, "aging", |ui| {
        aging_card(ui, &theme, d, selected, &mut picked);
    });
    ui.add_space(GAP);
    jumps.section(ui, "paths", |ui| {
        paths_card(ui, &theme, d, selected, now, &mut picked);
    });
    if let Some(id) = picked {
        g.delivery.selected = Some(id);
    }
}

#[cfg(test)]
mod tests;
