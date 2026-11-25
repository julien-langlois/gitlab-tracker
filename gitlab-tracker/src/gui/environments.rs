//! The environments page: the tracked branches from feature to production,
//! each with the environment deployed from it, its last deployment and its
//! latest pipeline; what each branch has that the next one lacks; every gap
//! between two branches; two weeks of deployments; and the pair picked, with
//! what is ready to go and a way into Promote.

use std::collections::HashMap;

use bevel_theme::{fonts, Theme};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, GAP};
use super::promote::{self, Row, Stage};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::branch_flow::BranchRole;
use crate::gitlab_detail::{BranchPipeline, DeploymentRecord, ProjectOverview};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use gitlab_tracker_core::MergeabilityStatus;

mod cards;
mod deployments;
mod matrix;
mod model;
mod side_panel;

use cards::*;
use deployments::*;
use matrix::*;
pub(in crate::gui) use model::Model;
use model::*;
pub use side_panel::*;

/// The environments page's choices.
#[derive(Debug, Clone)]
pub struct State {
    /// The pair shown in the inspector: from, to.
    pub pair: Option<(String, String)>,
    /// Index in [`PERIODS`].
    pub period: usize,
    pub failed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pair: None,
            period: 1,
            failed: true,
        }
    }
}

const PERIODS: [(i64, &str); 3] = [(7, "7 d"), (14, "14 d"), (30, "30 d")];

/// Waiting this long on a branch is late.
const LATE_DAYS: f64 = 7.0;

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let app = &g.app;
    let model = g
        .memo
        .environments
        .get(&app.project_id, overview.is_some(), || {
            model(app, overview.as_ref())
        });
    let pair = model.pair(g.environments.pair.as_ref());
    let now = Utc::now();
    board::screen(
        g,
        ui,
        sizes,
        "environments",
        |g, ui| nav(g, ui, &model, overview.as_ref()),
        |g, ui| inspector(g, ui, &model, pair, overview.as_ref(), now),
        |g, ui| main(g, ui, &model, pair, overview.as_ref(), now),
    );
}

fn nav(g: &mut GuiApp, ui: &mut Ui, model: &Model, overview: Option<&ProjectOverview>) {
    let theme = g.theme;
    let envs: usize = model.branches.iter().map(|b| b.environments.len()).sum();
    let subtitle = format!(
        "{} · {}",
        tn(
            model.branches.len() as u64,
            "{n} tracked branch",
            "{n} tracked branches"
        ),
        tn(envs as u64, "{n} environment", "{n} environments")
    );
    board::nav_header(ui, &theme, tl("Environments"), &subtitle);
    let mut picked = None;
    board::nav_group(ui, &theme, tl("Branches"), |ui| {
        for (i, b) in model.branches.iter().enumerate() {
            let env = b.environments.first().cloned().unwrap_or_default();
            let selected = model
                .pair(g.environments.pair.as_ref())
                .is_some_and(|(from, _)| from == i);
            if board::nav_item(
                ui,
                &theme,
                &b.name,
                &env,
                Some(role_color(b.role, &theme)),
                selected,
            )
            .tip(tl("Show what this branch has that the next one lacks"))
            .clicked()
            {
                let to = if i + 1 < model.branches.len() {
                    i + 1
                } else {
                    i.saturating_sub(1)
                };
                if to != i {
                    picked = Some((b.name.clone(), model.branches[to].name.clone()));
                }
            }
        }
    });
    if let Some(pair) = picked {
        g.environments.pair = Some(pair);
    }
    board::nav_group(ui, &theme, tl("Period"), |ui| {
        let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
        if let Some(i) = board::segmented(ui, &theme, "envs_period", &labels, g.environments.period)
        {
            g.environments.period = i;
        }
    });
    board::nav_group(ui, &theme, "", |ui| {
        if board::nav_switch(ui, &theme, g.environments.failed, tl("Failed deployments")) {
            g.environments.failed = !g.environments.failed;
        }
    });
    let note = if overview.is_none() {
        tl("Reading the deployments and the branches' pipelines from GitLab…")
    } else if overview.is_some_and(|o| o.deployments.is_empty()) {
        tl("GitLab lists no deployment for this project: the branches show without their environments.")
    } else {
        tl("A click on a gap picks the pair of branches; Promote has the details and the commands.")
    };
    board::nav_note(ui, &theme, note);
}

// ── Main area ────────────────────────────────────────────────────────────────

fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    model: &Model,
    pair: Option<(usize, usize)>,
    overview: Option<&ProjectOverview>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    let mut copy = false;
    board::heading(
        ui,
        &theme,
        tl("Environments"),
        tl("From the feature branches to production: what is where, and what waits"),
        |ui| {
            copy = button(
                ui,
                &theme,
                Some(super::icons::Icon::Copy),
                tl("Copy the state"),
                false,
            )
            .clicked();
        },
    );
    if copy {
        ui.ctx()
            .copy_text(board::Links::of(&g.app).apply(&state_text(model, now)));
        g.app.flash(tl("State of the environments copied"), false);
    }
    ui.add_space(14.0);
    if model.branches.is_empty() {
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("No tracked branch: add some in the project's settings."),
            );
        });
        return;
    }
    let freezes = super::freeze::of(&g.app, overview);
    // The end of the flow (production) shows frozen while a freeze holds.
    let frozen = match freezes.status(now) {
        crate::freeze::Status::Frozen { until, .. } => Some(until),
        _ => None,
    };
    // The card below has the details: the banner leads nowhere here.
    if super::freeze::banner(ui, &theme, &freezes, now, None).is_some() {
        ui.add_space(GAP);
    }
    let mut picked = None;
    flow_strip(ui, &theme, model, pair, now, frozen, &mut picked);
    ui.add_space(GAP);
    super::freeze::card(ui, &theme, &freezes, now);
    let days = PERIODS[g.environments.period.min(PERIODS.len() - 1)].0;
    let failed = g.environments.failed;
    board::split(
        ui,
        0.42,
        280.0,
        320.0,
        |ui| matrix_card(ui, &theme, model, pair, now, &mut picked),
        |ui| deployments_card(ui, &theme, model, overview, days, failed, now),
    );
    ui.add_space(GAP);
    let mut open = None;
    waiting_card(ui, &theme, model, now, &mut open, &mut picked);
    if let Some((from, to)) = picked {
        g.environments.pair = Some((
            model.branches[from].name.clone(),
            model.branches[to].name.clone(),
        ));
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

#[cfg(test)]
mod tests;
