//! The milestones page: for the milestone picked, its MRs by where they stand
//! — on production, on the way there, to merge, in review, back on their
//! author, drafts —, whether it makes its due date (a Monte Carlo forecast
//! drawing from the deliveries of the last eight weeks), its burn-up, and its
//! tickets and people.

use std::collections::BTreeMap;

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, GAP};
use super::rail_badges::Badge;
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{avatar, button, muted, primary_button, semibold, split_person};
use super::GuiApp;
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, GitlabMrState, TrackedMr};
use crate::next_step::{self, Actor, NextStep};
use gitlab_tracker_core::MergeabilityStatus;

mod banner;
mod forecast;
mod items;
mod model;
mod side_panel;

use banner::*;
pub use forecast::*;
use items::*;
pub use model::*;
pub use side_panel::*;

/// The milestones page's choices.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The milestone shown, by title; `Some("")` for the MRs without one.
    pub selected: Option<String>,
    pub show_done: bool,
}

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_MILESTONE=v2.4.0`
    // shows that milestone, for a screenshot.
    #[cfg(debug_assertions)]
    if g.milestones.selected.is_none() {
        if let Ok(title) = std::env::var("GITLAB_TRACKER_GUI_MILESTONE") {
            g.milestones.selected = Some(title);
        }
    }
    let all = g
        .memo
        .milestones
        .get(&g.app.project_id, (), || milestones(&g.app));
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let weekly = weekly_merges(&g.app, overview.as_ref(), now);
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
        |g, ui| inspector(g, ui, current.as_ref(), &weekly, now),
        |g, ui| main(g, ui, current.as_ref(), &weekly, now),
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
            // A milestone's name is words ("Security audit") as often as a
            // version: one font for every name, "No milestone" included.
            let font = FontId::proportional(theme.text_size - 0.5);
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
        &tf("Delivered: on {branch}, the last branch of the flow. The forecast replays the MRs merged each of the last eight weeks (from the first merge the tracker knows), drawn at random, until every MR is merged; the release comes after.", &[("branch", &last)]),
    );
}

// ── Main area ────────────────────────────────────────────────────────────────

fn main(g: &mut GuiApp, ui: &mut Ui, m: Option<&Milestone>, weekly: &[u32], now: DateTime<Utc>) {
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
                Some(d) => format!("{} · {}", crate::utils::day_text(d), due),
                None => due.clone(),
            },
        );
    }
    // A milestone named like a version: its release, tagged or to come.
    let overview = g.details.project(&g.app, ui.ctx()).cloned();
    let release = version_key(&m.title).map(|key| {
        overview.as_ref().and_then(|o| {
            o.releases
                .iter()
                .find(|r| version_key(&r.tag).as_deref() == Some(key.as_str()))
                .cloned()
        })
    });
    // The version is the title: its tag's state comes right after the date.
    let tag_state = match &release {
        Some(Some(r)) => Some(tf(
            "tagged {when}",
            &[(
                "when",
                &board::time_of(r.released_at.as_deref())
                    .or_else(|| board::parse_time(&r.committed_at))
                    .map(|at| crate::utils::day_text(at.date_naive()))
                    .unwrap_or_default(),
            )],
        )),
        Some(None) => Some(tl("not tagged yet").to_string()),
        None => None,
    };
    if let Some(state) = tag_state {
        subtitle.insert(subtitle.len() - 1, state);
    }
    if let Some(d) = &m.description {
        subtitle.push(d.lines().next().unwrap_or_default().to_string());
    }
    let mut to_releases = false;
    let mut notes = false;
    let mut show = false;
    let mut changelog = false;
    // Offered when the project's repository holds a git-cliff config.
    let cliff = super::changelog::config(g, ui.ctx());
    board::heading(ui, &theme, &m.name(), &subtitle.join(" · "), |ui| {
        show = primary_button(ui, &theme, tl("Show in the table")).clicked();
        if release.is_some() {
            to_releases = button(ui, &theme, None, tl("See in Releases"), false).clicked();
        }
        notes = button(
            ui,
            &theme,
            Some(super::icons::Icon::Copy),
            tl("Copy the release notes"),
            false,
        )
        .clicked();
        if cliff.is_some() {
            changelog = button(
                ui,
                &theme,
                Some(super::icons::Icon::List),
                tl("Generate changelog"),
                false,
            )
            .tip(tl(
                "The changelog since the last release tag, with the project's git-cliff config",
            ))
            .clicked();
        }
    });
    if to_releases {
        let tag = release.flatten().map(|r| r.tag);
        super::releases::open_on(g, tag);
    }
    if let (true, Some(config)) = (changelog, cliff) {
        super::changelog::open(g, ui.ctx(), config, &m.title);
    }
    if notes {
        ui.ctx()
            .copy_text(board::Links::of(&g.app).apply(&release_notes(m, &last)));
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
        projection_banner(ui, &theme, m, weekly, &last, now);
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
        g.open_mr(&id);
    }
}

#[cfg(test)]
mod tests;
