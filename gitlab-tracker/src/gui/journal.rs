//! The journal: what happened on the tracked MRs of every open tab, in the
//! order it happened — opened, review asked, approved, threads, pipelines,
//! merged, arrived on a branch —, rebuilt from the dates the MRs carry, so it
//! needs no fetch of its own and has a past from the first launch.

use std::collections::{BTreeMap, HashMap, HashSet};

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Datelike, Duration, Local, Timelike, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Ui, Vec2};

use super::board::{self, GAP};
use super::rail_badges::Badge;
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button, split_person, FieldLook};
use super::{GuiApp, SlotState};
use crate::app::App;
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use crate::next_step::login;

mod activity;
mod reading;
mod rows;
mod side_panel;

use activity::*;
pub use reading::*;
use rows::*;
pub use side_panel::*;

/// What an event is about, for its color and the filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Reached a tracked branch.
    Branch,
    /// Merged or closed.
    Merge,
    Pipeline,
    /// Review asked, given, approved; threads.
    Review,
    /// Opened, pushed.
    Activity,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Branch,
        Kind::Merge,
        Kind::Pipeline,
        Kind::Review,
        Kind::Activity,
    ];

    fn label(self) -> &'static str {
        tl(match self {
            Kind::Branch => "Branch arrivals",
            Kind::Merge => "Merges and closes",
            Kind::Pipeline => "Pipelines",
            Kind::Review => "Reviews and threads",
            Kind::Activity => "Opened and pushed",
        })
    }

    fn short(self) -> &'static str {
        tl(match self {
            Kind::Branch => "Branches",
            Kind::Merge => "Merges",
            Kind::Pipeline => "CI",
            Kind::Review => "Reviews",
            Kind::Activity => "Activity",
        })
    }

    fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Kind::Branch => t.green,
            Kind::Merge => t.purple,
            Kind::Pipeline => t.red,
            Kind::Review => t.blue,
            Kind::Activity => t.gray,
        }
    }
}

/// One thing that happened.
#[derive(Debug, Clone)]
pub struct Event {
    pub tab: usize,
    pub project: String,
    pub id: String,
    pub title: String,
    pub url: String,
    pub kind: Kind,
    pub when: DateTime<Utc>,
    /// What happened, after the MR id: "reached staging".
    pub what: String,
    /// A chip after it, in its own tone.
    pub chip: Option<(String, Tone)>,
    /// The second line.
    pub detail: String,
    /// The glyph in the event's circle.
    pub glyph: &'static str,
    /// Its own tone when it differs from its kind's (a passed pipeline).
    pub tone: Option<Tone>,
    /// The people it involves, by login.
    pub people: Vec<String>,
}

impl Event {
    fn key(&self) -> (usize, String, i64, Kind, String) {
        (
            self.tab,
            self.id.clone(),
            self.when.timestamp(),
            self.kind,
            self.what.clone(),
        )
    }
}

/// The journal's choices.
#[derive(Debug, Clone)]
pub struct State {
    /// Index in [`PERIODS`].
    pub period: usize,
    pub hidden: HashSet<Kind>,
    pub hidden_tabs: HashSet<usize>,
    pub mine: bool,
    pub bots: bool,
    pub search: String,
    pub selected: Option<(usize, String, i64, Kind, String)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            period: 1,
            hidden: HashSet::new(),
            hidden_tabs: HashSet::new(),
            mine: false,
            bots: true,
            search: String::new(),
            selected: None,
        }
    }
}

const PERIODS: [(i64, &str); 3] = [(1, "24 h"), (7, "7 d"), (30, "30 d")];

/// The most events listed: older ones are left out.
const MOST: usize = 400;

fn name(display: &str) -> String {
    split_person(display).0.to_string()
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.journal.period.min(PERIODS.len() - 1)].0;
    let since = now - Duration::days(days);
    let open = g
        .tabs
        .iter()
        .filter(|t| matches!(t.state, SlotState::Active | SlotState::Parked(_)))
        .count();
    let light = g.theme.mode == bevel_theme::Mode::Light;
    let read = g
        .memo
        .journal
        .get("", (days, open, light), || collect(g, since));
    let (all, tabs) = (&read.0, &read.1);
    let me = g.app.config.gitlab_username.clone();
    let query = g.journal.search.trim().to_lowercase();
    let state = &g.journal;
    let filtered: Vec<&Event> = all
        .iter()
        .filter(|e| !state.hidden.contains(&e.kind))
        .filter(|e| !state.hidden_tabs.contains(&e.tab))
        .filter(|e| {
            !state.mine
                || me
                    .as_deref()
                    .is_some_and(|me| e.people.iter().any(|p| p == me))
        })
        .filter(|e| state.bots || !e.people.first().is_some_and(|p| board::is_bot(p)))
        .filter(|e| {
            query.is_empty()
                || format!(
                    "!{} {} {} {} {}",
                    e.id, e.title, e.what, e.detail, e.project
                )
                .to_lowercase()
                .contains(&query)
        })
        .collect();
    let shown: Vec<&Event> = filtered.iter().take(MOST).copied().collect();
    let selected = g
        .journal
        .selected
        .clone()
        .and_then(|key| shown.iter().find(|e| e.key() == key).copied().cloned())
        .or_else(|| shown.first().map(|e| (*e).clone()));
    board::screen(
        g,
        ui,
        sizes,
        "journal",
        |g, ui| nav(g, ui, all, tabs),
        |g, ui| inspector(g, ui, &filtered, selected.as_ref(), now),
        |g, ui| {
            main(
                g,
                ui,
                &shown,
                &filtered,
                days,
                selected.as_ref(),
                now,
                tabs.len() > 1,
            )
        },
    );
}

fn nav(g: &mut GuiApp, ui: &mut Ui, all: &[Event], tabs: &[(usize, String)]) {
    let theme = g.theme;
    let subtitle = tf(
        "{events} · {tabs}",
        &[
            ("events", &tn(all.len() as u64, "{n} event", "{n} events")),
            ("tabs", &tn(tabs.len() as u64, "{n} tab", "{n} tabs")),
        ],
    );
    board::nav_header(ui, &theme, tl("Journal"), &subtitle);
    board::nav_group(ui, &theme, tl("Types"), |ui| {
        for kind in Kind::ALL {
            let n = all.iter().filter(|e| e.kind == kind).count();
            let on = !g.journal.hidden.contains(&kind);
            if board::nav_check(
                ui,
                &theme,
                kind.label(),
                &n.to_string(),
                kind.tone(&theme).fg,
                on,
            )
            .clicked()
            {
                if on {
                    g.journal.hidden.insert(kind);
                } else {
                    g.journal.hidden.remove(&kind);
                }
            }
        }
    });
    if tabs.len() > 1 {
        board::nav_group(ui, &theme, tl("Tabs"), |ui| {
            for (index, name) in tabs {
                let n = all.iter().filter(|e| e.tab == *index).count();
                let on = !g.journal.hidden_tabs.contains(index);
                if board::nav_check(ui, &theme, name, &n.to_string(), theme.palette.accent, on)
                    .clicked()
                {
                    if on {
                        g.journal.hidden_tabs.insert(*index);
                    } else {
                        g.journal.hidden_tabs.remove(index);
                    }
                }
            }
        });
    }
    board::nav_group(ui, &theme, tl("Period"), |ui| {
        let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
        if let Some(i) = board::segmented(ui, &theme, "journal_period", &labels, g.journal.period) {
            g.journal.period = i;
        }
    });
    board::nav_group(ui, &theme, "", |ui| {
        let known = g.app.config.gitlab_username.is_some();
        ui.add_enabled_ui(known, |ui| {
            if board::nav_switch(ui, &theme, g.journal.mine, tl("Only what concerns me")) {
                g.journal.mine = !g.journal.mine;
            }
        });
        if board::nav_switch(ui, &theme, !g.journal.bots, tl("Hide the bots")) {
            g.journal.bots = !g.journal.bots;
        }
    });
    board::nav_note(
        ui,
        &theme,
        tl("Rebuilt from what the MRs carry: their dates, reviews, threads, pipelines and branch arrivals. Nothing is fetched for it."),
    );
}

/// The heading of a day: "Today", "Yesterday", "Monday 2 November".
pub(super) fn day_label(day: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    if day == today {
        return tl("Today").to_string();
    }
    if Some(day) == today.pred_opt() {
        return tl("Yesterday").to_string();
    }
    super::widgets::long_date(day, false)
}

#[allow(clippy::too_many_arguments)]
fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    shown: &[&Event],
    filtered: &[&Event],
    days: i64,
    selected: Option<&Event>,
    now: DateTime<Utc>,
    several: bool,
) {
    let theme = g.theme;
    let mut seen = false;
    let mut search = std::mem::take(&mut g.journal.search);
    board::heading(
        ui,
        &theme,
        tl("Journal"),
        tl("What moved, in the order it happened"),
        |ui| {
            seen = button(
                ui,
                &theme,
                Some(super::icons::Icon::Clipboard),
                tl("Mark all as read"),
                false,
            )
            .clicked();
            let width = (ui.available_width() - 8.0).clamp(140.0, 280.0);
            super::widgets::field(
                ui,
                &theme,
                &mut search,
                "journal_search",
                FieldLook {
                    hint: tl("MR, person, branch…"),
                    icon: Some(super::icons::Icon::Search),
                    width,
                    active: false,
                },
            );
        },
    );
    g.journal.search = search;
    if seen {
        g.feed_seen_at = Some(Local::now());
    }
    ui.add_space(14.0);
    activity_card(ui, &theme, filtered, days, now);
    ui.add_space(GAP);
    let seen_at = g.feed_seen_at.map(|t| t.with_timezone(&Utc));
    let mut picked = None;
    let mut open = None;
    board::card(ui, &theme, "", "", 0.0, |ui| {
        if shown.is_empty() {
            board::empty(
                ui,
                &theme,
                tl("Nothing happened in this period with these filters."),
            );
            return;
        }
        let today = Local::now().date_naive();
        let mut day = None;
        let mut divider = false;
        for event in shown {
            // The line between what is new and what was already seen.
            if !divider && seen_at.is_some_and(|s| event.when <= s) {
                divider = true;
                if day.is_some() {
                    seen_line(ui, &theme);
                }
            }
            let local = event.when.with_timezone(&Local).date_naive();
            if day != Some(local) {
                day_heading(ui, &theme, &day_label(local, today), day.is_none());
                day = Some(local);
            }
            let is_selected = selected.is_some_and(|s| s.key() == event.key());
            let unread = seen_at.is_none_or(|s| event.when > s);
            match event_row(ui, &theme, event, is_selected, unread, several, now) {
                Some(RowClick::Select) => picked = Some(event.key()),
                Some(RowClick::Open) => open = Some((event.tab, event.id.clone())),
                None => {}
            }
        }
        if shown.len() >= MOST {
            ui.add_space(8.0);
            board::caption(
                ui,
                &theme,
                &tf(
                    "The {n} latest events: narrow the period or the types to see older ones.",
                    &[("n", &MOST.to_string())],
                ),
            );
        }
    });
    if let Some(key) = picked {
        g.journal.selected = Some(key);
    }
    if let Some((tab, id)) = open {
        g.open_link(tab, &id);
    }
}

#[cfg(test)]
mod tests;
