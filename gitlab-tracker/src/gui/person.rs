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
use super::tip::{Tip, TipExt};
use super::widgets::{avatar, button, muted, primary_button, semibold};
use super::{GuiApp, Page};
use crate::app::App;
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{Effort, GitlabMrState, PipelineState};
use crate::next_step::{login, median};

mod now_cards;
mod profile_model;
mod work_cards;

use now_cards::*;
pub use profile_model::*;
use work_cards::*;

/// The weeks the activity chart shows.
const WEEKS: usize = 12;
/// A review answered within this many days is a quick one.
const QUICK: f64 = 4.0 / 24.0;

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let now = Utc::now();
    let period = g.team.period.min(PERIODS.len() - 1);
    let days = PERIODS[period].0;
    let bots = g.team.bots;
    let analysis = g.memo.team.get(&g.app.project_id, (days, bots), || {
        team::analyze(&g.app, now, days, bots)
    });
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

#[cfg(test)]
mod tests;
