//! The CI page: how often the pipelines of the tracked MRs pass and how long
//! they take, each job's last runs with its failure rate and the flaky ones (a
//! failure, then a pass on the same commit), the MRs the CI holds back, the
//! latest pipeline of each tracked branch, and the job picked.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use bevel_theme::{fonts, Theme};
use chrono::{DateTime, Duration, Local, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::jump::{Entry, Jumps};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};

mod analysis;
mod budget;
mod charts;
mod jobs;
mod side_panel;

pub use analysis::*;
use budget::*;
use charts::*;
use jobs::*;
pub use side_panel::*;

/// The views of the side panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Overview,
    Failing,
    Flaky,
}

/// The CI page's choices.
#[derive(Debug, Clone)]
pub struct State {
    pub view: View,
    /// Index in [`PERIODS`].
    pub period: usize,
    pub hidden_stages: HashSet<String>,
    pub selected: Option<String>,
    /// The shortcuts to the page's sections.
    pub jumps: Jumps,
}

impl Default for State {
    fn default() -> Self {
        Self {
            view: View::default(),
            period: 1,
            hidden_stages: HashSet::new(),
            selected: None,
            jumps: Jumps::default(),
        }
    }
}

const PERIODS: [(i64, &str); 3] = [(7, "7 d"), (14, "14 d"), (30, "30 d")];

/// The runs a job's strip shows.
const STRIP: usize = 20;

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.ci.period.min(PERIODS.len() - 1)].0;
    let ci = g
        .memo
        .ci
        .get(&g.app.project_id, days, || analyze(&g.app, now, days));
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let selected =
        g.ci.selected
            .clone()
            .filter(|s| ci.jobs.iter().any(|j| &j.name == s))
            .or_else(|| ci.jobs.first().map(|j| j.name.clone()));
    let mut entries = Vec::new();
    let schedules = overview.as_ref().map_or(0, |o| o.schedules.len());
    if schedules > 0 {
        entries.push(Entry::new("schedules", tl("Pipeline schedules")).count(schedules));
    }
    let jobs = ci
        .jobs
        .iter()
        .filter(|j| shows(j, g.ci.view) && !g.ci.hidden_stages.contains(&j.stage))
        .count();
    entries.extend([
        Entry::new("jobs", tl("Jobs")).count(jobs),
        Entry::new("budget", tl("CI budget")),
        Entry::new("durations", tl("Pipeline durations")),
        Entry::new("stages", tl("Failures by stage")),
    ]);
    board::screen_with_jumps(
        g,
        ui,
        sizes,
        "ci",
        Some((|g| &mut g.ci.jumps, entries)),
        |g, ui| nav(g, ui, &ci, days),
        |g, ui| inspector(g, ui, &ci, overview.as_ref(), selected.as_deref(), now),
        |g, ui, jumps| {
            main(
                g,
                ui,
                jumps,
                &ci,
                overview.as_ref(),
                selected.as_deref(),
                now,
            )
        },
    );
}

fn shows(job: &Job, view: View) -> bool {
    match view {
        View::Overview => true,
        View::Failing => job.failures() > 0,
        View::Flaky => job.is_flaky(),
    }
}

fn nav(g: &mut GuiApp, ui: &mut Ui, ci: &Ci, days: i64) {
    let theme = g.theme;
    let subtitle = format!(
        "{} · {}",
        tn(ci.pipelines as u64, "{n} pipeline", "{n} pipelines"),
        tn(days as u64, "{n} day", "{n} days")
    );
    board::nav_header(ui, &theme, tl("CI / Pipelines"), &subtitle);
    board::nav_group(ui, &theme, tl("Views"), |ui| {
        for (view, label) in [
            (View::Overview, tl("Every job")),
            (View::Failing, tl("Failing jobs")),
            (View::Flaky, tl("Flaky jobs")),
        ] {
            let n = ci
                .jobs
                .iter()
                .filter(|j| shows(j, view))
                .count()
                .to_string();
            if board::nav_item(ui, &theme, label, &n, None, g.ci.view == view).clicked() {
                g.ci.view = view;
            }
        }
    });
    board::nav_group(ui, &theme, tl("Period"), |ui| {
        let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
        if let Some(i) = board::segmented(ui, &theme, "ci_period", &labels, g.ci.period) {
            g.ci.period = i;
        }
    });
    let stages: BTreeSet<&str> = ci.jobs.iter().map(|j| j.stage.as_str()).collect();
    if !stages.is_empty() {
        board::nav_group(ui, &theme, tl("Stages"), |ui| {
            for stage in stages {
                let n = ci.jobs.iter().filter(|j| j.stage == stage).count();
                let on = !g.ci.hidden_stages.contains(stage);
                if board::nav_check(ui, &theme, stage, &n.to_string(), theme.palette.accent, on)
                    .clicked()
                {
                    if on {
                        g.ci.hidden_stages.insert(stage.to_string());
                    } else {
                        g.ci.hidden_stages.remove(stage);
                    }
                }
            }
        });
    }
    board::nav_note(
        ui,
        &theme,
        tl("Flaky: a job that fails, then passes on the same commit without a change. From the last pipelines of each tracked MR."),
    );
}

// ── Main area ────────────────────────────────────────────────────────────────

#[allow(
    clippy::too_many_arguments,
    reason = "the page's data, and its sections"
)]
fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    jumps: &mut Jumps,
    ci: &Ci,
    overview: Option<&crate::gitlab_detail::ProjectOverview>,
    selected: Option<&str>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    let mut show_blocked = false;
    board::heading(
        ui,
        &theme,
        tl("CI / Pipelines"),
        tl("What breaks, what slows down, and what it holds back"),
        |ui| {
            if !ci.blocked.is_empty() {
                show_blocked =
                    button(ui, &theme, None, tl("Show the blocked MRs"), false).clicked();
            }
        },
    );
    if show_blocked {
        g.next_focus = Some(Some(Focus {
            key: String::new(),
            label: tl("Held back by the CI").to_string(),
            ids: ci.blocked.iter().map(|b| b.id.clone()).collect(),
        }));
        g.open_page(super::Page::MergeRequests);
    }
    ui.add_space(14.0);
    // A schedule failing run after run: nobody watches the nightly runs.
    let schedules = overview.map(|o| o.schedules.as_slice()).unwrap_or(&[]);
    let project = g
        .app
        .project_name
        .clone()
        .unwrap_or_else(|| g.app.project_id.clone());
    if super::schedules::worst(schedules).is_some() {
        if let Some(url) = super::schedules::banner(ui, &theme, schedules, &project) {
            board::open_url(ui, &url);
        }
        ui.add_space(GAP);
    }
    let t = &theme.tones;
    let points = |now: Option<f64>, before: Option<f64>| -> (String, Option<Color32>) {
        match (now, before) {
            (Some(a), Some(b)) => {
                let delta = (a - b) * 100.0;
                if delta.abs() < 0.5 {
                    (tl("as before").to_string(), None)
                } else if delta > 0.0 {
                    (
                        tf(
                            "+{n} pts on the period before",
                            &[("n", &format!("{delta:.0}"))],
                        ),
                        Some(t.green.fg),
                    )
                } else {
                    (
                        tf(
                            "{n} pts on the period before",
                            &[("n", &format!("{delta:.0}"))],
                        ),
                        Some(t.red.fg),
                    )
                }
            }
            _ => (tl("nothing to compare with").to_string(), None),
        }
    };
    let (success_caption, success_color) = points(ci.success, ci.success_before);
    let (median_caption, median_color) = match (ci.median, ci.median_before, ci.p90) {
        (Some(a), Some(b), _) if (a - b).abs() >= 10.0 => {
            let text = board::seconds_text((a - b).abs());
            if a > b {
                (
                    tf("{d} slower than before", &[("d", &text)]),
                    Some(t.amber.fg),
                )
            } else {
                (
                    tf("{d} faster than before", &[("d", &text)]),
                    Some(t.green.fg),
                )
            }
        }
        (Some(_), _, Some(p90)) => (tf("P90 {d}", &[("d", &board::seconds_text(p90))]), None),
        _ => (tl("no finished pipeline").to_string(), None),
    };
    let flaky_jobs = ci.jobs.iter().filter(|j| j.is_flaky()).count();
    let flaky_runs: usize = ci.jobs.iter().map(|j| j.flaky_runs).sum();
    let oldest = ci.blocked.iter().filter_map(|b| b.since).min();
    let figures = [
        Figure {
            label: tl("Pipelines passing").into(),
            value: ci
                .success
                .map_or_else(|| board::dash().to_string(), board::percent),
            value_color: None,
            caption: success_caption,
            caption_color: success_color,
        },
        Figure {
            label: tl("Median duration").into(),
            value: ci
                .median
                .map_or_else(|| board::dash().to_string(), board::seconds_text),
            value_color: None,
            caption: median_caption,
            caption_color: median_color,
        },
        Figure {
            label: tl("Flaky jobs").into(),
            value: flaky_jobs.to_string(),
            value_color: (flaky_jobs > 0).then_some(t.amber.fg),
            caption: tn(
                flaky_runs as u64,
                "{n} failure a rerun fixed",
                "{n} failures a rerun fixed",
            ),
            caption_color: None,
        },
        Figure {
            label: tl("MRs held back by the CI").into(),
            value: ci.blocked.len().to_string(),
            value_color: (!ci.blocked.is_empty()).then_some(t.red.fg),
            caption: match oldest {
                Some(o) => tf(
                    "the oldest for {time}",
                    &[("time", &board::duration_text(board::days_between(o, now)))],
                ),
                None => tl("none").to_string(),
            },
            caption_color: None,
        },
    ];
    board::figures(ui, &theme, &figures);
    if !schedules.is_empty() {
        let page =
            super::releases::project_url(&g.app).map(|p| format!("{p}/-/pipeline_schedules"));
        let url = jumps.section(ui, "schedules", |ui| {
            super::schedules::card(ui, &theme, schedules, page.as_deref(), now)
        });
        if let Some(url) = url {
            board::open_url(ui, &url);
        }
        ui.add_space(GAP);
    }

    let jobs: Vec<&Job> = ci
        .jobs
        .iter()
        .filter(|j| shows(j, g.ci.view))
        .filter(|j| !g.ci.hidden_stages.contains(&j.stage))
        .collect();
    let mut picked = None;
    jumps.section(ui, "jobs", |ui| {
        board::card(
            ui,
            &theme,
            tl("Jobs"),
            tl("the most failing first"),
            0.0,
            |ui| {
                if jobs.is_empty() {
                    board::empty(ui, &theme, tl("No job in this view for the period."));
                    return;
                }
                jobs_table(ui, &theme, &jobs, selected, &mut picked);
            },
        );
    });
    if let Some(name) = picked {
        g.ci.selected = Some(name);
    }
    ui.add_space(GAP);
    jumps.section(ui, "budget", |ui| budget_card(ui, &theme, ci));
    ui.add_space(GAP);
    // Side by side, or one under the other: each its own section.
    let cell = std::cell::RefCell::new(jumps);
    board::split(
        ui,
        0.58,
        320.0,
        240.0,
        |ui| {
            cell.borrow_mut()
                .section(ui, "durations", |ui| durations_card(ui, &theme, ci));
        },
        |ui| {
            cell.borrow_mut()
                .section(ui, "stages", |ui| stages_card(ui, &theme, ci));
        },
    );
}

#[cfg(test)]
mod tests;
