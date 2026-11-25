//! The Today page: what waits for me across every open tab — the reviews asked of
//! me, my MRs that need a fix, those ready to merge, those still waiting, and
//! what I merged today, whoever wrote it. "Me" is the
//! project's `gitlab_username`, read from GitLab when the tab opens. A click
//! shows the MR in its project's tab.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, Vec2};

use super::tip::{Tip, TipExt};
use super::widgets::{avatar, card, mono, muted, page_title, split_person};
use super::{tones, GuiApp, SlotState};
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use crate::snooze::{Snooze, Until};
use crate::utils::{awaiting_review_by, matches_gitlab_username};
use gitlab_tracker_core::MergeabilityStatus;

mod feed;
mod model;
mod rows;
mod snoozing;

use feed::*;
pub use model::*;
pub(in crate::gui) use rows::*;
use snoozing::*;

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let read = collect(g);
    let (items, users) = (&read.0, &read.1);
    let projects = g
        .tabs
        .iter()
        .filter(|t| matches!(t.state, SlotState::Active | SlotState::Parked(_)))
        .count();
    let subtitle = if users.is_empty() {
        tl("Your GitLab username is not known yet: it is read from GitLab when a project opens.")
            .to_string()
    } else {
        let who = users
            .iter()
            .map(|u| format!("@{u}"))
            .collect::<Vec<_>>()
            .join(", ");
        tf(
            "Across {n} open projects · as {user}",
            &[("n", &projects.to_string()), ("user", &who)],
        )
    };
    ui.horizontal(|ui| {
        page_title(ui, &theme, tl("Today"), subtitle);
        if !users.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                super::standup::button(g, ui, items);
            });
        }
    });
    ui.add_space(14.0);

    // A deploy freeze of the open project, holding or about to.
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let freezes = super::freeze::of(&g.app, overview.as_ref());
    let name = g
        .app
        .project_name
        .clone()
        .unwrap_or_else(|| g.app.project_id.clone());
    if let Some(r) = super::freeze::banner(ui, &theme, &freezes, chrono::Utc::now(), Some(&name)) {
        if r.clicked() {
            g.open_page(super::Page::Environments);
        }
        ui.add_space(14.0);
    }

    // The counts, one tile per kind that matters for the day.
    let mut show_merged = None;
    let shown = g.active();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for kind in [Kind::Review, Kind::Fix, Kind::Ready, Kind::Merged] {
            let n = items
                .iter()
                .filter(|i| i.kind == kind && i.snoozed.is_none())
                .count();
            if tile(ui, &theme, kind, n) {
                // The project of the first MR merged today, else the one shown.
                let tab = items
                    .iter()
                    .find(|i| i.kind == kind)
                    .map_or(shown, |i| i.tab);
                show_merged = Some(tab);
            }
        }
    });
    ui.add_space(12.0);

    let several = projects > 1;
    let mut pick = None;
    let mut copied = false;
    // (tab, MR, snooze until; `None`: wake it now).
    let mut snooze: Option<(usize, String, Option<Until>)> = None;
    let mut seen = false;
    let changes = feed(g);
    let wide = ui.available_width() >= 1100.0;
    type Pick = Option<(usize, String, Option<super::mr_page::Tab>)>;
    type Snoozing = Option<(usize, String, Option<Until>)>;
    let lists = |ui: &mut Ui, pick: &mut Pick, copied: &mut bool, snooze: &mut Snoozing| {
        let mut empty = Vec::new();
        for kind in Kind::ALL {
            let list: Vec<&Item> = items
                .iter()
                .filter(|i| i.kind == kind && i.snoozed.is_none())
                .collect();
            // The quiet sections only when they have something.
            if list.is_empty() && matches!(kind, Kind::Assigned | Kind::Waiting) {
                continue;
            }
            // The empty ones in a line at the end: they take no room.
            if list.is_empty() {
                empty.push(kind);
                continue;
            }
            let title = format!("{} · {}", kind.title(), list.len());
            card(ui, &theme, &title, |ui| {
                if kind == Kind::Waiting {
                    ui.label(muted(
                        &theme,
                        tl("Your MRs blocked by someone else: a reminder can unblock them."),
                    ));
                    ui.add_space(4.0);
                }
                ui.spacing_mut().item_spacing.y = 2.0;
                for item in list {
                    match row(ui, &theme, item, several) {
                        Some(RowClick::Open) => *pick = Some((item.tab, item.id.clone(), None)),
                        Some(RowClick::Page(tab)) => {
                            *pick = Some((item.tab, item.id.clone(), Some(tab)))
                        }
                        Some(RowClick::Copied) => *copied = true,
                        Some(RowClick::Snooze(until)) => {
                            *snooze = Some((item.tab, item.id.clone(), Some(until)))
                        }
                        Some(RowClick::Wake) => *snooze = Some((item.tab, item.id.clone(), None)),
                        None => {}
                    }
                }
            });
            ui.add_space(10.0);
        }
        let snoozed: Vec<&Item> = items.iter().filter(|i| i.snoozed.is_some()).collect();
        if !snoozed.is_empty() {
            match snoozed_card(ui, &theme, &snoozed, several) {
                Some((item, RowClick::Wake)) => {
                    *snooze = Some((item.tab, item.id.clone(), None));
                }
                Some((item, _)) => *pick = Some((item.tab, item.id.clone(), None)),
                None => {}
            }
            ui.add_space(10.0);
        }
        if !empty.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for kind in empty {
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0, theme.palette.border))
                        .corner_radius(CornerRadius::same(theme.radii.medium))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(muted(
                                &theme,
                                tf("{section} · nothing yet", &[("section", kind.title())]),
                            ))
                            .tip(kind.empty());
                        });
                }
            });
        }
    };
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if wide {
                ui.horizontal_top(|ui| {
                    let side = 360.0;
                    let left = ui.available_width() - side - 16.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(left, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(left);
                            lists(ui, &mut pick, &mut copied, &mut snooze);
                        },
                    );
                    ui.add_space(16.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(side, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(side);
                            match feed_card(ui, &theme, &changes, several) {
                                Some(FeedClick::Open(tab, id)) => pick = Some((tab, id, None)),
                                Some(FeedClick::Seen) => seen = true,
                                None => {}
                            }
                        },
                    );
                });
            } else {
                lists(ui, &mut pick, &mut copied, &mut snooze);
                ui.add_space(10.0);
                match feed_card(ui, &theme, &changes, several) {
                    Some(FeedClick::Open(tab, id)) => pick = Some((tab, id, None)),
                    Some(FeedClick::Seen) => seen = true,
                    None => {}
                }
            }
        });
    if copied {
        g.app.flash(
            tl("Reminder copied: paste it in a chat or a comment"),
            false,
        );
    }
    if let Some((tab, id, until)) = snooze {
        let now = chrono::Local::now();
        let mut text = None;
        g.edit_tab(tab, |app| {
            let Some(mr) = app.mrs.iter_mut().find(|m| m.id == id) else {
                return;
            };
            mr.snooze = until.map(|until| Snooze::new(mr, until.resolve(&now)));
            text = Some(match &mr.snooze {
                Some(s) => tf(
                    "!{id} snoozed {until}",
                    &[("id", &id), ("until", &s.text(&now))],
                ),
                None => tf("!{id} is back in Today", &[("id", &id)]),
            });
        });
        if let Some(text) = text {
            g.app.flash(text, false);
        }
    }
    if seen {
        g.feed_seen_at = Some(chrono::Local::now());
        g.app.last_changes.clear();
        for slot in &mut g.tabs {
            if let SlotState::Parked(tab) = &mut slot.state {
                tab.app.last_changes.clear();
            }
        }
    }
    if let Some(tab) = show_merged {
        merged_by_me(g, tab);
    }
    match pick {
        Some((tab, id, None)) => {
            g.activate(tab);
            g.open_mr(&id);
        }
        Some((tab, id, Some(page_tab))) => {
            g.activate(tab);
            g.open_mr(&id);
            g.mr_tab = page_tab;
        }
        None => {}
    }
}

#[cfg(test)]
mod tests;
