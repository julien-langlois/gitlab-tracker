//! The milestones page: for the milestone picked, its MRs by where they stand
//! — on production, on the way there, to merge, in review, back on their
//! author, drafts —, whether it makes its due date at the pace of the last
//! four weeks, its burn-up, and its tickets and people.

use std::collections::BTreeMap;

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, GAP};
use super::rail_badges::Badge;
use super::shell::Sizes;
use super::widgets::{avatar, button, muted, primary_button, semibold, split_person};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, GitlabMrState, TrackedMr};
use crate::next_step::{self, Actor, NextStep};
use gitlab_tracker_core::MergeabilityStatus;

/// The milestones page's choices.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The milestone shown, by title; `Some("")` for the MRs without one.
    pub selected: Option<String>,
    pub show_done: bool,
}

/// Where an MR of a milestone stands, from the closest to done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stand {
    Delivered,
    OnTheWay,
    ToMerge,
    InReview,
    OnAuthor,
    Draft,
}

impl Stand {
    const ALL: [Stand; 6] = [
        Stand::Delivered,
        Stand::OnTheWay,
        Stand::ToMerge,
        Stand::InReview,
        Stand::OnAuthor,
        Stand::Draft,
    ];

    fn label(self, last: &str) -> String {
        match self {
            Stand::Delivered => tf("On {branch}", &[("branch", last)]),
            Stand::OnTheWay => tf("Merged, not on {branch} yet", &[("branch", last)]),
            Stand::ToMerge => tl("Ready to merge").into(),
            Stand::InReview => tl("In review").into(),
            Stand::OnAuthor => tl("Back on their author").into(),
            Stand::Draft => tl("Drafts").into(),
        }
    }

    fn short(self) -> &'static str {
        tl(match self {
            Stand::Delivered => "Delivered",
            Stand::OnTheWay => "On the way",
            Stand::ToMerge => "To merge",
            Stand::InReview => "In review",
            Stand::OnAuthor => "On the author",
            Stand::Draft => "Draft",
        })
    }

    fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Stand::Delivered => t.green,
            Stand::OnTheWay => t.purple,
            Stand::ToMerge => t.teal,
            Stand::InReview => t.blue,
            Stand::OnAuthor => t.red,
            Stand::Draft => t.gray,
        }
    }
}

/// An MR of a milestone.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub author: String,
    pub stand: Stand,
    pub step: NextStep,
    pub created: Option<DateTime<Utc>>,
    /// When it reached the last branch of the flow.
    pub delivered: Option<DateTime<Utc>>,
    /// Its presence on each tracked branch, in flow order.
    pub branches: Vec<Option<BranchPresence>>,
    pub ticket: Option<(String, Option<u32>, Option<u32>)>,
}

/// A milestone and its MRs.
#[derive(Debug, Clone)]
pub struct Milestone {
    /// Empty for the MRs without a milestone.
    pub title: String,
    pub due: Option<NaiveDate>,
    pub description: Option<String>,
    pub items: Vec<Item>,
}

impl Milestone {
    fn delivered(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.stand == Stand::Delivered)
            .count()
    }

    fn done(&self) -> bool {
        !self.items.is_empty() && self.delivered() == self.items.len()
    }

    fn name(&self) -> String {
        if self.title.is_empty() {
            tl("No milestone").to_string()
        } else {
            self.title.clone()
        }
    }
}

/// The last branch of the flow (production), and the tracked branches in order.
fn flow_of(app: &App) -> (String, Vec<String>) {
    let flow = app.branch_flow();
    let ordered: Vec<String> = flow.in_order().iter().map(|b| b.name.clone()).collect();
    (ordered.last().cloned().unwrap_or_default(), ordered)
}

/// When `mr` reached `branch`: its recorded arrival, else its merge when
/// `branch` is its target.
fn arrived(app: &App, mr: &TrackedMr, branch: &str) -> Option<DateTime<Utc>> {
    app.branch_arrivals
        .get(&(mr.id.clone(), branch.to_string()))
        .copied()
        .or_else(|| {
            (mr.target_branch == branch)
                .then(|| board::time_of(mr.merged_at.as_deref()))
                .flatten()
        })
}

fn stand_of(app: &App, mr: &TrackedMr, step: &NextStep, last: &str) -> Stand {
    let on_last = app
        .branch_presence(mr, last)
        .is_some_and(|p| !p.is_missing());
    if mr.state == GitlabMrState::Merged {
        return if on_last || last.is_empty() {
            Stand::Delivered
        } else {
            Stand::OnTheWay
        };
    }
    if mr.mergeability == MergeabilityStatus::Draft {
        return Stand::Draft;
    }
    match step.actor {
        Actor::Merger => Stand::ToMerge,
        Actor::Reviewers => Stand::InReview,
        _ => Stand::OnAuthor,
    }
}

/// The milestones of `app`: those GitLab lists and those its MRs carry, with
/// their MRs (closed ones left out), the soonest due first, then the MRs
/// without a milestone.
pub fn milestones(app: &App) -> Vec<Milestone> {
    let me = app.config.gitlab_username.as_deref();
    let (last, ordered) = flow_of(app);
    let mut by_title: BTreeMap<String, Milestone> = BTreeMap::new();
    for m in &app.milestones {
        by_title.insert(
            m.title.clone(),
            Milestone {
                title: m.title.clone(),
                due: m.due_date.as_deref().and_then(|d| d.parse().ok()),
                description: m.description.clone().filter(|d| !d.trim().is_empty()),
                items: Vec::new(),
            },
        );
    }
    for mr in &app.mrs {
        if mr.title.is_empty() || mr.state == GitlabMrState::Closed {
            continue;
        }
        let title = mr.milestone.clone().unwrap_or_default();
        let entry = by_title.entry(title.clone()).or_insert_with(|| Milestone {
            title: title.clone(),
            due: None,
            description: None,
            items: Vec::new(),
        });
        if entry.due.is_none() {
            entry.due = mr
                .milestone_due_date
                .as_deref()
                .and_then(|d| d.parse().ok());
        }
        if entry.description.is_none() {
            entry.description = mr
                .milestone_description
                .clone()
                .filter(|d| !d.trim().is_empty());
        }
        let step = next_step::of(app, mr, me);
        let stand = stand_of(app, mr, &step, &last);
        entry.items.push(Item {
            id: mr.id.clone(),
            title: mr.title.clone(),
            author: mr.author.clone(),
            stand,
            created: board::time_of(mr.created_at.as_deref()),
            delivered: (stand == Stand::Delivered)
                .then(|| {
                    arrived(app, mr, &last).or_else(|| board::time_of(mr.merged_at.as_deref()))
                })
                .flatten(),
            branches: ordered.iter().map(|b| app.branch_presence(mr, b)).collect(),
            ticket: mr
                .linked_ticket
                .as_ref()
                .map(|t| (t.status.clone(), t.time_spent, t.time_estimate)),
            step,
        });
    }
    let mut list: Vec<Milestone> = by_title.into_values().collect();
    for m in &mut list {
        m.items
            .sort_by(|a, b| a.stand.cmp(&b.stand).then(a.id.cmp(&b.id)));
    }
    list.sort_by(|a, b| {
        (a.title.is_empty(), a.due.is_none(), a.due, a.title.clone()).cmp(&(
            b.title.is_empty(),
            b.due.is_none(),
            b.due,
            b.title.clone(),
        ))
    });
    list
}

/// MRs delivered per week over the last four weeks, all milestones together.
pub fn pace(app: &App, now: DateTime<Utc>) -> f64 {
    let (last, _) = flow_of(app);
    let since = now - Duration::days(28);
    let delivered = app
        .mrs
        .iter()
        .filter(|mr| mr.state == GitlabMrState::Merged)
        .filter(|mr| {
            app.branch_presence(mr, &last)
                .is_some_and(|p| !p.is_missing())
        })
        .filter_map(|mr| {
            arrived(app, mr, &last).or_else(|| board::time_of(mr.merged_at.as_deref()))
        })
        .filter(|at| *at >= since)
        .count();
    delivered as f64 / 4.0
}

/// When the milestone will be done at `pace` per week, from `now`.
fn projection(m: &Milestone, pace: f64, now: DateTime<Utc>) -> Option<NaiveDate> {
    let left = m.items.len() - m.delivered();
    if left == 0 {
        return Some(now.date_naive());
    }
    (pace > 0.0)
        .then(|| (now + Duration::minutes((left as f64 / pace * 7.0 * 1440.0) as i64)).date_naive())
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let all = milestones(&g.app);
    let pace = pace(&g.app, now);
    let shown = g
        .milestones
        .selected
        .clone()
        .filter(|s| all.iter().any(|m| &m.title == s))
        .or_else(|| {
            all.iter()
                .find(|m| !m.title.is_empty() && !m.done())
                .or_else(|| all.first())
                .map(|m| m.title.clone())
        });
    let current = shown
        .as_ref()
        .and_then(|s| all.iter().find(|m| &m.title == s))
        .cloned();
    board::screen(
        g,
        ui,
        sizes,
        "milestones",
        |g, ui| nav(g, ui, &all, shown.as_deref(), now),
        |g, ui| inspector(g, ui, current.as_ref(), pace, now),
        |g, ui| main(g, ui, current.as_ref(), pace, now),
    );
}

/// "in 6 d", "today", "3 d late", in the tone it deserves.
fn due_text(
    due: Option<NaiveDate>,
    now: DateTime<Utc>,
    done: bool,
    theme: &Theme,
) -> (String, Color32) {
    let Some(due) = due else {
        return (tl("no due date").to_string(), theme.palette.text_muted);
    };
    let days = (due - now.date_naive()).num_days();
    let color = if done {
        theme.palette.text_muted
    } else if days < 0 {
        theme.tones.red.fg
    } else if days <= 7 {
        theme.tones.amber.fg
    } else {
        theme.palette.text_muted
    };
    let text = match days {
        0 => tl("due today").to_string(),
        d if d > 0 => tn(d as u64, "in {n} day", "in {n} days"),
        d => tn((-d) as u64, "{n} day late", "{n} days late"),
    };
    (text, color)
}

fn nav(g: &mut GuiApp, ui: &mut Ui, all: &[Milestone], shown: Option<&str>, now: DateTime<Utc>) {
    let theme = g.theme;
    let open = all
        .iter()
        .filter(|m| !m.title.is_empty() && !m.done())
        .count();
    let subtitle = tn(
        open as u64,
        "{n} milestone in progress",
        "{n} milestones in progress",
    );
    board::nav_header(ui, &theme, tl("Milestones"), &subtitle);
    let mut picked = None;
    board::nav_group(ui, &theme, tl("Milestones"), |ui| {
        let list: Vec<&Milestone> = all
            .iter()
            .filter(|m| g.milestones.show_done || !m.done() || shown == Some(m.title.as_str()))
            .collect();
        if list.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(muted(&theme, tl("No milestone on the tracked MRs.")));
            });
        }
        for m in list {
            let selected = shown == Some(m.title.as_str());
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
            let fill = if selected {
                theme.palette.selection
            } else if response.hovered() {
                theme.palette.raised
            } else {
                Color32::TRANSPARENT
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
            let inner = rect.shrink2(Vec2::new(10.0, 8.0));
            let (due, color) = due_text(m.due, now, m.done(), &theme);
            let due_w = if m.title.is_empty() {
                0.0
            } else {
                board::paint_text(
                    ui,
                    egui::pos2(inner.right(), inner.top() + 8.0),
                    Align2::RIGHT_CENTER,
                    &due,
                    FontId::proportional(theme.text_size - 2.0),
                    color,
                    inner.width() * 0.5,
                ) + 8.0
            };
            let font = if m.title.is_empty() {
                FontId::proportional(theme.text_size - 0.5)
            } else {
                egui::FontId::monospace(theme.text_size - 0.5)
            };
            board::paint_text(
                ui,
                egui::pos2(inner.left(), inner.top() + 8.0),
                Align2::LEFT_CENTER,
                &m.name(),
                font,
                if selected {
                    theme.palette.text
                } else {
                    theme.palette.text_secondary
                },
                inner.width() - due_w,
            );
            let total = m.items.len().max(1) as f32;
            let track = Rect::from_min_size(
                egui::pos2(inner.left(), inner.top() + 20.0),
                Vec2::new(inner.width(), 5.0),
            );
            ui.painter()
                .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
            let mut fill = track;
            fill.set_width(track.width() * m.delivered() as f32 / total);
            if fill.width() > 0.5 {
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(3), theme.tones.green.fg);
            }
            let caption = tf(
                "{done} / {total} delivered",
                &[
                    ("done", &m.delivered().to_string()),
                    ("total", &m.items.len().to_string()),
                ],
            );
            board::paint_text(
                ui,
                egui::pos2(inner.left(), inner.bottom() - 4.0),
                Align2::LEFT_CENTER,
                &caption,
                FontId::proportional(theme.text_size - 2.5),
                theme.palette.text_muted,
                inner.width(),
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                picked = Some(m.title.clone());
            }
        }
    });
    if let Some(title) = picked {
        g.milestones.selected = Some(title);
    }
    board::nav_group(ui, &theme, "", |ui| {
        if board::nav_switch(
            ui,
            &theme,
            g.milestones.show_done,
            tl("Delivered milestones"),
        ) {
            g.milestones.show_done = !g.milestones.show_done;
        }
    });
    let (last, _) = flow_of(&g.app);
    board::nav_note(
        ui,
        &theme,
        &tf("Delivered: on {branch}, the last branch of the flow. The pace is the MRs delivered over the last four weeks.", &[("branch", &last)]),
    );
}

// ── Main area ────────────────────────────────────────────────────────────────

fn main(g: &mut GuiApp, ui: &mut Ui, m: Option<&Milestone>, pace: f64, now: DateTime<Utc>) {
    let theme = g.theme;
    let Some(m) = m else {
        board::heading(
            ui,
            &theme,
            tl("Milestones"),
            tl("No milestone on the tracked MRs"),
            |_| {},
        );
        ui.add_space(14.0);
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("Give the MRs a milestone in GitLab: this page follows each one to production."),
            );
        });
        return;
    };
    let (last, ordered) = flow_of(&g.app);
    let (due, _) = due_text(m.due, now, m.done(), &theme);
    let mut subtitle = vec![tn(m.items.len() as u64, "{n} MR", "{n} MRs")];
    if !m.title.is_empty() {
        subtitle.insert(
            0,
            match m.due {
                Some(d) => format!("{} · {}", d.format("%Y-%m-%d"), due),
                None => due.clone(),
            },
        );
    }
    if let Some(d) = &m.description {
        subtitle.push(d.lines().next().unwrap_or_default().to_string());
    }
    let mut notes = false;
    let mut show = false;
    board::heading(ui, &theme, &m.name(), &subtitle.join(" · "), |ui| {
        show = primary_button(ui, &theme, tl("Show in the table")).clicked();
        notes = button(
            ui,
            &theme,
            Some(super::icons::Icon::Copy),
            tl("Copy the release notes"),
            false,
        )
        .clicked();
    });
    if notes {
        ui.ctx().copy_text(release_notes(m, &last));
        g.app.flash(tl("Release notes copied"), false);
    }
    if show {
        g.next_focus = Some(Some(Focus {
            key: String::new(),
            label: tf("Milestone {name}", &[("name", &m.name())]),
            ids: m.items.iter().map(|i| i.id.clone()).collect(),
        }));
        g.open_page(super::Page::MergeRequests);
    }
    ui.add_space(14.0);

    // Progress.
    let total = m.items.len();
    let delivered = m.delivered();
    board::card(ui, &theme, "", "", 0.0, |ui| {
        let (line, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let share = if total == 0 {
            0.0
        } else {
            delivered as f64 / total as f64
        };
        let w = board::paint_text(
            ui,
            egui::pos2(line.left(), line.center().y),
            Align2::LEFT_CENTER,
            &board::percent(share),
            fonts::semibold(28.0),
            theme.palette.text,
            120.0,
        );
        let caption = tf(
            "delivered · {done} of {total} on {branch}",
            &[
                ("done", &delivered.to_string()),
                ("total", &total.to_string()),
                ("branch", &last),
            ],
        );
        board::paint_text(
            ui,
            egui::pos2(line.left() + w + 12.0, line.center().y + 3.0),
            Align2::LEFT_CENTER,
            &caption,
            FontId::proportional(theme.text_size - 0.5),
            theme.palette.text_muted,
            line.width() - w - 12.0,
        );
        ui.add_space(10.0);
        let parts: Vec<(f32, Color32)> = Stand::ALL
            .iter()
            .map(|s| {
                (
                    m.items.iter().filter(|i| i.stand == *s).count() as f32,
                    s.tone(&theme).fg,
                )
            })
            .collect();
        board::stacked_bar(ui, &theme, &parts, 14.0, 2.0);
        ui.add_space(10.0);
        let legend: Vec<(Color32, String)> = Stand::ALL
            .iter()
            .filter_map(|s| {
                let n = m.items.iter().filter(|i| i.stand == *s).count();
                (n > 0).then(|| (s.tone(&theme).fg, format!("{} · {n}", s.short())))
            })
            .collect();
        board::legend(ui, &theme, &legend);
    });
    ui.add_space(GAP);

    // Will it make its date?
    if !m.title.is_empty() && delivered < total {
        projection_banner(ui, &theme, m, pace, now);
        ui.add_space(GAP);
    }

    // The MRs, by where they stand.
    let mut open = None;
    board::card(
        ui,
        &theme,
        tl("MRs"),
        &tf(
            "branches: {list}",
            &[(
                "list",
                &ordered
                    .iter()
                    .map(|b| crate::branch_flow::short_name(b))
                    .collect::<Vec<_>>()
                    .join(" · "),
            )],
        ),
        0.0,
        |ui| {
            if m.items.is_empty() {
                board::empty(ui, &theme, tl("No tracked MR in this milestone yet."));
                return;
            }
            let mut first = true;
            for stand in Stand::ALL {
                let items: Vec<&Item> = m.items.iter().filter(|i| i.stand == stand).collect();
                if items.is_empty() {
                    continue;
                }
                if !first {
                    ui.add_space(6.0);
                }
                first = false;
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
                ui.painter().circle_filled(
                    egui::pos2(head.left() + 4.0, head.center().y),
                    4.0,
                    stand.tone(&theme).fg,
                );
                let w = board::paint_text(
                    ui,
                    egui::pos2(head.left() + 14.0, head.center().y),
                    Align2::LEFT_CENTER,
                    &stand.label(&last),
                    fonts::semibold(theme.text_size - 1.0),
                    theme.palette.text_secondary,
                    head.width() - 60.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(head.left() + 22.0 + w, head.center().y),
                    Align2::LEFT_CENTER,
                    &items.len().to_string(),
                    FontId::proportional(theme.text_size - 1.5),
                    theme.palette.text_muted,
                    40.0,
                );
                for item in items {
                    if item_row(ui, &theme, item, &ordered) {
                        open = Some(item.id.clone());
                    }
                }
            }
        },
    );
    if let Some(id) = open {
        g.show_mr(&id);
    }
}

fn projection_banner(ui: &mut Ui, theme: &Theme, m: &Milestone, pace: f64, now: DateTime<Utc>) {
    let eta = projection(m, pace, now);
    let left = m.items.len() - m.delivered();
    let held: Vec<&Item> = m
        .items
        .iter()
        .filter(|i| i.stand == Stand::OnAuthor)
        .collect();
    // The sentence says when; the tile beside it says how far from the date.
    let (tone, headline) = match (eta, m.due) {
        (None, _) => (
            theme.tones.gray,
            tl("No MR reached production in the last four weeks: no pace to project from.").to_string(),
        ),
        (Some(eta), due) => (
            match due {
                Some(due) if eta > due => theme.tones.amber,
                Some(_) => theme.tones.green,
                None => theme.tones.blue,
            },
            tf(
                "At the pace of the last four weeks ({pace} a week), everything is on production around {eta}.",
                &[("pace", &format!("{pace:.1}")), ("eta", &eta.format("%Y-%m-%d").to_string())],
            ),
        ),
    };
    let detail = if held.is_empty() {
        tn(
            left as u64,
            "{n} MR left to deliver.",
            "{n} MRs left to deliver.",
        )
    } else {
        let ids = held
            .iter()
            .map(|i| format!("!{}", i.id))
            .collect::<Vec<_>>()
            .join(", ");
        tf(
            "{left} · back on their author, the first to unblock: {ids}",
            &[
                ("left", &tn(left as u64, "{n} MR left", "{n} MRs left")),
                ("ids", &ids),
            ],
        )
    };
    // The banner (two thirds) and the tile (one third) side by side, as tall
    // as the banner's text needs, the tile's figure at least; one above the
    // other on a narrow page.
    let width = ui.available_width();
    let side_by_side = width >= 560.0;
    let banner_w = if side_by_side {
        ((width - GAP) * 2.0 / 3.0).floor()
    } else {
        width
    };
    let text_left = 16.0 + 18.0 + 12.0;
    let wrap = banner_w - text_left - 16.0;
    let layout = |text: &str, font: FontId, color: Color32| {
        ui.painter().layout(text.to_owned(), font, color, wrap)
    };
    let headline = layout(
        &headline,
        fonts::semibold(theme.text_size - 0.5),
        theme.palette.text,
    );
    let detail = layout(
        &detail,
        FontId::proportional(theme.text_size - 1.0),
        theme.palette.text_secondary,
    );
    let text_h = headline.size().y + 6.0 + detail.size().y;
    let tile_min = 104.0;
    let banner_h = (text_h + 28.0).max(if side_by_side { tile_min } else { 0.0 });
    let (row, _) = ui.allocate_exact_size(
        Vec2::new(
            width,
            if side_by_side {
                banner_h
            } else {
                banner_h + GAP + tile_min
            },
        ),
        Sense::hover(),
    );
    let banner = Rect::from_min_size(row.min, Vec2::new(banner_w, banner_h));
    ui.painter().rect(
        banner,
        CornerRadius::same(theme.radii.large),
        tone.bg.gamma_multiply(0.55),
        Stroke::new(1.0, tone.fg.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    // The text block centered in the banner's height.
    let top = banner.top() + (banner_h - text_h) / 2.0;
    let icon = Rect::from_min_size(egui::pos2(banner.left() + 16.0, top), Vec2::splat(18.0));
    super::icons::paint(ui.painter(), icon, super::icons::Icon::Flag, tone.fg);
    let x = banner.left() + text_left;
    let headline_h = headline.size().y;
    ui.painter()
        .galley(egui::pos2(x, top), headline, theme.palette.text);
    ui.painter().galley(
        egui::pos2(x, top + headline_h + 6.0),
        detail,
        theme.palette.text_secondary,
    );
    let tile = if side_by_side {
        Rect::from_min_max(egui::pos2(banner.right() + GAP, row.top()), row.max)
    } else {
        Rect::from_min_max(egui::pos2(row.left(), banner.bottom() + GAP), row.max)
    };
    landing_tile(ui, theme, tile, eta, m.due);
}

/// How far from its due date the milestone lands, large: `34` days late in
/// red, `12` days early in green, `0` on the date; the projected date under
/// it; painted in `rect`.
fn landing_tile(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    eta: Option<NaiveDate>,
    due: Option<NaiveDate>,
) {
    let response = ui.interact(rect, ui.scope_id().with("landing_tile"), Sense::hover());
    let t = &theme.tones;
    let (value, caption, tone) = match (eta, due) {
        (None, _) => (
            board::dash().to_string(),
            tl("no pace yet").to_string(),
            t.gray,
        ),
        (Some(_), None) => (
            board::dash().to_string(),
            tl("no due date").to_string(),
            t.gray,
        ),
        (Some(eta), Some(due)) => {
            let days = (eta - due).num_days();
            match days {
                d if d > 0 => (d.to_string(), tn(d as u64, "day late", "days late"), t.red),
                d if d < 0 => (
                    (-d).to_string(),
                    tn((-d) as u64, "day early", "days early"),
                    t.green,
                ),
                _ => ("0".to_string(), tl("on the due date").to_string(), t.green),
            }
        }
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(theme.radii.large),
        tone.bg.gamma_multiply(0.7),
        Stroke::new(1.0, tone.fg.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink2(Vec2::new(14.0, 0.0));
    // The figure and its caption, as a block centered in the tile.
    let figure = board::one_line(ui, &value, fonts::semibold(36.0), tone.fg, inner.width());
    let under = board::one_line(
        ui,
        &caption,
        fonts::medium(theme.text_size),
        tone.fg,
        inner.width(),
    );
    let landing = eta.map(|e| {
        tf(
            "lands around {date}",
            &[("date", &e.format("%Y-%m-%d").to_string())],
        )
    });
    let small = landing.as_ref().map(|text| {
        board::one_line(
            ui,
            text,
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_secondary,
            inner.width(),
        )
    });
    let block =
        figure.size().y + 2.0 + under.size().y + small.as_ref().map_or(0.0, |g| 6.0 + g.size().y);
    let mut y = rect.center().y - block / 2.0;
    let center = rect.center().x;
    let fh = figure.size();
    ui.painter()
        .galley(egui::pos2(center - fh.x / 2.0, y), figure, tone.fg);
    y += fh.y + 2.0;
    let uh = under.size();
    ui.painter()
        .galley(egui::pos2(center - uh.x / 2.0, y), under, tone.fg);
    y += uh.y + 6.0;
    if let Some(small) = small {
        let sw = small.size().x;
        ui.painter().galley(
            egui::pos2(center - sw / 2.0, y),
            small,
            theme.palette.text_secondary,
        );
    }
    if let (Some(eta), Some(due)) = (eta, due) {
        response.on_hover_text(tf(
            "Due {due}, projected {eta}",
            &[
                ("due", &due.format("%Y-%m-%d").to_string()),
                ("eta", &eta.format("%Y-%m-%d").to_string()),
            ],
        ));
    }
}

/// A line of the MR list: id, title, next step, a mark per branch, author.
fn item_row(ui: &mut Ui, theme: &Theme, item: &Item, ordered: &[String]) -> bool {
    let (rect, response) = board::row(ui, theme, 36.0, false);
    board::row_rule(ui, theme, rect);
    let cy = rect.center().y;
    let left = rect.left() + 8.0;
    let w = board::paint_tag(
        ui,
        theme,
        egui::pos2(left, cy),
        &format!("!{}", item.id),
        board::id_tone(theme),
        true,
    );
    // From the right: the author, the branch marks, the next step.
    let author_w = 130.0;
    let (name, _) = split_person(&item.author);
    let author_rect = Rect::from_min_size(
        egui::pos2(rect.right() - 8.0 - author_w, rect.top()),
        Vec2::new(author_w, rect.height()),
    );
    {
        let mut cell = board::inside(ui, author_rect);
        cell.spacing_mut().item_spacing.x = 6.0;
        avatar(&mut cell, theme, name, 20.0);
        cell.add(
            egui::Label::new(
                egui::RichText::new(board::first_name(&item.author))
                    .size(theme.text_size - 1.5)
                    .color(theme.palette.text_secondary),
            )
            .truncate(),
        );
    }
    let marks_w = ordered.len() as f32 * 22.0;
    let marks_left = author_rect.left() - 12.0 - marks_w;
    for (k, presence) in item.branches.iter().enumerate() {
        let (glyph, label, tone) = super::tones::presence(*presence, false, &theme.tones);
        let cell = Rect::from_min_size(
            egui::pos2(marks_left + k as f32 * 22.0, cy - 9.0),
            Vec2::splat(18.0),
        );
        ui.painter()
            .rect_filled(cell, CornerRadius::same(4), tone.bg);
        board::paint_text(
            ui,
            cell.center(),
            Align2::CENTER_CENTER,
            glyph,
            fonts::semibold(theme.text_size - 3.0),
            tone.fg,
            18.0,
        );
        ui.interact(
            cell,
            ui.scope_id().with(("mark", item.id.as_str(), k)),
            Sense::hover(),
        )
        .on_hover_text(format!(
            "{} · {label}",
            ordered.get(k).map_or("", String::as_str)
        ));
    }
    let step = match item.stand {
        Stand::Delivered => tl("delivered").to_string(),
        Stand::OnTheWay => item
            .step
            .todos
            .first()
            .map(|t| t.short())
            .unwrap_or_else(|| tl("to promote").to_string()),
        _ => item
            .step
            .todos
            .first()
            .map(|t| t.short())
            .unwrap_or_else(|| item.step.who()),
    };
    let tone = if item.stand == Stand::Delivered {
        theme.tones.green
    } else {
        super::next::tone(&item.step, theme)
    };
    let step_w = {
        let galley = ui.painter().layout_no_wrap(
            step.clone(),
            fonts::medium(theme.text_size - 2.5),
            tone.fg,
        );
        (galley.size().x + 12.0).min(170.0)
    };
    let step_left = marks_left - 14.0 - step_w;
    board::paint_tag(ui, theme, egui::pos2(step_left, cy), &step, tone, false);
    board::paint_text(
        ui,
        egui::pos2(left + w + 10.0, cy),
        Align2::LEFT_CENTER,
        &item.title,
        FontId::proportional(theme.text_size - 0.5),
        theme.palette.text,
        (step_left - 12.0 - (left + w + 10.0)).max(0.0),
    );
    response
        .on_hover_text(tl("Show it on the merge requests page"))
        .clicked()
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(g: &mut GuiApp, ui: &mut Ui, m: Option<&Milestone>, pace: f64, now: DateTime<Utc>) {
    let theme = g.theme;
    let Some(m) = m else {
        board::inspector_empty(ui, &theme, tl("Pick a milestone."));
        return;
    };
    board::section(
        ui,
        &theme,
        tl("Burn-up"),
        tl("delivered against the scope"),
        |ui| {
            burn_up(ui, &theme, m, pace, now);
        },
    );
    let tickets: Vec<&(String, Option<u32>, Option<u32>)> =
        m.items.iter().filter_map(|i| i.ticket.as_ref()).collect();
    if !tickets.is_empty() {
        board::section(
            ui,
            &theme,
            tl("Tickets"),
            &tn(tickets.len() as u64, "{n} linked", "{n} linked"),
            |ui| {
                let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
                for t in &tickets {
                    *by_status.entry(t.0.as_str()).or_default() += 1;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    for (status, n) in &by_status {
                        board::chip(
                            ui,
                            &theme,
                            &format!("{n} · {status}"),
                            super::tones::ticket_status(status, &theme.tones),
                        );
                    }
                });
                let spent: u32 = tickets.iter().filter_map(|t| t.1).sum();
                let estimated: u32 = tickets.iter().filter_map(|t| t.2).sum();
                if estimated > 0 || spent > 0 {
                    ui.add_space(10.0);
                    let hours = |s: u32| format!("{:.0} h", f64::from(s) / 3600.0);
                    ui.horizontal(|ui| {
                        ui.label(muted(&theme, tl("Time spent")));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(muted(
                                &theme,
                                tf("/ {h} estimated", &[("h", &hours(estimated))]),
                            ));
                            ui.label(semibold(hours(spent), theme.text_size));
                        });
                    });
                    let fraction = if estimated > 0 {
                        spent as f32 / estimated as f32
                    } else {
                        0.0
                    };
                    let color = if fraction > 1.0 {
                        theme.tones.red.fg
                    } else {
                        theme.palette.accent
                    };
                    super::widgets::bar(ui, &theme, fraction.min(1.0), color, 6.0);
                }
            },
        );
    }
    board::section(ui, &theme, tl("People"), "", |ui| {
        let mut by_author: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for i in &m.items {
            let entry = by_author
                .entry(split_person(&i.author).0.to_string())
                .or_default();
            entry.0 += 1;
            if i.stand == Stand::Delivered {
                entry.1 += 1;
            }
        }
        let mut list: Vec<(String, (usize, usize))> = by_author.into_iter().collect();
        list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        for (name, (n, done)) in list {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                avatar(ui, &theme, &name, 22.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(&name).size(theme.text_size - 0.5))
                        .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(muted(
                        &theme,
                        tf(
                            "{done} / {n}",
                            &[("done", &done.to_string()), ("n", &n.to_string())],
                        ),
                    ));
                });
            });
        }
    });
}

/// The scope (MRs created so far) and the MRs delivered, day by day, from the
/// first MR to the due date (or the projection), today and the due date marked.
fn burn_up(ui: &mut Ui, theme: &Theme, m: &Milestone, pace: f64, now: DateTime<Utc>) {
    let today = now.date_naive();
    // From the first MR, two months back at most, a week at least.
    let start = m
        .items
        .iter()
        .filter_map(|i| i.created.map(|c| c.date_naive()))
        .min()
        .unwrap_or(today - Duration::days(14))
        .min(today - Duration::days(7))
        .max(today - Duration::days(60));
    let eta = projection(m, pace, now);
    let end = [Some(today + Duration::days(3)), m.due, eta]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(today);
    let end = end.min(today + Duration::days(120));
    let span = (end - start).num_days().max(1) as f32;
    let total = m.items.len().max(1);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 150.0), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(26.0, 8.0),
        rect.max - Vec2::new(6.0, 18.0),
    );
    let x_of = |d: NaiveDate| plot.left() + plot.width() * (d - start).num_days() as f32 / span;
    let top = (total as f32 * 1.15).ceil().max(2.0);
    let y_of = |n: f32| plot.bottom() - plot.height() * n / top;
    // Grid and its labels.
    for k in 0..=3 {
        let n = top * k as f32 / 3.0;
        let y = y_of(n);
        ui.painter().hline(
            plot.x_range(),
            y,
            Stroke::new(1.0, theme.palette.border_soft),
        );
        board::paint_text(
            ui,
            egui::pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            &format!("{:.0}", n),
            FontId::proportional(theme.text_size - 3.0),
            theme.palette.text_muted,
            22.0,
        );
    }
    let mut scope = Vec::new();
    let mut done = Vec::new();
    let mut day = start;
    while day <= today.min(end) {
        let created = m
            .items
            .iter()
            .filter(|i| i.created.is_none_or(|c| c.date_naive() <= day))
            .count();
        let delivered = m
            .items
            .iter()
            .filter(|i| i.delivered.is_some_and(|d| d.date_naive() <= day))
            .count();
        scope.push(egui::pos2(x_of(day), y_of(created as f32)));
        done.push(egui::pos2(x_of(day), y_of(delivered as f32)));
        day += Duration::days(1);
    }
    let muted_line = theme.palette.text_secondary;
    if scope.len() >= 2 {
        ui.painter().add(egui::Shape::line(
            scope.clone(),
            Stroke::new(2.0, muted_line),
        ));
        ui.painter().add(egui::Shape::line(
            done.clone(),
            Stroke::new(2.5, theme.tones.green.fg),
        ));
    }
    if let (Some(last_scope), Some(last_done)) = (scope.last().copied(), done.last().copied()) {
        // The scope ahead, dashed; the projection to it.
        dashed(
            ui,
            last_scope,
            egui::pos2(x_of(end), last_scope.y),
            muted_line,
        );
        if let Some(eta) = eta.filter(|e| *e <= end) {
            let target = egui::pos2(x_of(eta), last_scope.y);
            dashed(ui, last_done, target, theme.tones.green.fg);
            ui.painter()
                .circle_stroke(target, 3.5, Stroke::new(1.5, theme.tones.green.fg));
        }
        ui.painter()
            .circle_filled(last_done, 3.5, theme.tones.green.fg);
    }
    // Today and the due date; a label too close to another is left out.
    let today_x = x_of(today);
    let due_x = m.due.filter(|d| *d >= start && *d <= end).map(x_of);
    ui.painter().vline(
        today_x,
        plot.y_range(),
        Stroke::new(1.0, theme.palette.border),
    );
    let small = FontId::proportional(theme.text_size - 3.0);
    let start_w = board::paint_text(
        ui,
        egui::pos2(plot.left(), rect.bottom() - 7.0),
        Align2::LEFT_CENTER,
        &start.format("%d/%m").to_string(),
        small.clone(),
        theme.palette.text_muted,
        40.0,
    );
    let clear_of_due = due_x.is_none_or(|x| (x - today_x).abs() >= 44.0);
    if clear_of_due && today_x - plot.left() >= start_w + 24.0 {
        board::paint_text(
            ui,
            egui::pos2(today_x, rect.bottom() - 7.0),
            Align2::CENTER_CENTER,
            tl("today"),
            small,
            theme.palette.text_muted,
            50.0,
        );
    }
    if let (Some(due), Some(x)) = (m.due, due_x) {
        let mut y = plot.top();
        while y < plot.bottom() {
            ui.painter().vline(
                x,
                y..=(y + 4.0).min(plot.bottom()),
                Stroke::new(1.5, theme.tones.amber.fg),
            );
            y += 7.0;
        }
        let align = if x > plot.right() - 30.0 {
            Align2::RIGHT_CENTER
        } else {
            Align2::CENTER_CENTER
        };
        board::paint_text(
            ui,
            egui::pos2(x, rect.bottom() - 7.0),
            align,
            &due.format("%d/%m").to_string(),
            fonts::medium(theme.text_size - 3.0),
            theme.tones.amber.fg,
            50.0,
        );
    }
    ui.add_space(6.0);
    board::legend(
        ui,
        theme,
        &[
            (muted_line, tl("Scope").to_string()),
            (theme.tones.green.fg, tl("Delivered").to_string()),
            (theme.tones.amber.fg, tl("Due date").to_string()),
        ],
    );
}

fn dashed(ui: &Ui, from: egui::Pos2, to: egui::Pos2, color: Color32) {
    let length = from.distance(to);
    if length < 1.0 {
        return;
    }
    let dir = (to - from) / length;
    let mut t = 0.0;
    while t < length {
        let a = from + dir * t;
        let b = from + dir * (t + 4.0).min(length);
        ui.painter().line_segment([a, b], Stroke::new(1.5, color));
        t += 8.0;
    }
}

/// The milestone's MRs as Markdown release notes, delivered first.
fn release_notes(m: &Milestone, last: &str) -> String {
    let mut out = format!("## {}\n\n", m.name());
    for stand in Stand::ALL {
        let items: Vec<&Item> = m.items.iter().filter(|i| i.stand == stand).collect();
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("### {}\n\n", stand.label(last)));
        for i in items {
            out.push_str(&format!(
                "- !{} {} ({})\n",
                i.id,
                i.title,
                split_person(&i.author).0
            ));
        }
        out.push('\n');
    }
    out
}

/// The rail's badge: the MRs left in the next milestone with a due date, in the
/// tone of its outlook: red when past due or projected late, orange when due
/// within 3 days with MRs back on their author or in draft, amber when the
/// projection lands 2 days or less before the date (or cannot be made), green
/// when on schedule.
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let all = milestones(&g.app);
    // Sorted by due date: the first undone with a date is the next.
    let m = all
        .iter()
        .find(|m| !m.title.is_empty() && !m.done() && m.due.is_some())?;
    let due = m.due?;
    let left = m.items.len() - m.delivered();
    let today = now.date_naive();
    let days = (due - today).num_days();
    let slip = projection(m, pace(&g.app, now), now).map(|p| (p - due).num_days());
    let stuck = m
        .items
        .iter()
        .filter(|i| matches!(i.stand, Stand::OnAuthor | Stand::Draft))
        .count();
    let tones = &g.theme.tones;
    let tone = if days < 0 || slip.is_some_and(|s| s > 0) {
        tones.red
    } else if days <= 3 && stuck > 0 {
        tones.orange
    } else if slip.is_none_or(|s| s >= -2) {
        tones.amber
    } else {
        tones.green
    };
    let (due_text, _) = due_text(m.due, now, false, &g.theme);
    let mut hint = format!(
        "{} · {} · {}",
        m.title,
        tn(left as u64, "{n} MR left", "{n} MRs left"),
        due_text
    );
    hint.push('\n');
    hint.push_str(&match slip {
        None => tl("No delivery in the last 4 weeks: no projection").to_string(),
        Some(s) if s > 0 => tn(
            s as u64,
            "Projected {n} day late",
            "Projected {n} days late",
        ),
        Some(0) => tl("Projected on its due date").to_string(),
        Some(s) => tn(
            (-s) as u64,
            "Projected {n} day early",
            "Projected {n} days early",
        ),
    });
    if stuck > 0 {
        hint.push('\n');
        hint.push_str(&tn(
            stuck as u64,
            "{n} MR back on its author or in draft",
            "{n} MRs back on their author or in draft",
        ));
    }
    Some(Badge {
        n: left,
        tone,
        hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_demo_milestones_have_their_mrs() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let list = milestones(&app);
        let v240 = list.iter().find(|m| m.title == "v2.4.0").expect("v2.4.0");
        assert!(!v240.items.is_empty());
        // The MR on every branch is delivered.
        let delivered: Vec<&str> = v240
            .items
            .iter()
            .filter(|i| i.stand == Stand::Delivered)
            .map(|i| i.id.as_str())
            .collect();
        assert!(delivered.contains(&"104"), "{delivered:?}");
        // The closed MR is left out.
        assert!(list.iter().all(|m| m.items.iter().all(|i| i.id != "105")));
    }

    #[test]
    fn projection_follows_the_pace() {
        let now = board::parse_time("2026-11-05T12:00:00Z").unwrap();
        let m = Milestone {
            title: "v1".into(),
            due: None,
            description: None,
            items: Vec::new(),
        };
        // Nothing left: done today.
        assert_eq!(projection(&m, 0.0, now), Some(now.date_naive()));
    }
}
