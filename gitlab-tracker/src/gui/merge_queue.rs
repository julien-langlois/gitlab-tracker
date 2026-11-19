//! The merge queue: which MRs are about to merge, in what order, and when.
//!
//! On GitLab Premium with merge trains, the real train ([`TrainCar`]): each
//! car's pipeline runs on top of every car before it, and a car merges once
//! its pipeline passes and the cars before it merged. On GitLab Free, or a
//! project without trains, a simulated queue: the MRs set to auto-merge (each
//! merges when its own pipeline passes), in the order the app expects them to
//! merge. It says so wherever it shows: GitLab keeps no such order.
//!
//! The times come from the median of the tracked MRs' passed pipelines over
//! the last two weeks.

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, paint_text};
use super::tip::{Tip, TipExt};
use crate::app::App;
use crate::gitlab_detail::{ProjectOverview, TrainCar};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, MergeabilityStatus, PipelineState, TrackedMr};

/// Where the order comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// GitLab's merge train.
    Train,
    /// The app's estimate from the MRs set to auto-merge.
    Simulated,
}

/// Where a car stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// Its pipeline runs, since then.
    Running(DateTime<Utc>),
    /// Its pipeline has not started.
    Waiting,
    /// Its pipeline passed: it merges now.
    Passed,
    /// It will not merge as it is: why.
    Stuck(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Car {
    pub iid: String,
    pub title: String,
    pub state: State,
    /// When it should be on the target branch.
    pub eta: Option<DateTime<Utc>>,
    /// When it joined the train (none for a simulated queue).
    pub joined: Option<DateTime<Utc>>,
}

/// The queue into one target branch.
#[derive(Debug, Clone, PartialEq)]
pub struct Queue {
    pub target: String,
    pub source: Source,
    pub cars: Vec<Car>,
    /// Ready to merge but not set to: open, mergeable, pipeline passed
    /// (simulated queues only), by iid.
    pub ready: Vec<String>,
    /// The median pipeline the times come from, in seconds.
    pub median: Option<i64>,
}

/// How far back the pipelines count for the median.
const MEDIAN_DAYS: i64 = 14;

/// The median duration of the tracked MRs' passed pipelines over the last
/// [`MEDIAN_DAYS`], in seconds.
fn median_pipeline(app: &App, now: DateTime<Utc>) -> Option<i64> {
    let mut seconds: Vec<i64> = app
        .mrs
        .iter()
        .flat_map(|m| m.pipelines.iter())
        .filter(|p| p.status == PipelineState::Success)
        .filter_map(|p| {
            let start = board::parse_time(p.created_at.as_deref()?)?;
            let end = board::parse_time(p.updated_at.as_deref()?)?;
            (now - start <= Duration::days(MEDIAN_DAYS) && end > start)
                .then(|| (end - start).num_seconds())
        })
        .collect();
    if seconds.is_empty() {
        return None;
    }
    seconds.sort_unstable();
    Some(seconds[seconds.len() / 2])
}

/// Why an MR set to auto-merge will not merge as it is.
fn stuck(mr: &TrackedMr) -> Option<&'static str> {
    let why = match mr.mergeability {
        MergeabilityStatus::Conflict => "conflicts",
        MergeabilityStatus::NeedsRebase => "needs a rebase",
        MergeabilityStatus::NotApproved => "not approved",
        MergeabilityStatus::DiscussionsNotResolved => "open threads",
        MergeabilityStatus::RequestedChanges => "changes requested",
        MergeabilityStatus::Draft => "draft",
        _ => match mr.pipelines.first().map(|p| p.status) {
            Some(PipelineState::Failed) => "pipeline failed",
            Some(PipelineState::Canceled) => "pipeline canceled",
            _ => return None,
        },
    };
    Some(why)
}

/// The project's queues: GitLab's trains when it has some, else the queues
/// the MRs set to auto-merge make. Empty when nothing waits.
pub fn queues(app: &App, overview: Option<&ProjectOverview>, now: DateTime<Utc>) -> Vec<Queue> {
    let median = median_pipeline(app, now);
    let trains = overview.map(|o| o.trains.as_slice()).unwrap_or(&[]);
    if !trains.is_empty() {
        return trains_of(trains, median, now);
    }
    simulated(app, median, now)
}

fn trains_of(cars: &[TrainCar], median: Option<i64>, now: DateTime<Utc>) -> Vec<Queue> {
    let mut queues: Vec<Queue> = Vec::new();
    for car in cars {
        let state = match car.pipeline {
            Some(PipelineState::Running) => car
                .pipeline_created_at
                .as_deref()
                .and_then(board::parse_time)
                .map_or(State::Waiting, State::Running),
            Some(PipelineState::Success) => State::Passed,
            Some(PipelineState::Failed) => State::Stuck("pipeline failed"),
            _ => State::Waiting,
        };
        let queue = match queues
            .iter_mut()
            .position(|q| q.target == car.target_branch)
        {
            Some(i) => &mut queues[i],
            None => {
                queues.push(Queue {
                    target: car.target_branch.clone(),
                    source: Source::Train,
                    cars: Vec::new(),
                    ready: Vec::new(),
                    median,
                });
                queues.last_mut().expect("just pushed")
            }
        };
        // Each car runs on top of the ones before: it merges after them.
        let before = queue.cars.last().and_then(|c| c.eta);
        let own = match state {
            State::Running(since) => median.map(|m| (since + Duration::seconds(m)).max(now)),
            State::Waiting => median.map(|m| now + Duration::seconds(m)),
            State::Passed => Some(now),
            State::Stuck(_) => None,
        };
        let eta = match (own, before) {
            (Some(own), Some(before)) => Some(own.max(before)),
            (own, None) if queue.cars.is_empty() => own,
            _ => None,
        };
        queue.cars.push(Car {
            iid: car.iid.clone(),
            title: car.title.clone(),
            state,
            eta,
            joined: board::parse_time(&car.added_at),
        });
    }
    queues
}

fn simulated(app: &App, median: Option<i64>, now: DateTime<Utc>) -> Vec<Queue> {
    let open = || {
        app.mrs
            .iter()
            .filter(|m| m.state == GitlabMrState::Opened && !m.title.is_empty())
    };
    let mut queues: Vec<Queue> = Vec::new();
    for mr in open().filter(|m| m.auto_merge) {
        let pipeline = mr.pipelines.first();
        let state = match stuck(mr) {
            Some(why) => State::Stuck(why),
            None => match pipeline.map(|p| p.status) {
                Some(PipelineState::Success) | None => State::Passed,
                Some(PipelineState::Running) => pipeline
                    .and_then(|p| p.created_at.as_deref())
                    .and_then(board::parse_time)
                    .map_or(State::Waiting, State::Running),
                _ => State::Waiting,
            },
        };
        let eta = match state {
            State::Passed => Some(now),
            State::Running(since) => median.map(|m| (since + Duration::seconds(m)).max(now)),
            State::Waiting => median.map(|m| now + Duration::seconds(m)),
            State::Stuck(_) => None,
        };
        let car = Car {
            iid: mr.id.clone(),
            title: mr.title.clone(),
            state,
            eta,
            joined: None,
        };
        match queues.iter_mut().find(|q| q.target == mr.target_branch) {
            Some(q) => q.cars.push(car),
            None => queues.push(Queue {
                target: mr.target_branch.clone(),
                source: Source::Simulated,
                cars: vec![car],
                ready: Vec::new(),
                median,
            }),
        }
    }
    for q in &mut queues {
        // Soonest first; the stuck ones last.
        q.cars.sort_by_key(|c| (c.eta.is_none(), c.eta));
        q.ready = open()
            .filter(|m| !m.auto_merge && m.target_branch == q.target)
            .filter(|m| m.mergeability == MergeabilityStatus::Mergeable)
            .filter(|m| m.pipelines.first().map(|p| p.status) == Some(PipelineState::Success))
            .map(|m| m.id.clone())
            .collect();
    }
    queues
}

/// The MRs on a GitLab train (not a simulated queue), by id: their car (from
/// 1), the train's length, and when they joined it.
pub fn train_places(queues: &[Queue]) -> std::collections::HashMap<String, crate::app::TrainPlace> {
    queues
        .iter()
        .filter(|q| q.source == Source::Train)
        .flat_map(|q| {
            q.cars.iter().enumerate().map(move |(i, c)| {
                (
                    c.iid.clone(),
                    crate::app::TrainPlace {
                        car: i + 1,
                        of: q.cars.len(),
                        joined: c.joined,
                    },
                )
            })
        })
        .collect()
}

/// The queue `iid` is in, and its place (0: first).
pub fn place<'a>(queues: &'a [Queue], iid: &str) -> Option<(&'a Queue, usize)> {
    queues
        .iter()
        .find_map(|q| q.cars.iter().position(|c| c.iid == iid).map(|i| (q, i)))
}

/// The table's cell: "2/4", in blue for a train, gray for an estimate, red
/// when stuck; its tooltip.
pub fn cell(queues: &[Queue], iid: &str, theme: &Theme) -> Option<(String, Tone, String)> {
    let (q, i) = place(queues, iid)?;
    let car = &q.cars[i];
    let text = format!("{}/{}", i + 1, q.cars.len());
    let (tone, hint) = match (car.state, q.source) {
        (State::Stuck(why), _) => (
            theme.tones.red,
            tf("In the merge queue, stuck: {why}", &[("why", tl(why))]),
        ),
        (_, Source::Train) => (
            theme.tones.blue,
            tf(
                "On the merge train to {target}, car {n}",
                &[("target", &q.target), ("n", &(i + 1).to_string())],
            ),
        ),
        (_, Source::Simulated) => (
            theme.tones.gray,
            tf(
                "Set to auto-merge into {target}: {n} in the estimated order (simulated, GitLab Free has no train)",
                &[("target", &q.target), ("n", &(i + 1).to_string())],
            ),
        ),
    };
    Some((text, tone, hint))
}

fn state_text(state: State, now: DateTime<Utc>) -> String {
    match state {
        State::Running(since) => tf(
            "running {d}",
            &[("d", &crate::freeze::left_text(since, now))],
        ),
        State::Waiting => tl("waiting").to_string(),
        State::Passed => tl("passed, merging").to_string(),
        State::Stuck(why) => tf("stuck: {why}", &[("why", tl(why))]),
    }
}

fn eta_text(eta: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match eta {
        Some(at) if at <= now + Duration::minutes(1) => tl("any moment").to_string(),
        Some(at) => tf("~{left}", &[("left", &crate::freeze::left_text(now, at))]),
        None => board::dash().to_string(),
    }
}

/// The card of the MR page: the queue the MR is in, each car in turn, the MR
/// picked out, and what it means for it. Nothing when it is in none.
pub fn card(
    ui: &mut Ui,
    theme: &Theme,
    queues: &[Queue],
    mr: &TrackedMr,
    now: DateTime<Utc>,
) -> bool {
    let Some((q, index)) = place(queues, &mr.id) else {
        return false;
    };
    let me = &q.cars[index];
    let title = match q.source {
        Source::Train => tf(
            "Merge train to {target} · {n}",
            &[
                ("target", &q.target),
                ("n", &tn(q.cars.len() as u64, "{n} car", "{n} cars")),
            ],
        ),
        Source::Simulated => tf(
            "Merge queue to {target} · {n}",
            &[
                ("target", &q.target),
                ("n", &tn(q.cars.len() as u64, "{n} MR", "{n} MRs")),
            ],
        ),
    };
    let note = match q.source {
        Source::Train => tl("a car merges when its pipeline, with every car before it, passes"),
        Source::Simulated => tl("simulated: GitLab Free has no merge train"),
    };
    board::card(ui, theme, &title, note, 0.0, |ui| {
        strip(ui, theme, q, index, now);
        ui.add_space(12.0);
        // What it means for this MR.
        let ahead = index;
        let (line, tone) = match (me.state, me.eta) {
            (State::Stuck(why), _) => (
                tf(
                    "Stuck: {why}. It will not merge until that is fixed.",
                    &[("why", tl(why))],
                ),
                theme.tones.red,
            ),
            (_, Some(eta)) => {
                let when = crate::snooze::when_text(eta, &chrono::Local::now());
                let line = if ahead == 0 {
                    tf(
                        "First in line: on {target} around {when}",
                        &[("target", &q.target), ("when", &when)],
                    )
                } else {
                    tf(
                        "{ahead} · expected on {target} around {when}",
                        &[
                            ("ahead", &tn(ahead as u64, "{n} ahead", "{n} ahead")),
                            ("target", &q.target),
                            ("when", &when),
                        ],
                    )
                };
                (line, theme.tones.blue)
            }
            (_, None) => (tn(ahead as u64, "{n} ahead", "{n} ahead"), theme.tones.blue),
        };
        ui.label(
            egui::RichText::new(line)
                .font(fonts::semibold(theme.text_size))
                .color(tone.fg),
        );
        let mut notes = Vec::new();
        match q.source {
            Source::Train => notes.push(
                tl("A car whose pipeline fails drops out, and every car behind it restarts its pipeline.")
                    .to_string(),
            ),
            Source::Simulated => notes.push(
                tl("Each MR set to auto-merge merges when its own pipeline passes: the order and the times are the app's estimate.")
                    .to_string(),
            ),
        }
        if let Some(m) = q.median {
            notes.push(tf(
                "Times from the median pipeline of the last two weeks ({d}).",
                &[("d", &board::seconds_text(m as f64))],
            ));
        }
        if !q.ready.is_empty() {
            let ids: Vec<String> = q.ready.iter().map(|i| format!("!{i}")).collect();
            notes.push(tf(
                "Ready to merge, not set to: {ids}",
                &[("ids", &ids.join(", "))],
            ));
        }
        ui.add_space(4.0);
        for n in notes {
            ui.add(egui::Label::new(super::widgets::muted(theme, n)).wrap());
        }
    });
    true
}

/// The target branch, then each car from the first, the MR's own picked out.
fn strip(ui: &mut Ui, theme: &Theme, q: &Queue, mine: usize, now: DateTime<Utc>) {
    let width = ui.available_width();
    let arrow = 22.0;
    let head_w = 96.0;
    let n = q.cars.len().max(1);
    let car_w = ((width - head_w - arrow * n as f32) / n as f32).clamp(110.0, 220.0);
    // As many cars as fit; the rest counted.
    let fit = (((width - head_w) / (car_w + arrow)).floor() as usize).max(1);
    let shown = q.cars.len().min(fit);
    let height = 70.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = CornerRadius::same(theme.radii.medium);
    let head = Rect::from_min_size(rect.min, Vec2::new(head_w, height));
    ui.painter().rect(
        head,
        radius,
        theme.palette.border_soft.gamma_multiply(0.6),
        Stroke::NONE,
        egui::StrokeKind::Inside,
    );
    paint_text(
        ui,
        egui::pos2(head.left() + 10.0, head.top() + 22.0),
        Align2::LEFT_CENTER,
        &q.target,
        FontId::monospace(theme.text_size - 1.0),
        theme.palette.text,
        head_w - 16.0,
    );
    paint_text(
        ui,
        egui::pos2(head.left() + 10.0, head.top() + 44.0),
        Align2::LEFT_CENTER,
        tl("target"),
        FontId::proportional(theme.text_size - 2.5),
        theme.palette.text_muted,
        head_w - 16.0,
    );
    let mut x = head.right();
    for (i, car) in q.cars.iter().take(shown).enumerate() {
        // The arrow: each car follows the one on its left.
        let cy = rect.center().y;
        ui.painter().text(
            egui::pos2(x + arrow / 2.0, cy),
            Align2::CENTER_CENTER,
            "←",
            FontId::proportional(theme.text_size),
            theme.palette.text_muted,
        );
        x += arrow;
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(car_w, height));
        let tone = match car.state {
            State::Stuck(_) => theme.tones.red,
            State::Running(_) | State::Passed => theme.tones.blue,
            State::Waiting => theme.tones.gray,
        };
        let own = i == mine;
        ui.painter().rect(
            r,
            radius,
            theme.palette.panel,
            Stroke::new(
                if own { 2.0 } else { 1.0 },
                if own {
                    theme.palette.accent
                } else {
                    tone.fg.gamma_multiply(0.45)
                },
            ),
            egui::StrokeKind::Inside,
        );
        let inner = r.shrink2(Vec2::new(10.0, 0.0));
        let w = paint_text(
            ui,
            egui::pos2(inner.left(), r.top() + 16.0),
            Align2::LEFT_CENTER,
            &format!("!{}", car.iid),
            FontId::monospace(theme.text_size - 1.0),
            theme.palette.text,
            inner.width() * 0.4,
        );
        paint_text(
            ui,
            egui::pos2(inner.left() + w + 8.0, r.top() + 16.0),
            Align2::LEFT_CENTER,
            &state_text(car.state, now),
            fonts::medium(theme.text_size - 2.5),
            tone.fg,
            inner.width() - w - 8.0,
        );
        paint_text(
            ui,
            egui::pos2(inner.left(), r.top() + 35.0),
            Align2::LEFT_CENTER,
            &car.title,
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_secondary,
            inner.width(),
        );
        let eta = if own {
            tf("yours · {eta}", &[("eta", &eta_text(car.eta, now))])
        } else {
            eta_text(car.eta, now)
        };
        paint_text(
            ui,
            egui::pos2(inner.left(), r.top() + 54.0),
            Align2::LEFT_CENTER,
            &eta,
            FontId::proportional(theme.text_size - 2.5),
            if own {
                theme.palette.accent
            } else {
                theme.palette.text_muted
            },
            inner.width(),
        );
        ui.interact(
            r,
            ui.scope_id().with(("queue_car", &car.iid)),
            Sense::hover(),
        )
        .rich(Tip::new().mr(&car.iid, &car.title));
        x += car_w;
    }
    if shown < q.cars.len() {
        paint_text(
            ui,
            egui::pos2(x + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            &format!("+{}", q.cars.len() - shown),
            fonts::medium(theme.text_size - 1.0),
            theme.palette.text_muted,
            60.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn car(iid: &str, pipeline: Option<PipelineState>, created: Option<&str>) -> TrainCar {
        TrainCar {
            iid: iid.into(),
            title: format!("MR {iid}"),
            target_branch: "main".into(),
            added_at: String::new(),
            pipeline,
            pipeline_created_at: created.map(str::to_string),
            user: String::new(),
        }
    }

    #[test]
    fn a_train_car_merges_after_the_ones_before_it() {
        let now = utc("2026-11-18T12:00:00Z");
        let q = &trains_of(
            &[
                car(
                    "1",
                    Some(PipelineState::Running),
                    Some("2026-11-18T11:46:00Z"),
                ),
                car(
                    "2",
                    Some(PipelineState::Running),
                    Some("2026-11-18T11:51:00Z"),
                ),
                car("3", Some(PipelineState::Pending), None),
            ],
            Some(20 * 60),
            now,
        )[0];
        assert_eq!(q.source, Source::Train);
        let etas: Vec<_> = q.cars.iter().map(|c| c.eta.unwrap()).collect();
        assert_eq!(etas[0], utc("2026-11-18T12:06:00Z"));
        assert_eq!(etas[1], utc("2026-11-18T12:11:00Z"));
        // Waiting: its pipeline would start now.
        assert_eq!(etas[2], utc("2026-11-18T12:20:00Z"));
        assert_eq!(place(std::slice::from_ref(q), "2").map(|(_, i)| i), Some(1));
    }

    #[test]
    fn a_failed_car_has_no_time_and_neither_have_those_behind() {
        let now = utc("2026-11-18T12:00:00Z");
        let q = &trains_of(
            &[
                car("1", Some(PipelineState::Failed), None),
                car(
                    "2",
                    Some(PipelineState::Running),
                    Some("2026-11-18T11:51:00Z"),
                ),
            ],
            Some(600),
            now,
        )[0];
        assert_eq!(q.cars[0].state, State::Stuck("pipeline failed"));
        assert_eq!(q.cars[1].eta, None);
    }
}
