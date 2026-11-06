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
use super::widgets::{button, muted, primary_button, split_person, FieldLook};
use super::{GuiApp, SlotState};
use crate::app::App;
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use crate::next_step::login;

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

/// The events of one tab's MRs since `since`.
pub fn events_of(app: &App, tab: usize, since: DateTime<Utc>, theme: &Theme) -> Vec<Event> {
    let t = &theme.tones;
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let mut out = Vec::new();
    let names: HashMap<&str, String> = app
        .mrs
        .iter()
        .flat_map(|mr| {
            std::iter::once(&mr.author)
                .chain(&mr.reviewers)
                .chain(&mr.approvers)
                .chain(mr.merged_by.iter())
        })
        .map(|d| (login(d), name(d)))
        .collect();
    let person = |login: &str| -> String {
        names
            .get(login)
            .cloned()
            .unwrap_or_else(|| login.to_string())
    };
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let author = login(&mr.author).to_string();
        let mut push = |kind: Kind,
                        at: Option<&str>,
                        what: String,
                        chip: Option<(String, Tone)>,
                        detail: String,
                        glyph: &'static str,
                        tone: Option<Tone>,
                        people: Vec<String>| {
            let Some(when) = board::time_of(at).filter(|w| *w >= since) else {
                return;
            };
            out.push(Event {
                tab,
                project: project.clone(),
                id: mr.id.clone(),
                title: mr.title.clone(),
                url: mr.web_url.clone(),
                kind,
                when,
                what,
                chip,
                detail,
                glyph,
                tone,
                people,
            });
        };
        push(
            Kind::Activity,
            mr.created_at.as_deref(),
            tf("opened by {name}", &[("name", &name(&mr.author))]),
            None,
            tf(
                "{source} → {target}",
                &[("source", &mr.source_branch), ("target", &mr.target_branch)],
            ),
            "+",
            None,
            vec![author.clone()],
        );
        if let Some(review) = &mr.review {
            for (reviewer, at) in &review.review_requested_at {
                push(
                    Kind::Review,
                    Some(at),
                    tf("review asked of {name}", &[("name", &person(reviewer))]),
                    None,
                    tf("by {name}", &[("name", &name(&mr.author))]),
                    "?",
                    None,
                    vec![reviewer.clone(), author.clone()],
                );
            }
            if let Some(at) = &review.approved_at {
                let who = if mr.approvers.is_empty() {
                    String::new()
                } else {
                    mr.approvers
                        .iter()
                        .map(|a| name(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                push(
                    Kind::Review,
                    Some(at),
                    tl("approved").to_string(),
                    Some((tl("Approved").to_string(), t.green)),
                    if who.is_empty() {
                        String::new()
                    } else {
                        tf("by {names}", &[("names", &who)])
                    },
                    "✓",
                    Some(t.green),
                    mr.approved_by.clone(),
                );
            }
            if let Some(at) = &review.last_push_at {
                push(
                    Kind::Activity,
                    Some(at),
                    tl("new commits pushed").to_string(),
                    None,
                    tf("by {name}", &[("name", &name(&mr.author))]),
                    "↑",
                    None,
                    vec![author.clone()],
                );
            }
        }
        for thread in &mr.threads {
            let state = if thread.resolved {
                Some((tl("Resolved").to_string(), t.green))
            } else {
                Some((tl("Open").to_string(), t.amber))
            };
            push(
                Kind::Review,
                Some(&thread.created_at),
                tf(
                    "thread opened by {name}",
                    &[("name", &person(&thread.author))],
                ),
                state,
                thread.excerpt.clone(),
                "“",
                None,
                vec![thread.author.clone(), author.clone()],
            );
        }
        for pipeline in &mr.pipelines {
            let (label, tone) = super::tones::pipeline(pipeline.status, t);
            let failed: Vec<&str> = pipeline
                .jobs
                .iter()
                .filter(|j| j.status == "failed")
                .map(|j| j.name.as_str())
                .collect();
            let detail = if failed.is_empty() {
                tn(
                    pipeline.jobs.len() as u64,
                    "#{id} · {n} job",
                    "#{id} · {n} jobs",
                )
                .replace("{id}", &pipeline.id.to_string())
            } else {
                tf(
                    "#{id} · failed: {jobs}",
                    &[
                        ("id", &pipeline.id.to_string()),
                        ("jobs", &failed.join(", ")),
                    ],
                )
            };
            let (what, glyph) = match pipeline.status {
                PipelineState::Failed => (tl("pipeline failed").to_string(), "✕"),
                PipelineState::Success => (tl("pipeline passed").to_string(), "✓"),
                _ => (tl("pipeline started").to_string(), "▸"),
            };
            push(
                Kind::Pipeline,
                pipeline.created_at.as_deref(),
                what,
                Some((label.to_string(), tone)),
                detail,
                glyph,
                Some(tone),
                vec![author.clone()],
            );
        }
        match mr.state {
            GitlabMrState::Merged => {
                let by = mr.merged_by.as_deref().map(name);
                push(
                    Kind::Merge,
                    mr.merged_at.as_deref(),
                    match &by {
                        Some(by) => tf("merged by {name}", &[("name", by)]),
                        None => tl("merged").to_string(),
                    },
                    None,
                    tf("into {target}", &[("target", &mr.target_branch)]),
                    "⇥",
                    None,
                    mr.merged_by
                        .iter()
                        .map(|m| login(m).to_string())
                        .chain([author.clone()])
                        .collect(),
                );
            }
            GitlabMrState::Closed => push(
                Kind::Merge,
                mr.updated_at.as_deref(),
                tl("closed without merging").to_string(),
                None,
                String::new(),
                "×",
                Some(t.gray),
                vec![author.clone()],
            ),
            _ => {}
        }
        for branch in &app.branches {
            let Some(at) = app.branch_arrivals.get(&(mr.id.clone(), branch.clone())) else {
                continue;
            };
            if *at < since {
                continue;
            }
            let presence = app.branch_presence(mr, branch);
            let (glyph, label, tone) = super::tones::presence(presence, false, t);
            let how = match presence {
                Some(crate::models::BranchPresence::CherryPicked) => {
                    tl("cherry-picked with its trailer")
                }
                Some(crate::models::BranchPresence::Copied) => {
                    tl("copied without trailer: inferred from the author, date and title")
                }
                Some(crate::models::BranchPresence::Partial) => tl("only some of its commits"),
                _ => tl("its own commits"),
            };
            out.push(Event {
                tab,
                project: project.clone(),
                id: mr.id.clone(),
                title: mr.title.clone(),
                url: mr.web_url.clone(),
                kind: Kind::Branch,
                when: *at,
                what: tf("reached {branch}", &[("branch", branch)]),
                chip: Some((format!("{glyph} {label}"), tone)),
                detail: how.to_string(),
                glyph,
                tone: Some(tone),
                people: vec![author.clone()],
            });
        }
    }
    out
}

/// The events of every open tab since `since`, newest first.
fn collect(g: &GuiApp, since: DateTime<Utc>) -> (Vec<Event>, Vec<(usize, String)>) {
    let mut events = Vec::new();
    let mut tabs = Vec::new();
    for (index, slot) in g.tabs.iter().enumerate() {
        let app = match &slot.state {
            SlotState::Active => &g.app,
            SlotState::Parked(tab) => &tab.app,
            _ => continue,
        };
        tabs.push((
            index,
            app.project_name
                .clone()
                .unwrap_or_else(|| app.project_id.clone()),
        ));
        events.extend(events_of(app, index, since, &g.theme));
    }
    events.sort_by(|a, b| b.when.cmp(&a.when).then(a.kind.cmp(&b.kind)));
    (events, tabs)
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.journal.period.min(PERIODS.len() - 1)].0;
    let since = now - Duration::days(days);
    let (all, tabs) = collect(g, since);
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
        |g, ui| nav(g, ui, &all, &tabs),
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
fn day_label(day: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    if day == today {
        return tl("Today").to_string();
    }
    if Some(day) == today.pred_opt() {
        return tl("Yesterday").to_string();
    }
    let weekday = tc(
        "date",
        match day.weekday() {
            chrono::Weekday::Mon => "Monday",
            chrono::Weekday::Tue => "Tuesday",
            chrono::Weekday::Wed => "Wednesday",
            chrono::Weekday::Thu => "Thursday",
            chrono::Weekday::Fri => "Friday",
            chrono::Weekday::Sat => "Saturday",
            chrono::Weekday::Sun => "Sunday",
        },
    );
    let month = tc(
        "date",
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ][day.month0() as usize],
    );
    tf(
        "{weekday} {day} {month}",
        &[
            ("weekday", weekday),
            ("day", &day.day().to_string()),
            ("month", month),
        ],
    )
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
        g.activate(tab);
        g.show_mr(&id);
    }
}

fn day_heading(ui: &mut Ui, theme: &Theme, label: &str, first: bool) {
    if !first {
        ui.add_space(6.0);
    }
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let width = board::paint_text(
        ui,
        egui::pos2(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::semibold(theme.text_size - 1.0),
        theme.palette.text_secondary,
        rect.width() * 0.6,
    );
    ui.painter().hline(
        (rect.left() + width + 16.0)..=rect.right(),
        rect.center().y,
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
}

/// The line under which the events were already seen.
fn seen_line(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let color = theme.palette.accent;
    let text = tl("Already seen below");
    let galley = board::one_line(
        ui,
        text,
        fonts::medium(theme.text_size - 2.0),
        color,
        rect.width(),
    );
    let w = galley.size().x;
    let cx = rect.center().x;
    let line = egui::Stroke::new(1.0, color.gamma_multiply(0.5));
    ui.painter()
        .hline(rect.left()..=(cx - w / 2.0 - 10.0), rect.center().y, line);
    ui.painter()
        .hline((cx + w / 2.0 + 10.0)..=rect.right(), rect.center().y, line);
    ui.painter().galley(
        egui::pos2(cx - w / 2.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

enum RowClick {
    Select,
    Open,
}

fn event_row(
    ui: &mut Ui,
    theme: &Theme,
    event: &Event,
    selected: bool,
    unread: bool,
    several: bool,
    now: DateTime<Utc>,
) -> Option<RowClick> {
    let (rect, response) = board::row(ui, theme, 46.0, selected);
    if !ui.is_rect_visible(rect) {
        return response.clicked().then_some(RowClick::Select);
    }
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
            CornerRadius {
                nw: 3,
                sw: 3,
                ne: 0,
                se: 0,
            },
            theme.palette.accent,
        );
    }
    let cy = rect.center().y;
    let left = rect.left() + 12.0;
    // The time.
    let time = event.when.with_timezone(&Local).format("%H:%M").to_string();
    board::paint_text(
        ui,
        egui::pos2(left, cy),
        Align2::LEFT_CENTER,
        &time,
        FontId::monospace(theme.text_size - 2.0),
        if unread {
            theme.palette.text
        } else {
            theme.palette.text_muted
        },
        44.0,
    );
    // The circle.
    let tone = event.tone.unwrap_or_else(|| event.kind.tone(theme));
    let center = egui::pos2(left + 64.0, cy);
    ui.painter().circle_filled(center, 13.0, tone.bg);
    board::paint_text(
        ui,
        center,
        Align2::CENTER_CENTER,
        event.glyph,
        fonts::semibold(theme.text_size - 1.0),
        tone.fg,
        24.0,
    );
    // On the right: the ago, the project.
    let ago_w = 76.0;
    board::paint_text(
        ui,
        egui::pos2(rect.right() - 12.0, cy),
        Align2::RIGHT_CENTER,
        &board::ago(event.when, now),
        FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
        ago_w,
    );
    let mut right = rect.right() - 12.0 - ago_w - 10.0;
    if several {
        let galley = ui.painter().layout_no_wrap(
            event.project.clone(),
            fonts::medium(theme.text_size - 2.5),
            theme.tones.gray.fg,
        );
        let w = (galley.size().x + 12.0).min(120.0);
        right -= w;
        board::paint_tag(
            ui,
            theme,
            egui::pos2(right, cy),
            &event.project,
            theme.tones.gray,
            false,
        );
        right -= 10.0;
    }
    // The text: id, what, chip, title; the detail below.
    let text_left = left + 92.0;
    let top = cy - 9.0;
    let mut x = text_left;
    x += board::paint_tag(
        ui,
        theme,
        egui::pos2(x, top),
        &format!("!{}", event.id),
        board::id_tone(theme),
        true,
    ) + 8.0;
    x += board::paint_text(
        ui,
        egui::pos2(x, top),
        Align2::LEFT_CENTER,
        &event.what,
        fonts::medium(theme.text_size - 0.5),
        theme.palette.text,
        (right - x).max(0.0),
    ) + 8.0;
    if let Some((label, tone)) = &event.chip {
        if x + 60.0 < right {
            x += board::paint_tag(ui, theme, egui::pos2(x, top), label, *tone, false) + 8.0;
        }
    }
    if x + 40.0 < right {
        board::paint_text(
            ui,
            egui::pos2(x, top),
            Align2::LEFT_CENTER,
            &event.title,
            FontId::proportional(theme.text_size - 1.0),
            theme.palette.text_muted,
            right - x,
        );
    }
    board::paint_text(
        ui,
        egui::pos2(text_left, cy + 11.0),
        Align2::LEFT_CENTER,
        &event.detail,
        FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
        (right - text_left).max(0.0),
    );
    let response = response.on_hover_text(tl("Double click: show the MR"));
    if response.double_clicked() {
        Some(RowClick::Open)
    } else if response.clicked() {
        Some(RowClick::Select)
    } else {
        None
    }
}

/// The events of the period, filters applied, as stacked bars by kind: one
/// bar an hour over 24 h, one a day over 7 or 30 days.
fn activity_card(ui: &mut Ui, theme: &Theme, events: &[&Event], days: i64, now: DateTime<Utc>) {
    let hourly = days <= 1;
    let local_now = now.with_timezone(&Local);
    // The buckets, oldest first: their start (local) and label.
    let buckets: Vec<chrono::DateTime<Local>> = if hourly {
        let hour = local_now
            .date_naive()
            .and_hms_opt(local_now.hour(), 0, 0)
            .and_then(|t| t.and_local_timezone(Local).earliest())
            .unwrap_or(local_now);
        (0..24).rev().map(|h| hour - Duration::hours(h)).collect()
    } else {
        let today = local_now.date_naive();
        (0..days)
            .rev()
            .filter_map(|d| today.checked_sub_days(chrono::Days::new(d as u64)))
            .filter_map(|d| d.and_hms_opt(0, 0, 0))
            .filter_map(|t| t.and_local_timezone(Local).earliest())
            .collect()
    };
    let step = if hourly {
        Duration::hours(1)
    } else {
        Duration::days(1)
    };
    let mut counts = vec![BTreeMap::<Kind, usize>::new(); buckets.len()];
    for e in events {
        let at = e.when.with_timezone(&Local);
        if let Some(i) = buckets.iter().position(|b| at >= *b && at < *b + step) {
            *counts[i].entry(e.kind).or_default() += 1;
        }
    }
    let total: usize = counts.iter().map(|c| c.values().sum::<usize>()).sum();
    let note = tn(total as u64, "{n} event", "{n} events");
    let title = if hourly {
        tl("Activity by hour")
    } else {
        tl("Activity by day")
    };
    board::card(ui, theme, title, &note, 0.0, |ui| {
        let max = counts
            .iter()
            .map(|c| c.values().sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(1);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 110.0), Sense::hover());
        let n = buckets.len().max(1);
        let gap = if n > 14 { 4.0 } else { 8.0 };
        let w = ((rect.width() - gap * (n - 1) as f32) / n as f32).max(2.0);
        for (i, day) in counts.iter().enumerate() {
            let x = rect.left() + i as f32 * (w + gap);
            let column =
                Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + w, rect.bottom()));
            let sum: usize = day.values().sum();
            if sum == 0 {
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        egui::pos2(x, rect.bottom() - 2.0),
                        egui::pos2(x + w, rect.bottom()),
                    ),
                    CornerRadius::same(1),
                    theme.palette.border,
                );
            }
            let mut y = rect.bottom();
            for kind in Kind::ALL {
                let k = day.get(&kind).copied().unwrap_or(0);
                if k == 0 {
                    continue;
                }
                let h = (rect.height() * k as f32 / max as f32).max(3.0);
                ui.painter().rect_filled(
                    Rect::from_min_max(egui::pos2(x, y - h), egui::pos2(x + w, y)),
                    CornerRadius::same(2),
                    kind.tone(theme).fg,
                );
                y -= h + 1.5;
            }
            let when = if hourly {
                buckets[i].format("%H:00").to_string()
            } else {
                buckets[i].format("%Y-%m-%d").to_string()
            };
            let mut tip = format!("{when} · {}", tn(sum as u64, "{n} event", "{n} events"));
            for (kind, k) in day {
                tip.push_str(&format!("\n{} · {k}", kind.short()));
            }
            ui.interact(column, ui.scope_id().with(("bucket", i)), Sense::hover())
                .on_hover_text(tip);
        }
        // The labels: as many as fit, evenly spread, the last one always.
        let (labels, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
        let font = FontId::proportional(theme.text_size - 2.5);
        let label = |i: usize| -> String {
            if hourly {
                buckets[i].format("%H:00").to_string()
            } else if n <= 7 {
                tc(
                    "date",
                    match buckets[i].weekday() {
                        chrono::Weekday::Mon => "Mon",
                        chrono::Weekday::Tue => "Tue",
                        chrono::Weekday::Wed => "Wed",
                        chrono::Weekday::Thu => "Thu",
                        chrono::Weekday::Fri => "Fri",
                        chrono::Weekday::Sat => "Sat",
                        chrono::Weekday::Sun => "Sun",
                    },
                )
                .to_string()
            } else {
                buckets[i].format("%d/%m").to_string()
            }
        };
        let room = 52.0;
        let every = ((room / (w + gap)).ceil() as usize).max(1);
        let mut right_edge = f32::NEG_INFINITY;
        let last_cx = labels.left() + (n - 1) as f32 * (w + gap) + w / 2.0;
        for i in (0..n).filter(|i| (n - 1 - i) % every == 0) {
            let cx = labels.left() + i as f32 * (w + gap) + w / 2.0;
            let text = label(i);
            let galley = board::one_line(ui, &text, font.clone(), theme.palette.text_muted, room);
            let half = galley.size().x / 2.0;
            let left = (cx - half).clamp(labels.left(), labels.right() - galley.size().x);
            if left < right_edge + 8.0
                || (i + 1 < n && left + galley.size().x > last_cx - half - 8.0 && i != n - 1)
            {
                continue;
            }
            right_edge = left + galley.size().x;
            ui.painter().galley(
                egui::pos2(left, labels.center().y - galley.size().y / 2.0),
                galley,
                theme.palette.text_muted,
            );
        }
        ui.add_space(8.0);
        let legend: Vec<(Color32, String)> = Kind::ALL
            .iter()
            .map(|k| (k.tone(theme).fg, k.short().to_string()))
            .collect();
        board::legend(ui, theme, &legend);
    });
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    shown: &[&Event],
    selected: Option<&Event>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    board::section(
        ui,
        &theme,
        tl("In this period"),
        &tn(shown.len() as u64, "{n} event", "{n} events"),
        |ui| {
            let mut by_kind: BTreeMap<Kind, usize> = BTreeMap::new();
            for e in shown {
                *by_kind.entry(e.kind).or_default() += 1;
            }
            if by_kind.is_empty() {
                ui.label(muted(&theme, tl("Nothing yet.")));
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for (kind, n) in &by_kind {
                    board::chip(
                        ui,
                        &theme,
                        &format!("{n} · {}", kind.short()),
                        kind.tone(&theme),
                    );
                }
            });
        },
    );
    board::section(ui, &theme, tl("Most active"), "", |ui| {
        let mut by_mr: HashMap<(usize, &str), (usize, &str)> = HashMap::new();
        for e in shown {
            let entry = by_mr
                .entry((e.tab, e.id.as_str()))
                .or_insert((0, e.title.as_str()));
            entry.0 += 1;
        }
        let mut list: Vec<_> = by_mr.into_iter().collect();
        list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0 .1.cmp(b.0 .1)));
        if list.is_empty() {
            ui.label(muted(&theme, tl("Nothing yet.")));
        }
        let mut open = None;
        for ((tab, id), (n, title)) in list.into_iter().take(5) {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
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
                egui::pos2(rect.left(), rect.center().y),
                &format!("!{id}"),
                board::id_tone(&theme),
                true,
            );
            let count = n.to_string();
            board::paint_text(
                ui,
                egui::pos2(rect.right(), rect.center().y),
                Align2::RIGHT_CENTER,
                &count,
                fonts::medium(theme.text_size - 1.5),
                theme.palette.text_muted,
                30.0,
            );
            board::paint_text(
                ui,
                egui::pos2(rect.left() + w + 8.0, rect.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text,
                rect.width() - w - 44.0,
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some((tab, id.to_string()));
            }
        }
        if let Some((tab, id)) = open {
            g.activate(tab);
            g.show_mr(&id);
        }
    });
    let Some(event) = selected else {
        return;
    };
    let mut open = false;
    let mut web = false;
    board::section(
        ui,
        &theme,
        tl("Selected"),
        &board::ago(event.when, now),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                board::mr_pill(ui, &theme, &event.id, &event.title);
                if let Some((label, tone)) = &event.chip {
                    board::chip(ui, &theme, label, *tone);
                }
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(super::widgets::semibold(&event.title, theme.text_size)).wrap(),
            );
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("!{} {}", event.id, event.what))
                        .color(theme.palette.text_secondary),
                )
                .wrap(),
            );
            if !event.detail.is_empty() {
                ui.add(egui::Label::new(muted(&theme, &event.detail)).wrap());
            }
            ui.add(egui::Label::new(
                muted(
                    &theme,
                    event
                        .when
                        .with_timezone(&Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string(),
                )
                .size(theme.text_size - 2.0),
            ));
            ui.add_space(GAP);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                open = primary_button(ui, &theme, tl("Show the MR")).clicked();
                web = button(
                    ui,
                    &theme,
                    Some(super::icons::Icon::ExternalLink),
                    tl("Open in GitLab"),
                    false,
                )
                .clicked();
            });
        },
    );
    if web {
        board::open_url(ui, &event.url);
    }
    if open {
        g.activate(event.tab);
        g.show_mr(&event.id);
    }
}

/// The rail's badge: the events since the journal was last marked as read (the
/// last day at most when it never was), others' and not the bots'; red when a
/// pipeline failed among them, else blue.
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let floor = now - Duration::days(PERIODS[1].0);
    let since = match g.feed_seen_at {
        Some(seen) => seen.with_timezone(&Utc).max(floor),
        None => now - Duration::days(1),
    };
    let me = g.app.config.gitlab_username.as_deref();
    let (all, _) = collect(g, since);
    let new: Vec<&Event> = all
        .iter()
        .filter(|e| e.when > since)
        .filter(|e| {
            e.people
                .first()
                .is_none_or(|who| !board::is_bot(who) && me.is_none_or(|me| login(who) != me))
        })
        .collect();
    if new.is_empty() {
        return None;
    }
    let failed = new
        .iter()
        .filter(|e| e.kind == Kind::Pipeline && e.glyph == "✕")
        .count();
    let tones = &g.theme.tones;
    let mut hint = tn(
        new.len() as u64,
        "{n} new event since the journal was last read",
        "{n} new events since the journal was last read",
    );
    if failed > 0 {
        hint.push('\n');
        hint.push_str(&tn(
            failed as u64,
            "{n} failed pipeline among them",
            "{n} failed pipelines among them",
        ));
    }
    Some(Badge {
        n: new.len(),
        tone: if failed > 0 { tones.red } else { tones.blue },
        hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MrData, Pipeline, ReviewActivity, TrackedMr};

    #[test]
    fn events_come_from_the_dates_of_the_mrs() {
        let mut app = crate::app::tests::test_app();
        let mut mr = TrackedMr::placeholder("7".into(), "Fix it".into(), None);
        mr.data = MrData {
            title: "Fix it".into(),
            author: "Ann Lee (@ann)".into(),
            state: GitlabMrState::Merged,
            created_at: Some("2026-11-01T08:00:00Z".into()),
            merged_at: Some("2026-11-04T08:00:00Z".into()),
            merged_by: Some("Bob Ray (@bob)".into()),
            review: Some(ReviewActivity {
                review_requested_at: [("bob".to_string(), "2026-11-02T08:00:00Z".to_string())]
                    .into(),
                ..ReviewActivity::default()
            }),
            pipelines: vec![Pipeline {
                id: 9,
                status: PipelineState::Failed,
                created_at: Some("2026-11-03T08:00:00Z".into()),
                ..Pipeline::default()
            }],
            ..MrData::default()
        };
        app.mrs = vec![mr];
        let theme = Theme::dark();
        let since = board::parse_time("2026-10-01T00:00:00Z").unwrap();
        let events = events_of(&app, 0, since, &theme);
        let kinds: Vec<Kind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![Kind::Activity, Kind::Review, Kind::Pipeline, Kind::Merge]
        );
        assert!(events[1].what.contains("Bob Ray"));
        assert!(events[3].what.contains("Bob Ray"));
        // Only what happened since the cut.
        let since = board::parse_time("2026-11-03T12:00:00Z").unwrap();
        assert_eq!(events_of(&app, 0, since, &theme).len(), 1);
    }
}
