//! The Promote page: between two tracked branches (staging → production, say),
//! what is ready to go — on the first, not yet on the second —, what is not on
//! the first yet and why, and what already went. The ready MRs are listed in
//! merge order with how long they have been on the first branch and their
//! ticket; those ticked copy as a release checklist, show in the table, or
//! give the `git cherry-pick` commands that bring them to the second branch.

use bevel_theme::{fonts, Chip, Theme};
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, Vec2};

use super::widgets::{button, mono, muted, page_title, semibold, split_person};
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
    /// When it was merged (RFC 3339), for the merge order.
    pub merged_at: Option<String>,
    /// Since when it is on `from`: its arrival there, else its merge when
    /// `from` is its target.
    pub on_from_since: Option<chrono::DateTime<chrono::Utc>>,
    /// Its ticket: id and status.
    pub ticket: Option<(String, String)>,
    pub effort: Option<crate::models::Effort>,
    /// The commit that landed it (merge or squash), else none.
    pub landed: Option<String>,
    /// Its own commits, oldest first.
    pub commits: Vec<String>,
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
        let parse = |at: &str| {
            chrono::DateTime::parse_from_rfc3339(at)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc))
        };
        let on_from_since = app
            .branch_arrivals
            .get(&(mr.id.clone(), from.to_string()))
            .copied()
            .or_else(|| {
                (mr.target_branch == from)
                    .then(|| mr.merged_at.as_deref().and_then(parse))
                    .flatten()
            });
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
            merged_at: mr.merged_at.clone(),
            on_from_since,
            ticket: mr
                .linked_ticket
                .as_ref()
                .map(|t| (t.id.clone(), t.status.clone())),
            effort: mr.diff_stats.as_ref().map(|d| {
                crate::models::Effort::from_score(d.difficulty(&app.config.complexity_profile))
            }),
            landed: mr.sha.clone().filter(|_| mr.state == GitlabMrState::Merged),
            // GitLab lists them newest first.
            commits: mr.commits.iter().rev().map(|c| c.sha.clone()).collect(),
        });
    }
    // In merge order: the order `from` got them, and the order to bring them.
    rows.sort_by(|a, b| match (&a.merged_at, &b.merged_at) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    rows
}

/// The commands that bring `rows` (in their order) from `from` to `to`, one
/// `git cherry-pick -x` per MR: its merge or squash commit with `-m 1` (git
/// 2.20+ takes it for a plain commit too), else its own commits.
pub fn commands(rows: &[&Row], from: &str, to: &str) -> String {
    let mut out = format!(
        "# {}\ngit fetch origin\ngit switch {to}\ngit pull --ff-only\n",
        tf(
            &tn(
                rows.len() as u64,
                "Bring {n} MR from {from} to {to}",
                "Bring {n} MRs from {from} to {to}",
            ),
            &[("from", from), ("to", to)],
        ),
    );
    for r in rows {
        out.push_str(&format!("\n# !{} {}\n", r.id, r.title));
        match (&r.landed, r.commits.is_empty()) {
            (Some(sha), _) => out.push_str(&format!("git cherry-pick -x -m 1 {sha}\n")),
            (None, false) => {
                out.push_str(&format!(
                    "git fetch origin refs/merge-requests/{}/head\ngit cherry-pick -x {}\n",
                    r.id,
                    r.commits.join(" ")
                ));
            }
            (None, true) => {
                out.push_str(&format!("# {}\n", tl("No commit known yet: refresh first")))
            }
        }
    }
    out.push_str(&format!("\ngit push origin {to}\n"));
    out
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
    let done: Vec<&Row> = all.iter().filter(|r| r.stage == Stage::Done).collect();
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
            let side = 340.0;
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
                                    .on_hover_text(tl("The selected MRs as a Markdown list"))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(release_notes(&picked, &to));
                                    *copied = Some(Copied::Notes);
                                }
                                if button(ui, &theme, None, tl("Markdown checklist"), false)
                                    .on_hover_text(tl(
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
                        if compact_row(ui, &theme, r, r.merged_at.as_deref().map(|at| {
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
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for (i, r) in open_upstream.iter().enumerate() {
                            if i > 0 {
                                ui.label(muted(&theme, "·"));
                            }
                            let (status, _) =
                                tones::status(r.state, &r.mergeability, &theme.tones);
                            // The main blocker: a failed pipeline, else the status.
                            let ci = tl("CI failed");
                            let why = if r.blockers.iter().any(|b| b == ci) {
                                ci.to_lowercase()
                            } else {
                                status.to_lowercase()
                            };
                            let response = ui
                                .add(
                                    egui::Label::new(
                                        RichText::new(format!("!{} {why}", r.id))
                                            .color(theme.palette.text_secondary),
                                    )
                                    .sense(Sense::click()),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(format!(
                                    "{}\n{}",
                                    r.title,
                                    r.blockers
                                        .iter()
                                        .cloned()
                                        .chain(std::iter::once(status.to_string()))
                                        .collect::<Vec<_>>()
                                        .join(" · ")
                                ));
                            if response.clicked() {
                                *show = Some(r.id.clone());
                            }
                        }
                    });
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
                    egui::CollapsingHeader::new(muted(&theme, tl("Show them")))
                        .id_salt("promote_done")
                        .show(ui, |ui| {
                            for r in &done {
                                // How it reached it: merged, picked, copied.
                                let how = tones::presence(r.on_to, false, &theme.tones).1;
                                if compact_row(ui, &theme, r, Some(how.to_lowercase())) {
                                    *show = Some(r.id.clone());
                                }
                            }
                        });
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
        g.show_mr(&id);
    }
}

enum Action {
    Copy,
    Show,
    ShowOpen,
}

enum Copied {
    Commands,
    Notes,
}

/// The selected MRs as release notes: a Markdown list, newest merge last.
pub fn release_notes(rows: &[&Row], to: &str) -> String {
    let mut out = format!("## {}\n\n", tf("Released to {to}", &[("to", to)]));
    for r in rows {
        out.push_str(&format!("- {} (!{})", r.title, r.id));
        if let Some((id, _)) = &r.ticket {
            out.push_str(&format!(" · #{id}"));
        }
        out.push('\n');
    }
    out
}

/// Text in the accent that reads as a link.
fn link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(theme.text_size - 1.0)
                .color(theme.palette.accent),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A line of a side card: the id, the title, a note on the right.
fn compact_row(ui: &mut Ui, theme: &Theme, r: &Row, note: Option<String>) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(4.0, 0.0)),
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let mut line = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    line.style_mut().interaction.selectable_labels = false;
    line.spacing_mut().item_spacing.x = 8.0;
    line.label(
        mono(format!("!{}", r.id), theme.text_size - 1.0).color(theme.palette.text_secondary),
    );
    line.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if let Some(note) = &note {
            ui.label(muted(theme, note).size(theme.text_size - 1.5));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(&r.title)).truncate());
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open it on the merge requests page"))
        .clicked()
}

/// The ready table's columns, from the row's left edge: the box, the MR, the
/// title (the rest), then fixed ones.
const COL_BOX: f32 = 28.0;
const COL_MR: f32 = 56.0;
const COL_SINCE: f32 = 140.0;
const COL_TICKET: f32 = 150.0;
const COL_EFFORT: f32 = 90.0;
const COL_HOW: f32 = 110.0;
const COL_PAD: f32 = 16.0;
const COL_GAP: f32 = 10.0;

/// The x of each column in a row of `width`: box, MR, title, since, ticket,
/// effort, how; and the title's width.
fn columns(left: f32, width: f32) -> ([f32; 7], f32) {
    let fixed = COL_BOX + COL_MR + COL_SINCE + COL_TICKET + COL_EFFORT + COL_HOW + COL_GAP * 6.0;
    let title = (width - COL_PAD * 2.0 - fixed).max(160.0);
    let mut x = left + COL_PAD;
    let mut xs = [0.0; 7];
    for (i, w) in [
        COL_BOX, COL_MR, title, COL_SINCE, COL_TICKET, COL_EFFORT, COL_HOW,
    ]
    .iter()
    .enumerate()
    {
        xs[i] = x;
        x += w + COL_GAP;
    }
    (xs, title)
}

fn ready_header(ui: &mut Ui, theme: &Theme, from: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    let (xs, _) = columns(rect.left(), rect.width());
    let on = tf("On {branch} for", &[("branch", from)]);
    let names = [
        "",
        tl("MR"),
        tl("Title"),
        on.as_str(),
        tl("Ticket"),
        tl("Effort"),
        tl("How"),
    ];
    for (x, name) in xs.iter().zip(names) {
        painter.text(
            egui::pos2(*x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            name.to_uppercase(),
            fonts::medium(theme.text_size - 2.5),
            theme.palette.text_muted,
        );
    }
}

/// What a click on a ready row asked for.
enum ReadyClick {
    Toggle,
    Open,
}

/// A ready MR as a table row: its box, the MR, its title over when it was
/// merged, how long it has been on the first branch, its ticket, its effort,
/// how it got there.
fn ready_row(
    ui: &mut Ui,
    theme: &Theme,
    r: &Row,
    ticked: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<ReadyClick> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme.palette.raised);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    let (xs, title_width) = columns(rect.left(), rect.width());
    let cell = |i: usize, w: f32| {
        egui::Rect::from_min_size(egui::pos2(xs[i], rect.top()), Vec2::new(w, rect.height()))
    };
    let mut click = None;
    let child = |ui: &mut Ui, r: egui::Rect| {
        let mut c = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r)
                .layout(Layout::left_to_right(Align::Center)),
        );
        c.style_mut().interaction.selectable_labels = false;
        c
    };
    let mut c = child(ui, cell(0, COL_BOX));
    let mut checked = ticked;
    if c.checkbox(&mut checked, "")
        .on_hover_text(tl("In the release or not"))
        .changed()
    {
        click = Some(ReadyClick::Toggle);
    }
    let mut c = child(ui, cell(1, COL_MR));
    c.label(mono(format!("!{}", r.id), theme.text_size - 1.0).color(theme.palette.text_secondary));
    let mut c = child(ui, cell(2, title_width));
    c.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(RichText::new(&r.title).font(fonts::semibold(theme.text_size)))
                .truncate(),
        );
        let (name, _) = split_person(&r.author);
        let note = match r.merged_at.as_deref() {
            Some(at) => tf(
                "merged {when} · {who}",
                &[
                    ("when", &crate::utils::format_relative_date(at)),
                    ("who", name),
                ],
            ),
            None => name.to_string(),
        };
        ui.add(egui::Label::new(muted(theme, note).size(theme.text_size - 2.0)).truncate());
    });
    let soak = r
        .on_from_since
        .map(|since| (now - since).num_minutes().max(0) as f64 / 1440.0);
    let mut c = child(ui, cell(3, COL_SINCE));
    c.label(
        RichText::new(soak.map_or("—".into(), crate::next_step::days_text))
            .font(fonts::semibold(theme.text_size - 0.5))
            .color(match soak {
                Some(d) if d < 1.0 => theme.tones.amber.fg,
                Some(_) => theme.tones.green.fg,
                None => theme.palette.text_muted,
            }),
    )
    .on_hover_text(tl(
        "How long it has been tested there: under a day, in amber",
    ));
    let mut c = child(ui, cell(4, COL_TICKET));
    match &r.ticket {
        Some((id, status)) => {
            c.add(
                Chip::new(&format!("#{id} {status}"), theme.tones.blue)
                    .font(fonts::medium(theme.text_size - 2.5)),
            )
            .on_hover_text(tl("The linked ticket and its status"));
        }
        None => {
            c.label(muted(theme, "—"));
        }
    }
    let mut c = child(ui, cell(5, COL_EFFORT));
    match r.effort {
        Some(effort) => {
            let (label, tone) = tones::effort(effort, &theme.tones);
            c.label(RichText::new(label).color(tone.fg));
        }
        None => {
            c.label(muted(theme, "—"));
        }
    }
    let mut c = child(ui, cell(6, COL_HOW));
    let (_, label, tone) = tones::presence(r.on_from, false, &theme.tones);
    c.label(mono(label.to_lowercase(), theme.text_size - 1.5).color(tone.fg));
    if click.is_some() {
        return click;
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open it on the merge requests page"))
        .clicked()
        .then_some(ReadyClick::Open)
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
