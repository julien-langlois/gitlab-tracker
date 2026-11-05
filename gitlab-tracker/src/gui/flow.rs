//! The cockpit's delivery flow and trends: where the open and merged MRs wait
//! now (and for how long, the slowest stage flagged), and four figures week by
//! week over the last six weeks, read from the dates the MRs already carry.

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Utc};
use egui::{CornerRadius, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::widgets::muted;
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use crate::next_step::{self, days_text, median, Actor};

/// A stage of the flow: who the MRs wait on, and for a deployment where they
/// are and where they go next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Reviewers,
    Author,
    Merger,
    /// Merged and on `from` (its target at least), not on `to` yet.
    Deploy {
        from: String,
        to: String,
    },
}

impl Stage {
    pub fn name(&self, target_first: bool) -> String {
        match self {
            Stage::Reviewers => tl("Waiting for a review").into(),
            Stage::Author => tl("Waiting on the author").into(),
            Stage::Merger => tl("Approved, not merged").into(),
            Stage::Deploy { to, .. } if target_first => tf("Merged, not on {to}", &[("to", to)]),
            Stage::Deploy { from, to } => {
                tf("On {from}, not on {to}", &[("from", from), ("to", to)])
            }
        }
    }

    /// Its focus key (see `GuiApp::focus_on`).
    pub fn key(&self) -> String {
        match self {
            Stage::Reviewers => "stage:reviewers".into(),
            Stage::Author => "stage:author".into(),
            Stage::Merger => "stage:merge".into(),
            Stage::Deploy { from, to } => format!("stage:deploy:{from}:{to}"),
        }
    }

    fn of(step: &next_step::NextStep, target: &str) -> Option<(Stage, bool)> {
        Some(match step.actor {
            Actor::Reviewers => (Stage::Reviewers, false),
            Actor::Author => (Stage::Author, false),
            Actor::Merger => (Stage::Merger, false),
            Actor::Deploy => {
                let to = match step.todos.first()? {
                    next_step::Todo::Promote { to } => to.clone(),
                    _ => return None,
                };
                let from = step.reached.clone().unwrap_or_else(|| target.to_string());
                let first = from == target;
                (Stage::Deploy { from, to }, first)
            }
            _ => return None,
        })
    }
}

/// The MRs of `app` at the stage of `key`, or `None` when `key` is no stage.
pub fn stage_members(app: &App, key: &str) -> Option<Vec<String>> {
    if !key.starts_with("stage:") {
        return None;
    }
    let me = app.config.gitlab_username.as_deref();
    Some(
        app.mrs
            .iter()
            .filter(|mr| {
                Stage::of(&next_step::of(app, mr, me), &mr.target_branch)
                    .is_some_and(|(s, _)| s.key() == key)
            })
            .map(|mr| mr.id.clone())
            .collect(),
    )
}

/// A stage now: how many MRs, how many on me, and the median time they wait.
pub struct StageNow {
    pub stage: Stage,
    pub name: String,
    pub count: usize,
    pub mine: usize,
    pub median_days: Option<f64>,
}

/// The stages in the flow's order: review, author, merge, then the branches
/// up to production, each shown even when empty (but the deployments).
pub fn stages_now(app: &App) -> Vec<StageNow> {
    let me = app.config.gitlab_username.as_deref();
    let now = Utc::now();
    let mut found: Vec<(Stage, bool, next_step::NextStep)> = app
        .mrs
        .iter()
        .filter_map(|mr| {
            let step = next_step::of(app, mr, me);
            let (stage, first) = Stage::of(&step, &mr.target_branch)?;
            Some((stage, first, step))
        })
        .collect();
    let flow = app.branch_flow();
    // The deployments in the order of the flow: nearest to production last.
    let rank = |b: &str| flow.role(b);
    found.sort_by_key(|(s, _, _)| match s {
        Stage::Deploy { from, to } => Some((rank(to), rank(from))),
        _ => None,
    });
    let mut stages: Vec<Stage> = vec![Stage::Reviewers, Stage::Author, Stage::Merger];
    for (s, _, _) in &found {
        if !stages.contains(s) {
            stages.push(s.clone());
        }
    }
    stages
        .into_iter()
        .map(|stage| {
            let at: Vec<&(Stage, bool, next_step::NextStep)> =
                found.iter().filter(|(s, _, _)| *s == stage).collect();
            let first = at.first().is_some_and(|(_, f, _)| *f);
            StageNow {
                name: stage.name(first),
                count: at.len(),
                mine: at.iter().filter(|(_, _, s)| s.mine).count(),
                median_days: median(
                    at.iter()
                        .filter_map(|(_, _, s)| s.waited_days(now))
                        .collect(),
                ),
                stage,
            }
        })
        .collect()
}

/// The stage MRs wait longest at, when it stands out: the highest median,
/// over a day.
pub fn bottleneck(stages: &[StageNow]) -> Option<usize> {
    stages
        .iter()
        .enumerate()
        .filter(|(_, s)| s.count > 0)
        .filter_map(|(i, s)| Some((i, s.median_days?)))
        .filter(|(_, d)| *d >= 1.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The delivery flow card; the stage clicked: its key and name.
pub fn flow_card(ui: &mut Ui, theme: &Theme, app: &App) -> Option<(String, String)> {
    let stages = stages_now(app);
    let slowest = bottleneck(&stages);
    let mut picked = None;
    theme.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tl("Delivery flow · where MRs wait").to_uppercase())
                    .font(fonts::semibold(theme.text_size - 2.5))
                    .color(theme.palette.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    muted(theme, tl("Count now · median time spent in the stage"))
                        .size(theme.text_size - 1.5),
                );
            });
        });
        ui.add_space(12.0);
        let per_row = if ui.available_width() < 760.0 {
            2
        } else {
            stages.len().max(1)
        };
        for (row, chunk) in stages.chunks(per_row).enumerate() {
            if row > 0 {
                ui.add_space(10.0);
            }
            ui.columns(per_row, |columns| {
                for (j, (ui, s)) in columns.iter_mut().zip(chunk).enumerate() {
                    let index = row * per_row + j;
                    let hot = slowest == Some(index);
                    let tone = theme.tones.orange;
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 104.0),
                        Sense::click(),
                    );
                    let fill = if hot {
                        tone.bg.gamma_multiply(0.6)
                    } else if response.hovered() {
                        theme.palette.raised
                    } else {
                        theme.palette.content
                    };
                    let radius = CornerRadius::same(theme.radii.medium);
                    ui.painter().rect_filled(rect, radius, fill);
                    ui.painter().rect_stroke(
                        rect,
                        radius,
                        Stroke::new(
                            1.0,
                            if hot {
                                tone.fg.gamma_multiply(0.5)
                            } else {
                                theme.palette.border_soft
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let mut inner = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect.shrink2(Vec2::new(12.0, 12.0)))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    inner.style_mut().interaction.selectable_labels = false;
                    inner.spacing_mut().item_spacing.y = 6.0;
                    inner.add(
                        egui::Label::new(
                            RichText::new(&s.name)
                                .size(theme.text_size - 1.5)
                                .color(theme.palette.text_secondary),
                        )
                        .truncate(),
                    );
                    inner.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            RichText::new(s.count.to_string())
                                .font(fonts::semibold(theme.text_size + 10.0))
                                .color(theme.palette.text),
                        );
                        ui.label(muted(theme, tn(s.count as u64, "MR", "MRs")));
                        if s.mine > 0 {
                            ui.label(
                                RichText::new(tn(s.mine as u64, "· {n} on me", "· {n} on me"))
                                    .size(theme.text_size - 1.5)
                                    .color(theme.tones.orange.fg),
                            );
                        }
                    });
                    inner.label(
                        RichText::new(s.median_days.map(days_text).unwrap_or_else(|| "—".into()))
                            .font(fonts::semibold(theme.text_size - 0.5))
                            .color(if hot { tone.fg } else { theme.palette.text }),
                    );
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tl("Show these MRs in the list"))
                        .clicked()
                    {
                        picked = Some((s.stage.key(), s.name.clone()));
                    }
                }
            });
        }
        if let Some(s) = slowest.and_then(|i| stages.get(i)) {
            ui.add_space(12.0);
            ui.label(
                RichText::new(tf(
                    "Bottleneck: {stage} — {time} median.",
                    &[
                        ("stage", &s.name.to_lowercase()),
                        ("time", &s.median_days.map(days_text).unwrap_or_default()),
                    ],
                ))
                .color(theme.tones.orange.fg),
            );
        }
    });
    picked
}

/// The period the trends compare with: the four weeks before, or the
/// quarter (twelve weeks) before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    Month,
    Quarter,
}

impl Period {
    /// Weeks compared with the current one.
    pub fn before(self) -> usize {
        match self {
            Period::Month => 4,
            Period::Quarter => 12,
        }
    }

    /// Bars drawn, the current week included.
    fn weeks(self) -> usize {
        self.before() + 2
    }
}

/// A figure week by week, oldest first, the current week last.
pub struct Trend {
    pub name: String,
    pub weeks: Vec<Option<f64>>,
    pub period: Period,
    /// How the value reads ("3", "1.5 d", "86 %").
    pub format: fn(f64) -> String,
    /// A rise is good news (more merges) or bad news (longer waits).
    pub higher_is_better: bool,
    pub caption: String,
}

fn week_of(at: DateTime<Utc>, now: DateTime<Utc>, weeks: usize) -> Option<usize> {
    let ago = (now - at).num_days();
    (0..(weeks as i64 * 7))
        .contains(&ago)
        .then(|| weeks - 1 - (ago / 7) as usize)
}

fn parse(at: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn by_week(
    samples: Vec<(DateTime<Utc>, f64)>,
    now: DateTime<Utc>,
    weeks: usize,
    sum: bool,
) -> Vec<Option<f64>> {
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); weeks];
    for (at, v) in samples {
        if let Some(w) = week_of(at, now, weeks) {
            buckets[w].push(v);
        }
    }
    buckets
        .into_iter()
        .map(|b| if sum { Some(b.iter().sum()) } else { median(b) })
        .collect()
}

fn days_of(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    (b - a).num_minutes().max(0) as f64 / 1440.0
}

/// The four trends of `app` at `now`, over `period`.
pub fn trends(app: &App, now: DateTime<Utc>, period: Period) -> Vec<Trend> {
    let weeks = period.weeks();
    let mrs = || app.mrs.iter().filter(|m| !m.title.is_empty());
    let merged = by_week(
        mrs()
            .filter(|m| m.state == GitlabMrState::Merged)
            .filter_map(|m| Some((parse(m.merged_at.as_deref())?, 1.0)))
            .collect(),
        now,
        weeks,
        true,
    );
    let first_review = by_week(
        mrs()
            .filter_map(|m| {
                let created = parse(m.created_at.as_deref())?;
                let first = parse(m.review.as_ref()?.first_review_at.as_deref())?;
                Some((first, days_of(created, first)))
            })
            .collect(),
        now,
        weeks,
        false,
    );
    // Lead time: created → the last branch of the flow (production).
    let flow = app.branch_flow();
    let last = flow
        .branches
        .iter()
        .max_by_key(|b| b.role)
        .map(|b| b.name.clone());
    let lead = by_week(
        mrs()
            .filter_map(|m| {
                let created = parse(m.created_at.as_deref())?;
                let branch = last.as_ref()?;
                let at = if &m.target_branch == branch {
                    parse(m.merged_at.as_deref())?
                } else {
                    *app.branch_arrivals.get(&(m.id.clone(), branch.clone()))?
                };
                Some((at, days_of(created, at)))
            })
            .collect(),
        now,
        weeks,
        false,
    );
    let mut runs: Vec<(u32, u32)> = vec![(0, 0); weeks];
    for p in mrs().flat_map(|m| m.pipelines.iter()) {
        let Some(w) = parse(p.created_at.as_deref()).and_then(|at| week_of(at, now, weeks)) else {
            continue;
        };
        match p.status {
            PipelineState::Success => runs[w].0 += 1,
            PipelineState::Failed => runs[w].1 += 1,
            _ => {}
        }
    }
    let ci: Vec<Option<f64>> = runs
        .into_iter()
        .map(|(ok, ko)| (ok + ko > 0).then(|| ok as f64 * 100.0 / (ok + ko) as f64))
        .collect();
    let count: fn(f64) -> String = |v| format!("{v:.0}");
    let span: fn(f64) -> String = days_text;
    let percent: fn(f64) -> String = |v| format!("{v:.0} %");
    vec![
        Trend {
            name: tl("Merged").into(),
            weeks: merged,
            period,
            format: count,
            higher_is_better: true,
            caption: tl("MRs merged per week").into(),
        },
        Trend {
            name: tl("Time to first review").into(),
            weeks: first_review,
            period,
            format: span,
            higher_is_better: false,
            caption: tl("median, by week of the review").into(),
        },
        Trend {
            name: match &last {
                Some(b) => tf("Lead time to {branch}", &[("branch", b)]),
                None => tl("Lead time").into(),
            },
            weeks: lead,
            period,
            format: span,
            higher_is_better: false,
            caption: tl("created → there, median by week of arrival").into(),
        },
        Trend {
            name: tl("CI success").into(),
            weeks: ci,
            period,
            format: percent,
            higher_is_better: true,
            caption: tl("pipelines of the tracked MRs").into(),
        },
    ]
}

/// "4 weeks | Quarter": the period the trends compare with.
pub fn period_toggle(ui: &mut Ui, theme: &Theme, period: &mut Period) {
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut items = [
                    (Period::Month, tl("4 weeks")),
                    (Period::Quarter, tl("Quarter")),
                ];
                // In a right-to-left bar the first drawn is the rightmost.
                if ui.layout().prefer_right_to_left() {
                    items.reverse();
                }
                for (p, name) in items {
                    let on = *period == p;
                    let galley = ui.painter().layout_no_wrap(
                        name.to_owned(),
                        fonts::medium(theme.text_size - 1.5),
                        theme.palette.text,
                    );
                    let (rect, response) = ui
                        .allocate_exact_size(galley.size() + Vec2::new(20.0, 10.0), Sense::click());
                    let radius = CornerRadius::same(theme.radii.medium);
                    let color = if on {
                        ui.painter()
                            .rect_filled(rect, radius, theme.palette.selection);
                        theme.palette.text
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, radius, theme.palette.raised);
                        theme.palette.text
                    } else {
                        theme.palette.text_secondary
                    };
                    ui.painter()
                        .galley(rect.center() - galley.size() / 2.0, galley, color);
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        *period = p;
                    }
                }
            });
        });
}

/// The trends in a row of tiles: this week's value, the change against the
/// four weeks before, and a bar per week.
pub fn trend_tiles(ui: &mut Ui, theme: &Theme, trends: &[Trend]) {
    let per_row = if ui.available_width() < 900.0 {
        2
    } else {
        trends.len().max(1)
    };
    for (i, chunk) in trends.chunks(per_row).enumerate() {
        if i > 0 {
            ui.add_space(12.0);
        }
        ui.columns(per_row, |columns| {
            for (ui, t) in columns.iter_mut().zip(chunk) {
                theme
                    .card_frame()
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        tile(ui, theme, t);
                    });
            }
        });
    }
}

fn tile(ui: &mut Ui, theme: &Theme, t: &Trend) {
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.label(muted(theme, &t.name));
    let n = t.weeks.len();
    let current = t.weeks[n - 1];
    let before: Vec<f64> = t.weeks[n - 1 - t.period.before()..n - 1]
        .iter()
        .flatten()
        .copied()
        .collect();
    let average = (!before.is_empty()).then(|| before.iter().sum::<f64>() / before.len() as f64);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(current.map_or("—".to_string(), t.format))
                .font(fonts::semibold(theme.text_size + 8.0))
                .color(theme.palette.text),
        );
        if let (Some(now), Some(avg)) = (current, average) {
            let diff = now - avg;
            if diff.abs() > f64::EPSILON {
                let better = (diff > 0.0) == t.higher_is_better;
                let tone: Tone = if better {
                    theme.tones.green
                } else {
                    theme.tones.amber
                };
                let sign = if diff > 0.0 { "↑" } else { "↓" };
                ui.label(
                    RichText::new(format!("{sign} {}", (t.format)(diff.abs())))
                        .font(fonts::semibold(theme.text_size - 1.5))
                        .color(tone.fg),
                )
                .on_hover_text(tf(
                    "against {avg} on average the weeks before",
                    &[("avg", &(t.format)(avg))],
                ));
            }
        }
    });
    // A bar per week, the current one in the accent.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    let max = t.weeks.iter().flatten().fold(0.0_f64, |a, b| a.max(*b));
    let bar_gap = 4.0;
    let bar_w = (rect.width() - bar_gap * (n as f32 - 1.0)) / n as f32;
    for (i, v) in t.weeks.iter().enumerate() {
        let x = rect.left() + i as f32 * (bar_w + bar_gap);
        let color = if i == n - 1 {
            theme.palette.accent
        } else {
            theme.palette.accent.gamma_multiply(0.35)
        };
        let h = match v {
            Some(v) if max > 0.0 => ((*v / max) as f32 * rect.height()).max(3.0),
            Some(_) => 3.0,
            None => 0.0,
        };
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, rect.bottom() - h),
            egui::pos2(x + bar_w, rect.bottom()),
        );
        if h > 0.0 {
            ui.painter().rect_filled(bar, CornerRadius::same(2), color);
        } else {
            ui.painter().hline(
                x..=x + bar_w,
                rect.bottom() - 0.5,
                Stroke::new(1.0, theme.palette.border_soft),
            );
        }
    }
    let caption = if t.weeks.iter().all(Option::is_none) {
        tn(
            t.weeks.len() as u64,
            "No data over the last {n} week",
            "No data over the last {n} weeks",
        )
    } else {
        t.caption.clone()
    };
    ui.label(muted(theme, caption).size(theme.text_size - 2.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_count_back_from_now() {
        let now = Utc::now();
        const WEEKS: usize = 6;
        assert_eq!(week_of(now, now, WEEKS), Some(WEEKS - 1));
        assert_eq!(
            week_of(now - chrono::Duration::days(8), now, WEEKS),
            Some(WEEKS - 2)
        );
        assert_eq!(week_of(now - chrono::Duration::days(60), now, WEEKS), None);
        let weeks = by_week(
            vec![
                (now, 1.0),
                (now, 1.0),
                (now - chrono::Duration::days(9), 1.0),
            ],
            now,
            WEEKS,
            true,
        );
        assert_eq!(weeks[WEEKS - 1], Some(2.0));
        assert_eq!(weeks[WEEKS - 2], Some(1.0));
    }

    #[tokio::test]
    async fn the_demo_waits_most_on_its_authors() {
        let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        app.config.gitlab_username = Some("thomas_db".into());
        let stages = stages_now(&app);
        let total: usize = stages.iter().map(|s| s.count).sum();
        assert!(total > 0);
        // Review, author, merge, then !101 from staging to production.
        assert_eq!(stages.len(), 4);
        assert_eq!(stages[3].name, "On staging, not on production");
        assert_eq!(
            stage_members(&app, &stages[3].stage.key()).map(|v| v.len()),
            Some(1)
        );
        assert!(stage_members(&app, "Blocked").is_none());
    }
}
