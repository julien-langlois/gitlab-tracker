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
use super::tip::Tip;
use super::widgets::{avatar, button, muted, primary_button, semibold, split_person};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{Effort, GitlabMrState};
use crate::next_step::{self, login, median, NextStep, Turn};

mod analysis;
mod matrix;
mod people;
mod side_panel;
mod signals;

pub use analysis::*;
use matrix::*;
use people::*;
pub use side_panel::*;
use signals::*;

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

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.team.period.min(PERIODS.len() - 1)].0;
    let bots = g.team.bots;
    let analysis = g.memo.team.get(&g.app.project_id, (days, bots), || {
        analyze(&g.app, now, days, bots)
    });
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
            copy = Tip::new()
                .text(tl("Copy who waits on whom, to paste in a chat"))
                .show(button(
                    ui,
                    &theme,
                    Some(super::icons::Icon::Copy),
                    tl("Copy the summary"),
                    false,
                ))
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

#[cfg(test)]
mod tests;
