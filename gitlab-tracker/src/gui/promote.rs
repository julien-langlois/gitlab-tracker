//! The Promote page: between two tracked branches (staging → production, say),
//! what is ready to go — on the first, not yet on the second —, what is not on
//! the first yet and why, and what already went. The ready list copies as a
//! release checklist or shows in the table.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, Vec2};

use super::widgets::{avatar, badge, button, card, mono, muted, page_title, split_person};
use super::{tones, GuiApp};
use crate::app::{App, Focus};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, GitlabMrState, PipelineState};
use gitlab_tracker_core::MergeabilityStatus;

/// Where an MR stands between the two branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// On `from`, not (or partly) on `to`.
    Ready,
    /// Not on `from` yet.
    Upstream,
    /// Already on `to`.
    Done,
}

/// An MR on the page.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub author: String,
    pub stage: Stage,
    /// How it is on each branch (`None`: not merged yet, or loading).
    pub on_from: Option<BranchPresence>,
    pub on_to: Option<BranchPresence>,
    /// What holds it back, for the MRs not on `from` yet.
    pub blockers: Vec<String>,
    pub state: GitlabMrState,
    pub mergeability: MergeabilityStatus,
}

/// The MRs between `from` and `to`; closed MRs and those still loading are left out.
pub fn rows(app: &App, from: &str, to: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for mr in &app.mrs {
        if mr.title.is_empty() || mr.state == GitlabMrState::Closed {
            continue;
        }
        let on_from = app.branch_presence(mr, from);
        let on_to = app.branch_presence(mr, to);
        let arrived = |p: Option<BranchPresence>| p.is_some_and(|p| !p.is_missing());
        let stage = if arrived(on_to) {
            Stage::Done
        } else if arrived(on_from) {
            Stage::Ready
        } else {
            Stage::Upstream
        };
        let mut blockers = Vec::new();
        if stage == Stage::Upstream {
            if mr.state == GitlabMrState::Opened {
                if mr
                    .pipelines
                    .first()
                    .is_some_and(|p| p.status == PipelineState::Failed)
                {
                    blockers.push(tl("CI failed").to_string());
                }
                if mr.user_notes_count > 0 {
                    blockers.push(tn(
                        u64::from(mr.user_notes_count),
                        "{n} open thread",
                        "{n} open threads",
                    ));
                }
                if mr.approved_by.is_empty() && mr.mergeability != MergeabilityStatus::Draft {
                    blockers.push(tl("Not approved").to_string());
                }
            } else if on_from == Some(BranchPresence::Partial) {
                blockers.push(tl("Partly copied").to_string());
            }
        }
        rows.push(Row {
            id: mr.id.clone(),
            title: mr.title.clone(),
            author: mr.author.clone(),
            stage,
            on_from,
            on_to,
            blockers,
            state: mr.state,
            mergeability: mr.mergeability,
        });
    }
    rows
}

/// The pair of branches shown: the one picked when still tracked, else the
/// last two tracked branches (the last hop of the release train).
fn pair(g: &GuiApp) -> Option<(String, String)> {
    let branches = &g.app.branches;
    if branches.len() < 2 {
        return None;
    }
    if let Some((from, to)) = &g.promote {
        if from != to && branches.contains(from) && branches.contains(to) {
            return Some((from.clone(), to.clone()));
        }
    }
    let n = branches.len();
    Some((branches[n - 2].clone(), branches[n - 1].clone()))
}

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
    let all = rows(&g.app, &from, &to);
    let ready: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Ready).collect();
    let upstream: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Upstream).collect();
    let done: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Done).collect();

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
            let enabled = !ready.is_empty();
            ui.add_enabled_ui(enabled, |ui| {
                if button(
                    ui,
                    &theme,
                    Some(super::icons::Icon::Clipboard),
                    tl("Copy the checklist"),
                    true,
                )
                .on_hover_text(tl(
                    "The ready MRs as a Markdown table, for the release ticket",
                ))
                .clicked()
                {
                    action = Some(Action::Copy);
                }
                if button(ui, &theme, None, tl("Show in the table"), false).clicked() {
                    action = Some(Action::Show);
                }
            });
            ui.add_space(12.0);
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
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let ready_title = tf("Ready to go · {n}", &[("n", &ready.len().to_string())]);
            card(ui, &theme, &ready_title, |ui| {
                if ready.is_empty() {
                    ui.label(muted(
                        &theme,
                        tf(
                            "Nothing on {from} is missing from {to}.",
                            &[("from", &from), ("to", &to)],
                        ),
                    ));
                }
                for row_data in &ready {
                    if row(ui, &theme, row_data, &from, &to, theme.tones.green) {
                        show = Some(row_data.id.clone());
                    }
                }
            });
            ui.add_space(10.0);
            let upstream_title = tf(
                "Not on {from} yet · {n}",
                &[("from", &from), ("n", &upstream.len().to_string())],
            );
            card(ui, &theme, &upstream_title, |ui| {
                if upstream.is_empty() {
                    ui.label(muted(&theme, tl("Every tracked MR reached it.")));
                }
                for row_data in &upstream {
                    if row(ui, &theme, row_data, &from, &to, theme.tones.amber) {
                        show = Some(row_data.id.clone());
                    }
                }
            });
            ui.add_space(10.0);
            let done_title = tf(
                "Already on {to} · {n}",
                &[("to", &to), ("n", &done.len().to_string())],
            );
            card(ui, &theme, &done_title, |ui| {
                egui::CollapsingHeader::new(muted(&theme, tl("Show them")))
                    .id_salt("promote_done")
                    .show(ui, |ui| {
                        for row_data in &done {
                            if row(ui, &theme, row_data, &from, &to, theme.tones.gray) {
                                show = Some(row_data.id.clone());
                            }
                        }
                    });
            });
        });

    let ready_ids: std::collections::HashSet<String> = ready.iter().map(|r| r.id.clone()).collect();
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
        None => {}
    }
    if let Some(id) = show {
        g.show_mr(&id);
    }
}

enum Action {
    Copy,
    Show,
}

/// One MR: its presence on both branches, what blocks it; `true` when clicked.
fn row(ui: &mut Ui, theme: &Theme, r: &Row, from: &str, to: &str, tone: Tone) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 6.0), Vec2::new(3.0, 24.0)),
        CornerRadius::same(2),
        tone.fg,
    );
    let mut line = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The row takes the click: its texts, selectable, would take it over them.
    line.style_mut().interaction.selectable_labels = false;
    line.spacing_mut().item_spacing.x = 8.0;
    line.label(
        mono(format!("!{}", r.id), theme.text_size - 1.0).color(theme.palette.text_secondary),
    );
    let (name, _) = split_person(&r.author);
    avatar(&mut line, theme, name, 18.0).on_hover_text(name);
    line.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (branch, presence) in [(to, r.on_to), (from, r.on_from)] {
            let (glyph, label, tone) = match presence {
                None => ("·", tl("Not merged"), theme.tones.gray),
                p => tones::presence(p, false, &theme.tones),
            };
            ui.add(
                Chip::new(&format!("{glyph} {branch}"), tone)
                    .font(fonts::medium(theme.text_size - 2.5)),
            )
            .on_hover_text(format!("{branch}: {label}"));
        }
        let (status, status_tone) = tones::status(r.state, &r.mergeability, &theme.tones);
        badge(ui, theme, status, status_tone);
        for blocker in r.blockers.iter().rev() {
            ui.add(Chip::new(blocker, theme.tones.red).font(fonts::medium(theme.text_size - 2.5)));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&r.title).font(fonts::medium(theme.text_size)))
                    .truncate(),
            );
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open it on the merge requests page"))
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_mr_lands_in_one_stage_by_its_branches() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let (from, to) = ("staging", "production");
        for r in rows(&app, from, to) {
            let arrived = |p: Option<BranchPresence>| p.is_some_and(|p| !p.is_missing());
            match r.stage {
                Stage::Done => assert!(arrived(r.on_to), "!{}", r.id),
                Stage::Ready => assert!(arrived(r.on_from) && !arrived(r.on_to), "!{}", r.id),
                Stage::Upstream => assert!(!arrived(r.on_from) && !arrived(r.on_to), "!{}", r.id),
            }
            // Only what is not on the first branch yet says what holds it.
            if r.stage != Stage::Upstream {
                assert!(r.blockers.is_empty());
            }
        }
    }
}
