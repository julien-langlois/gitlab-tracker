//! The Promote page: between two tracked branches (staging → production, say),
//! what is ready to go — on the first, not yet on the second —, what is not on
//! the first yet and why, and what already went. The ready MRs are listed in
//! merge order with how long they have been on the first branch and their
//! ticket; those ticked copy as a release checklist, show in the table, or
//! give the `git cherry-pick` commands that bring them to the second branch.

use bevel_theme::{fonts, Chip, Theme};
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, Vec2};

use super::tip::{Tip, TipExt};
use super::widgets::{button, mono, muted, page_title, semibold, split_person};
use super::{tones, GuiApp};
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, GitlabMrState, PipelineState};
use gitlab_tracker_core::MergeabilityStatus;

mod model;
mod ready_list;
mod side_cards;

pub use model::*;
use ready_list::*;
pub use side_cards::*;

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let Some((from, to)) = pair(g) else {
        page_title(ui, &theme, tl("Promote"), "");
        ui.add_space(12.0);
        ui.label(muted(
            &theme,
            tl("Track at least two branches (in the settings) to see what is ready to go from one to the next."),
        ));
        return;
    };
    let all = kept_rows(g, &from, &to);
    let ready: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Ready).collect();
    let upstream: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Upstream).collect();
    let merged_upstream: Vec<&Row> = upstream
        .iter()
        .copied()
        .filter(|r| r.state == GitlabMrState::Merged)
        .collect();
    let open_upstream: Vec<&Row> = upstream
        .iter()
        .copied()
        .filter(|r| r.state != GitlabMrState::Merged)
        .collect();
    let mut done: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Done).collect();
    // The latest arrivals first: what just went out.
    done.sort_by_key(|r| std::cmp::Reverse(r.on_to_since));
    // The ready MRs ticked for the release (all unless unticked).
    let picked: Vec<&Row> = ready
        .iter()
        .copied()
        .filter(|r| !g.promote_skipped.contains(&r.id))
        .collect();

    let mut picked_pair = None;
    let mut action = None;
    ui.horizontal(|ui| {
        page_title(
            ui,
            &theme,
            tl("Promote"),
            tf(
                "{n} ready to go from {from} to {to}",
                &[
                    ("n", &ready.len().to_string()),
                    ("from", &from),
                    ("to", &to),
                ],
            ),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // The two branches, picked among the tracked ones.
            let pick = |ui: &mut Ui, id: &str, current: &str| -> Option<String> {
                let mut chosen = None;
                egui::ComboBox::from_id_salt(id)
                    .selected_text(RichText::new(current).font(fonts::medium(theme.text_size)))
                    .show_ui(ui, |ui| {
                        for branch in &g.app.branches {
                            if ui.selectable_label(branch == current, branch).clicked() {
                                chosen = Some(branch.clone());
                            }
                        }
                    });
                chosen
            };
            if let Some(b) = pick(ui, "promote_to", &to) {
                picked_pair = Some((from.clone(), b));
            }
            ui.label(muted(&theme, "→"));
            if let Some(b) = pick(ui, "promote_from", &from) {
                picked_pair = Some((b, to.clone()));
            }
        });
    });
    ui.add_space(14.0);

    let mut show = None;
    let mut toggled = None;
    let mut copied = None;
    let now = chrono::Utc::now();
    // A deploy freeze, holding or about to: promoting still works, the
    // deploy waits.
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let freezes = super::freeze::of(&g.app, overview.as_ref());
    if let Some(r) = super::freeze::banner(ui, &theme, &freezes, now, None) {
        if r.clicked() {
            g.open_page(super::Page::Environments);
        }
        ui.add_space(14.0);
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // The ready MRs, as a table.
            theme.card_frame().inner_margin(egui::Margin::ZERO).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(semibold(
                                tf("Ready to go · {n}", &[("n", &ready.len().to_string())]),
                                theme.text_size + 1.0,
                            ));
                            ui.label(muted(
                                &theme,
                                tf(
                                    "on {from}, not on {to} · in merge order",
                                    &[("from", &from), ("to", &to)],
                                ),
                            ));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                if !picked.is_empty()
                                    && link(ui, &theme, tl("Show in the table")).clicked()
                                {
                                    action = Some(Action::Show);
                                }
                                if !ready.is_empty() {
                                    ui.label(muted(
                                        &theme,
                                        tf(
                                            "{n} selected",
                                            &[("n", &picked.len().to_string())],
                                        ),
                                    ));
                                }
                            });
                        });
                    });
                if ready.is_empty() {
                    egui::Frame::new()
                        .inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 14 })
                        .show(ui, |ui| {
                            ui.label(muted(
                                &theme,
                                tf(
                                    "Nothing on {from} is missing from {to}.",
                                    &[("from", &from), ("to", &to)],
                                ),
                            ));
                        });
                    return;
                }
                ready_header(ui, &theme, &from);
                for row_data in &ready {
                    let ticked = !g.promote_skipped.contains(&row_data.id);
                    match ready_row(ui, &theme, row_data, ticked, now) {
                        Some(ReadyClick::Toggle) => toggled = Some(row_data.id.clone()),
                        Some(ReadyClick::Open) => show = Some(row_data.id.clone()),
                        None => {}
                    }
                }
                ui.add_space(4.0);
            });
            ui.add_space(16.0);

            // The commands on the left, what is not ready on the right.
            let wide = ui.available_width() >= 900.0;
            // The side cards take a good share: they list many MRs.
            let side = (ui.available_width() * 0.4).clamp(380.0, 560.0);
            let commands_card = |ui: &mut Ui, copied: &mut Option<Copied>, action: &mut Option<Action>| {
                let commands = commands(&picked, &from, &to);
                theme.card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(semibold(
                            tf(
                                &tn(
                                    picked.len() as u64,
                                    "To ship the {n} selected MR",
                                    "To ship the {n} selected MRs",
                                ),
                                &[],
                            ),
                            theme.text_size + 1.0,
                        ));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.add_enabled_ui(!picked.is_empty(), |ui| {
                                if button(ui, &theme, None, tl("Copy commands"), true).clicked() {
                                    ui.ctx().copy_text(commands.clone());
                                    *copied = Some(Copied::Commands);
                                }
                                if button(ui, &theme, None, tl("Release notes"), false)
                                    .tip(tl("The selected MRs as a Markdown list"))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(
                                        super::board::Links::of(&g.app)
                                            .apply(&release_notes(&picked, &to)),
                                    );
                                    *copied = Some(Copied::Notes);
                                }
                                if button(ui, &theme, None, tl("Markdown checklist"), false)
                                    .tip(tl(
                                        "The selected MRs as a Markdown table, for the release ticket",
                                    ))
                                    .clicked()
                                {
                                    *action = Some(Action::Copy);
                                }
                            });
                        });
                    });
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(theme.palette.content)
                        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
                        .corner_radius(CornerRadius::same(theme.radii.medium))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 3.0;
                            if picked.is_empty() {
                                ui.label(muted(&theme, tl("Tick the MRs to ship above.")));
                            }
                            for line in commands.lines().filter(|_| !picked.is_empty()) {
                                let color = if line.starts_with('#') {
                                    theme.palette.text_muted
                                } else {
                                    theme.palette.text
                                };
                                ui.label(mono(line, theme.text_size - 1.5).color(color));
                            }
                        });
                    ui.add_space(8.0);
                    ui.label(muted(
                        &theme,
                        tl("Commits listed oldest merge first, so they apply in the order the branch got them."),
                    ));
                });
            };
            let side_cards = |ui: &mut Ui, show: &mut Option<String>, action: &mut Option<Action>| {
                ui.spacing_mut().item_spacing.y = 16.0;
                theme.card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(semibold(
                        tf(
                            "Merged, not on {from} yet · {n}",
                            &[("from", &from), ("n", &merged_upstream.len().to_string())],
                        ),
                        theme.text_size,
                    ));
                    if merged_upstream.is_empty() {
                        ui.label(muted(&theme, tl("Every merged MR reached it.")));
                    }
                    for r in &merged_upstream {
                        if side_row(ui, &theme, r, None, r.merged_at.as_deref().map(|at| {
                            tf("merged {when}", &[("when", &crate::utils::format_relative_date(at))])
                        })) {
                            *show = Some(r.id.clone());
                        }
                    }
                    if !merged_upstream.is_empty() {
                        ui.label(muted(
                            &theme,
                            tf("Promote them to {from} first.", &[("from", &from)]),
                        ));
                    }
                });
                theme.card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(semibold(
                        tf("Still open · {n}", &[("n", &open_upstream.len().to_string())]),
                        theme.text_size,
                    ));
                    if open_upstream.is_empty() {
                        ui.label(muted(&theme, tl("Every tracked MR reached it.")));
                    }
                    let groups = open_groups(&open_upstream, &theme);
                    if !groups.is_empty() {
                        share_bar(ui, &groups);
                        ui.add_space(2.0);
                    }
                    // A line per group: what holds them on the left, their ids
                    // on the right, wrapped.
                    const LABEL: f32 = 136.0;
                    for (label, tone, rows) in &groups {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(LABEL, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_min_size(Vec2::new(LABEL, 22.0));
                                    let (dot, _) =
                                        ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                                    ui.painter().circle_filled(dot.center(), 3.5, tone.fg);
                                    ui.label(
                                        RichText::new(sentence_case(label))
                                            .font(fonts::medium(theme.text_size - 1.0))
                                            .color(theme.palette.text),
                                    );
                                    ui.label(
                                        muted(&theme, rows.len().to_string())
                                            .size(theme.text_size - 1.5),
                                    );
                                },
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
                                for r in rows {
                                    if id_pill(ui, &theme, r, *tone).clicked() {
                                        *show = Some(r.id.clone());
                                    }
                                }
                            });
                        });
                    }
                    ui.add_space(2.0);
                    if !open_upstream.is_empty()
                        && link(ui, &theme, tl("Show in the table")).clicked()
                    {
                        *action = Some(Action::ShowOpen);
                    }
                });
                theme.card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(semibold(
                        tf(
                            "Already on {to} · {n}",
                            &[("to", &to), ("n", &done.len().to_string())],
                        ),
                        theme.text_size,
                    ));
                    if done.is_empty() {
                        ui.label(muted(&theme, tf("Nothing reached {to} yet.", &[("to", &to)])));
                        return;
                    }
                    // The latest few, the rest on demand.
                    const FIRST: usize = 5;
                    let all_id = ui.make_persistent_id("promote_done_all");
                    let all = ui.data(|d| d.get_temp::<bool>(all_id)).unwrap_or(false);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let shown = if all { done.len() } else { done.len().min(FIRST) };
                    for r in &done[..shown] {
                        // How it reached it: merged, picked, copied; and when.
                        let (_, how, tone) = tones::presence(r.on_to, false, &theme.tones);
                        let when = r
                            .on_to_since
                            .map(|t| crate::utils::format_relative_date(&t.to_rfc3339()));
                        if side_row(ui, &theme, r, Some((how, tone)), when) {
                            *show = Some(r.id.clone());
                        }
                    }
                    if done.len() > FIRST {
                        ui.add_space(6.0);
                        let text = if all {
                            tl("Show fewer").to_string()
                        } else {
                            tn(
                                (done.len() - FIRST) as u64,
                                "Show {n} more",
                                "Show {n} more",
                            )
                        };
                        if link(ui, &theme, &text).clicked() {
                            ui.data_mut(|d| d.insert_temp(all_id, !all));
                        }
                    }
                });
            };
            if wide {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    let left = ui.available_width() - side - 16.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(left, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(left);
                            commands_card(ui, &mut copied, &mut action);
                        },
                    );
                    ui.allocate_ui_with_layout(
                        Vec2::new(side, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(side);
                            side_cards(ui, &mut show, &mut action);
                        },
                    );
                });
            } else {
                commands_card(ui, &mut copied, &mut action);
                ui.add_space(16.0);
                side_cards(ui, &mut show, &mut action);
            }
            ui.add_space(16.0);
        });

    let ready_ids: std::collections::HashSet<String> =
        picked.iter().map(|r| r.id.clone()).collect();
    if let Some(id) = toggled {
        if !g.promote_skipped.remove(&id) {
            g.promote_skipped.insert(id);
        }
    }
    match copied {
        Some(Copied::Commands) => g.app.flash(tl("Commands copied"), false),
        Some(Copied::Notes) => g.app.flash(tl("Release notes copied"), false),
        None => {}
    }
    if let Some(pair) = picked_pair {
        g.promote = Some(pair);
    }
    match action {
        Some(Action::Copy) => {
            g.marked = ready_ids;
            g.bulk = Some(super::table::Bulk::CopyChecklist);
        }
        Some(Action::Show) => {
            g.next_focus = Some(Some(Focus {
                key: String::new(),
                label: tf("Ready for {to}", &[("to", &to)]),
                ids: ready_ids,
            }));
            g.open_page(super::Page::MergeRequests);
        }
        Some(Action::ShowOpen) => {
            g.next_focus = Some(Some(Focus {
                key: String::new(),
                label: tf("Not on {from} yet", &[("from", &from)]),
                ids: open_upstream.iter().map(|r| r.id.clone()).collect(),
            }));
            g.open_page(super::Page::MergeRequests);
        }
        None => {}
    }
    if let Some(id) = show {
        g.open_mr(&id);
    }
}

#[cfg(test)]
mod tests;
