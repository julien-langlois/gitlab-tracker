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
use super::widgets::{button, muted, primary_button};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::branch_flow::BranchRole;
use crate::gitlab_detail::{BranchPipeline, DeploymentRecord, ProjectOverview};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use gitlab_tracker_core::MergeabilityStatus;

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

/// A tracked branch, as the page shows it.
struct Branch {
    name: String,
    alias: String,
    role: BranchRole,
    /// The environments deployed from it, the latest deployment first.
    environments: Vec<String>,
    last: Option<DeploymentRecord>,
    pipeline: Option<BranchPipeline>,
}

/// An MR on a branch and not on another: id, title, since when on the first.
type Waiting = (String, String, Option<DateTime<Utc>>);

/// What `app` has on each pair of branches, and the branches in flow order.
struct Model {
    branches: Vec<Branch>,
    /// (from, to) → the MRs on `from` not (all) on `to`, with since when.
    gaps: HashMap<(usize, usize), Vec<Waiting>>,
    open: Vec<OpenMr>,
}

struct OpenMr {
    blocked: bool,
    ready: bool,
}

fn role_color(role: BranchRole, theme: &Theme) -> Color32 {
    let t = &theme.tones;
    match role {
        BranchRole::Feature => t.gray.fg,
        BranchRole::Integration => t.blue.fg,
        BranchRole::Staging => t.purple.fg,
        BranchRole::Production => t.green.fg,
    }
}

fn model(app: &App, overview: Option<&ProjectOverview>) -> Model {
    let flow = app.branch_flow();
    let ordered = flow.in_order();
    let branches: Vec<Branch> = ordered
        .iter()
        .map(|b| {
            let deployments: Vec<&DeploymentRecord> = overview
                .map(|o| {
                    o.deployments
                        .iter()
                        .filter(|d| d.reference == b.name)
                        .collect()
                })
                .unwrap_or_default();
            let mut environments: Vec<String> = Vec::new();
            for d in &deployments {
                if !environments.contains(&d.environment) {
                    environments.push(d.environment.clone());
                }
            }
            Branch {
                name: b.name.clone(),
                alias: b.alias.clone(),
                role: b.role,
                environments,
                last: deployments
                    .iter()
                    .find(|d| d.status == "success")
                    .map(|d| (*d).clone()),
                pipeline: overview
                    .and_then(|o| o.pipelines.iter().find(|p| p.branch == b.name).cloned()),
            }
        })
        .collect();
    let mut gaps = HashMap::new();
    for (i, from) in branches.iter().enumerate() {
        for (j, to) in branches.iter().enumerate() {
            if i == j {
                continue;
            }
            let list: Vec<Waiting> = promote::rows(app, &from.name, &to.name)
                .into_iter()
                .filter(|r| r.stage == Stage::Ready)
                .map(|r| (r.id, r.title, r.on_from_since))
                .collect();
            gaps.insert((i, j), list);
        }
    }
    let open = app
        .mrs
        .iter()
        .filter(|mr| mr.state == GitlabMrState::Opened && !mr.title.is_empty())
        .map(|mr| {
            let failed = mr
                .pipelines
                .first()
                .is_some_and(|p| p.status == PipelineState::Failed);
            let blocked = failed
                || matches!(
                    mr.mergeability,
                    MergeabilityStatus::Conflict
                        | MergeabilityStatus::RequestedChanges
                        | MergeabilityStatus::CiMustPass
                );
            OpenMr {
                blocked,
                ready: !blocked
                    && mr.mergeability == MergeabilityStatus::Mergeable
                    && !mr.approved_by.is_empty(),
            }
        })
        .collect();
    Model {
        branches,
        gaps,
        open,
    }
}

impl Model {
    fn gap(&self, from: usize, to: usize) -> &[Waiting] {
        self.gaps.get(&(from, to)).map_or(&[], Vec::as_slice)
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.branches.iter().position(|b| b.name == name)
    }

    /// The pair to show: the one picked, else the consecutive pair with the
    /// most waiting, else the last hop.
    fn pair(&self, picked: Option<&(String, String)>) -> Option<(usize, usize)> {
        if let Some((from, to)) = picked {
            if let (Some(i), Some(j)) = (self.index(from), self.index(to)) {
                return Some((i, j));
            }
        }
        let n = self.branches.len();
        if n < 2 {
            return None;
        }
        (0..n - 1)
            .max_by_key(|i| (self.gap(*i, i + 1).len(), *i))
            .map(|i| (i, i + 1))
    }
}

fn oldest(list: &[Waiting]) -> Option<DateTime<Utc>> {
    list.iter().filter_map(|r| r.2).min()
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let model = model(&g.app, overview.as_ref());
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
            .on_hover_text(tl("Show what this branch has that the next one lacks"))
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
    let mut picked = None;
    flow_strip(ui, &theme, model, pair, now, &mut picked);
    ui.add_space(GAP);
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
        g.show_mr(&id);
    }
}

/// The branches as cards, in flow order, after a card for the open MRs; under
/// each, what it has that the next one lacks.
fn flow_strip(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    pair: Option<(usize, usize)>,
    now: DateTime<Utc>,
    picked: &mut Option<(usize, usize)>,
) {
    let n = model.branches.len() + 1;
    let arrow = 26.0;
    let width = ui.available_width();
    let card_w = ((width - arrow * (n - 1) as f32) / n as f32).floor();
    // Narrow: two per line, without the arrows.
    let per_line = if card_w >= 150.0 {
        n
    } else {
        2.max((width / 220.0).floor() as usize)
    };
    let card_w = if per_line == n {
        card_w
    } else {
        ((width - GAP * (per_line - 1) as f32) / per_line as f32).floor()
    };
    let height = 176.0;
    let mut index = 0;
    while index < n {
        let line = (n - index).min(per_line);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        for k in 0..line {
            let i = index + k;
            let gap = if per_line == n { arrow } else { GAP };
            let left = rect.left() + k as f32 * (card_w + gap);
            let card = Rect::from_min_size(egui::pos2(left, rect.top()), Vec2::new(card_w, height));
            if i == 0 {
                open_card(ui, theme, card, model);
            } else {
                branch_card(ui, theme, card, model, i - 1, pair, now, picked);
            }
            if per_line == n && k + 1 < line {
                let cx = card.right() + arrow / 2.0;
                let cy = card.top() + 60.0;
                let color = theme.palette.text_muted;
                ui.painter().line_segment(
                    [egui::pos2(cx - 7.0, cy), egui::pos2(cx + 6.0, cy)],
                    Stroke::new(1.5, color),
                );
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(cx + 1.0, cy - 5.0),
                        egui::pos2(cx + 6.0, cy),
                        egui::pos2(cx + 1.0, cy + 5.0),
                    ],
                    Stroke::new(1.5, color),
                ));
            }
        }
        index += line;
        if index < n {
            ui.add_space(GAP);
        }
    }
}

fn card_ground(ui: &Ui, theme: &Theme, rect: Rect, accent: Color32, selected: bool) {
    let radius = CornerRadius::same(theme.radii.large);
    ui.painter().rect(
        rect,
        radius,
        theme.palette.panel,
        Stroke::new(
            1.0,
            if selected {
                theme.palette.accent.gamma_multiply(0.8)
            } else {
                theme.palette.border_soft
            },
        ),
        egui::StrokeKind::Inside,
    );
    let strip = Rect::from_min_size(
        rect.min + Vec2::new(1.0, 1.0),
        Vec2::new(rect.width() - 2.0, 3.0),
    );
    ui.painter().rect_filled(
        strip,
        CornerRadius {
            nw: theme.radii.large,
            ne: theme.radii.large,
            sw: 0,
            se: 0,
        },
        accent,
    );
}

/// A caption and its value on one line of a card: the value keeps its room,
/// the caption takes what is left.
fn key_value(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    y: f32,
    key: &str,
    value: &str,
    color: Option<Color32>,
) {
    let font = FontId::proportional(theme.text_size - 1.5);
    let value_w = board::paint_text(
        ui,
        egui::pos2(rect.right(), y),
        Align2::RIGHT_CENTER,
        value,
        font.clone(),
        color.unwrap_or(theme.palette.text_secondary),
        rect.width() * 0.7,
    );
    board::paint_text(
        ui,
        egui::pos2(rect.left(), y),
        Align2::LEFT_CENTER,
        key,
        font,
        theme.palette.text_muted,
        rect.width() - value_w - 8.0,
    );
}

fn open_card(ui: &Ui, theme: &Theme, rect: Rect, model: &Model) {
    card_ground(ui, theme, rect, theme.tones.gray.fg, false);
    let inner = rect.shrink2(Vec2::new(14.0, 14.0));
    let mut y = inner.top() + 10.0;
    board::paint_text(
        ui,
        egui::pos2(inner.left(), y),
        Align2::LEFT_CENTER,
        tl("Open MRs"),
        fonts::semibold(theme.text_size),
        theme.palette.text,
        inner.width(),
    );
    y += 30.0;
    let n = model.open.len();
    let w = board::paint_text(
        ui,
        egui::pos2(inner.left(), y),
        Align2::LEFT_CENTER,
        &n.to_string(),
        fonts::semibold(26.0),
        theme.palette.text,
        inner.width(),
    );
    board::paint_text(
        ui,
        egui::pos2(inner.left() + w + 8.0, y + 3.0),
        Align2::LEFT_CENTER,
        tl("in review or work"),
        FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
        inner.width() - w - 8.0,
    );
    y += 30.0;
    let ready = model.open.iter().filter(|m| m.ready).count();
    let blocked = model.open.iter().filter(|m| m.blocked).count();
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Ready to merge"),
        &ready.to_string(),
        (ready > 0).then_some(theme.tones.green.fg),
    );
    y += 22.0;
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Blocked"),
        &blocked.to_string(),
        (blocked > 0).then_some(theme.tones.red.fg),
    );
    y += 22.0;
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("In progress"),
        &(n - ready - blocked).to_string(),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn branch_card(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    model: &Model,
    i: usize,
    pair: Option<(usize, usize)>,
    now: DateTime<Utc>,
    picked: &mut Option<(usize, usize)>,
) {
    let b = &model.branches[i];
    let selected = pair.is_some_and(|(from, _)| from == i);
    card_ground(ui, theme, rect, role_color(b.role, theme), selected);
    let inner = rect.shrink2(Vec2::new(14.0, 14.0));
    let mut y = inner.top() + 10.0;
    // The name, the environment on the right.
    let env_w = b.environments.first().map_or(0.0, |env| {
        let galley = ui.painter().layout_no_wrap(
            env.clone(),
            fonts::medium(theme.text_size - 2.5),
            theme.tones.blue.fg,
        );
        let w = (galley.size().x + 12.0).min(inner.width() * 0.5);
        board::paint_tag(
            ui,
            theme,
            egui::pos2(inner.right() - w, y),
            env,
            theme.tones.blue,
            false,
        );
        w + 8.0
    });
    board::paint_text(
        ui,
        egui::pos2(inner.left(), y),
        Align2::LEFT_CENTER,
        &b.name,
        egui::FontId::monospace(theme.text_size),
        theme.palette.text,
        inner.width() - env_w,
    );
    y += 30.0;
    // The state of what is deployed.
    let (state, tone) = match (&b.last, &b.pipeline) {
        (None, _) if b.environments.is_empty() => (tl("No environment"), theme.tones.gray),
        (None, _) => (tl("Never deployed"), theme.tones.gray),
        (Some(d), Some(p))
            if !p.sha.is_empty()
                && !d.sha.is_empty()
                && !p.sha.starts_with(&d.sha)
                && !d.sha.starts_with(&p.sha) =>
        {
            (tl("Newer commits not deployed"), theme.tones.amber)
        }
        (Some(_), _) => (tl("Up to date"), theme.tones.green),
    };
    board::paint_tag_within(
        ui,
        theme,
        egui::pos2(inner.left(), y),
        state,
        tone,
        false,
        inner.width(),
    );
    y += 28.0;
    let deployed = b
        .last
        .as_ref()
        .and_then(|d| board::parse_time(&d.created_at));
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Deployed"),
        &deployed.map_or_else(|| board::dash().to_string(), |d| board::ago(d, now)),
        None,
    );
    y += 22.0;
    let (pipeline, color) = match &b.pipeline {
        Some(p) => {
            let (label, tone) = super::tones::pipeline(p.status, &theme.tones);
            (
                format!("#{} · {}", p.id, label.to_lowercase()),
                Some(tone.fg),
            )
        }
        None => (board::dash().to_string(), None),
    };
    key_value(ui, theme, inner, y, tl("Pipeline"), &pipeline, color);
    y += 22.0;
    let sha = b
        .last
        .as_ref()
        .map(|d| d.sha.chars().take(8).collect::<String>());
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Commit"),
        &sha.unwrap_or_else(|| board::dash().to_string()),
        None,
    );
    // At the bottom: what waits for the next branch.
    let foot = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - 24.0), inner.max);
    if i + 1 < model.branches.len() {
        let list = model.gap(i, i + 1);
        let late = oldest(list).is_some_and(|o| board::days_between(o, now) >= LATE_DAYS);
        let (text, tone) = if list.is_empty() {
            (
                tf(
                    "In sync with {branch}",
                    &[("branch", &model.branches[i + 1].alias)],
                ),
                theme.tones.green,
            )
        } else {
            let wait = oldest(list).map(|o| board::duration_text(board::days_between(o, now)));
            let mut text = tn(list.len() as u64, "{n} to promote", "{n} to promote");
            if let Some(w) = wait {
                text = format!("{text} · {w}");
            }
            (
                text,
                if late {
                    theme.tones.red
                } else {
                    theme.tones.amber
                },
            )
        };
        let response = ui
            .interact(
                foot.expand2(Vec2::new(6.0, 4.0)),
                ui.scope_id().with(("gap", i)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tf(
                "What {from} has that {to} lacks",
                &[("from", &b.name), ("to", &model.branches[i + 1].name)],
            ));
        let fill = if response.hovered() {
            tone.bg.gamma_multiply(1.6)
        } else {
            tone.bg
        };
        ui.painter()
            .rect_filled(foot, CornerRadius::same(theme.radii.medium), fill);
        board::paint_text(
            ui,
            foot.center(),
            Align2::CENTER_CENTER,
            &text,
            fonts::medium(theme.text_size - 2.0),
            tone.fg,
            foot.width() - 12.0,
        );
        if response.clicked() {
            *picked = Some((i, i + 1));
        }
    } else {
        board::paint_text(
            ui,
            foot.center(),
            Align2::CENTER_CENTER,
            tl("End of the flow"),
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_muted,
            foot.width(),
        );
    }
}

/// Every pair of branches: the MRs on the row's branch the column's lacks.
fn matrix_card(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    pair: Option<(usize, usize)>,
    now: DateTime<Utc>,
    picked: &mut Option<(usize, usize)>,
) {
    board::card(
        ui,
        theme,
        tl("Gaps between branches"),
        tl("on the row, not on the column"),
        0.0,
        |ui| {
            let n = model.branches.len();
            let label_w = 90.0;
            let gap = 6.0;
            let cell_w = ((ui.available_width() - label_w - gap * n as f32) / n as f32).max(40.0);
            let small = egui::FontId::monospace(theme.text_size - 2.0);
            let (head, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
            for (j, b) in model.branches.iter().enumerate() {
                let cx = head.left() + label_w + gap + j as f32 * (cell_w + gap) + cell_w / 2.0;
                board::paint_text(
                    ui,
                    egui::pos2(cx, head.center().y),
                    Align2::CENTER_CENTER,
                    &b.alias,
                    small.clone(),
                    theme.tones.blue.fg,
                    cell_w,
                );
            }
            ui.add_space(4.0);
            for (i, from) in model.branches.iter().enumerate() {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::hover());
                board::paint_text(
                    ui,
                    egui::pos2(line.left(), line.center().y),
                    Align2::LEFT_CENTER,
                    &from.alias,
                    small.clone(),
                    theme.tones.blue.fg,
                    label_w - 6.0,
                );
                for j in 0..n {
                    let cell = Rect::from_min_size(
                        egui::pos2(
                            line.left() + label_w + gap + j as f32 * (cell_w + gap),
                            line.top() + 2.0,
                        ),
                        Vec2::new(cell_w, 42.0),
                    );
                    if i == j {
                        ui.painter().rect_filled(
                            cell,
                            CornerRadius::same(theme.radii.medium),
                            theme.palette.raised.gamma_multiply(0.4),
                        );
                        board::paint_text(
                            ui,
                            cell.center(),
                            Align2::CENTER_CENTER,
                            board::dash(),
                            FontId::proportional(theme.text_size - 1.0),
                            theme.palette.text_muted,
                            cell_w,
                        );
                        continue;
                    }
                    let list = model.gap(i, j);
                    // Down the flow: a promotion to do; up it: a fix to bring back.
                    let up = j < i;
                    let tone = if list.is_empty() {
                        None
                    } else if up {
                        Some(theme.tones.orange)
                    } else if oldest(list).is_some_and(|o| board::days_between(o, now) >= LATE_DAYS)
                        || list.len() >= 6
                    {
                        Some(theme.tones.red)
                    } else {
                        Some(theme.tones.amber)
                    };
                    let response = ui
                        .interact(cell, ui.scope_id().with(("mx", i, j)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let (fill, fg) = match tone {
                        Some(t) => (
                            if response.hovered() {
                                t.bg.gamma_multiply(1.6)
                            } else {
                                t.bg
                            },
                            t.fg,
                        ),
                        None => (
                            theme.palette.raised.gamma_multiply(0.7),
                            theme.palette.text_muted,
                        ),
                    };
                    ui.painter()
                        .rect_filled(cell, CornerRadius::same(theme.radii.medium), fill);
                    if pair == Some((i, j)) {
                        ui.painter().rect_stroke(
                            cell,
                            CornerRadius::same(theme.radii.medium),
                            Stroke::new(2.0, theme.palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let caption = if list.is_empty() {
                        String::new()
                    } else if up {
                        tl("to bring back").to_string()
                    } else {
                        oldest(list).map_or_else(String::new, |o| {
                            board::duration_text(board::days_between(o, now))
                        })
                    };
                    let cy = if caption.is_empty() {
                        cell.center().y
                    } else {
                        cell.center().y - 6.0
                    };
                    board::paint_text(
                        ui,
                        egui::pos2(cell.center().x, cy),
                        Align2::CENTER_CENTER,
                        &list.len().to_string(),
                        fonts::semibold(theme.text_size + 1.0),
                        fg,
                        cell_w,
                    );
                    if !caption.is_empty() {
                        board::paint_text(
                            ui,
                            egui::pos2(cell.center().x, cell.center().y + 9.0),
                            Align2::CENTER_CENTER,
                            &caption,
                            FontId::proportional(theme.text_size - 3.0),
                            fg,
                            cell_w - 6.0,
                        );
                    }
                    let hover = tf(
                        "{n} on {from}, not on {to}",
                        &[
                            ("n", &list.len().to_string()),
                            ("from", &from.name),
                            ("to", &model.branches[j].name),
                        ],
                    );
                    if response.on_hover_text(hover).clicked() {
                        *picked = Some((i, j));
                    }
                }
            }
            // Fixes made on a branch further down the flow, missing upstream.
            let back: usize = (0..n)
                .flat_map(|i| (0..i).map(move |j| (i, j)))
                .map(|(i, j)| model.gap(i, j).len())
                .sum();
            if back > 0 {
                ui.add_space(8.0);
                board::caption(
                    ui,
                    theme,
                    &tn(
                        back as u64,
                        "{n} change further down the flow is missing upstream: bring it back.",
                        "{n} changes further down the flow are missing upstream: bring them back.",
                    ),
                );
            }
        },
    );
}

/// The deployments of the period, one lane per environment, dots by date.
fn deployments_card(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    overview: Option<&ProjectOverview>,
    days: i64,
    failed: bool,
    now: DateTime<Utc>,
) {
    let note = tf("last {n} days", &[("n", &days.to_string())]);
    board::card(ui, theme, tl("Deployments"), &note, 0.0, |ui| {
        let Some(overview) = overview else {
            ui.horizontal(|ui| {
                super::widgets::spinner(ui, 14.0, theme.palette.text_muted);
                ui.label(muted(theme, tl("Reading the deployments…")));
            });
            return;
        };
        let start = now - Duration::days(days);
        let lanes: Vec<(&Branch, &String)> = model
            .branches
            .iter()
            .flat_map(|b| b.environments.iter().map(move |e| (b, e)))
            .collect();
        if lanes.is_empty() {
            board::empty(
                ui,
                theme,
                tl("GitLab lists no deployment of the tracked branches."),
            );
            return;
        }
        // The names' column as wide as the longest name (two fifths of the
        // card at most), then a gap before the lanes start.
        let width = ui.available_width();
        let label_font = FontId::proportional(theme.text_size - 1.5);
        let gap = 18.0;
        let longest = lanes
            .iter()
            .map(|(b, env)| {
                ui.painter()
                    .layout_no_wrap(
                        format!("{env} · {}", b.alias),
                        label_font.clone(),
                        theme.palette.text_secondary,
                    )
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max);
        let label_w = (14.0 + longest + gap).clamp(110.0, (width * 0.4).max(110.0));
        let track_w = width - label_w - 8.0;
        for (b, env) in lanes.iter().rev() {
            let (line, _) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
            board::row_rule(ui, theme, line);
            ui.painter().circle_filled(
                egui::pos2(line.left() + 4.0, line.center().y),
                4.0,
                role_color(b.role, theme),
            );
            let name = format!("{env} · {}", b.alias);
            let galley = board::one_line(
                ui,
                &name,
                label_font.clone(),
                theme.palette.text_secondary,
                label_w - 14.0 - gap,
            );
            let cut = galley.elided;
            let name_rect = Rect::from_min_size(
                egui::pos2(line.left() + 14.0, line.center().y - galley.size().y / 2.0),
                galley.size(),
            );
            ui.painter()
                .galley(name_rect.min, galley, theme.palette.text_secondary);
            if cut {
                ui.interact(
                    name_rect,
                    ui.scope_id().with(("lane", env.as_str())),
                    Sense::hover(),
                )
                .on_hover_text(&name);
            }
            let track = Rect::from_min_max(
                egui::pos2(line.left() + label_w, line.top()),
                egui::pos2(line.left() + label_w + track_w, line.bottom()),
            );
            ui.painter().hline(
                track.x_range(),
                track.center().y,
                Stroke::new(1.0, theme.palette.border),
            );
            let lane: Vec<&DeploymentRecord> = overview
                .deployments
                .iter()
                .filter(|d| &d.environment == *env && d.reference == b.name)
                .filter(|d| board::parse_time(&d.created_at).is_some_and(|at| at >= start))
                .filter(|d| failed || d.status == "success")
                .collect();
            // Smaller dots when they crowd the lane.
            let radius = (track.width() / lane.len().max(1) as f32 / 2.6).clamp(3.0, 5.5);
            for d in lane {
                let Some(at) = board::parse_time(&d.created_at) else {
                    continue;
                };
                if at < start {
                    continue;
                }
                let ok = d.status == "success";
                if !ok && !failed {
                    continue;
                }
                let x = track.left()
                    + 6.0
                    + (track.width() - 12.0)
                        * ((at - start).num_minutes() as f32
                            / (now - start).num_minutes().max(1) as f32);
                let color = match d.status.as_str() {
                    "success" => theme.tones.green.fg,
                    "failed" => theme.tones.red.fg,
                    _ => theme.tones.amber.fg,
                };
                let center = egui::pos2(x, track.center().y);
                ui.painter()
                    .circle(center, radius, color, Stroke::new(1.5, theme.palette.panel));
                let hit = Rect::from_center_size(center, Vec2::splat(14.0));
                ui.interact(
                    hit,
                    ui.scope_id()
                        .with(("dep", env.as_str(), d.created_at.as_str())),
                    Sense::hover(),
                )
                .on_hover_text(format!(
                    "{} · {} · {}\n{} {}",
                    d.environment,
                    d.sha.chars().take(8).collect::<String>(),
                    d.status,
                    board::ago(at, now),
                    if d.user.is_empty() {
                        String::new()
                    } else {
                        tf("by {name}", &[("name", &d.user)])
                    },
                ));
            }
        }
        // The dates under the lanes: today on the right, then the others
        // where they do not run into a neighbour.
        let (axis, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
        let track_left = axis.left() + label_w + 6.0;
        let track_width = track_w - 12.0;
        let font = FontId::proportional(theme.text_size - 2.5);
        let today_w = board::paint_text(
            ui,
            egui::pos2(track_left + track_width, axis.center().y),
            Align2::RIGHT_CENTER,
            tl("today"),
            font.clone(),
            theme.palette.text_muted,
            90.0,
        );
        let limit = track_left + track_width - today_w - 10.0;
        let mut previous_right = f32::NEG_INFINITY;
        for k in 0..4 {
            let f = k as f32 / 4.0;
            let at = start + Duration::minutes(((now - start).num_minutes() as f32 * f) as i64);
            let text = at.format("%d/%m").to_string();
            let galley = board::one_line(ui, &text, font.clone(), theme.palette.text_muted, 60.0);
            let w = galley.size().x;
            let x = track_left + track_width * f;
            let left = if k == 0 { x } else { x - w / 2.0 };
            if left < previous_right + 10.0 || left + w > limit {
                continue;
            }
            ui.painter().galley(
                egui::pos2(left, axis.center().y - galley.size().y / 2.0),
                galley,
                theme.palette.text_muted,
            );
            previous_right = left + w;
        }
        ui.add_space(6.0);
        board::legend(
            ui,
            theme,
            &[
                (theme.tones.green.fg, tl("Succeeded").to_string()),
                (theme.tones.red.fg, tl("Failed").to_string()),
            ],
        );
    });
}

/// The MRs that wait the longest on a branch for the next one.
fn waiting_card(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    now: DateTime<Utc>,
    open: &mut Option<String>,
    picked: &mut Option<(usize, usize)>,
) {
    board::card(
        ui,
        theme,
        tl("Waiting the longest"),
        tl("on a branch, not on the next one"),
        0.0,
        |ui| {
            let mut list: Vec<(usize, &Waiting)> = (0..model.branches.len().saturating_sub(1))
                .flat_map(|i| model.gap(i, i + 1).iter().map(move |r| (i, r)))
                .collect();
            list.sort_by_key(|(_, r)| r.2.unwrap_or(now));
            if list.is_empty() {
                board::empty(
                    ui,
                    theme,
                    tl("Every branch has what the one before it has."),
                );
                return;
            }
            for (i, (id, title, since)) in list.into_iter().take(6) {
                let (rect, response) = board::row(ui, theme, 36.0, false);
                board::row_rule(ui, theme, rect);
                let cy = rect.center().y;
                let left = rect.left() + 8.0;
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(left, cy),
                    &format!("!{id}"),
                    board::id_tone(theme),
                    true,
                );
                let wait = since.map(|s| board::days_between(s, now));
                let late = wait.is_some_and(|w| w >= LATE_DAYS);
                let right_text = match wait {
                    Some(w) => tf(
                        "on {branch} for {time}",
                        &[
                            ("branch", &model.branches[i].alias),
                            ("time", &board::duration_text(w)),
                        ],
                    ),
                    None => tf("on {branch}", &[("branch", &model.branches[i].alias)]),
                };
                let to = &model.branches[i + 1].alias;
                let tag = tf("to {branch}", &[("branch", to)]);
                let tag_galley = ui.painter().layout_no_wrap(
                    tag.clone(),
                    fonts::medium(theme.text_size - 2.5),
                    theme.tones.amber.fg,
                );
                let tag_w = tag_galley.size().x + 12.0;
                board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.right() - 8.0 - tag_w, cy),
                    &tag,
                    if late {
                        theme.tones.red
                    } else {
                        theme.tones.amber
                    },
                    false,
                );
                let text_right = rect.right() - 8.0 - tag_w - 12.0;
                let rw = board::paint_text(
                    ui,
                    egui::pos2(text_right, cy),
                    Align2::RIGHT_CENTER,
                    &right_text,
                    FontId::proportional(theme.text_size - 1.5),
                    if late {
                        theme.tones.red.fg
                    } else {
                        theme.palette.text_secondary
                    },
                    220.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(left + w + 10.0, cy),
                    Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(theme.text_size - 0.5),
                    theme.palette.text,
                    text_right - rw - 16.0 - (left + w + 10.0),
                );
                let response =
                    response.on_hover_text(tl("Click: pick the pair · double click: show the MR"));
                if response.double_clicked() {
                    *open = Some(id.clone());
                } else if response.clicked() {
                    *picked = Some((i, i + 1));
                }
            }
        },
    );
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    model: &Model,
    pair: Option<(usize, usize)>,
    overview: Option<&ProjectOverview>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    let Some((i, j)) = pair else {
        board::inspector_empty(
            ui,
            &theme,
            tl("Track at least two branches to compare them."),
        );
        return;
    };
    let from = &model.branches[i];
    let to = &model.branches[j];
    let rows: Vec<Row> = promote::rows(&g.app, &from.name, &to.name);
    let ready: Vec<&Row> = rows.iter().filter(|r| r.stage == Stage::Ready).collect();
    let upstream = rows
        .iter()
        .filter(|r| r.stage == Stage::Upstream && r.state == GitlabMrState::Opened)
        .count();
    let partial = ready
        .iter()
        .filter(|r| r.on_to == Some(crate::models::BranchPresence::Partial))
        .count();
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
            let font = egui::FontId::monospace(theme.text_size - 0.5);
            let w = board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &from.name,
                font.clone(),
                theme.tones.blue.fg,
                line.width() / 2.0 - 16.0,
            );
            board::paint_text(
                ui,
                egui::pos2(line.left() + w + 8.0, line.center().y),
                Align2::LEFT_CENTER,
                "→",
                FontId::proportional(theme.text_size),
                theme.palette.text_muted,
                20.0,
            );
            board::paint_text(
                ui,
                egui::pos2(line.left() + w + 28.0, line.center().y),
                Align2::LEFT_CENTER,
                &to.name,
                font,
                theme.tones.blue.fg,
                line.width() - w - 28.0,
            );
            ui.add_space(6.0);
            let title = if j > i {
                tn(
                    ready.len() as u64,
                    "{n} MR ahead of {to}",
                    "{n} MRs ahead of {to}",
                )
                .replace("{to}", &to.alias)
            } else {
                tn(
                    ready.len() as u64,
                    "{n} MR to bring back to {to}",
                    "{n} MRs to bring back to {to}",
                )
                .replace("{to}", &to.alias)
            };
            ui.add(egui::Label::new(super::widgets::semibold(title, theme.text_size + 2.0)).wrap());
            let last = to
                .last
                .as_ref()
                .and_then(|d| board::parse_time(&d.created_at));
            let detail = match (to.environments.first(), last) {
                (Some(env), Some(at)) => tf(
                    "Last deployment of {env}: {when}",
                    &[("env", env), ("when", &board::ago(at, now))],
                ),
                (Some(env), None) => tf("{env} was never deployed", &[("env", env)]),
                _ => tl("No environment deployed from it").to_string(),
            };
            ui.add(egui::Label::new(muted(&theme, detail)).wrap());
        });
    super::widgets::rule(ui, &theme);
    board::section(ui, &theme, tl("Where they stand"), "", |ui| {
        let t = &theme.tones;
        board::stacked_bar(
            ui,
            &theme,
            &[
                ((ready.len() - partial) as f32, t.green.fg),
                (partial as f32, t.amber.fg),
                (upstream as f32, t.gray.fg),
            ],
            10.0,
            2.0,
        );
        ui.add_space(8.0);
        board::legend(
            ui,
            &theme,
            &[
                (
                    t.green.fg,
                    tn((ready.len() - partial) as u64, "{n} ready", "{n} ready"),
                ),
                (
                    t.amber.fg,
                    tn(partial as u64, "{n} partly there", "{n} partly there"),
                ),
                (
                    t.gray.fg,
                    tf(
                        "{n} not on {from} yet",
                        &[("n", &upstream.to_string()), ("from", &from.alias)],
                    ),
                ),
            ],
        );
    });
    let mut open = None;
    board::section(ui, &theme, tl("MRs"), tl("in merge order"), |ui| {
        if ready.is_empty() {
            ui.label(muted(
                &theme,
                tf(
                    "{to} has everything {from} has.",
                    &[("to", &to.alias), ("from", &from.alias)],
                ),
            ));
        }
        for r in ready.iter().take(12) {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect.expand2(Vec2::new(6.0, 0.0)),
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            let cy = rect.center().y;
            let w = board::paint_tag(
                ui,
                &theme,
                egui::pos2(rect.left(), cy),
                &format!("!{}", r.id),
                board::id_tone(&theme),
                true,
            );
            let since = r
                .on_from_since
                .map(|s| board::duration_text(board::days_between(s, now)));
            let note_w = since.as_ref().map_or(0.0, |s| {
                board::paint_text(
                    ui,
                    egui::pos2(rect.right(), cy),
                    Align2::RIGHT_CENTER,
                    s,
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    70.0,
                ) + 8.0
            });
            board::paint_text(
                ui,
                egui::pos2(rect.left() + w + 8.0, cy),
                Align2::LEFT_CENTER,
                &r.title,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text,
                rect.width() - w - 8.0 - note_w,
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some(r.id.clone());
            }
        }
        if ready.len() > 12 {
            ui.label(muted(
                &theme,
                tf("and {n} more", &[("n", &(ready.len() - 12).to_string())]),
            ));
        }
    });
    if let Some(env) = to.environments.first() {
        let dates: Vec<DateTime<Utc>> = overview
            .map(|o| {
                o.deployments
                    .iter()
                    .filter(|d| &d.environment == env && d.status == "success")
                    .filter_map(|d| board::parse_time(&d.created_at))
                    .collect()
            })
            .unwrap_or_default();
        let gaps: Vec<f64> = dates
            .windows(2)
            .map(|w| board::days_between(w[1], w[0]))
            .collect();
        let month = dates
            .iter()
            .filter(|d| **d >= now - Duration::days(30))
            .count();
        board::section(
            ui,
            &theme,
            &tf("Rhythm of {env}", &[("env", env)]),
            "",
            |ui| {
                board::columns(ui, 2, |ui, k| {
                    let (value, caption) = if k == 0 {
                        (
                            crate::next_step::median(gaps.clone())
                                .map_or_else(|| board::dash().to_string(), board::duration_text),
                            tl("between two deployments"),
                        )
                    } else {
                        (month.to_string(), tl("deployments in 30 days"))
                    };
                    ui.label(super::widgets::semibold(value, theme.text_size + 7.0));
                    ui.add(
                        egui::Label::new(muted(&theme, caption).size(theme.text_size - 2.0)).wrap(),
                    );
                });
            },
        );
    }
    let mut promote_it = false;
    let mut show = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                promote_it = primary_button(ui, &theme, tl("Open in Promote")).clicked();
                show = button(ui, &theme, None, tl("Show in the table"), false).clicked();
            });
        });
    if promote_it {
        g.promote = Some((from.name.clone(), to.name.clone()));
        g.open_page(super::Page::Promote);
    }
    if show {
        g.next_focus = Some(Some(Focus {
            key: String::new(),
            label: tf(
                "On {from}, not on {to}",
                &[("from", &from.name), ("to", &to.name)],
            ),
            ids: ready.iter().map(|r| r.id.clone()).collect(),
        }));
        g.open_page(super::Page::MergeRequests);
    }
    if let Some(id) = open {
        g.show_mr(&id);
    }
}

/// The state of the environments as Markdown, to paste.
fn state_text(model: &Model, now: DateTime<Utc>) -> String {
    let mut out = format!("### {}\n\n", tl("Environments"));
    for (i, b) in model.branches.iter().enumerate() {
        let env = b
            .environments
            .first()
            .map_or(String::new(), |e| format!(" ({e})"));
        let deployed = b
            .last
            .as_ref()
            .and_then(|d| board::parse_time(&d.created_at))
            .map_or(String::new(), |at| {
                format!(
                    " · {}",
                    tf("deployed {when}", &[("when", &board::ago(at, now))])
                )
            });
        out.push_str(&format!("- `{}`{env}{deployed}", b.name));
        if i + 1 < model.branches.len() {
            let list = model.gap(i, i + 1);
            if !list.is_empty() {
                let ids = list
                    .iter()
                    .map(|r| format!("!{}", r.0))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    " · {}: {ids}",
                    tf("to {branch}", &[("branch", &model.branches[i + 1].name)])
                ));
            }
        }
        out.push('\n');
    }
    out
}

/// Waiting this long on a branch already shows.
const SLOW_DAYS: f64 = 3.0;

/// The rail's badge: the environments whose last deployment failed (red), and
/// the MRs waiting to go one branch further for [`LATE_DAYS`] (orange) or
/// [`SLOW_DAYS`] (amber).
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let overview = g.details.project_ready();
    let mut failed: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    // The deployments come newest first: an environment's first is its last.
    for d in overview.map_or(&[][..], |o| o.deployments.as_slice()) {
        if seen.contains(&d.environment.as_str()) {
            continue;
        }
        seen.push(&d.environment);
        if d.status == "failed" {
            failed.push(d.environment.clone());
        }
    }
    let model = model(&g.app, overview);
    let mut waiting: Vec<(&str, f64)> = Vec::new();
    for i in 0..model.branches.len().saturating_sub(1) {
        for (id, _, since) in model.gap(i, i + 1) {
            let Some(days) = since.map(|s| board::days_between(s, now)) else {
                continue;
            };
            if days >= SLOW_DAYS && !waiting.iter().any(|w| w.0 == id) {
                waiting.push((id, days));
            }
        }
    }
    let late = waiting.iter().filter(|w| w.1 >= LATE_DAYS).count();
    let tones = &g.theme.tones;
    let tone = if !failed.is_empty() {
        tones.red
    } else if late > 0 {
        tones.orange
    } else if !waiting.is_empty() {
        tones.amber
    } else {
        return None;
    };
    let mut lines = Vec::new();
    if !failed.is_empty() {
        lines.push(rail_badges::listed(
            tn(
                failed.len() as u64,
                "{n} environment whose last deployment failed",
                "{n} environments whose last deployment failed",
            ),
            rail_badges::names(&failed, 3),
        ));
    }
    if late > 0 {
        lines.push(tn(
            late as u64,
            "{n} MR waits for a week or more to go one branch further",
            "{n} MRs wait for a week or more to go one branch further",
        ));
    }
    if waiting.len() > late {
        lines.push(tn(
            (waiting.len() - late) as u64,
            "{n} MR waits for 3 days or more to go one branch further",
            "{n} MRs wait for 3 days or more to go one branch further",
        ));
    }
    Some(Badge {
        n: failed.len() + waiting.len(),
        tone,
        hint: lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_demo_overview_follows_the_flow() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let overview = crate::demo::demo_project_overview(&app);
        let model = model(&app, Some(&overview));
        assert_eq!(model.branches.len(), app.branches.len());
        // Each tracked branch has an environment and a last deployment.
        assert!(model
            .branches
            .iter()
            .all(|b| !b.environments.is_empty() && b.last.is_some()));
        // Production last.
        assert_eq!(model.branches.last().unwrap().role, BranchRole::Production);
        assert!(model.pair(None).is_some());
    }
}
