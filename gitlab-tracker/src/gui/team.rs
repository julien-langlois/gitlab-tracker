//! The team page: for each person, the reviews waiting on them, their open MRs
//! and what those wait on, the threads they have to answer, how fast they answer
//! a review request, and a load from all of it; who reviews whom; and the person
//! picked in the inspector. Everything comes from the MRs already tracked.

use std::collections::{BTreeMap, HashMap};

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::widgets::{avatar, button, muted, primary_button, semibold, split_person};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{Effort, GitlabMrState};
use crate::next_step::{self, login, median, NextStep, Turn};

/// The views of the side panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Everyone,
    /// Someone waits on their review.
    Waiting,
    Overloaded,
    Available,
}

/// How the people are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Load,
    /// The oldest review waiting on them first.
    Wait,
    Name,
}

/// The team page's choices.
#[derive(Debug, Clone)]
pub struct State {
    pub view: View,
    /// Index in [`PERIODS`].
    pub period: usize,
    pub sort: Sort,
    pub bots: bool,
    /// The person shown in the inspector, and on their page, by login.
    pub selected: Option<String>,
    /// Enter was pressed: open the page of the person shown.
    pub open: bool,
    /// J / K on a person's page: people to move by, in the list's order.
    pub step: i32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            view: View::default(),
            period: 1,
            sort: Sort::default(),
            bots: false,
            selected: None,
            open: false,
            step: 0,
        }
    }
}

/// The periods the response times and the matrix look at, in days.
pub(super) const PERIODS: [(i64, &str); 3] = [(7, "7 d"), (30, "30 d"), (90, "90 d")];

/// How loaded someone is, from their [`Person::load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Overloaded,
    Busy,
    Normal,
    Available,
    Bot,
}

impl Level {
    pub(super) fn label(self) -> &'static str {
        tl(match self {
            Level::Overloaded => "Overloaded",
            Level::Busy => "Busy",
            Level::Normal => "Normal",
            Level::Available => "Available",
            Level::Bot => "Bot",
        })
    }

    pub(super) fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Level::Overloaded => t.red,
            Level::Busy => t.amber,
            Level::Normal => t.blue,
            Level::Available => t.green,
            Level::Bot => t.gray,
        }
    }
}

/// A review waiting on someone.
#[derive(Debug, Clone)]
pub struct Waiting {
    pub id: String,
    pub title: String,
    pub author: String,
    pub since: Option<DateTime<Utc>>,
}

/// One of someone's open MRs and its next step.
#[derive(Debug, Clone)]
pub struct Own {
    pub id: String,
    pub title: String,
    pub step: NextStep,
    pub effort: Option<Effort>,
}

/// A person of the team, as the tracked MRs show them.
#[derive(Debug, Clone)]
pub struct Person {
    pub login: String,
    pub name: String,
    pub reviews: Vec<Waiting>,
    pub mrs: Vec<Own>,
    /// Unresolved threads on their MRs whose turn is theirs.
    pub threads: usize,
    /// Median days from a review request to their next activity on the MR.
    pub response: Option<f64>,
    /// 0 to 1, against the most loaded person.
    pub load: f32,
    pub level: Level,
    /// Reviews they took part in, per week, oldest first (8 weeks).
    pub weeks: [u32; 8],
    /// When they were last asked for a review.
    pub last_request: Option<DateTime<Utc>>,
}

impl Person {
    /// Someone the tracked MRs say nothing about yet.
    pub fn blank(login: &str) -> Self {
        Self {
            login: login.to_string(),
            name: String::new(),
            reviews: Vec::new(),
            mrs: Vec::new(),
            threads: 0,
            response: None,
            load: 0.0,
            level: Level::Normal,
            weeks: [0; 8],
            last_request: None,
        }
    }

    /// The oldest review waiting on them.
    pub(super) fn oldest(&self) -> Option<DateTime<Utc>> {
        self.reviews.iter().filter_map(|r| r.since).min()
    }
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub people: Vec<Person>,
    /// Review requests in the period: author login → reviewer login → count.
    pub matrix: BTreeMap<String, BTreeMap<String, u32>>,
    /// Median response over the period, and over the period before.
    pub response: Option<f64>,
    pub response_before: Option<f64>,
    pub open: usize,
}

/// The display name of a "Full Name (@login)" string.
fn name_of(display: &str) -> String {
    split_person(display).0.to_string()
}

/// Works the team out of `app`'s MRs at `now`, over the last `days`.
pub fn analyze(app: &App, now: DateTime<Utc>, days: i64, bots: bool) -> Analysis {
    let me = app.config.gitlab_username.as_deref();
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let mut names: HashMap<String, String> = HashMap::new();
    let mut remember = |display: &str| {
        let key = login(display).to_string();
        if key.is_empty() {
            return key;
        }
        let entry = names.entry(key.clone()).or_default();
        if entry.is_empty() || (display.contains("(@") && !entry.contains(' ')) {
            *entry = name_of(display);
        }
        key
    };
    let mut people: BTreeMap<String, Person> = BTreeMap::new();
    let blank = Person::blank;
    let mut responses: HashMap<String, Vec<f64>> = HashMap::new();
    let mut now_period = Vec::new();
    let mut before_period = Vec::new();
    let mut matrix: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut open = 0;
    let profile = &app.config.complexity_profile;
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let author = remember(&mr.author);
        let reviewers: Vec<String> = mr.reviewers.iter().map(|r| remember(r)).collect();
        for approver in &mr.approvers {
            remember(approver);
        }
        if mr.state == GitlabMrState::Opened {
            open += 1;
            if !author.is_empty() {
                let step = next_step::of(app, mr, me);
                let effort = mr
                    .diff_stats
                    .as_ref()
                    .map(|d| Effort::from_score(d.difficulty(profile)));
                let threads = mr
                    .threads
                    .iter()
                    .filter(|t| !t.resolved && next_step::thread_turn(mr, t) == Turn::Author)
                    .count();
                let person = people
                    .entry(author.clone())
                    .or_insert_with(|| blank(&author));
                person.threads += threads;
                person.mrs.push(Own {
                    id: mr.id.clone(),
                    title: mr.title.clone(),
                    step,
                    effort,
                });
            }
            for reviewer in &reviewers {
                if reviewer.is_empty() || *reviewer == author {
                    continue;
                }
                let waiting = crate::utils::awaiting_review_by(
                    mr.state,
                    mr.mergeability,
                    &mr.reviewers,
                    &mr.approved_by,
                    reviewer,
                );
                let person = people
                    .entry(reviewer.clone())
                    .or_insert_with(|| blank(reviewer));
                if waiting {
                    person.reviews.push(Waiting {
                        id: mr.id.clone(),
                        title: mr.title.clone(),
                        author: mr.author.clone(),
                        since: crate::utils::review_waiting_since(
                            mr.review.as_ref(),
                            mr.created_at.as_deref(),
                            reviewer,
                        ),
                    });
                }
            }
        }
        // The review history: requests, answers, activity by week.
        let Some(review) = mr.review.as_ref() else {
            continue;
        };
        for (reviewer, requested) in &review.review_requested_at {
            let Some(requested) = board::parse_time(requested) else {
                continue;
            };
            if *reviewer == author {
                continue;
            }
            let person = people
                .entry(reviewer.clone())
                .or_insert_with(|| blank(reviewer));
            person.last_request = person.last_request.max(Some(requested));
            let bot = board::is_bot(&author) || board::is_bot(reviewer);
            if requested >= start && !author.is_empty() && (bots || !bot) {
                *matrix
                    .entry(author.clone())
                    .or_default()
                    .entry(reviewer.clone())
                    .or_default() += 1;
            }
            let answered = review
                .last_activity_by
                .get(reviewer)
                .and_then(|at| board::parse_time(at))
                .filter(|at| *at >= requested);
            if let Some(at) = answered {
                let days = board::days_between(requested, at);
                if requested >= start {
                    responses.entry(reviewer.clone()).or_default().push(days);
                    now_period.push(days);
                } else if requested >= before {
                    before_period.push(days);
                }
            }
        }
        for (who, at) in &review.last_activity_by {
            if *who == author {
                continue;
            }
            let Some(at) = board::parse_time(at) else {
                continue;
            };
            let weeks_ago = (now - at).num_days() / 7;
            if (0..8).contains(&weeks_ago) {
                let person = people.entry(who.clone()).or_insert_with(|| blank(who));
                person.weeks[7 - weeks_ago as usize] += 1;
            }
        }
    }

    // The load: reviews weighted by how long they wait, own MRs by their effort,
    // threads to answer. Relative to the most loaded person.
    let raw = |p: &Person| -> f32 {
        let reviews: f32 = p
            .reviews
            .iter()
            .map(|r| {
                let age = r.since.map_or(0.0, |s| board::days_between(s, now) as f32);
                1.0 + age.clamp(0.0, 5.0) / 2.5
            })
            .sum();
        let own: f32 = p
            .mrs
            .iter()
            .map(|m| match m.effort {
                Some(Effort::Complex) => 1.5,
                Some(Effort::Medium) => 1.0,
                _ => 0.5,
            })
            .sum();
        reviews + own + p.threads as f32 * 0.3
    };
    let mut list: Vec<Person> = people
        .into_values()
        .filter(|p| !p.login.is_empty())
        .map(|mut p| {
            p.name = names
                .get(&p.login)
                .cloned()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| p.login.clone());
            p.response = median(responses.remove(&p.login).unwrap_or_default());
            p
        })
        .filter(|p| bots || !(board::is_bot(&p.login) || board::is_bot(&p.name)))
        // Someone the tracked MRs say nothing recent about stays out.
        .filter(|p| {
            !p.reviews.is_empty()
                || !p.mrs.is_empty()
                || p.weeks.iter().any(|w| *w > 0)
                || p.last_request.is_some_and(|r| r >= start)
        })
        .collect();
    let most = list
        .iter()
        .filter(|p| !board::is_bot(&p.login))
        .map(&raw)
        .fold(0.0_f32, f32::max);
    for p in &mut list {
        let value = raw(p);
        p.load = if most > 0.0 {
            (value / most).clamp(0.0, 1.0)
        } else {
            0.0
        };
        p.level = if board::is_bot(&p.login) || board::is_bot(&p.name) {
            Level::Bot
        } else if p.load >= 0.8 && value >= 3.0 {
            Level::Overloaded
        } else if p.load >= 0.55 && value >= 2.0 {
            Level::Busy
        } else if p.reviews.is_empty() && p.load < 0.3 {
            Level::Available
        } else {
            Level::Normal
        };
    }
    Analysis {
        people: list,
        matrix,
        response: median(now_period),
        response_before: median(before_period),
        open,
    }
}

/// The reviewers' share of the requests: the busiest's login and share.
pub(super) fn busiest(matrix: &BTreeMap<String, BTreeMap<String, u32>>) -> Option<(String, f64)> {
    let mut by_reviewer: BTreeMap<&str, u32> = BTreeMap::new();
    for row in matrix.values() {
        for (reviewer, n) in row {
            *by_reviewer.entry(reviewer).or_default() += n;
        }
    }
    let total: u32 = by_reviewer.values().sum();
    let (who, n) = by_reviewer.iter().max_by_key(|(_, n)| **n)?;
    (total > 0).then(|| (who.to_string(), f64::from(*n) / f64::from(total)))
}

// ── Screen ───────────────────────────────────────────────────────────────────

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.team.period.min(PERIODS.len() - 1)].0;
    let analysis = analyze(&g.app, now, days, g.team.bots);
    let reminder = g.app.config.review_reminder_hours.max(1) as f64 / 24.0;
    // The person shown: the one picked, else the most loaded.
    let shown = g
        .team
        .selected
        .clone()
        .filter(|s| analysis.people.iter().any(|p| &p.login == s))
        .or_else(|| {
            analysis
                .people
                .iter()
                .filter(|p| p.level != Level::Bot)
                .max_by(|a, b| a.load.total_cmp(&b.load))
                .map(|p| p.login.clone())
        });
    if std::mem::take(&mut g.team.open) {
        if let Some(login) = &shown {
            g.open_person(login);
            return;
        }
    }
    board::screen(
        g,
        ui,
        sizes,
        "team",
        |g, ui| nav(g, ui, &analysis),
        |g, ui| inspector(g, ui, &analysis, shown.as_deref(), now, reminder),
        |g, ui| {
            main(
                g,
                ui,
                &analysis,
                shown.as_deref(),
                now,
                reminder,
                sizes.inspector.is_none(),
            )
        },
    );
}

fn counts(analysis: &Analysis, view: View) -> usize {
    analysis.people.iter().filter(|p| shows(p, view)).count()
}

fn shows(p: &Person, view: View) -> bool {
    match view {
        View::Everyone => true,
        View::Waiting => !p.reviews.is_empty(),
        View::Overloaded => matches!(p.level, Level::Overloaded | Level::Busy),
        View::Available => p.level == Level::Available,
    }
}

fn nav(g: &mut GuiApp, ui: &mut Ui, analysis: &Analysis) {
    let theme = g.theme;
    let subtitle = format!(
        "{} · {}",
        tn(analysis.people.len() as u64, "{n} person", "{n} people"),
        tn(analysis.open as u64, "{n} open MR", "{n} open MRs")
    );
    board::nav_header(ui, &theme, tl("Team"), &subtitle);
    board::nav_group(ui, &theme, tl("Views"), |ui| {
        for (view, label) in [
            (View::Everyone, tl("Everyone")),
            (View::Waiting, tl("Waited on for a review")),
            (View::Overloaded, tl("Overloaded")),
            (View::Available, tl("Available")),
        ] {
            let n = counts(analysis, view).to_string();
            if board::nav_item(ui, &theme, label, &n, None, g.team.view == view).clicked() {
                g.team.view = view;
            }
        }
    });
    board::nav_group(ui, &theme, tl("Period"), |ui| {
        let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
        if let Some(i) = board::segmented(ui, &theme, "team_period", &labels, g.team.period) {
            g.team.period = i;
        }
    });
    board::nav_group(ui, &theme, tl("Sort by"), |ui| {
        let sorts = [Sort::Load, Sort::Wait, Sort::Name];
        let labels = [tl("Load"), tl("Wait"), tl("Name")];
        let current = sorts.iter().position(|s| *s == g.team.sort).unwrap_or(0);
        if let Some(i) = board::segmented(ui, &theme, "team_sort", &labels, current) {
            g.team.sort = sorts[i];
        }
    });
    board::nav_group(ui, &theme, "", |ui| {
        if board::nav_switch(ui, &theme, g.team.bots, tl("Include bots")) {
            g.team.bots = !g.team.bots;
        }
    });
    board::nav_note(
        ui,
        &theme,
        tl("Load: the reviews waiting on someone, heavier as they wait, plus the effort of their open MRs and the threads they have to answer, against the most loaded person."),
    );
}

// ── Main area ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    analysis: &Analysis,
    shown: Option<&str>,
    now: DateTime<Utc>,
    reminder: f64,
    narrow: bool,
) {
    let theme = g.theme;
    let mut copy = false;
    board::heading(
        ui,
        &theme,
        tl("Team"),
        tl("Who waits on what, and who can take the next review"),
        |ui| {
            copy = button(
                ui,
                &theme,
                Some(super::icons::Icon::Copy),
                tl("Copy the summary"),
                false,
            )
            .on_hover_text(tl("Copy who waits on whom, to paste in a chat"))
            .clicked();
        },
    );
    if copy {
        ui.ctx()
            .copy_text(board::Links::of(&g.app).apply(&summary(analysis, now)));
        g.app.flash(tl("Summary copied"), false);
    }
    ui.add_space(14.0);

    // The figures.
    let t = &theme.tones;
    let waiting: Vec<&Waiting> = analysis.people.iter().flat_map(|p| &p.reviews).collect();
    let late = waiting
        .iter()
        .filter(|w| {
            w.since
                .is_some_and(|s| board::days_between(s, now) >= reminder)
        })
        .count();
    let threads: usize = analysis.people.iter().map(|p| p.threads).sum();
    let threads_mrs = analysis
        .people
        .iter()
        .flat_map(|p| &p.mrs)
        .filter(|m| {
            m.step
                .todos
                .iter()
                .any(|t| matches!(t, next_step::Todo::AnswerThreads { .. }))
        })
        .count();
    let (response_caption, response_color) = match (analysis.response, analysis.response_before) {
        (Some(now), Some(before)) => {
            let delta = now - before;
            let text = board::duration_text(delta.abs());
            if delta <= 0.0 {
                (
                    tf("{d} faster than the period before", &[("d", &text)]),
                    Some(t.green.fg),
                )
            } else {
                (
                    tf("{d} slower than the period before", &[("d", &text)]),
                    Some(t.amber.fg),
                )
            }
        }
        (Some(_), None) => (tl("Nothing to compare with yet").to_string(), None),
        _ => (tl("No answer in the period").to_string(), None),
    };
    let balance = busiest(&analysis.matrix);
    let figures = [
        Figure {
            label: tl("Reviews waiting").into(),
            value: waiting.len().to_string(),
            value_color: None,
            caption: if late > 0 {
                tn(
                    late as u64,
                    "{n} past the reminder delay",
                    "{n} past the reminder delay",
                )
            } else {
                tl("None past the reminder delay").into()
            },
            caption_color: (late > 0).then_some(t.red.fg),
        },
        Figure {
            label: tl("Review response").into(),
            value: analysis
                .response
                .map_or_else(|| board::dash().to_string(), board::duration_text),
            value_color: None,
            caption: response_caption,
            caption_color: response_color,
        },
        Figure {
            label: tl("Threads to answer").into(),
            value: threads.to_string(),
            value_color: None,
            caption: tn(threads_mrs as u64, "on {n} MR", "on {n} MRs"),
            caption_color: None,
        },
        Figure {
            label: tl("Review balance").into(),
            value: balance.as_ref().map_or_else(
                || board::dash().to_string(),
                |(_, share)| board::percent(*share),
            ),
            value_color: None,
            caption: match &balance {
                Some((who, _)) => {
                    let name = analysis
                        .people
                        .iter()
                        .find(|p| &p.login == who)
                        .map_or(who.as_str(), |p| p.name.as_str());
                    tf(
                        "of the requests go to {name}",
                        &[("name", board::first_name(name))],
                    )
                }
                None => tl("No request in the period").into(),
            },
            caption_color: balance
                .as_ref()
                .filter(|(_, share)| *share >= 0.4)
                .map(|_| t.amber.fg),
        },
    ];
    board::figures(ui, &theme, &figures);

    // The people.
    let list = ordered(analysis, &g.team, now);
    let mut picked = None;
    let mut opened = None;
    let mut open_mr = None;
    board::card(ui, &theme, "", "", 0.0, |ui| {
        if list.is_empty() {
            board::empty(ui, &theme, tl("Nobody in this view."));
            return;
        }
        people_table(
            ui,
            &theme,
            &list,
            shown,
            now,
            reminder,
            &mut picked,
            &mut opened,
            &mut open_mr,
        );
    });
    ui.add_space(GAP);

    board::split(
        ui,
        0.56,
        300.0,
        260.0,
        |ui| matrix_card(ui, &theme, analysis, PERIODS[g.team.period.min(2)].1),
        |ui| signals_card(ui, &theme, analysis, now, reminder, &mut open_mr),
    );

    if narrow {
        // No inspector: the person picked shows below.
        if let Some(person) = shown.and_then(|s| analysis.people.iter().find(|p| p.login == s)) {
            ui.add_space(GAP);
            board::card(ui, &theme, &person.name, person.level.label(), 0.0, |ui| {
                waiting_list(ui, &theme, person, now, reminder, &mut open_mr);
            });
        }
    }
    if let Some(login) = picked {
        g.team.selected = Some(login);
    }
    if let Some(login) = opened {
        g.open_person(&login);
    } else if let Some(id) = open_mr {
        g.open_mr(&id);
    }
}

/// The people of the view picked, in the order picked: the list's order, and
/// the one J / K follow on a person's page.
pub(super) fn ordered<'a>(
    analysis: &'a Analysis,
    state: &State,
    now: DateTime<Utc>,
) -> Vec<&'a Person> {
    let mut list: Vec<&Person> = analysis
        .people
        .iter()
        .filter(|p| shows(p, state.view))
        .collect();
    match state.sort {
        Sort::Load => list.sort_by(|a, b| b.load.total_cmp(&a.load).then(a.name.cmp(&b.name))),
        Sort::Wait => list.sort_by(|a, b| {
            a.oldest()
                .unwrap_or(now)
                .cmp(&b.oldest().unwrap_or(now))
                .then(a.name.cmp(&b.name))
        }),
        Sort::Name => list.sort_by_key(|p| p.name.to_lowercase()),
    }
    list
}

/// The table's columns: their left edges, from the row's left, and the widths.
struct Cols {
    person: (f32, f32),
    reviews: (f32, f32),
    mrs: (f32, f32),
    threads: Option<(f32, f32)>,
    response: Option<(f32, f32)>,
    load: (f32, f32),
}

fn cols(width: f32) -> Cols {
    let pad = 12.0;
    let gap = 14.0;
    let person = 190.0;
    let mrs = 200.0;
    let load = 150.0;
    let threads = 64.0;
    let response = 84.0;
    let fixed = pad * 2.0 + person + mrs + load + gap * 3.0;
    // The narrow columns go first when the room runs out.
    let with_both = fixed + threads + response + gap * 2.0;
    let (show_threads, show_response) = if width - with_both >= 200.0 {
        (true, true)
    } else if width - (fixed + response + gap) >= 180.0 {
        (false, true)
    } else {
        (false, false)
    };
    let reviews = width
        - fixed
        - if show_threads { threads + gap } else { 0.0 }
        - if show_response { response + gap } else { 0.0 };
    let mut x = pad;
    let mut take = |w: f32| {
        let at = x;
        x += w + gap;
        (at, w)
    };
    let person = take(person);
    let reviews = take(reviews.max(120.0));
    let mrs = take(mrs);
    let threads = show_threads.then(|| take(threads));
    let response = show_response.then(|| take(response));
    let load = take(load);
    Cols {
        person,
        reviews,
        mrs,
        threads,
        response,
        load,
    }
}

#[allow(clippy::too_many_arguments)]
fn people_table(
    ui: &mut Ui,
    theme: &Theme,
    list: &[&Person],
    shown: Option<&str>,
    now: DateTime<Utc>,
    reminder: f64,
    picked: &mut Option<String>,
    opened: &mut Option<String>,
    open_mr: &mut Option<String>,
) {
    let c = cols(ui.available_width());
    // Header.
    let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let head_font = FontId::proportional(theme.text_size - 1.5);
    let muted_color = theme.palette.text_muted;
    let head = |col: (f32, f32), text: &str| {
        board::paint_text(
            ui,
            egui::pos2(header.left() + col.0, header.center().y),
            Align2::LEFT_CENTER,
            text,
            head_font.clone(),
            muted_color,
            col.1,
        );
    };
    head(c.person, tl("Person"));
    head(c.reviews, tl("Reviews waiting on them"));
    head(c.mrs, tl("Their open MRs"));
    if let Some(col) = c.threads {
        head(col, tl("Threads"));
    }
    if let Some(col) = c.response {
        head(col, tl("Response"));
    }
    head(c.load, tl("Load"));

    for person in list {
        let selected = shown == Some(person.login.as_str());
        let (rect, response) = board::row(ui, theme, 50.0, selected);
        if !selected {
            board::row_rule(ui, theme, rect);
        }
        let x = |col: (f32, f32)| rect.left() + col.0;
        let cy = rect.center().y;
        // Person: avatar, name over login.
        {
            let mut cell = board::inside(
                ui,
                Rect::from_min_size(
                    egui::pos2(x(c.person), rect.top()),
                    Vec2::new(26.0, rect.height()),
                ),
            );
            avatar(&mut cell, theme, &person.name, 24.0);
            let text_left = x(c.person) + 34.0;
            let width = c.person.1 - 34.0;
            board::paint_text(
                ui,
                egui::pos2(text_left, cy - 1.0),
                Align2::LEFT_BOTTOM,
                &person.name,
                fonts::medium(theme.text_size),
                theme.palette.text,
                width,
            );
            board::paint_text(
                ui,
                egui::pos2(text_left, cy + 2.0),
                Align2::LEFT_TOP,
                &format!("@{}", person.login),
                FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
                width,
            );
        }
        // Reviews waiting: an id and how long, as many as fit, then "+n".
        let pairs: Vec<(String, String, Tone, String)> = person
            .reviews
            .iter()
            .map(|r| {
                let age = r.since.map(|s| board::days_between(s, now));
                let tone = match age {
                    Some(a) if a >= reminder => theme.tones.red,
                    Some(a) if a >= reminder / 2.0 => theme.tones.amber,
                    _ => theme.tones.gray,
                };
                let text = age.map_or_else(|| board::dash().to_string(), board::duration_text);
                (r.id.clone(), text, tone, r.title.clone())
            })
            .collect();
        if pairs.is_empty() {
            let text = match person.last_request {
                Some(at) if person.level != Level::Bot => {
                    tf("Last asked {when}", &[("when", &board::ago(at, now))])
                }
                _ => board::dash().to_string(),
            };
            board::paint_text(
                ui,
                egui::pos2(x(c.reviews), cy),
                Align2::LEFT_CENTER,
                &text,
                FontId::proportional(theme.text_size - 1.5),
                theme.palette.text_muted,
                c.reviews.1,
            );
        } else if let Some(id) = pair_cell(ui, theme, rect, c.reviews, &pairs) {
            *open_mr = Some(id);
        }
        // Own MRs: an id and its next step.
        let own: Vec<(String, String, Tone, String)> = person
            .mrs
            .iter()
            .map(|m| {
                let tone = super::next::tone(&m.step, theme);
                let what = m
                    .step
                    .todos
                    .first()
                    .map(|t| t.short())
                    .unwrap_or_else(|| m.step.who());
                (m.id.clone(), what, tone, m.title.clone())
            })
            .collect();
        if own.is_empty() {
            board::paint_text(
                ui,
                egui::pos2(x(c.mrs), cy),
                Align2::LEFT_CENTER,
                board::dash(),
                FontId::proportional(theme.text_size - 1.5),
                theme.palette.text_muted,
                c.mrs.1,
            );
        } else if let Some(id) = pair_cell(ui, theme, rect, c.mrs, &own) {
            *open_mr = Some(id);
        }
        if let Some(col) = c.threads {
            let color = if person.threads > 0 {
                theme.tones.amber.fg
            } else {
                theme.palette.text_muted
            };
            let icon = Rect::from_center_size(egui::pos2(x(col) + 7.0, cy), Vec2::splat(14.0));
            super::icons::paint(ui.painter(), icon, super::icons::Icon::Comment, color);
            board::paint_text(
                ui,
                egui::pos2(x(col) + 20.0, cy),
                Align2::LEFT_CENTER,
                &person.threads.to_string(),
                fonts::medium(theme.text_size - 1.0),
                color,
                col.1 - 20.0,
            );
        }
        if let Some(col) = c.response {
            board::paint_text(
                ui,
                egui::pos2(x(col), cy),
                Align2::LEFT_CENTER,
                &person
                    .response
                    .map_or_else(|| board::dash().to_string(), board::duration_text),
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_secondary,
                col.1,
            );
        }
        // Load: the level and the share above a bar.
        {
            let tone = person.level.tone(theme);
            let left = x(c.load);
            board::paint_text(
                ui,
                egui::pos2(left, cy - 3.0),
                Align2::LEFT_BOTTOM,
                person.level.label(),
                fonts::medium(theme.text_size - 2.0),
                tone.fg,
                c.load.1 - 44.0,
            );
            board::paint_text(
                ui,
                egui::pos2(left + c.load.1, cy - 3.0),
                Align2::RIGHT_BOTTOM,
                &board::percent(f64::from(person.load)),
                FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
                44.0,
            );
            let track = Rect::from_min_size(egui::pos2(left, cy + 3.0), Vec2::new(c.load.1, 6.0));
            ui.painter()
                .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
            let mut fill = track;
            fill.set_width((track.width() * person.load).max(if person.load > 0.0 {
                3.0
            } else {
                0.0
            }));
            if fill.width() > 0.0 {
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(3), tone.fg);
            }
        }
        if response.double_clicked() {
            *opened = Some(person.login.clone());
        } else if response.clicked() {
            *picked = Some(person.login.clone());
        }
    }
}

/// Pairs of (MR id, tag) painted in a cell, as many as fit, then "+n"; the MR
/// whose id was clicked.
fn pair_cell(
    ui: &mut Ui,
    theme: &Theme,
    row: Rect,
    col: (f32, f32),
    pairs: &[(String, String, Tone, String)],
) -> Option<String> {
    let cy = row.center().y;
    let right = row.left() + col.0 + col.1;
    let mut x = row.left() + col.0;
    let mut clicked = None;
    let id_font = FontId::monospace(theme.text_size - 2.0);
    let tag_font = fonts::medium(theme.text_size - 2.5);
    let more_room = 30.0;
    for (index, (id, tag, tone, title)) in pairs.iter().enumerate() {
        let id_text = format!("!{id}");
        let id_galley = ui
            .painter()
            .layout_no_wrap(id_text, id_font.clone(), theme.palette.text);
        let tag_galley = ui
            .painter()
            .layout_no_wrap(tag.clone(), tag_font.clone(), tone.fg);
        let width = id_galley.size().x + 12.0 + tag_galley.size().x + 12.0;
        let left_after = pairs.len() - index - 1;
        let limit = if left_after > 0 {
            right - more_room
        } else {
            right
        };
        if x + width > limit {
            let rest = pairs.len() - index;
            board::paint_text(
                ui,
                egui::pos2(x, cy),
                Align2::LEFT_CENTER,
                &format!("+{rest}"),
                fonts::medium(theme.text_size - 2.0),
                theme.palette.text_muted,
                right - x,
            );
            break;
        }
        let id_rect = Rect::from_min_size(
            egui::pos2(x, cy - 10.0),
            Vec2::new(id_galley.size().x + 12.0, 20.0),
        );
        let tag_rect = Rect::from_min_size(
            egui::pos2(id_rect.right(), cy - 10.0),
            Vec2::new(tag_galley.size().x + 12.0, 20.0),
        );
        let r = theme.radii.small;
        ui.painter().rect_filled(
            id_rect,
            CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            theme.palette.raised,
        );
        ui.painter().rect_filled(
            tag_rect,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: r,
                se: r,
            },
            tone.bg,
        );
        ui.painter().galley(
            id_rect.center() - id_galley.size() / 2.0,
            id_galley,
            theme.palette.text,
        );
        ui.painter().galley(
            tag_rect.center() - tag_galley.size() / 2.0,
            tag_galley,
            tone.fg,
        );
        let whole = id_rect.union(tag_rect);
        let response = ui
            .interact(
                whole,
                ui.scope_id()
                    .with(("pair", row.top() as i32, col.0 as i32, index)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(title.as_str());
        if response.hovered() {
            ui.painter().rect_stroke(
                whole,
                CornerRadius::same(r),
                egui::Stroke::new(1.0, theme.palette.text_muted),
                egui::StrokeKind::Inside,
            );
        }
        if response.clicked() {
            clicked = Some(id.clone());
        }
        x = whole.right() + 6.0;
    }
    clicked
}

/// Who reviews whom: authors in rows, reviewers in columns, the count of
/// requests in each cell, shaded by it.
fn matrix_card(ui: &mut Ui, theme: &Theme, analysis: &Analysis, period: &str) {
    let note = tf(
        "{period} · rows: authors, columns: reviewers",
        &[("period", tl(period))],
    );
    board::card(ui, theme, tl("Who reviews whom"), &note, 250.0, |ui| {
        let mut totals: BTreeMap<&str, u32> = BTreeMap::new();
        for row in analysis.matrix.values() {
            for (reviewer, n) in row {
                *totals.entry(reviewer.as_str()).or_default() += n;
            }
        }
        if totals.is_empty() {
            board::empty(ui, theme, tl("No review request in the period."));
            return;
        }
        let mut reviewers: Vec<(&str, u32)> = totals.into_iter().collect();
        reviewers.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let label_w = 44.0;
        let total_w = 34.0;
        let room = ui.available_width() - label_w - total_w;
        let fit = ((room + 4.0) / 34.0).floor().max(1.0) as usize;
        reviewers.truncate(fit.min(8));
        let mut authors: Vec<(&String, u32)> = analysis
            .matrix
            .iter()
            .map(|(a, row)| (a, row.values().sum()))
            .collect();
        authors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        authors.truncate(8);
        let max = analysis
            .matrix
            .values()
            .flat_map(|r| r.values())
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        let cell_w =
            ((room - 4.0 * (reviewers.len() as f32 - 1.0)) / reviewers.len() as f32).min(56.0);
        // The totals' column right after the last reviewer's, not at the
        // card's edge (the cells stop growing on a wide card).
        let sum_right = label_w + reviewers.len() as f32 * (cell_w + 4.0) - 4.0 + 12.0 + total_w;
        let initials = |login: &str| -> String {
            let name = analysis
                .people
                .iter()
                .find(|p| p.login == login)
                .map_or(login, |p| p.name.as_str());
            let letters: String = name
                .split_whitespace()
                .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
                .take(2)
                .flat_map(char::to_uppercase)
                .collect();
            if letters.is_empty() {
                login.chars().take(2).collect::<String>().to_uppercase()
            } else {
                letters
            }
        };
        let name_of = |login: &str| -> String {
            analysis
                .people
                .iter()
                .find(|p| p.login == login)
                .map_or(login.to_string(), |p| p.name.clone())
        };
        let small = FontId::proportional(theme.text_size - 2.0);
        let strong = fonts::semibold(theme.text_size - 2.0);
        // Header: the reviewers.
        let (head, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
        for (i, (reviewer, _)) in reviewers.iter().enumerate() {
            let cx = head.left() + label_w + i as f32 * (cell_w + 4.0) + cell_w / 2.0;
            let r =
                Rect::from_center_size(egui::pos2(cx, head.center().y), Vec2::new(cell_w, 20.0));
            board::paint_text(
                ui,
                r.center(),
                Align2::CENTER_CENTER,
                &initials(reviewer),
                strong.clone(),
                theme.palette.text_secondary,
                cell_w,
            );
            ui.interact(r, ui.scope_id().with(("rev", i)), Sense::hover())
                .on_hover_text(name_of(reviewer));
        }
        board::paint_text(
            ui,
            egui::pos2(head.left() + sum_right, head.center().y),
            Align2::RIGHT_CENTER,
            "Σ",
            small.clone(),
            theme.palette.text_muted,
            total_w,
        );
        ui.add_space(4.0);
        for (author, total) in &authors {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
            let label_rect = Rect::from_min_size(line.min, Vec2::new(label_w, 26.0));
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &initials(author),
                strong.clone(),
                theme.palette.text_secondary,
                label_w - 4.0,
            );
            ui.interact(
                label_rect,
                ui.scope_id().with(("author", author.as_str())),
                Sense::hover(),
            )
            .on_hover_text(name_of(author));
            let row = analysis.matrix.get(*author);
            for (i, (reviewer, _)) in reviewers.iter().enumerate() {
                let n = row.and_then(|r| r.get(*reviewer)).copied().unwrap_or(0);
                let cell = Rect::from_min_size(
                    egui::pos2(
                        line.left() + label_w + i as f32 * (cell_w + 4.0),
                        line.top() + 1.0,
                    ),
                    Vec2::new(cell_w, 24.0),
                );
                let (fill, color) = if n == 0 {
                    (
                        theme.palette.raised.gamma_multiply(0.6),
                        theme.palette.text_muted.gamma_multiply(0.6),
                    )
                } else {
                    let strength = 0.18 + 0.6 * n as f32 / max as f32;
                    (
                        theme.palette.accent.gamma_multiply(strength),
                        theme.palette.text,
                    )
                };
                ui.painter().rect_filled(cell, CornerRadius::same(4), fill);
                let text = if n == 0 {
                    "·".to_string()
                } else {
                    n.to_string()
                };
                board::paint_text(
                    ui,
                    cell.center(),
                    Align2::CENTER_CENTER,
                    &text,
                    fonts::medium(theme.text_size - 2.0),
                    color,
                    cell_w,
                );
                let tip = if n == 0 {
                    tf(
                        "No request from {author} to {reviewer}",
                        &[
                            ("author", &name_of(author)),
                            ("reviewer", &name_of(reviewer)),
                        ],
                    )
                } else {
                    tn(
                        u64::from(n),
                        "{author} asked {reviewer} for {n} review",
                        "{author} asked {reviewer} for {n} reviews",
                    )
                    .replace("{author}", &name_of(author))
                    .replace("{reviewer}", &name_of(reviewer))
                };
                ui.interact(
                    cell,
                    ui.scope_id().with(("cell", author.as_str(), i)),
                    Sense::hover(),
                )
                .on_hover_text(tip);
            }
            board::paint_text(
                ui,
                egui::pos2(line.left() + sum_right, line.center().y),
                Align2::RIGHT_CENTER,
                &total.to_string(),
                small.clone(),
                theme.palette.text_muted,
                total_w,
            );
        }
        // Totals per reviewer.
        let (foot, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
        board::paint_text(
            ui,
            egui::pos2(foot.left(), foot.center().y),
            Align2::LEFT_CENTER,
            "Σ",
            small.clone(),
            theme.palette.text_muted,
            label_w,
        );
        let top = reviewers.first().map_or(0, |r| r.1);
        for (i, (_, total)) in reviewers.iter().enumerate() {
            let cx = foot.left() + label_w + i as f32 * (cell_w + 4.0) + cell_w / 2.0;
            let color = if *total == top && reviewers.len() > 1 {
                theme.tones.amber.fg
            } else {
                theme.palette.text_secondary
            };
            board::paint_text(
                ui,
                egui::pos2(cx, foot.center().y),
                Align2::CENTER_CENTER,
                &total.to_string(),
                fonts::semibold(theme.text_size - 2.0),
                color,
                cell_w,
            );
        }
    });
}

/// A line of the Signals card: its dot, its sentence, and the MR it names
/// (id, title), shown on hover and opened on a click.
struct Signal {
    color: Color32,
    text: String,
    mr: Option<(String, String)>,
}

/// What stands out, in sentences: late reviews, an unbalanced load, someone
/// free, the trend.
fn signals(analysis: &Analysis, now: DateTime<Utc>, reminder: f64, theme: &Theme) -> Vec<Signal> {
    let line = |color: Color32, text: String| Signal {
        color,
        text,
        mr: None,
    };
    let t = &theme.tones;
    let mut out = Vec::new();
    let oldest = analysis
        .people
        .iter()
        .flat_map(|p| p.reviews.iter().map(move |r| (p, r)))
        .filter_map(|(p, r)| r.since.map(|s| (p, r, board::days_between(s, now))))
        .max_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((p, r, days)) = oldest.filter(|o| o.2 >= reminder) {
        out.push(Signal {
            mr: Some((r.id.clone(), r.title.clone())),
            ..line(
                t.red.fg,
                tf(
                    "!{id} has waited for {name}'s review for {time}, past the reminder delay.",
                    &[
                        ("id", &r.id),
                        ("name", board::first_name(&p.name)),
                        ("time", &board::duration_text(days)),
                    ],
                ),
            )
        });
    }
    if let Some((who, share)) = busiest(&analysis.matrix).filter(|b| b.1 >= 0.4) {
        let name = analysis
            .people
            .iter()
            .find(|p| p.login == who)
            .map_or(who.clone(), |p| p.name.clone());
        out.push(line(
            t.amber.fg,
            tf(
                "{name} gets {share} of the review requests.",
                &[
                    ("name", board::first_name(&name)),
                    ("share", &board::percent(share)),
                ],
            ),
        ));
    }
    if let Some(free) = analysis
        .people
        .iter()
        .filter(|p| p.level == Level::Available)
        .min_by(|a, b| a.load.total_cmp(&b.load))
    {
        out.push(line(
            t.green.fg,
            tf(
                "{name} has no review waiting: they can take the next one.",
                &[("name", board::first_name(&free.name))],
            ),
        ));
    }
    if let Some(slow) = analysis
        .people
        .iter()
        .filter(|p| p.level != Level::Bot)
        .filter_map(|p| p.response.map(|r| (p, r)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, r)| analysis.response.is_some_and(|m| *r > m * 1.5))
    {
        out.push(line(
            t.blue.fg,
            tf(
                "{name} answers a review request in {time}, the slowest of the team.",
                &[
                    ("name", board::first_name(&slow.0.name)),
                    ("time", &board::duration_text(slow.1)),
                ],
            ),
        ));
    }
    if out.is_empty() {
        out.push(line(
            t.green.fg,
            tl("Nothing stands out: the reviews flow.").to_string(),
        ));
    }
    out
}

fn signals_card(
    ui: &mut Ui,
    theme: &Theme,
    analysis: &Analysis,
    now: DateTime<Utc>,
    reminder: f64,
    open_mr: &mut Option<String>,
) {
    board::card(ui, theme, tl("Signals"), "", 250.0, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        for (index, Signal { color, text, mr }) in signals(analysis, now, reminder, theme)
            .into_iter()
            .enumerate()
        {
            let frame = egui::Frame::new()
                .fill(theme.palette.raised.gamma_multiply(0.7))
                .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
                .corner_radius(CornerRadius::same(theme.radii.medium))
                .inner_margin(egui::Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.vertical(|ui| {
                            ui.add_space(5.0);
                            super::widgets::dot(ui, color, 7.0);
                        });
                        ui.add(
                            egui::Label::new(RichText::new(text).size(theme.text_size - 0.5))
                                .wrap()
                                .selectable(false),
                        );
                    });
                });
            let Some((id, title)) = mr else {
                continue;
            };
            let rect = frame.response.rect;
            let response = ui
                .interact(rect, ui.scope_id().with(("signal", index)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("!{id} · {title}\n{}", tl("Open its page")));
            if response.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    egui::Stroke::new(1.0, theme.palette.text_muted),
                    egui::StrokeKind::Inside,
                );
            }
            if response.clicked() {
                *open_mr = Some(id);
            }
        }
    });
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    analysis: &Analysis,
    shown: Option<&str>,
    now: DateTime<Utc>,
    reminder: f64,
) {
    let theme = g.theme;
    let Some(person) = shown.and_then(|s| analysis.people.iter().find(|p| p.login == s)) else {
        board::inspector_empty(
            ui,
            &theme,
            tl("Pick someone in the list to see what waits on them."),
        );
        return;
    };
    // Who.
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                avatar(ui, &theme, &person.name, 42.0);
                let chip_w = 90.0;
                ui.allocate_ui(Vec2::new(ui.available_width() - chip_w, 42.0), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add_space(3.0);
                        ui.add(
                            egui::Label::new(semibold(&person.name, theme.text_size + 2.0))
                                .truncate(),
                        );
                        let last = match person.weeks.iter().rposition(|w| *w > 0) {
                            Some(_) => format!("@{}", person.login),
                            None => format!("@{} · {}", person.login, tl("no review lately")),
                        };
                        ui.add(egui::Label::new(muted(&theme, last)).truncate());
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    board::chip(ui, &theme, person.level.label(), person.level.tone(&theme));
                });
            });
        });
    super::widgets::rule(ui, &theme);
    let mut open_mr = None;
    board::section(
        ui,
        &theme,
        tl("Waiting on them"),
        &person.reviews.len().to_string(),
        |ui| {
            waiting_list(ui, &theme, person, now, reminder, &mut open_mr);
        },
    );
    board::section(
        ui,
        &theme,
        tl("Their MRs"),
        &person.mrs.len().to_string(),
        |ui| {
            if person.mrs.is_empty() {
                ui.label(muted(&theme, tl("No open MR.")));
            }
            for m in &person.mrs {
                let caption = super::next::caption(&m.step);
                let summary = m.step.summary();
                let detail = if summary.is_empty() {
                    caption
                } else {
                    format!("{} · {summary}", super::next::caption(&m.step))
                };
                if line(
                    ui,
                    &theme,
                    &m.id,
                    &m.title,
                    &detail,
                    Some(super::next::tone(&m.step, &theme).fg),
                ) {
                    open_mr = Some(m.id.clone());
                }
            }
        },
    );
    board::section(ui, &theme, tl("Reviews"), tl("8 weeks"), |ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
        let max = person.weeks.iter().copied().max().unwrap_or(0).max(1);
        let gap = 6.0;
        let w = (rect.width() - gap * 7.0) / 8.0;
        for (i, n) in person.weeks.iter().enumerate() {
            let h = if *n == 0 {
                2.0
            } else {
                (rect.height() * *n as f32 / max as f32).max(4.0)
            };
            let bar = Rect::from_min_max(
                egui::pos2(rect.left() + i as f32 * (w + gap), rect.bottom() - h),
                egui::pos2(rect.left() + i as f32 * (w + gap) + w, rect.bottom()),
            );
            let color = if i == 7 {
                theme.palette.accent
            } else {
                theme.palette.accent.gamma_multiply(0.5)
            };
            ui.painter().rect_filled(
                bar,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                if *n == 0 { theme.palette.border } else { color },
            );
            ui.interact(
                bar.expand2(Vec2::new(gap / 2.0, 30.0)).intersect(rect),
                ui.scope_id().with(("week", i)),
                Sense::hover(),
            )
            .on_hover_text(tn(u64::from(*n), "{n} review", "{n} reviews"));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(muted(&theme, tl("8 weeks ago")).size(theme.text_size - 2.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    muted(
                        &theme,
                        tf("this week: {n}", &[("n", &person.weeks[7].to_string())]),
                    )
                    .size(theme.text_size - 2.0),
                );
            });
        });
    });
    let mut page = false;
    let mut show = false;
    let mut remind = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                page = primary_button(ui, &theme, tl("Open their page"))
                    .on_hover_text(tl("Or double click their row, or press Enter"))
                    .clicked();
                show = button(ui, &theme, None, tl("Show their MRs"), false).clicked();
                remind = button(
                    ui,
                    &theme,
                    Some(super::icons::Icon::Copy),
                    tl("Copy a reminder"),
                    false,
                )
                .clicked();
            });
        });
    if page {
        g.open_person(&person.login);
    } else if show {
        show_mrs(g, person);
    } else if remind {
        copy_reminder(g, ui, person, now);
    } else if let Some(id) = open_mr {
        g.open_mr(&id);
    }
}

/// The MRs list, narrowed to `person`'s MRs and the reviews waiting on them.
pub(super) fn show_mrs(g: &mut GuiApp, person: &Person) {
    let ids = person
        .mrs
        .iter()
        .map(|m| m.id.clone())
        .chain(person.reviews.iter().map(|r| r.id.clone()))
        .collect();
    g.next_focus = Some(Some(Focus {
        key: String::new(),
        label: tf("{name}: their MRs and reviews", &[("name", &person.name)]),
        ids,
    }));
    g.open_page(super::Page::MergeRequests);
}

/// Copies the reminder of the reviews waiting on `person`.
pub(super) fn copy_reminder(g: &mut GuiApp, ui: &Ui, person: &Person, now: DateTime<Utc>) {
    ui.ctx()
        .copy_text(board::Links::of(&g.app).apply(&reminder_text(person, now)));
    g.app.flash(
        tl("Reminder copied: paste it in a chat or a comment"),
        false,
    );
}

/// The reviews waiting on `person`, oldest first.
pub(super) fn waiting_list(
    ui: &mut Ui,
    theme: &Theme,
    person: &Person,
    now: DateTime<Utc>,
    reminder: f64,
    open_mr: &mut Option<String>,
) {
    if person.reviews.is_empty() {
        ui.label(muted(theme, tl("No review waiting on them.")));
        return;
    }
    let mut reviews: Vec<&Waiting> = person.reviews.iter().collect();
    reviews.sort_by_key(|r| r.since.unwrap_or(now));
    for r in reviews {
        let age = r.since.map(|s| board::days_between(s, now));
        let color = match age {
            Some(a) if a >= reminder => Some(theme.tones.red.fg),
            Some(a) if a >= reminder / 2.0 => Some(theme.tones.amber.fg),
            _ => None,
        };
        let detail = match age {
            Some(a) => format!(
                "{} · {}",
                name_of(&r.author),
                tf("asked {time} ago", &[("time", &board::duration_text(a))])
            ),
            None => name_of(&r.author),
        };
        if line(ui, theme, &r.id, &r.title, &detail, color) {
            *open_mr = Some(r.id.clone());
        }
    }
}

/// A two-line entry of the inspector: the id and the title, a detail under
/// it (its leading dot in `color`); whether it was clicked.
pub(super) fn line(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    title: &str,
    detail: &str,
    color: Option<Color32>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(6.0, 0.0)),
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let id_w = board::paint_tag(
        ui,
        theme,
        egui::pos2(rect.left(), rect.top() + 12.0),
        &format!("!{id}"),
        board::id_tone(theme),
        true,
    );
    let left = rect.left() + id_w + 8.0;
    board::paint_text(
        ui,
        egui::pos2(left, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        title,
        fonts::medium(theme.text_size - 0.5),
        theme.palette.text,
        rect.right() - left,
    );
    let mut x = left;
    if let Some(color) = color {
        ui.painter()
            .circle_filled(egui::pos2(x + 3.0, rect.top() + 31.0), 3.0, color);
        x += 11.0;
    }
    board::paint_text(
        ui,
        egui::pos2(x, rect.top() + 31.0),
        Align2::LEFT_CENTER,
        detail,
        FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
        rect.right() - x,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open its page"))
        .clicked()
}

/// The reminder to paste: the reviews waiting on `person`.
fn reminder_text(person: &Person, now: DateTime<Utc>) -> String {
    let mut text = tf(
        "Hi {name}, these reviews are waiting on you:",
        &[("name", board::first_name(&person.name))],
    );
    text.push('\n');
    for r in &person.reviews {
        let age = r
            .since
            .map(|s| {
                format!(
                    " ({})",
                    tf(
                        "{time} ago",
                        &[("time", &board::duration_text(board::days_between(s, now)))]
                    )
                )
            })
            .unwrap_or_default();
        text.push_str(&format!("- !{} {}{age}\n", r.id, r.title));
    }
    if person.reviews.is_empty() {
        text = tf(
            "Hi {name}, nothing waits on you right now.",
            &[("name", board::first_name(&person.name))],
        );
    }
    text
}

/// The team's summary to paste: who waits on whom.
fn summary(analysis: &Analysis, now: DateTime<Utc>) -> String {
    let mut text = format!("{}\n", tl("Reviews waiting"));
    let mut any = false;
    for p in &analysis.people {
        if p.reviews.is_empty() {
            continue;
        }
        any = true;
        let list = p
            .reviews
            .iter()
            .map(|r| match r.since {
                Some(s) => format!(
                    "!{} ({})",
                    r.id,
                    board::duration_text(board::days_between(s, now))
                ),
                None => format!("!{}", r.id),
            })
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!("- {}: {list}\n", p.name));
    }
    if !any {
        text.push_str(&format!("- {}\n", tl("none")));
    }
    text
}

/// The rail's badge: the reviews waiting past the reminder delay (red past
/// twice that delay, else orange), else the overloaded reviewers (amber).
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let days = PERIODS[g.team.period.min(PERIODS.len() - 1)].0;
    let analysis = analyze(&g.app, now, days, false);
    let reminder = g.app.config.review_reminder_hours.max(1) as f64 / 24.0;
    let tones = &g.theme.tones;
    let mut late: Vec<(&Person, &Waiting, f64)> = analysis
        .people
        .iter()
        .filter(|p| p.level != Level::Bot)
        .flat_map(|p| p.reviews.iter().map(move |r| (p, r)))
        .filter_map(|(p, r)| r.since.map(|s| (p, r, board::days_between(s, now))))
        .filter(|(_, _, d)| *d >= reminder)
        .collect();
    late.sort_by(|a, b| b.2.total_cmp(&a.2));
    if let Some((p, r, worst)) = late.first() {
        let mut hint = tn(
            late.len() as u64,
            "{n} review waits past the reminder delay",
            "{n} reviews wait past the reminder delay",
        );
        hint.push('\n');
        hint.push_str(&tf(
            "The oldest: !{id}, for {name}'s review, {time}",
            &[
                ("id", &r.id),
                ("name", board::first_name(&p.name)),
                ("time", &board::duration_text(*worst)),
            ],
        ));
        let tone = if *worst >= 2.0 * reminder {
            tones.red
        } else {
            tones.orange
        };
        return Some(Badge {
            n: late.len(),
            tone,
            hint,
        });
    }
    let over: Vec<String> = analysis
        .people
        .iter()
        .filter(|p| p.level == Level::Overloaded)
        .map(|p| board::first_name(&p.name).to_string())
        .collect();
    (!over.is_empty()).then(|| Badge {
        n: over.len(),
        tone: tones.amber,
        hint: rail_badges::listed(
            tn(
                over.len() as u64,
                "{n} reviewer overloaded",
                "{n} reviewers overloaded",
            ),
            rail_badges::names(&over, 4),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MrData, ReviewActivity, TrackedMr};
    use gitlab_tracker_core::MergeabilityStatus;

    fn mr(id: &str, author: &str, reviewers: &[&str], state: GitlabMrState) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
        mr.data = MrData {
            title: format!("MR {id}"),
            author: author.into(),
            reviewers: reviewers.iter().map(|r| r.to_string()).collect(),
            state,
            created_at: Some("2026-11-01T08:00:00Z".into()),
            ..MrData::default()
        };
        mr.mergeability = MergeabilityStatus::Mergeable;
        mr
    }

    #[test]
    fn reviews_waiting_and_load() {
        let mut app = crate::app::tests::test_app();
        let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
        let mut a = mr(
            "1",
            "Ann Lee (@ann)",
            &["Bob Ray (@bob)"],
            GitlabMrState::Opened,
        );
        a.review = Some(ReviewActivity {
            review_requested_at: [("bob".to_string(), "2026-11-03T08:00:00Z".to_string())].into(),
            ..ReviewActivity::default()
        });
        let mut b = mr(
            "2",
            "Bob Ray (@bob)",
            &["Ann Lee (@ann)"],
            GitlabMrState::Opened,
        );
        // Asked of Ann two hours ago: lighter than Bob's two-day-old request.
        b.review = Some(ReviewActivity {
            review_requested_at: [("ann".to_string(), "2026-11-05T06:00:00Z".to_string())].into(),
            ..ReviewActivity::default()
        });
        let mut c = mr(
            "3",
            "Cid Moe (@cid)",
            &["Bob Ray (@bob)"],
            GitlabMrState::Opened,
        );
        c.approved_by = vec!["bob".into()];
        app.mrs = vec![a, b, c];
        let analysis = analyze(&app, now, 30, false);
        let bob = analysis.people.iter().find(|p| p.login == "bob").unwrap();
        // Bob approved !3: only !1 waits on him, since its request two days ago.
        assert_eq!(bob.reviews.len(), 1);
        assert_eq!(bob.reviews[0].id, "1");
        assert_eq!(bob.name, "Bob Ray");
        assert_eq!(bob.mrs.len(), 1);
        // Bob is the most loaded.
        assert!((bob.load - 1.0).abs() < f32::EPSILON);
        let ann = analysis.people.iter().find(|p| p.login == "ann").unwrap();
        assert!(ann.load < 1.0);
        assert_eq!(analysis.matrix["ann"]["bob"], 1);
    }

    #[test]
    fn bots_stay_out_unless_asked() {
        let mut app = crate::app::tests::test_app();
        let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
        app.mrs = vec![mr(
            "1",
            "Renovate Bot (@renovate_bot)",
            &[],
            GitlabMrState::Opened,
        )];
        assert!(analyze(&app, now, 30, false).people.is_empty());
        let with = analyze(&app, now, 30, true);
        assert_eq!(with.people.len(), 1);
        assert_eq!(with.people[0].level, Level::Bot);
    }
}
