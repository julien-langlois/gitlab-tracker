//! A tracked branch red since an MR: its pipelines failed run after run since
//! the last green one, and the MRs that arrived on it in between, the likely
//! cause. Read from the branch's recent pipelines (the same call as its
//! latest) and the MRs' merges and arrivals: nothing of its own.

use super::*;
use crate::gitlab_detail::{BranchPipeline, PipelineRun};
use crate::gui::toasts::{Kind, Toasts};
use crate::models::TrackedMr;
use std::collections::{HashMap, HashSet};

/// Red pipelines in a row from which a branch gets a banner: one alone may
/// be a flaky job.
const STREAK: usize = 2;

/// A tracked branch whose last pipelines failed.
#[derive(Debug, Clone, PartialEq)]
pub struct RedBranch {
    pub branch: String,
    /// The failed pipelines in a row, newest first.
    pub failed: usize,
    /// The first of them: since when it is red.
    pub first_red: PipelineRun,
    /// The last pipeline that passed before, if one of those read did.
    pub last_green: Option<PipelineRun>,
    /// The jobs failing in the latest pipeline.
    pub jobs: Vec<String>,
    /// The MRs that reached the branch between the last green pipeline and
    /// the first red one: (id, title).
    pub arrived: Vec<(String, String)>,
}

/// The tracked branches red for [`STREAK`] pipelines or more, in the flow's
/// order.
pub fn red_branches(app: &App, pipelines: &[BranchPipeline]) -> Vec<RedBranch> {
    let flow = app.branch_flow();
    flow.in_order()
        .iter()
        .filter_map(|b| pipelines.iter().find(|p| p.branch == b.name))
        .filter_map(|p| red_branch(app, p))
        .collect()
}

/// The finished pipelines of `p` (a running one says nothing yet), newest
/// first, and how many of the latest failed in a row.
fn streak(p: &BranchPipeline) -> (Vec<&PipelineRun>, usize) {
    let finished: Vec<&PipelineRun> = p
        .history
        .iter()
        .filter(|r| matches!(r.status, PipelineState::Success | PipelineState::Failed))
        .collect();
    let failed = finished
        .iter()
        .take_while(|r| r.status == PipelineState::Failed)
        .count();
    (finished, failed)
}

fn red_branch(app: &App, p: &BranchPipeline) -> Option<RedBranch> {
    let (finished, failed) = streak(p);
    if failed < STREAK {
        return None;
    }
    let first_red = finished[failed - 1].clone();
    let last_green = finished.get(failed).map(|r| (*r).clone());
    let arrived = last_green
        .as_ref()
        .map(|green| arrived_between(app, &p.branch, green, &first_red))
        .unwrap_or_default();
    Some(RedBranch {
        branch: p.branch.clone(),
        failed,
        first_red,
        last_green,
        jobs: p.failed_jobs.clone(),
        arrived,
    })
}

/// The MRs that reached `branch` after the `green` pipeline and by the `red`
/// one: merged into it (the red pipeline ran on its merge commit, or it was
/// merged in between), else recorded arriving on it in between.
fn arrived_between(
    app: &App,
    branch: &str,
    green: &PipelineRun,
    red: &PipelineRun,
) -> Vec<(String, String)> {
    let (Some(from), Some(to)) = (
        board::parse_time(&green.created_at),
        board::parse_time(&red.created_at),
    ) else {
        return Vec::new();
    };
    let within = |at: DateTime<Utc>| at > from && at <= to;
    let reached = |mr: &TrackedMr| {
        if mr.state != GitlabMrState::Merged {
            return false;
        }
        if mr.target_branch == branch {
            mr.sha.as_deref() == Some(red.sha.as_str())
                || board::time_of(mr.merged_at.as_deref()).is_some_and(within)
        } else {
            app.branch_arrivals
                .get(&(mr.id.clone(), branch.to_string()))
                .is_some_and(|at| within(*at))
        }
    };
    app.mrs
        .iter()
        .filter(|mr| reached(mr))
        .map(|mr| (mr.id.clone(), mr.title.clone()))
        .collect()
}

/// What the bell was told of: the projects looked at, and the first red
/// pipeline of each red branch, by project and branch.
#[derive(Debug, Default)]
pub struct Seen {
    looked: HashSet<String>,
    red: HashMap<(String, String), u64>,
}

/// The id of a red branch's notification: it opens the CI page, not an MR.
pub fn branch_id(branch: &str) -> String {
    format!("branch:{branch}")
}

/// Adds a notification for each tracked branch that turned red since the last
/// look (a new first red pipeline). The first look only records what is there:
/// a branch red before the app opened shows in the CI page's banner, not in the
/// bell. Cheap while nothing turns red: the MRs are read only for a new one.
pub fn watch(
    toasts: &mut Toasts,
    seen: &mut Seen,
    app: &App,
    pipelines: &[BranchPipeline],
    theme: &Theme,
    now: f64,
) {
    let first_look = seen.looked.insert(app.project_id.clone());
    // The branches green again leave the list: red again later is news.
    seen.red.retain(|(project, branch), _| {
        *project != app.project_id
            || pipelines
                .iter()
                .find(|p| p.branch == *branch)
                .is_some_and(|p| streak(p).1 >= STREAK)
    });
    let tracked = app.branch_flow();
    for p in pipelines {
        if tracked.get(&p.branch).is_none() {
            continue;
        }
        let (finished, failed) = streak(p);
        if failed < STREAK {
            continue;
        }
        let first_red = finished[failed - 1].id;
        let key = (app.project_id.clone(), p.branch.clone());
        if seen.red.insert(key, first_red) == Some(first_red) || first_look {
            continue;
        }
        let Some(red) = red_branch(app, p) else {
            continue;
        };
        let (text, title) = bell_text(&red);
        toasts.push(
            &branch_id(&red.branch),
            &title,
            Kind::BranchRed,
            text,
            theme.tones.red,
            now,
        );
    }
}

/// A red branch's notification: what happened ("staging turned red since
/// !214 arrived"), and the line under it (that MR's title, else the streak).
pub fn bell_text(red: &RedBranch) -> (String, String) {
    match red.arrived.as_slice() {
        [(id, title)] => (
            tf(
                "{branch} turned red since !{id} arrived",
                &[("branch", &red.branch), ("id", id)],
            ),
            title.clone(),
        ),
        _ => (
            tf("{branch} turned red", &[("branch", &red.branch)]),
            tn(
                red.failed as u64,
                "{n} pipeline failed in a row",
                "{n} pipelines failed in a row",
            ),
        ),
    }
}

/// What a click on the banner asks for.
pub(in crate::gui) enum RedClick {
    Mr(String),
    Url(String),
}

/// The banner of a red branch: since when and since what, the failing jobs,
/// the last green pipeline; a button to the MR that arrived (one), else to
/// the first red pipeline.
pub(in crate::gui) fn red_banner(ui: &mut Ui, theme: &Theme, red: &RedBranch) -> Option<RedClick> {
    let tone = theme.tones.red;
    let since = board::parse_time(&red.first_red.created_at)
        .map(crate::utils::moment_text)
        .unwrap_or_default();
    let title = match red.arrived.as_slice() {
        [(id, _)] => tf(
            "{branch} is red since !{id} arrived ({since})",
            &[("branch", &red.branch), ("id", id), ("since", &since)],
        ),
        [] => tf(
            "{branch} is red since {since}",
            &[("branch", &red.branch), ("since", &since)],
        ),
        many => tf(
            "{branch} is red since {mrs} arrived ({since})",
            &[
                ("branch", &red.branch),
                (
                    "mrs",
                    &many
                        .iter()
                        .map(|(id, _)| format!("!{id}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                ("since", &since),
            ],
        ),
    };
    let mut detail = vec![tn(
        red.failed as u64,
        "{n} pipeline failed in a row",
        "{n} pipelines failed in a row",
    )];
    if !red.jobs.is_empty() {
        detail.push(red.jobs.join(", "));
    }
    detail.push(
        match red
            .last_green
            .as_ref()
            .and_then(|g| board::parse_time(&g.created_at))
        {
            Some(at) => tf(
                "last green: {when}",
                &[("when", &crate::utils::moment_text(at))],
            ),
            None => tf(
                "no green pipeline in the last {n}",
                &[("n", &red.failed.to_string())],
            ),
        },
    );
    if let [(_, title)] = red.arrived.as_slice() {
        detail.push(title.clone());
    }
    let mut clicked = None;
    egui::Frame::new()
        .fill(tone.bg)
        .stroke(Stroke::new(1.0, tone.fg.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                super::super::icons::paint(
                    ui.painter(),
                    icon,
                    super::super::icons::Icon::Pulse,
                    tone.fg,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        egui::RichText::new(&title)
                            .font(fonts::semibold(theme.text_size))
                            .color(tone.fg),
                    );
                    ui.label(muted(theme, detail.join(" · ")));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // The one MR that arrived, else the first red pipeline.
                    if let [(id, _)] = red.arrived.as_slice() {
                        let label = tf("Open !{id}", &[("id", id)]);
                        if button(ui, theme, None, &label, false).clicked() {
                            clicked = Some(RedClick::Mr(id.clone()));
                        }
                    } else if !red.first_red.web_url.is_empty()
                        && button(
                            ui,
                            theme,
                            Some(super::super::icons::Icon::ExternalLink),
                            tl("Open the first red pipeline"),
                            false,
                        )
                        .clicked()
                    {
                        clicked = Some(RedClick::Url(red.first_red.web_url.clone()));
                    }
                });
            });
        });
    clicked
}
