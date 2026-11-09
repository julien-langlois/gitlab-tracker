//! A person's page: the person picked on the team page, on a page of its own
//! (double click or Enter on their row, Esc back, J / K to the next or
//! previous person of the list).
//!
//! What waits on them now, as the team page's inspector shows it; then what
//! that page has no room for: their activity over twelve weeks, with whom they
//! work, where their MRs spend their time, and how they review. Everything
//! comes from the MRs already tracked, compared with the team's median, never
//! ranked.

use std::collections::BTreeMap;

use bevel_theme::{fonts, Theme};
use chrono::{DateTime, Duration, Utc};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Sense, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::team::{self, Analysis, Level, Person, PERIODS};
use super::widgets::{avatar, button, muted, primary_button, semibold};
use super::{GuiApp, Page};
use crate::app::App;
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{Effort, GitlabMrState, PipelineState};
use crate::next_step::{login, median};

/// The weeks the activity chart shows.
const WEEKS: usize = 12;
/// A review answered within this many days is a quick one.
const QUICK: f64 = 4.0 / 24.0;

/// One week of someone's activity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Week {
    /// Others' MRs they last commented on or approved that week.
    pub reviews: u32,
    /// Their MRs merged that week.
    pub merged: u32,
    /// Reviews last asked of them that week.
    pub asked: u32,
}

/// One of their MRs, the latest first.
#[derive(Debug, Clone)]
pub struct Recent {
    pub id: String,
    pub title: String,
    pub state: GitlabMrState,
    pub at: Option<DateTime<Utc>>,
}

/// What the tracked MRs tell about someone beyond what waits on them.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    /// Tracked MRs they appear in: author, reviewer, approver, commenter.
    pub seen: usize,
    /// Their MRs merged in the period, and in the period before.
    pub merged: u32,
    pub merged_before: u32,
    /// Median days from creation to merge: of their MRs, of the team's.
    pub cycle: Option<f64>,
    pub team_cycle: Option<f64>,
    /// Oldest first.
    pub weeks: [Week; WEEKS],
    /// Median days from the first review request to the first review, on
    /// their MRs first reviewed in the period.
    pub first_review: Option<f64>,
    /// Pushes after the first review, on average, on the same MRs.
    pub rounds: Option<f64>,
    /// Their MRs' pipelines started in the period: failed, finished.
    pub pipelines: (u32, u32),
    /// Median days of their MRs merged in the period: waiting for a review,
    /// from the first review to the approval, from the approval to the merge.
    pub stages: [Option<f64>; 3],
    /// Their open MRs and those opened in the period, by effort: easy,
    /// medium, complex.
    pub efforts: [u32; 3],
    /// The labels of the same MRs, the most used first.
    pub labels: Vec<(String, u32)>,
    /// Reviews asked of them in the period, those they approved, and the
    /// threads they opened on others' MRs.
    pub requested: u32,
    pub approved: u32,
    pub threads_opened: u32,
    /// The requests of the period answered within 4 h, 1 d, 3 d, later.
    pub buckets: [u32; 4],
    /// The requests of the period they have not answered.
    pub unanswered: u32,
    pub recent: Vec<Recent>,
}

/// Works out `who`'s profile from `app`'s MRs at `now`, over the last `days`.
pub fn profile(app: &App, who: &str, now: DateTime<Utc>, days: i64) -> Profile {
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let recent_enough = |t: Option<DateTime<Utc>>| t.is_some_and(|t| t >= start);
    let week_of = |t: DateTime<Utc>| -> Option<usize> {
        let ago = (now - t).num_days().div_euclid(7);
        (0..WEEKS as i64)
            .contains(&ago)
            .then(|| WEEKS - 1 - ago as usize)
    };
    let span = |from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>| -> Option<f64> {
        let (from, to) = (from?, to?);
        (to >= from).then(|| board::days_between(from, to))
    };
    let difficulty = &app.config.complexity_profile;
    let mut p = Profile::default();
    let mut cycles = Vec::new();
    let mut team_cycles = Vec::new();
    let mut first_reviews = Vec::new();
    let mut rounds = Vec::new();
    let mut stages: [Vec<f64>; 3] = Default::default();
    let mut labels: BTreeMap<String, u32> = BTreeMap::new();
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let author = login(&mr.author);
        let created = board::time_of(mr.created_at.as_deref());
        let merged = (mr.state == GitlabMrState::Merged)
            .then(|| board::time_of(mr.merged_at.as_deref()))
            .flatten();
        let cycle = span(created, merged);
        if recent_enough(merged) && !board::is_bot(author) {
            team_cycles.extend(cycle);
        }
        let review = mr.review.as_ref();
        let involved = author == who
            || mr.reviewers.iter().any(|r| login(r) == who)
            || mr.approved_by.iter().any(|a| a == who)
            || mr.threads.iter().any(|t| t.author == who)
            || review.is_some_and(|r| {
                r.last_activity_by.contains_key(who) || r.review_requested_at.contains_key(who)
            });
        if !involved {
            continue;
        }
        p.seen += 1;

        if author == who {
            let asked = review
                .and_then(|r| board::time_of(r.first_review_requested_at.as_deref()))
                .or(created);
            let first = review.and_then(|r| board::time_of(r.first_review_at.as_deref()));
            let approved = review.and_then(|r| board::time_of(r.approved_at.as_deref()));
            if let Some(at) = merged {
                if at >= start {
                    p.merged += 1;
                    cycles.extend(cycle);
                    stages[0].extend(span(asked, first));
                    stages[1].extend(span(first, approved));
                    stages[2].extend(span(approved, merged));
                } else if at >= before {
                    p.merged_before += 1;
                }
                if let Some(w) = week_of(at) {
                    p.weeks[w].merged += 1;
                }
            }
            if recent_enough(first) {
                first_reviews.extend(span(asked, first));
                rounds.push(f64::from(review.map_or(0, |r| r.pushes_after_review)));
            }
            for pipeline in &mr.pipelines {
                if !recent_enough(board::time_of(pipeline.created_at.as_deref())) {
                    continue;
                }
                match pipeline.status {
                    PipelineState::Failed => {
                        p.pipelines.0 += 1;
                        p.pipelines.1 += 1;
                    }
                    PipelineState::Success => p.pipelines.1 += 1,
                    _ => {}
                }
            }
            if mr.state == GitlabMrState::Opened || recent_enough(created) {
                if let Some(diff) = &mr.diff_stats {
                    let i = match Effort::from_score(diff.difficulty(difficulty)) {
                        Effort::Easy => 0,
                        Effort::Medium => 1,
                        Effort::Complex => 2,
                    };
                    p.efforts[i] += 1;
                }
                for label in &mr.labels {
                    *labels.entry(label.clone()).or_default() += 1;
                }
            }
            p.recent.push(Recent {
                id: mr.id.clone(),
                title: mr.title.clone(),
                state: mr.state,
                at: board::time_of(mr.updated_at.as_deref())
                    .or(merged)
                    .or(created),
            });
            continue;
        }

        // Someone else's MR: as a reviewer.
        if let Some(r) = review {
            let last = r
                .last_activity_by
                .get(who)
                .and_then(|at| board::parse_time(at));
            if let Some(w) = last.and_then(week_of) {
                p.weeks[w].reviews += 1;
            }
            if let Some(asked) = r
                .review_requested_at
                .get(who)
                .and_then(|at| board::parse_time(at))
            {
                if let Some(w) = week_of(asked) {
                    p.weeks[w].asked += 1;
                }
                if asked >= start {
                    p.requested += 1;
                    if mr.approved_by.iter().any(|a| a == who) {
                        p.approved += 1;
                    }
                    match span(Some(asked), last) {
                        Some(d) => {
                            let i = if d < QUICK {
                                0
                            } else if d < 1.0 {
                                1
                            } else if d < 3.0 {
                                2
                            } else {
                                3
                            };
                            p.buckets[i] += 1;
                        }
                        None => p.unanswered += 1,
                    }
                }
            }
        }
        p.threads_opened += mr
            .threads
            .iter()
            .filter(|t| t.author == who && recent_enough(board::parse_time(&t.created_at)))
            .count() as u32;
    }
    p.cycle = median(cycles);
    p.team_cycle = median(team_cycles);
    p.first_review = median(first_reviews);
    p.rounds = (!rounds.is_empty()).then(|| rounds.iter().sum::<f64>() / rounds.len() as f64);
    p.stages = stages.map(median);
    let mut labels: Vec<(String, u32)> = labels.into_iter().collect();
    labels.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    labels.truncate(4);
    p.labels = labels;
    p.recent.sort_by_key(|r| std::cmp::Reverse(r.at));
    p.recent.truncate(8);
    p
}

/// People by login, with a count.
type Counts = Vec<(String, u32)>;

/// Who asks `who` for reviews (`asked_by`) and whom they ask (`asks`), over
/// the period, the most first.
fn partners(analysis: &Analysis, who: &str) -> (Counts, Counts) {
    let mut asked_by: Counts = analysis
        .matrix
        .iter()
        .filter_map(|(author, row)| row.get(who).map(|n| (author.clone(), *n)))
        .filter(|(_, n)| *n > 0)
        .collect();
    let mut asks: Counts = analysis
        .matrix
        .get(who)
        .map(|row| row.iter().map(|(r, n)| (r.clone(), *n)).collect())
        .unwrap_or_default();
    for list in [&mut asked_by, &mut asks] {
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list.truncate(5);
    }
    (asked_by, asks)
}

/// When most of someone's requests go to `person` and `person` is loaded: who
/// could take some, preferring those who already reviewed that author.
fn relief(analysis: &Analysis, person: &Person) -> Option<String> {
    if !matches!(person.level, Level::Overloaded | Level::Busy) {
        return None;
    }
    let (author, n, total) = analysis
        .matrix
        .iter()
        .filter_map(|(author, row)| {
            let n = *row.get(&person.login)?;
            Some((author, n, row.values().sum::<u32>()))
        })
        .filter(|(_, n, total)| *total >= 3 && f64::from(*n) / f64::from(*total) >= 0.5)
        .max_by_key(|(_, n, _)| *n)?;
    let name = |login: &str| -> String {
        analysis
            .people
            .iter()
            .find(|p| p.login == login)
            .map_or(login.to_string(), |p| p.name.clone())
    };
    let author_name = name(author);
    let share = board::percent(f64::from(n) / f64::from(total));
    let candidates = analysis.people.iter().filter(|p| {
        p.login != person.login
            && p.login != *author
            && matches!(p.level, Level::Available | Level::Normal)
            && p.load < person.load
    });
    let reviewed = |p: &&Person| {
        analysis
            .matrix
            .get(author)
            .and_then(|row| row.get(&p.login))
            .is_some_and(|n| *n > 0)
    };
    let lightest = |a: &&Person, b: &&Person| a.load.total_cmp(&b.load);
    let known = candidates.clone().filter(reviewed).min_by(lightest);
    let mut text = tf(
        "{share} of {author}'s review requests go to {name}.",
        &[
            ("share", &share),
            ("author", board::first_name(&author_name)),
            ("name", board::first_name(&person.name)),
        ],
    );
    let other = match known {
        Some(p) => tf(
            "{other} already reviews {author} and is less loaded: they could take some.",
            &[
                ("other", board::first_name(&p.name)),
                ("author", board::first_name(&author_name)),
            ],
        ),
        None => match candidates.min_by(lightest) {
            Some(p) => tf(
                "{other} is less loaded and could take some.",
                &[("other", board::first_name(&p.name))],
            ),
            None => String::new(),
        },
    };
    if !other.is_empty() {
        text.push(' ');
        text.push_str(&other);
    }
    Some(text)
}

// ── Page ─────────────────────────────────────────────────────────────────────

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let now = Utc::now();
    let period = g.team.period.min(PERIODS.len() - 1);
    let days = PERIODS[period].0;
    let analysis = team::analyze(&g.app, now, days, g.team.bots);
    let list: Vec<String> = team::ordered(&analysis, &g.team, now)
        .into_iter()
        .map(|p| p.login.clone())
        .collect();
    // J / K: the next or previous person of the list.
    let step = std::mem::take(&mut g.team.step);
    if step != 0 && !list.is_empty() {
        let at = g
            .team
            .selected
            .as_ref()
            .and_then(|s| list.iter().position(|l| l == s));
        let to = match at {
            Some(at) => (at as i64 + i64::from(step)).clamp(0, list.len() as i64 - 1) as usize,
            None => 0,
        };
        g.team.selected = Some(list[to].clone());
    }
    let Some(who) = g.team.selected.clone() else {
        return;
    };
    // Out of the list (a bot, or nobody the period says anything about): the
    // page still shows what there is.
    let person = analysis
        .people
        .iter()
        .find(|p| p.login == who)
        .cloned()
        .unwrap_or_else(|| {
            let mut p = Person::blank(&who);
            p.name = who.clone();
            p
        });
    let profile = profile(&g.app, &who, now, days);
    let reminder = g.app.config.review_reminder_hours.max(1) as f64 / 24.0;

    crumbs(g, ui, &analysis, &list, &who);
    ui.add_space(10.0);
    header(g, ui, &person, now, period);
    ui.add_space(14.0);

    let mut go = Go::None;
    egui::ScrollArea::vertical()
        .id_salt(("person_page", &who))
        .auto_shrink(false)
        .show(ui, |ui| {
            let room = ui.available_width() - 10.0;
            ui.set_width(room);
            ui.set_max_width(room);
            let theme = g.theme;
            figures(ui, &theme, &analysis, &person, &profile, now, reminder);
            board::split(
                ui,
                0.5,
                340.0,
                340.0,
                |ui| now_card(ui, &theme, &person, now, reminder, &mut go),
                |ui| activity_card(ui, &theme, &profile, now),
            );
            ui.add_space(GAP);
            board::split(
                ui,
                0.5,
                340.0,
                340.0,
                |ui| partners_card(ui, &theme, &analysis, &person, &mut go),
                |ui| authoring_card(ui, &theme, &profile),
            );
            ui.add_space(GAP);
            board::split(
                ui,
                0.5,
                340.0,
                340.0,
                |ui| reviewing_card(ui, &theme, &profile),
                |ui| recent_card(ui, &theme, &profile, now, &mut go),
            );
            ui.add_space(GAP);
            board::caption(
                ui,
                &theme,
                &tn(
                    profile.seen as u64,
                    "From the {n} tracked MR they appear in: MRs not tracked are not counted.",
                    "From the {n} tracked MRs they appear in: MRs not tracked are not counted.",
                ),
            );
            ui.add_space(24.0);
        });
    match go {
        Go::None => {}
        Go::Mr(id) => g.open_mr(&id),
        Go::Person(login) => g.open_person(&login),
    }
}

/// Where a click on the page leads.
enum Go {
    None,
    Mr(String),
    Person(String),
}

/// Back to the team, where this person is in its list, and the keys to move.
fn crumbs(g: &mut GuiApp, ui: &mut Ui, analysis: &Analysis, list: &[String], who: &str) {
    let theme = g.theme;
    let at = list.iter().position(|l| l == who);
    let name = |login: &str| -> String {
        analysis
            .people
            .iter()
            .find(|p| p.login == login)
            .map_or(login.to_string(), |p| p.name.clone())
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let back = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(format!("← {}", tl("Team"))).color(theme.palette.accent),
                )
                .sense(Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if back.clicked() {
            g.open_page(Page::Team);
        }
        if let Some(at) = at {
            ui.label(muted(&theme, "/"));
            ui.label(muted(
                &theme,
                tf(
                    "{n} of {total}",
                    &[
                        ("n", &(at + 1).to_string()),
                        ("total", &list.len().to_string()),
                    ],
                ),
            ));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(muted(&theme, tl("back")));
            super::mr_page::key(ui, &theme, "Esc");
            let next = at.and_then(|i| list.get(i + 1));
            let prev = at.and_then(|i| i.checked_sub(1)).and_then(|i| list.get(i));
            if let Some(next) = next {
                ui.label(muted(&theme, "·"));
                super::mr_page::key(ui, &theme, "J");
                ui.label(muted(&theme, board::first_name(&name(next))));
            }
            if let Some(prev) = prev {
                ui.label(muted(&theme, "·"));
                ui.label(muted(&theme, board::first_name(&name(prev))));
                super::mr_page::key(ui, &theme, "K");
            }
        });
    });
}

/// Who, how loaded, the period, and what to do about them.
fn header(g: &mut GuiApp, ui: &mut Ui, person: &Person, now: DateTime<Utc>, period: usize) {
    let theme = g.theme;
    let mut show = false;
    let mut remind = false;
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        avatar(ui, &theme, &person.name, 52.0);
        let actions_width = 470.0_f32.min(ui.available_width() * 0.55);
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width() - actions_width, 52.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add(egui::Label::new(semibold(&person.name, theme.text_size + 7.0)).truncate());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(muted(&theme, format!("@{}", person.login)));
                    board::chip(ui, &theme, person.level.label(), person.level.tone(&theme));
                    let last = match person.last_request {
                        Some(at) => tf(
                            "last asked for a review {when}",
                            &[("when", &board::ago(at, now))],
                        ),
                        None => tl("no review asked lately").to_string(),
                    };
                    ui.add(egui::Label::new(muted(&theme, last)).truncate());
                });
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            show = primary_button(ui, &theme, tl("Show their MRs")).clicked();
            remind = button(
                ui,
                &theme,
                Some(super::icons::Icon::Copy),
                tl("Copy a reminder"),
                false,
            )
            .clicked();
            ui.allocate_ui(Vec2::new(150.0, 28.0), |ui| {
                let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
                pick = board::segmented(ui, &theme, "person_period", &labels, period);
            });
        });
    });
    if let Some(i) = pick {
        g.team.period = i;
    }
    if show {
        team::show_mrs(g, person);
    } else if remind {
        team::copy_reminder(g, ui, person, now);
    }
}

/// The figures: what waits now, and how they compare with the team.
fn figures(
    ui: &mut Ui,
    theme: &Theme,
    analysis: &Analysis,
    person: &Person,
    profile: &Profile,
    now: DateTime<Utc>,
    reminder: f64,
) {
    let t = &theme.tones;
    let oldest = person.oldest().map(|s| board::days_between(s, now));
    let on_them = person
        .mrs
        .iter()
        .filter(|m| m.step.actor == crate::next_step::Actor::Author)
        .count();
    let threads_mrs = person
        .mrs
        .iter()
        .filter(|m| {
            m.step
                .todos
                .iter()
                .any(|t| matches!(t, crate::next_step::Todo::AnswerThreads { .. }))
        })
        .count();
    let duration =
        |d: Option<f64>| d.map_or_else(|| board::dash().to_string(), board::duration_text);
    // Slower than the team by half again: amber; faster: green.
    let against = |mine: Option<f64>, team: Option<f64>| -> (String, Option<Color32>) {
        match (mine, team) {
            (_, Some(team_value)) => (
                tf("team: {d}", &[("d", &board::duration_text(team_value))]),
                mine.and_then(|m| {
                    if m > team_value * 1.5 {
                        Some(t.amber.fg)
                    } else if m < team_value {
                        Some(t.green.fg)
                    } else {
                        None
                    }
                }),
            ),
            _ => (tl("Nothing to compare with yet").to_string(), None),
        }
    };
    let (response_caption, response_color) = against(person.response, analysis.response);
    let (cycle_caption, cycle_color) = against(profile.cycle, profile.team_cycle);
    let figures = [
        Figure {
            label: tl("Reviews waiting on them").into(),
            value: person.reviews.len().to_string(),
            value_color: None,
            caption: match oldest {
                Some(d) => tf("the oldest: {time}", &[("time", &board::duration_text(d))]),
                None => tl("None").into(),
            },
            caption_color: oldest.filter(|d| *d >= reminder).map(|_| t.red.fg),
        },
        Figure {
            label: tl("Their open MRs").into(),
            value: person.mrs.len().to_string(),
            value_color: None,
            caption: tn(on_them as u64, "{n} waits on them", "{n} wait on them"),
            caption_color: (on_them > 0).then_some(t.amber.fg),
        },
        Figure {
            label: tl("Threads to answer").into(),
            value: person.threads.to_string(),
            value_color: None,
            caption: tn(threads_mrs as u64, "on {n} MR", "on {n} MRs"),
            caption_color: None,
        },
        Figure {
            label: tl("Review response").into(),
            value: duration(person.response),
            value_color: None,
            caption: response_caption,
            caption_color: response_color,
        },
        Figure {
            label: tl("Their MRs merged").into(),
            value: profile.merged.to_string(),
            value_color: None,
            caption: tf(
                "period before: {n}",
                &[("n", &profile.merged_before.to_string())],
            ),
            caption_color: None,
        },
        Figure {
            label: tl("Created → merged").into(),
            value: duration(profile.cycle),
            value_color: None,
            caption: cycle_caption,
            caption_color: cycle_color,
        },
    ];
    board::figures(ui, theme, &figures);
}

/// What waits on them, and what their MRs wait on.
fn now_card(
    ui: &mut Ui,
    theme: &Theme,
    person: &Person,
    now: DateTime<Utc>,
    reminder: f64,
    go: &mut Go,
) {
    board::card(ui, theme, tl("Now"), "", 0.0, |ui| {
        let mut open = None;
        ui.label(semibold(tl("Waiting on them"), theme.text_size - 1.0));
        ui.add_space(4.0);
        team::waiting_list(ui, theme, person, now, reminder, &mut open);
        ui.add_space(10.0);
        ui.label(semibold(tl("Their MRs"), theme.text_size - 1.0));
        ui.add_space(4.0);
        if person.mrs.is_empty() {
            ui.label(muted(theme, tl("No open MR.")));
        }
        for m in &person.mrs {
            let caption = super::next::caption(&m.step);
            let summary = m.step.summary();
            let detail = if summary.is_empty() {
                caption
            } else {
                format!("{caption} · {summary}")
            };
            if team::line(
                ui,
                theme,
                &m.id,
                &m.title,
                &detail,
                Some(super::next::tone(&m.step, theme).fg),
            ) {
                open = Some(m.id.clone());
            }
        }
        if let Some(id) = open {
            *go = Go::Mr(id);
        }
    });
}

/// Twelve weeks: reviews given and MRs merged in bars, requests in a line.
fn activity_card(ui: &mut Ui, theme: &Theme, profile: &Profile, now: DateTime<Utc>) {
    let reviews_color = theme.palette.accent;
    let merged_color = theme.tones.purple.fg;
    let asked_color = theme.tones.red.fg;
    board::card(ui, theme, tl("Activity"), tl("12 weeks"), 0.0, |ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), Sense::hover());
        let max = profile
            .weeks
            .iter()
            .flat_map(|w| [w.reviews, w.merged, w.asked])
            .max()
            .unwrap_or(0)
            .max(1) as f32;
        let painter = ui.painter();
        let plot = Rect::from_min_max(
            egui::pos2(rect.left() + 22.0, rect.top() + 6.0),
            egui::pos2(rect.right(), rect.bottom() - 2.0),
        );
        let y = |v: f32| plot.bottom() - plot.height() * v / max;
        // The grid: zero, and the top value.
        let grid = egui::Stroke::new(1.0, theme.palette.border_soft);
        painter.hline(plot.x_range(), plot.bottom(), grid);
        painter.hline(plot.x_range(), y(max), grid);
        let small = FontId::proportional(theme.text_size - 2.5);
        for v in [0.0, max] {
            painter.text(
                egui::pos2(plot.left() - 6.0, y(v)),
                Align2::RIGHT_CENTER,
                format!("{v:.0}"),
                small.clone(),
                theme.palette.text_muted,
            );
        }
        let slot = plot.width() / WEEKS as f32;
        let bar = ((slot - 6.0) / 2.0).clamp(3.0, 12.0);
        let mut points = Vec::with_capacity(WEEKS);
        for (i, w) in profile.weeks.iter().enumerate() {
            let cx = plot.left() + slot * (i as f32 + 0.5);
            for (n, color, dx) in [
                (w.reviews, reviews_color, -bar - 1.0),
                (w.merged, merged_color, 1.0),
            ] {
                if n == 0 {
                    continue;
                }
                let r = Rect::from_min_max(
                    egui::pos2(cx + dx, y(n as f32)),
                    egui::pos2(cx + dx + bar, plot.bottom()),
                );
                painter.rect_filled(
                    r,
                    CornerRadius {
                        nw: 2,
                        ne: 2,
                        sw: 0,
                        se: 0,
                    },
                    color,
                );
            }
            points.push(egui::pos2(cx, y(w.asked as f32)));
            let monday = now - Duration::weeks((WEEKS - 1 - i) as i64);
            ui.interact(
                Rect::from_center_size(
                    egui::pos2(cx, plot.center().y),
                    Vec2::new(slot, plot.height()),
                ),
                ui.scope_id().with(("week", i)),
                Sense::hover(),
            )
            .on_hover_text(format!(
                "{}\n{}\n{}\n{}",
                tf(
                    "Week of {date}",
                    &[("date", &monday.format("%d/%m").to_string())]
                ),
                tn(
                    u64::from(w.reviews),
                    "{n} review given",
                    "{n} reviews given"
                ),
                tn(
                    u64::from(w.merged),
                    "{n} MR of theirs merged",
                    "{n} MRs of theirs merged"
                ),
                tn(
                    u64::from(w.asked),
                    "{n} review asked of them",
                    "{n} reviews asked of them"
                ),
            ));
        }
        let painter = ui.painter();
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(2.0, asked_color),
        ));
        if let Some(last) = points.last() {
            painter.circle_filled(*last, 3.5, asked_color);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(muted(theme, tl("12 weeks ago")).size(theme.text_size - 2.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(muted(theme, tl("this week")).size(theme.text_size - 2.0));
            });
        });
        ui.add_space(4.0);
        board::legend(
            ui,
            theme,
            &[
                (reviews_color, tl("reviews given").to_string()),
                (merged_color, tl("their MRs merged").to_string()),
                (asked_color, tl("reviews asked of them").to_string()),
            ],
        );
        // The last four weeks: do the requests outrun the reviews?
        let last: Vec<&Week> = profile.weeks.iter().rev().take(4).collect();
        let asked: u32 = last.iter().map(|w| w.asked).sum();
        let given: u32 = last.iter().map(|w| w.reviews).sum();
        if asked > 0 || given > 0 {
            ui.add_space(6.0);
            let text = if asked >= 4 && f64::from(asked) > f64::from(given) * 1.3 {
                tf(
                    "Over the last 4 weeks, {asked} reviews were asked of them and they gave {given}: the queue grows.",
                    &[("asked", &asked.to_string()), ("given", &given.to_string())],
                )
            } else {
                tf(
                    "Over the last 4 weeks: {given} reviews given for {asked} asked.",
                    &[("asked", &asked.to_string()), ("given", &given.to_string())],
                )
            };
            board::caption(ui, theme, &text);
        }
    });
}

/// A horizontal bar: `label`, a track filled to `value / max`, `text` on the
/// right. Clickable when `link`.
#[allow(clippy::too_many_arguments)]
fn bar(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    value: f32,
    max: f32,
    text: &str,
    color: Color32,
    link: bool,
) -> egui::Response {
    let sense = if link { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), sense);
    if link && response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(4.0, 0.0)),
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let label_w = (rect.width() * 0.36).clamp(90.0, 150.0);
    let text_w = 56.0;
    let cy = rect.center().y;
    board::paint_text(
        ui,
        egui::pos2(rect.left(), cy),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.text_size - 1.0),
        if link {
            theme.palette.text
        } else {
            theme.palette.text_secondary
        },
        label_w - 8.0,
    );
    let track = Rect::from_min_max(
        egui::pos2(rect.left() + label_w, cy - 4.0),
        egui::pos2(rect.right() - text_w, cy + 4.0),
    );
    ui.painter()
        .rect_filled(track, CornerRadius::same(4), theme.palette.border_soft);
    if value > 0.0 && max > 0.0 {
        let mut fill = track;
        fill.set_width((track.width() * (value / max).min(1.0)).max(4.0));
        ui.painter().rect_filled(fill, CornerRadius::same(4), color);
    }
    board::paint_text(
        ui,
        egui::pos2(rect.right(), cy),
        Align2::RIGHT_CENTER,
        text,
        fonts::medium(theme.text_size - 1.5),
        theme.palette.text_secondary,
        text_w - 6.0,
    );
    if link {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A muted heading over a group of bars.
fn subheading(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(muted(theme, text).size(theme.text_size - 1.5));
    ui.add_space(2.0);
}

/// Who asks them for reviews, whom they ask, and who could take some.
fn partners_card(ui: &mut Ui, theme: &Theme, analysis: &Analysis, person: &Person, go: &mut Go) {
    let (asked_by, asks) = partners(analysis, &person.login);
    let name = |login: &str| -> String {
        analysis
            .people
            .iter()
            .find(|p| p.login == login)
            .map_or(login.to_string(), |p| p.name.clone())
    };
    board::card(
        ui,
        theme,
        tl("Who they work with"),
        tl("review requests in the period"),
        0.0,
        |ui| {
            if asked_by.is_empty() && asks.is_empty() {
                board::empty(ui, theme, tl("No review request in the period."));
                return;
            }
            for (title, list, color) in [
                (tl("Ask them for a review"), &asked_by, theme.palette.accent),
                (tl("They ask for a review"), &asks, theme.tones.purple.fg),
            ] {
                if list.is_empty() {
                    continue;
                }
                subheading(ui, theme, title);
                let max = list.first().map_or(1, |l| l.1) as f32;
                for (login, n) in list {
                    let who = name(login);
                    let known = analysis.people.iter().any(|p| &p.login == login);
                    let response = bar(
                        ui,
                        theme,
                        &who,
                        *n as f32,
                        max,
                        &n.to_string(),
                        color,
                        known,
                    );
                    if known && response.on_hover_text(tl("Open their page")).clicked() {
                        *go = Go::Person(login.clone());
                    }
                }
                ui.add_space(8.0);
            }
            if let Some(text) = relief(analysis, person) {
                board::caption(ui, theme, &text);
            }
        },
    );
}

/// Where their MRs spend their time, and what they are like.
fn authoring_card(ui: &mut Ui, theme: &Theme, profile: &Profile) {
    board::card(
        ui,
        theme,
        tl("Their MRs: where the time goes"),
        "",
        0.0,
        |ui| {
            let failed = (profile.pipelines.1 > 0)
                .then(|| f64::from(profile.pipelines.0) / f64::from(profile.pipelines.1));
            stats(
                ui,
                theme,
                &[
                    (
                        profile
                            .first_review
                            .map_or_else(|| board::dash().to_string(), board::duration_text),
                        tl("to the first review"),
                    ),
                    (
                        profile
                            .rounds
                            .map_or_else(|| board::dash().to_string(), |r| format!("{r:.1}")),
                        tl("pushes after a review, on average"),
                    ),
                    (
                        failed.map_or_else(|| board::dash().to_string(), board::percent),
                        tl("pipelines failed"),
                    ),
                ],
            );
            ui.add_space(10.0);
            subheading(
                ui,
                theme,
                &tn(
                    u64::from(profile.merged),
                    "Each step, median of their {n} MR merged in the period",
                    "Each step, median of their {n} MRs merged in the period",
                ),
            );
            let t = &theme.tones;
            let max = profile
                .stages
                .iter()
                .flatten()
                .copied()
                .fold(0.0_f64, f64::max) as f32;
            for (stage, label, color) in [
                (profile.stages[0], tl("Waiting for a review"), t.blue.fg),
                (profile.stages[1], tl("Review to approval"), t.amber.fg),
                (profile.stages[2], tl("Approval to merge"), t.green.fg),
            ] {
                let text = stage.map_or_else(|| board::dash().to_string(), board::duration_text);
                bar(
                    ui,
                    theme,
                    label,
                    stage.unwrap_or(0.0) as f32,
                    max,
                    &text,
                    color,
                    false,
                );
            }
            let total: u32 = profile.efforts.iter().sum();
            if total > 0 || !profile.labels.is_empty() {
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    if total > 0 {
                        ui.label(muted(theme, tl("Size:")).size(theme.text_size - 1.5));
                        for (n, label, tone) in [
                            (profile.efforts[0], tl("Easy"), t.green),
                            (profile.efforts[1], tl("Medium"), t.amber),
                            (profile.efforts[2], tl("Complex"), t.red),
                        ] {
                            if n > 0 {
                                board::chip(ui, theme, &format!("{label} · {n}"), tone);
                            }
                        }
                    }
                    if !profile.labels.is_empty() {
                        ui.label(muted(theme, tl("Labels:")).size(theme.text_size - 1.5));
                        for (label, n) in &profile.labels {
                            board::chip(ui, theme, &format!("{label} · {n}"), t.gray);
                        }
                    }
                });
            }
        },
    );
}

/// How they review: what they were asked, how fast they answer.
fn reviewing_card(ui: &mut Ui, theme: &Theme, profile: &Profile) {
    board::card(ui, theme, tl("As a reviewer"), "", 0.0, |ui| {
        stats(
            ui,
            theme,
            &[
                (
                    profile.requested.to_string(),
                    tl("reviews asked in the period"),
                ),
                (profile.approved.to_string(), tl("approved by them")),
                (
                    profile.threads_opened.to_string(),
                    tl("threads opened on others' MRs"),
                ),
            ],
        );
        ui.add_space(10.0);
        subheading(ui, theme, tl("Time to answer a review request"));
        let t = &theme.tones;
        let max = profile.buckets.iter().copied().max().unwrap_or(0) as f32;
        for (n, label, color) in [
            (profile.buckets[0], tl("under 4 h"), t.green.fg),
            (profile.buckets[1], tl("4 h to 1 d"), t.blue.fg),
            (profile.buckets[2], tl("1 to 3 d"), t.amber.fg),
            (profile.buckets[3], tl("over 3 d"), t.red.fg),
        ] {
            bar(
                ui,
                theme,
                label,
                n as f32,
                max,
                &n.to_string(),
                color,
                false,
            );
        }
        if profile.unanswered > 0 {
            ui.add_space(4.0);
            board::caption(
                ui,
                theme,
                &tn(
                    u64::from(profile.unanswered),
                    "{n} request of the period not answered yet.",
                    "{n} requests of the period not answered yet.",
                ),
            );
        }
    });
}

/// Their latest MRs, open or not.
fn recent_card(ui: &mut Ui, theme: &Theme, profile: &Profile, now: DateTime<Utc>, go: &mut Go) {
    board::card(ui, theme, tl("Their latest MRs"), "", 0.0, |ui| {
        if profile.recent.is_empty() {
            board::empty(ui, theme, tl("No tracked MR of theirs."));
            return;
        }
        let t = &theme.tones;
        for r in &profile.recent {
            let (state, color) = match r.state {
                GitlabMrState::Merged => (tc("state", "Merged"), t.green.fg),
                GitlabMrState::Closed => (tc("state", "Closed"), t.gray.fg),
                _ => (tc("state", "Open"), t.blue.fg),
            };
            let detail = match r.at {
                Some(at) => format!("{state} · {}", board::ago(at, now)),
                None => state.to_string(),
            };
            if team::line(ui, theme, &r.id, &r.title, &detail, Some(color)) {
                *go = Go::Mr(r.id.clone());
            }
        }
    });
}

/// Values over their captions, side by side.
fn stats(ui: &mut Ui, theme: &Theme, items: &[(String, &str)]) {
    board::columns(ui, items.len(), |ui, i| {
        let (value, caption) = &items[i];
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(egui::Label::new(semibold(value, theme.text_size + 4.0)).truncate());
        ui.add(egui::Label::new(muted(theme, *caption).size(theme.text_size - 2.0)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MrData, ReviewActivity, TrackedMr};

    fn mr(id: &str, author: &str, state: GitlabMrState) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
        mr.data = MrData {
            title: format!("MR {id}"),
            author: author.into(),
            state,
            created_at: Some("2026-10-20T08:00:00Z".into()),
            ..MrData::default()
        };
        mr
    }

    #[test]
    fn profile_as_author_and_reviewer() {
        let mut app = crate::app::tests::test_app();
        let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
        // Ann's MR, merged: asked on the 21st, reviewed the 22nd, approved the
        // 24th, merged the 25th.
        let mut a = mr("1", "Ann Lee (@ann)", GitlabMrState::Merged);
        a.data.merged_at = Some("2026-10-25T08:00:00Z".into());
        a.data.review = Some(ReviewActivity {
            first_review_requested_at: Some("2026-10-21T08:00:00Z".into()),
            first_review_at: Some("2026-10-22T08:00:00Z".into()),
            approved_at: Some("2026-10-24T08:00:00Z".into()),
            pushes_after_review: 2,
            ..ReviewActivity::default()
        });
        // Bob's MR: Ann asked on the 1st, answered 2 h later, and approved.
        let mut b = mr("2", "Bob Ray (@bob)", GitlabMrState::Opened);
        b.data.reviewers = vec!["Ann Lee (@ann)".into()];
        b.data.approved_by = vec!["ann".into()];
        b.data.review = Some(ReviewActivity {
            review_requested_at: [("ann".to_string(), "2026-11-01T08:00:00Z".to_string())].into(),
            last_activity_by: [("ann".to_string(), "2026-11-01T10:00:00Z".to_string())].into(),
            ..ReviewActivity::default()
        });
        // Cid's MR, which Ann has nothing to do with.
        let c = mr("3", "Cid Moe (@cid)", GitlabMrState::Opened);
        app.mrs = vec![a, b, c];
        let p = profile(&app, "ann", now, 30);
        assert_eq!(p.seen, 2);
        assert_eq!(p.merged, 1);
        assert_eq!(p.cycle, Some(5.0));
        assert_eq!(p.stages, [Some(1.0), Some(2.0), Some(1.0)]);
        assert_eq!(p.first_review, Some(1.0));
        assert_eq!(p.rounds, Some(2.0));
        assert_eq!(p.requested, 1);
        assert_eq!(p.approved, 1);
        assert_eq!(p.buckets, [1, 0, 0, 0]);
        assert_eq!(p.unanswered, 0);
        assert_eq!(p.weeks[WEEKS - 1].reviews, 1);
        assert_eq!(p.weeks[WEEKS - 1].asked, 1);
        assert_eq!(p.weeks.iter().map(|w| w.merged).sum::<u32>(), 1);
        assert_eq!(p.recent.len(), 1);
        assert_eq!(p.recent[0].id, "1");
    }
}
