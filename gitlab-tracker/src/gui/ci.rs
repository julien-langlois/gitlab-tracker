//! The CI page: how often the pipelines of the tracked MRs pass and how long
//! they take, each job's last runs with its failure rate and the flaky ones (a
//! failure, then a pass on the same commit), the MRs the CI holds back, the
//! latest pipeline of each tracked branch, and the job picked.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use bevel_theme::{fonts, Theme};
use chrono::{DateTime, Duration, Local, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::shell::Sizes;
use super::widgets::{button, muted, primary_button};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};

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
}

impl Default for State {
    fn default() -> Self {
        Self {
            view: View::default(),
            period: 1,
            hidden_stages: HashSet::new(),
            selected: None,
        }
    }
}

const PERIODS: [(i64, &str); 3] = [(7, "7 d"), (14, "14 d"), (30, "30 d")];

/// The runs a job's strip shows.
const STRIP: usize = 20;

/// One run of a job.
#[derive(Debug, Clone)]
pub struct Run {
    pub at: DateTime<Utc>,
    pub status: String,
    pub duration: Option<f64>,
    pub sha: Option<String>,
    pub mr: String,
    /// A failure followed by a pass of the same job on the same commit.
    pub flaky: bool,
}

/// A job, across the tracked MRs' pipelines.
#[derive(Debug, Clone)]
pub struct Job {
    pub name: String,
    pub stage: String,
    /// Oldest first.
    pub runs: Vec<Run>,
    pub failure_rate: f64,
    /// The failure rate of the first half of the period, to tell a trend.
    pub rate_before: Option<f64>,
    pub median: Option<f64>,
    /// Failures followed by a pass on the same commit.
    pub flaky_runs: usize,
}

impl Job {
    fn failures(&self) -> usize {
        self.runs.iter().filter(|r| r.status == "failed").count()
    }

    fn is_flaky(&self) -> bool {
        self.flaky_runs > 0
    }
}

/// An open MR whose latest pipeline failed.
#[derive(Debug, Clone)]
pub struct Blocked {
    pub id: String,
    pub title: String,
    pub author: String,
    pub jobs: Vec<String>,
    pub since: Option<DateTime<Utc>>,
    /// Its failed jobs include a flaky one: a rerun may do.
    pub flaky: bool,
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Ci {
    pub pipelines: usize,
    pub success: Option<f64>,
    pub success_before: Option<f64>,
    pub median: Option<f64>,
    pub median_before: Option<f64>,
    pub p90: Option<f64>,
    pub jobs: Vec<Job>,
    pub blocked: Vec<Blocked>,
    /// Per day of the period, oldest first: the median duration and the count.
    pub days: Vec<(chrono::NaiveDate, Option<f64>, usize)>,
    /// Failed jobs per stage.
    pub stages: BTreeMap<String, usize>,
}

fn seconds_between(from: &str, to: &str) -> Option<f64> {
    let (a, b) = (board::parse_time(from)?, board::parse_time(to)?);
    let s = (b - a).num_seconds() as f64;
    (s > 0.0).then_some(s)
}

fn rate(failed: usize, passed: usize) -> Option<f64> {
    let total = failed + passed;
    (total > 0).then(|| failed as f64 / total as f64)
}

fn percentile(mut values: Vec<f64>, p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * p).round() as usize;
    values.get(index).copied()
}

/// The CI of `app`'s MRs over the last `days`.
pub fn analyze(app: &App, now: DateTime<Utc>, days: i64) -> Ci {
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let middle = start + Duration::days(days) / 2;
    let mut ci = Ci::default();
    let (mut passed, mut failed, mut passed_before, mut failed_before) = (0, 0, 0, 0);
    let mut durations = Vec::new();
    let mut durations_before = Vec::new();
    let mut by_day: BTreeMap<chrono::NaiveDate, Vec<f64>> = BTreeMap::new();
    let mut count_by_day: BTreeMap<chrono::NaiveDate, usize> = BTreeMap::new();
    let mut jobs: BTreeMap<String, (BTreeMap<String, usize>, Vec<Run>)> = BTreeMap::new();
    for mr in app.mrs.iter().filter(|mr| !mr.title.is_empty()) {
        for p in &mr.pipelines {
            let Some(at) = board::time_of(p.created_at.as_deref()) else {
                continue;
            };
            let duration = p
                .updated_at
                .as_deref()
                .zip(p.created_at.as_deref())
                .and_then(|(end, begin)| seconds_between(begin, end))
                .filter(|_| matches!(p.status, PipelineState::Success | PipelineState::Failed));
            if at >= start {
                ci.pipelines += 1;
                match p.status {
                    PipelineState::Success => passed += 1,
                    PipelineState::Failed => failed += 1,
                    _ => {}
                }
                let day = at.with_timezone(&Local).date_naive();
                *count_by_day.entry(day).or_default() += 1;
                if let Some(d) = duration {
                    durations.push(d);
                    by_day.entry(day).or_default().push(d);
                }
                for job in &p.jobs {
                    let entry = jobs.entry(job.name.clone()).or_default();
                    *entry.0.entry(job.stage.clone()).or_default() += 1;
                    entry.1.push(Run {
                        at,
                        status: job.status.clone(),
                        duration: job.duration,
                        sha: p.sha.clone(),
                        mr: mr.id.clone(),
                        flaky: false,
                    });
                    if job.status == "failed" {
                        *ci.stages.entry(job.stage.clone()).or_default() += 1;
                    }
                }
            } else if at >= before {
                match p.status {
                    PipelineState::Success => passed_before += 1,
                    PipelineState::Failed => failed_before += 1,
                    _ => {}
                }
                if let Some(d) = duration {
                    durations_before.push(d);
                }
            }
        }
        // Held back by the CI: open, its latest pipeline failed.
        if mr.state == GitlabMrState::Opened {
            let latest = mr
                .pipelines
                .iter()
                .max_by_key(|p| p.created_at.clone().unwrap_or_default());
            if let Some(p) = latest.filter(|p| p.status == PipelineState::Failed) {
                ci.blocked.push(Blocked {
                    id: mr.id.clone(),
                    title: mr.title.clone(),
                    author: mr.author.clone(),
                    jobs: p
                        .jobs
                        .iter()
                        .filter(|j| j.status == "failed")
                        .map(|j| j.name.clone())
                        .collect(),
                    since: board::time_of(p.created_at.as_deref()),
                    flaky: false,
                });
            }
        }
    }
    ci.success = rate(passed, failed);
    ci.success_before = rate(passed_before, failed_before);
    ci.median = crate::next_step::median(durations.clone());
    ci.p90 = percentile(durations, 0.9);
    ci.median_before = crate::next_step::median(durations_before);
    let today = now.with_timezone(&Local).date_naive();
    ci.days = (0..days)
        .rev()
        .filter_map(|d| today.checked_sub_days(chrono::Days::new(d as u64)))
        .map(|day| {
            (
                day,
                by_day
                    .get(&day)
                    .and_then(|v| crate::next_step::median(v.clone())),
                count_by_day.get(&day).copied().unwrap_or(0),
            )
        })
        .collect();
    for (name, (stages, mut runs)) in jobs {
        runs.sort_by_key(|r| r.at);
        // Flaky: a failure, then a pass of the same job on the same commit.
        let mut flaky_runs = 0;
        for i in 0..runs.len() {
            if runs[i].status != "failed" {
                continue;
            }
            let Some(sha) = runs[i].sha.clone() else {
                continue;
            };
            let passed_later = runs[i + 1..]
                .iter()
                .any(|r| r.status == "success" && r.sha.as_deref() == Some(sha.as_str()));
            if passed_later {
                runs[i].flaky = true;
                flaky_runs += 1;
            }
        }
        let count = |runs: &mut dyn Iterator<Item = &Run>| {
            let (mut f, mut p) = (0, 0);
            for r in runs {
                match r.status.as_str() {
                    "failed" => f += 1,
                    "success" => p += 1,
                    _ => {}
                }
            }
            (f, p)
        };
        let (f, p) = count(&mut runs.iter());
        let (fb, pb) = count(&mut runs.iter().filter(|r| r.at < middle));
        let (fa, pa) = count(&mut runs.iter().filter(|r| r.at >= middle));
        let stage = stages
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(s, _)| s)
            .unwrap_or_default();
        let median = crate::next_step::median(runs.iter().filter_map(|r| r.duration).collect());
        ci.jobs.push(Job {
            name,
            stage,
            failure_rate: rate(f, p).unwrap_or(0.0),
            rate_before: rate(fb, pb).filter(|_| rate(fa, pa).is_some()),
            median,
            flaky_runs,
            runs,
        });
    }
    // The most failing first.
    ci.jobs.sort_by(|a, b| {
        b.failure_rate
            .total_cmp(&a.failure_rate)
            .then(b.flaky_runs.cmp(&a.flaky_runs))
            .then(a.name.cmp(&b.name))
    });
    let flaky: HashSet<&str> = ci
        .jobs
        .iter()
        .filter(|j| j.is_flaky())
        .map(|j| j.name.as_str())
        .collect();
    for b in &mut ci.blocked {
        b.flaky = b.jobs.iter().any(|j| flaky.contains(j.as_str()));
    }
    ci.blocked.sort_by_key(|b| b.since);
    ci
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.ci.period.min(PERIODS.len() - 1)].0;
    let ci = analyze(&g.app, now, days);
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let selected =
        g.ci.selected
            .clone()
            .filter(|s| ci.jobs.iter().any(|j| &j.name == s))
            .or_else(|| ci.jobs.first().map(|j| j.name.clone()));
    board::screen(
        g,
        ui,
        sizes,
        "ci",
        |g, ui| nav(g, ui, &ci, days),
        |g, ui| inspector(g, ui, &ci, overview.as_ref(), selected.as_deref(), now),
        |g, ui| main(g, ui, &ci, selected.as_deref(), now),
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

fn main(g: &mut GuiApp, ui: &mut Ui, ci: &Ci, selected: Option<&str>, now: DateTime<Utc>) {
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

    let jobs: Vec<&Job> = ci
        .jobs
        .iter()
        .filter(|j| shows(j, g.ci.view))
        .filter(|j| !g.ci.hidden_stages.contains(&j.stage))
        .collect();
    let mut picked = None;
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
    if let Some(name) = picked {
        g.ci.selected = Some(name);
    }
    ui.add_space(GAP);
    board::split(
        ui,
        0.58,
        320.0,
        240.0,
        |ui| durations_card(ui, &theme, ci),
        |ui| stages_card(ui, &theme, ci),
    );
}

struct Cols {
    job: (f32, f32),
    stage: Option<(f32, f32)>,
    strip: Option<(f32, f32)>,
    rate: (f32, f32),
    median: (f32, f32),
    trend: Option<(f32, f32)>,
}

fn cols(width: f32) -> Cols {
    let pad = 10.0;
    let gap = 14.0;
    let strip_w = STRIP as f32 * 11.0 - 3.0;
    let fixed = |stage: bool, strip: bool, trend: bool| {
        pad * 2.0
            + 130.0
            + if stage { 72.0 + gap } else { 0.0 }
            + if strip { strip_w + gap } else { 0.0 }
            + 120.0
            + gap
            + 76.0
            + gap
            + if trend { 80.0 + gap } else { 0.0 }
    };
    let (stage, strip, trend) = if fixed(true, true, true) <= width {
        (true, true, true)
    } else if fixed(false, true, true) <= width {
        (false, true, true)
    } else if fixed(false, true, false) <= width {
        (false, true, false)
    } else {
        (false, false, false)
    };
    let job_w = width - fixed(stage, strip, trend) + 130.0;
    let mut x = pad;
    let mut take = |w: f32| {
        let at = x;
        x += w + gap;
        (at, w)
    };
    let job = take(job_w);
    let stage = stage.then(|| take(72.0));
    let strip = strip.then(|| take(strip_w));
    let rate = take(120.0);
    let median = take(76.0);
    let trend = trend.then(|| take(80.0));
    Cols {
        job,
        stage,
        strip,
        rate,
        median,
        trend,
    }
}

fn status_color(status: &str, flaky: bool, theme: &Theme) -> Color32 {
    let t = &theme.tones;
    match status {
        "failed" if flaky => t.amber.fg,
        "failed" => t.red.fg,
        "success" => t.green.fg,
        "running" | "pending" | "created" => t.blue.fg,
        _ => theme.palette.border,
    }
}

fn jobs_table(
    ui: &mut Ui,
    theme: &Theme,
    jobs: &[&Job],
    selected: Option<&str>,
    picked: &mut Option<String>,
) {
    let c = cols(ui.available_width());
    let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let head = |col: (f32, f32), text: &str| {
        board::paint_text(
            ui,
            egui::pos2(header.left() + col.0, header.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(theme.text_size - 1.5),
            theme.palette.text_muted,
            col.1,
        );
    };
    head(c.job, tl("Job"));
    if let Some(col) = c.stage {
        head(col, tl("Stage"));
    }
    if let Some(col) = c.strip {
        head(col, &tf("{n} latest runs", &[("n", &STRIP.to_string())]));
    }
    head(c.rate, tl("Failures"));
    head(c.median, tl("Median"));
    if let Some(col) = c.trend {
        head(col, tl("Trend"));
    }
    for job in jobs {
        let is_selected = selected == Some(job.name.as_str());
        let (rect, response) = board::row(ui, theme, 40.0, is_selected);
        if !is_selected {
            board::row_rule(ui, theme, rect);
        }
        let cy = rect.center().y;
        let x = |col: (f32, f32)| rect.left() + col.0;
        // Name, and a chip when flaky.
        let chip_w = if job.is_flaky() {
            let galley = ui.painter().layout_no_wrap(
                tl("flaky").to_string(),
                fonts::medium(theme.text_size - 2.5),
                theme.tones.amber.fg,
            );
            galley.size().x + 12.0 + 8.0
        } else {
            0.0
        };
        let w = board::paint_text(
            ui,
            egui::pos2(x(c.job), cy),
            Align2::LEFT_CENTER,
            &job.name,
            egui::FontId::monospace(theme.text_size - 1.0),
            theme.palette.text,
            c.job.1 - chip_w,
        );
        if job.is_flaky() {
            board::paint_tag(
                ui,
                theme,
                egui::pos2(x(c.job) + w + 8.0, cy),
                tl("flaky"),
                theme.tones.amber,
                false,
            );
        }
        if let Some(col) = c.stage {
            board::paint_tag(
                ui,
                theme,
                egui::pos2(x(col), cy),
                &job.stage,
                theme.tones.gray,
                false,
            );
        }
        if let Some(col) = c.strip {
            let runs: Vec<&Run> = job
                .runs
                .iter()
                .rev()
                .take(STRIP)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            // Right-aligned: the latest run last; empty slots before.
            let offset = (STRIP - runs.len()) as f32 * 11.0;
            for k in 0..STRIP {
                let slot = Rect::from_min_size(
                    egui::pos2(x(col) + k as f32 * 11.0, cy - 8.0),
                    Vec2::new(8.0, 16.0),
                );
                if (k as f32 * 11.0) < offset {
                    ui.painter().rect_filled(
                        slot,
                        CornerRadius::same(2),
                        theme.palette.raised.gamma_multiply(0.6),
                    );
                }
            }
            for (k, run) in runs.iter().enumerate() {
                let slot = Rect::from_min_size(
                    egui::pos2(x(col) + offset + k as f32 * 11.0, cy - 8.0),
                    Vec2::new(8.0, 16.0),
                );
                ui.painter().rect_filled(
                    slot,
                    CornerRadius::same(2),
                    status_color(&run.status, run.flaky, theme),
                );
                let tip = format!(
                    "!{} · {}{}\n{}",
                    run.mr,
                    run.status,
                    if run.flaky {
                        format!(" · {}", tl("passed on a rerun"))
                    } else {
                        String::new()
                    },
                    run.at.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
                );
                ui.interact(
                    slot.expand2(Vec2::new(1.5, 4.0)),
                    ui.scope_id().with(("run", job.name.as_str(), k)),
                    Sense::hover(),
                )
                .on_hover_text(tip);
            }
        }
        // Failure rate: the figure and a bar.
        let (tone_color, text_color) = match job.failure_rate {
            r if r >= 0.3 => (theme.tones.red.fg, theme.tones.red.fg),
            r if r >= 0.1 => (theme.tones.amber.fg, theme.tones.amber.fg),
            _ => (theme.tones.green.fg, theme.palette.text_secondary),
        };
        let label_w = board::paint_text(
            ui,
            egui::pos2(x(c.rate), cy),
            Align2::LEFT_CENTER,
            &board::percent(job.failure_rate),
            fonts::semibold(theme.text_size - 1.0),
            text_color,
            44.0,
        );
        let track = Rect::from_min_size(
            egui::pos2(x(c.rate) + label_w.max(34.0) + 8.0, cy - 3.0),
            Vec2::new(c.rate.1 - label_w.max(34.0) - 8.0, 6.0),
        );
        ui.painter()
            .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
        let mut fill = track;
        fill.set_width(track.width() * job.failure_rate as f32);
        if fill.width() > 0.5 {
            ui.painter()
                .rect_filled(fill, CornerRadius::same(3), tone_color);
        }
        board::paint_text(
            ui,
            egui::pos2(x(c.median), cy),
            Align2::LEFT_CENTER,
            &job.median
                .map_or_else(|| board::dash().to_string(), board::seconds_text),
            egui::FontId::monospace(theme.text_size - 2.0),
            theme.palette.text_secondary,
            c.median.1,
        );
        if let Some(col) = c.trend {
            let (text, color) = match job.rate_before {
                Some(before) => {
                    let delta = (job.failure_rate - before) * 100.0;
                    if delta >= 3.0 {
                        (format!("↑ +{delta:.0} {}", tl("pts")), theme.tones.red.fg)
                    } else if delta <= -3.0 {
                        (format!("↓ {delta:.0} {}", tl("pts")), theme.tones.green.fg)
                    } else {
                        (format!("→ {}", tl("stable")), theme.palette.text_muted)
                    }
                }
                None => (board::dash().to_string(), theme.palette.text_muted),
            };
            board::paint_text(
                ui,
                egui::pos2(x(col), cy),
                Align2::LEFT_CENTER,
                &text,
                FontId::proportional(theme.text_size - 1.5),
                color,
                col.1,
            );
        }
        if response.clicked() {
            *picked = Some(job.name.clone());
        }
    }
}

/// The median duration of each day's pipelines, the period's median dashed.
fn durations_card(ui: &mut Ui, theme: &Theme, ci: &Ci) {
    let note = ci
        .median
        .map(|m| tf("median {d}", &[("d", &board::seconds_text(m))]))
        .unwrap_or_default();
    board::card(ui, theme, tl("Pipeline durations"), &note, 210.0, |ui| {
        let max = ci.days.iter().filter_map(|d| d.1).fold(0.0_f64, f64::max);
        if max <= 0.0 {
            board::empty(ui, theme, tl("No finished pipeline in the period."));
            return;
        }
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), Sense::hover());
        let n = ci.days.len().max(1);
        let gap = if n > 20 { 3.0 } else { 5.0 };
        let w = (rect.width() - gap * (n - 1) as f32) / n as f32;
        let top = max * 1.15;
        let median = ci.median.unwrap_or(0.0);
        for (i, (day, value, count)) in ci.days.iter().enumerate() {
            let x = rect.left() + i as f32 * (w + gap);
            let column =
                Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + w, rect.bottom()));
            match value {
                Some(v) => {
                    let h = (rect.height() * (*v / top) as f32).max(3.0);
                    let bar = Rect::from_min_max(
                        egui::pos2(x, rect.bottom() - h),
                        egui::pos2(x + w, rect.bottom()),
                    );
                    let color = if *v > median * 1.2 {
                        theme.tones.amber.fg
                    } else {
                        theme.palette.accent
                    };
                    ui.painter().rect_filled(
                        bar,
                        CornerRadius {
                            nw: 3,
                            ne: 3,
                            sw: 0,
                            se: 0,
                        },
                        color,
                    );
                }
                None => {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            egui::pos2(x, rect.bottom() - 2.0),
                            egui::pos2(x + w, rect.bottom()),
                        ),
                        CornerRadius::same(1),
                        theme.palette.border,
                    );
                }
            }
            let tip = match value {
                Some(v) => format!(
                    "{} · {} · {}",
                    day.format("%d/%m"),
                    board::seconds_text(*v),
                    tn(*count as u64, "{n} pipeline", "{n} pipelines")
                ),
                None => format!("{} · {}", day.format("%d/%m"), tl("no pipeline")),
            };
            ui.interact(column, ui.scope_id().with(("day", i)), Sense::hover())
                .on_hover_text(tip);
        }
        if median > 0.0 {
            let y = rect.bottom() - rect.height() * (median / top) as f32;
            let mut x = rect.left();
            while x < rect.right() {
                ui.painter().hline(
                    x..=(x + 5.0).min(rect.right()),
                    y,
                    Stroke::new(1.0, theme.tones.amber.fg),
                );
                x += 9.0;
            }
        }
        let (axis, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        if let (Some(first), Some(last)) = (ci.days.first(), ci.days.last()) {
            let font = FontId::proportional(theme.text_size - 2.5);
            board::paint_text(
                ui,
                egui::pos2(axis.left(), axis.center().y),
                Align2::LEFT_CENTER,
                &first.0.format("%d/%m").to_string(),
                font.clone(),
                theme.palette.text_muted,
                60.0,
            );
            board::paint_text(
                ui,
                egui::pos2(axis.right(), axis.center().y),
                Align2::RIGHT_CENTER,
                &last.0.format("%d/%m").to_string(),
                font,
                theme.palette.text_muted,
                60.0,
            );
        }
    });
}

fn stages_card(ui: &mut Ui, theme: &Theme, ci: &Ci) {
    board::card(ui, theme, tl("Failures by stage"), "", 210.0, |ui| {
        if ci.stages.is_empty() {
            board::empty(ui, theme, tl("No failed job in the period."));
            return;
        }
        let mut stages: Vec<(&String, &usize)> = ci.stages.iter().collect();
        stages.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let max = *stages[0].1 as f32;
        for (stage, n) in &stages {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
            let label_w = 70.0;
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                stage,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_secondary,
                label_w - 6.0,
            );
            let count = n.to_string();
            board::paint_text(
                ui,
                egui::pos2(line.right(), line.center().y),
                Align2::RIGHT_CENTER,
                &count,
                fonts::medium(theme.text_size - 1.5),
                theme.palette.text_muted,
                30.0,
            );
            let track = Rect::from_min_size(
                egui::pos2(line.left() + label_w, line.center().y - 5.0),
                Vec2::new(line.width() - label_w - 36.0, 10.0),
            );
            ui.painter()
                .rect_filled(track, CornerRadius::same(5), theme.palette.border_soft);
            let mut fill = track;
            fill.set_width(track.width() * **n as f32 / max);
            ui.painter()
                .rect_filled(fill, CornerRadius::same(5), theme.tones.red.fg);
        }
        // The job that fails the most, and its share of its stage.
        let top = ci
            .jobs
            .iter()
            .max_by_key(|j| j.failures())
            .filter(|j| j.failures() > 0);
        if let Some(job) = top {
            let of_stage = ci.stages.get(&job.stage).copied().unwrap_or(0).max(1);
            ui.add_space(6.0);
            board::caption(
                ui,
                theme,
                &tf(
                    "{job} makes {share} of the failures of the {stage} stage.",
                    &[
                        ("job", &job.name),
                        (
                            "share",
                            &board::percent(job.failures() as f64 / of_stage as f64),
                        ),
                        ("stage", &job.stage),
                    ],
                ),
            );
        }
    });
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    ci: &Ci,
    overview: Option<&crate::gitlab_detail::ProjectOverview>,
    selected: Option<&str>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    board::section(
        ui,
        &theme,
        tl("Tracked branches"),
        tl("latest pipeline"),
        |ui| {
            let Some(overview) = overview else {
                ui.horizontal(|ui| {
                    super::widgets::spinner(ui, 14.0, theme.palette.text_muted);
                    ui.label(muted(&theme, tl("Reading the branches' pipelines…")));
                });
                return;
            };
            let flow = g.app.branch_flow();
            let ordered: Vec<String> = flow.in_order().iter().map(|b| b.name.clone()).collect();
            for branch in &ordered {
                let pipeline = overview.pipelines.iter().find(|p| &p.branch == branch);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
                let (color, line2) = match pipeline {
                    Some(p) => {
                        let (label, tone) = super::tones::pipeline(p.status, &theme.tones);
                        let when = board::parse_time(&p.created_at)
                            .map(|t| board::ago(t, now))
                            .unwrap_or_default();
                        let jobs = if p.failed_jobs.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", p.failed_jobs.join(", "))
                        };
                        (
                            tone.fg,
                            format!("#{} · {}{jobs} · {when}", p.id, label.to_lowercase()),
                        )
                    }
                    None => (theme.palette.text_muted, tl("no pipeline").to_string()),
                };
                ui.painter().circle_filled(
                    egui::pos2(rect.left() + 4.0, rect.top() + 12.0),
                    4.0,
                    color,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 16.0, rect.top() + 12.0),
                    Align2::LEFT_CENTER,
                    branch,
                    egui::FontId::monospace(theme.text_size - 1.0),
                    theme.tones.blue.fg,
                    rect.width() - 16.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 16.0, rect.top() + 30.0),
                    Align2::LEFT_CENTER,
                    &line2,
                    FontId::proportional(theme.text_size - 2.0),
                    if pipeline.is_some_and(|p| p.status == PipelineState::Failed) {
                        theme.tones.red.fg
                    } else {
                        theme.palette.text_muted
                    },
                    rect.width() - 16.0,
                );
            }
        },
    );
    let mut open = None;
    board::section(
        ui,
        &theme,
        tl("Held back by the CI"),
        &ci.blocked.len().to_string(),
        |ui| {
            if ci.blocked.is_empty() {
                ui.label(muted(
                    &theme,
                    tl("No open MR has its latest pipeline failed."),
                ));
            }
            for b in &ci.blocked {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(
                        rect.expand2(Vec2::new(6.0, 0.0)),
                        CornerRadius::same(theme.radii.medium),
                        theme.palette.raised,
                    );
                }
                let w = board::paint_tag(
                    ui,
                    &theme,
                    egui::pos2(rect.left(), rect.top() + 12.0),
                    &format!("!{}", b.id),
                    board::id_tone(&theme),
                    true,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + w + 8.0, rect.top() + 12.0),
                    Align2::LEFT_CENTER,
                    &b.title,
                    fonts::medium(theme.text_size - 0.5),
                    theme.palette.text,
                    rect.width() - w - 8.0,
                );
                let since = b
                    .since
                    .map(|s| board::duration_text(board::days_between(s, now)))
                    .unwrap_or_default();
                let mut text = format!("{} · {}", board::first_name(&b.author), b.jobs.join(", "));
                if !since.is_empty() {
                    text = format!("{text} · {}", tf("for {time}", &[("time", &since)]));
                }
                let mut x = rect.left() + w + 8.0;
                if b.flaky {
                    x += board::paint_tag(
                        ui,
                        &theme,
                        egui::pos2(x, rect.top() + 31.0),
                        tl("rerun?"),
                        theme.tones.amber,
                        false,
                    ) + 6.0;
                }
                board::paint_text(
                    ui,
                    egui::pos2(x, rect.top() + 31.0),
                    Align2::LEFT_CENTER,
                    &text,
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    rect.right() - x,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tl("Open its pipelines"))
                    .clicked()
                {
                    open = Some(b.id.clone());
                }
            }
        },
    );
    if let Some(id) = open {
        g.open_mr(&id);
        g.mr_tab = super::mr_page::Tab::Pipelines;
        return;
    }
    let Some(job) = selected.and_then(|s| ci.jobs.iter().find(|j| j.name == s)) else {
        return;
    };
    let mut show = false;
    board::section(ui, &theme, tl("Job picked"), &job.stage, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(super::widgets::mono(&job.name, theme.text_size).color(theme.palette.text));
            if job.is_flaky() {
                board::chip(ui, &theme, tl("flaky"), theme.tones.amber);
            }
        });
        ui.add_space(8.0);
        let failures = job.failures();
        let last_failure = job
            .runs
            .iter()
            .rev()
            .find(|r| r.status == "failed")
            .map(|r| board::ago(r.at, now));
        board::facts(
            ui,
            &theme,
            &[
                (tl("Runs"), job.runs.len().to_string()),
                (
                    tl("Failures"),
                    format!("{failures} · {}", board::percent(job.failure_rate)),
                ),
                (
                    tl("Flaky"),
                    tn(
                        job.flaky_runs as u64,
                        "{n} rerun fixed it",
                        "{n} reruns fixed it",
                    ),
                ),
                (
                    tl("Median"),
                    job.median
                        .map_or_else(|| board::dash().to_string(), board::seconds_text),
                ),
                (
                    tl("Last failure"),
                    last_failure
                        .clone()
                        .unwrap_or_else(|| board::dash().to_string()),
                ),
            ],
        );
        let mut mrs: Vec<&str> = job
            .runs
            .iter()
            .filter(|r| r.status == "failed")
            .map(|r| r.mr.as_str())
            .collect();
        mrs.sort_unstable();
        mrs.dedup();
        if !mrs.is_empty() {
            ui.add_space(10.0);
            ui.label(muted(&theme, tl("Failed on")).size(theme.text_size - 1.5));
            ui.add_space(2.0);
            let mut clicked = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for id in &mrs {
                    if board::mr_pill(ui, &theme, id, "").clicked() {
                        clicked = Some(id.to_string());
                    }
                }
            });
            if let Some(id) = clicked {
                open = Some(id);
            }
        }
        ui.add_space(GAP);
        show = primary_button(ui, &theme, tl("Show these MRs")).clicked();
    });
    if show {
        let ids = job
            .runs
            .iter()
            .filter(|r| r.status == "failed")
            .map(|r| r.mr.clone())
            .collect();
        g.next_focus = Some(Some(Focus {
            key: String::new(),
            label: tf("{job} failed on them", &[("job", &job.name)]),
            ids,
        }));
        g.open_page(super::Page::MergeRequests);
    }
    if let Some(id) = open {
        g.show_mr(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MrData, Pipeline, PipelineJob, TrackedMr};

    fn job(name: &str, status: &str) -> PipelineJob {
        PipelineJob {
            id: 0,
            name: name.into(),
            stage: "test".into(),
            status: status.into(),
            duration: Some(60.0),
        }
    }

    #[test]
    fn a_failure_then_a_pass_on_the_same_commit_is_flaky() {
        let mut app = crate::app::tests::test_app();
        let now = board::parse_time("2026-11-05T12:00:00Z").unwrap();
        let mut mr = TrackedMr::placeholder("1".into(), "A".into(), None);
        let pipeline = |id, at: &str, sha: &str, status, jobs| Pipeline {
            id,
            status,
            created_at: Some(at.into()),
            updated_at: Some(at.replace("00:00Z", "10:00Z")),
            sha: Some(sha.into()),
            jobs,
        };
        mr.data = MrData {
            title: "A".into(),
            state: GitlabMrState::Opened,
            pipelines: vec![
                pipeline(
                    3,
                    "2026-11-04T10:00:00Z",
                    "b",
                    PipelineState::Failed,
                    vec![job("unit", "failed"), job("e2e", "success")],
                ),
                pipeline(
                    2,
                    "2026-11-03T10:00:00Z",
                    "a",
                    PipelineState::Success,
                    vec![job("unit", "success"), job("e2e", "success")],
                ),
                pipeline(
                    1,
                    "2026-11-03T09:00:00Z",
                    "a",
                    PipelineState::Failed,
                    vec![job("unit", "success"), job("e2e", "failed")],
                ),
            ],
            ..MrData::default()
        };
        app.mrs = vec![mr];
        let ci = analyze(&app, now, 7);
        assert_eq!(ci.pipelines, 3);
        let e2e = ci.jobs.iter().find(|j| j.name == "e2e").unwrap();
        assert_eq!(e2e.flaky_runs, 1);
        let unit = ci.jobs.iter().find(|j| j.name == "unit").unwrap();
        // Failed on its last commit, never passed on it: not flaky, failing.
        assert_eq!(unit.flaky_runs, 0);
        assert!((unit.failure_rate - 1.0 / 3.0).abs() < 1e-9);
        // The MR is held back by its latest pipeline.
        assert_eq!(ci.blocked.len(), 1);
        assert_eq!(ci.blocked[0].jobs, vec!["unit".to_string()]);
        assert!((ci.success.unwrap() - 1.0 / 3.0).abs() < 1e-9);
    }
}
