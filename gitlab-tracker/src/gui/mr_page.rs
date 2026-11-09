//! The MR page: the selected MR on a page of its own (double click or Enter on
//! the table, Esc back, J / K to the next or previous MR).
//!
//! Left, the work: why it cannot merge, then its discussions, changes, commits
//! and pipelines. Right, the context: its branches and deployments, a timeline,
//! approvals, people, ticket, related MRs, how it compares, and its description.
//! What the list does not keep (merge checks, approval rules, the threads in full,
//! deployments) is read when the page opens (`gitlab_detail`).

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};

use super::details::Folder;
use super::icons::Icon;
use super::inspector::{self, Step};
use super::widgets::{
    avatar, badge, button, label_chip, mono, muted, semibold, spinner, split_person,
};
use super::{details, tones, GuiApp, Page};
use crate::actions::Action;
use crate::app::App;
use crate::gitlab_detail::{ConflictFile, Overview, PageData, Rule, Thread};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, Effort, GitlabMrState, MrStatus, PipelineState, TrackedMr};
use crate::utils::{format_relative_date, matches_gitlab_username};
use gitlab_tracker_core::MergeabilityStatus;

/// The left column's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Discussions,
    Changes,
    Commits,
    Pipelines,
}

/// The right column's width, and the page width below which it goes under.
const SIDE_WIDTH: f32 = 420.0;
/// The context tiles' height, and the page width below which they stack.
const TILE_HEIGHT: f32 = 158.0;
/// A branch line in the "Where it is" tile.
const BRANCH_LINE: f32 = 26.0;
/// Branch lines shown before "+N more branches".
const MAX_BRANCH_LINES: usize = 8;
const TILES_IN_A_ROW: f32 = 760.0;
const TWO_COLUMNS: f32 = 1060.0;

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let Some(mr) = g.app.selected_mr().cloned() else {
        return;
    };
    let data = g.details.page(&g.app, &mr, ui.ctx()).cloned();
    crumbs(g, ui, &mr);
    ui.add_space(10.0);
    header(g, ui, &mr);
    ui.add_space(14.0);
    if let Some(id) = facts(g, ui, &mr, data.as_ref()) {
        g.open_mr(&id);
    }
    ui.add_space(14.0);

    let width = ui.available_width();
    egui::ScrollArea::vertical()
        .id_salt(("mr_page", &mr.id))
        .auto_shrink(false)
        .show(ui, |ui| {
            next_banner(g, ui, &mr);
            tiles(g, ui, &mr, data.as_ref(), width);
            ui.add_space(12.0);
            lifecycle(g, ui, &mr);
            ui.add_space(16.0);
            if width >= TWO_COLUMNS {
                let left = width - SIDE_WIDTH - 16.0;
                // Each column in a rect of its own, clipped to it: content that
                // does not wrap (a long path, a wide table) can never spill into
                // the other column.
                let top = ui.cursor().min;
                let viewport = ui.clip_rect();
                let column = |x: f32, w: f32| {
                    egui::Rect::from_min_size(egui::pos2(x, top.y), Vec2::new(w, f32::INFINITY))
                };
                let left_rect = column(top.x, left);
                let right_rect = column(top.x + left + 16.0, SIDE_WIDTH);
                let mut left_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                left_ui.set_clip_rect(left_rect.intersect(viewport));
                work(g, &mut left_ui, &mr, data.as_ref());
                let mut right_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                right_ui.set_clip_rect(right_rect.intersect(viewport));
                context(g, &mut right_ui, &mr, data.as_ref());
                let height = left_ui
                    .min_rect()
                    .height()
                    .max(right_ui.min_rect().height());
                ui.allocate_space(Vec2::new(width, height));
            } else {
                work(g, ui, &mr, data.as_ref());
                ui.add_space(16.0);
                context(g, ui, &mr, data.as_ref());
            }
            ui.add_space(24.0);
        });
}

/// Who the MR waits on, what they have to do, and what comes after.
fn next_banner(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
    if step.actor == crate::next_step::Actor::Unknown {
        return;
    }
    let action = super::next::card(ui, &theme, mr, &step, true);
    ui.add_space(12.0);
    if let Some(action) = action {
        super::next::apply(g, action, true);
    }
}

/// The MR's stages in a line, with the time each hop took against the usual.
fn lifecycle(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
    let stages = super::next::lifecycle(&g.app, mr, &step);
    let tone = super::next::tone(&step, &theme);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                tile_title(ui, &theme, tl("Lifecycle"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        muted(
                            &theme,
                            tl("Time between steps · the median of the tracked MRs in grey"),
                        )
                        .size(theme.text_size - 2.5),
                    );
                });
            });
            super::next::lifecycle_strip(ui, &theme, &stages, tone);
        });
}

// ── Top ──────────────────────────────────────────────────────────────────────

/// Back to the list, where this MR is in it, and the keys to move.
fn crumbs(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let mrs: Vec<(String, String)> = g
        .app
        .visible_mrs()
        .map(|m| (m.id.clone(), m.title.clone()))
        .collect();
    let ids: Vec<&String> = mrs.iter().map(|(id, _)| id).collect();
    let at = ids.iter().position(|id| **id == mr.id);
    // The next and previous MRs, their titles on hover.
    let neighbour = |ui: &mut Ui, (id, title): &(String, String)| {
        ui.label(muted(&theme, format!("!{id}")))
            .on_hover_text(format!("!{id} · {title}"));
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let back = ui
            .add(
                egui::Label::new(
                    RichText::new(format!("← {}", tl("Merge requests")))
                        .color(theme.palette.accent),
                )
                .sense(Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if back.clicked() {
            g.open_page(Page::MergeRequests);
        }
        if let Some(at) = at {
            ui.label(muted(&theme, "/"));
            ui.label(muted(
                &theme,
                tf(
                    "{n} of {total}",
                    &[
                        ("n", &(at + 1).to_string()),
                        ("total", &ids.len().to_string()),
                    ],
                ),
            ));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(muted(&theme, tl("back")));
            key(ui, &theme, "Esc");
            ui.label(muted(&theme, "·"));
            if let Some(next) = at.and_then(|i| mrs.get(i + 1)) {
                key(ui, &theme, "J");
                neighbour(ui, next);
            }
            if let Some(prev) = at.and_then(|i| i.checked_sub(1)).and_then(|i| mrs.get(i)) {
                if at.and_then(|i| mrs.get(i + 1)).is_some() {
                    ui.label(muted(&theme, "·"));
                }
                neighbour(ui, prev);
                key(ui, &theme, "K");
            }
        });
    });
}

fn key(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.label(mono(text, theme.text_size - 2.0).color(theme.palette.text_secondary));
        });
}

fn header(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let me = g.app.config.gitlab_username.clone();
    let mut pick = None;
    let mut label_clicked = None;
    ui.horizontal_top(|ui| {
        let actions_width = 360.0_f32.min(ui.available_width() * 0.4);
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width() - actions_width, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        mono(format!("!{}", mr.id), theme.text_size + 3.0)
                            .color(theme.palette.text_secondary),
                    );
                    ui.label(semibold(&mr.title, theme.text_size + 7.0));
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                    let (status, tone) = tones::status(mr.state, &mr.mergeability, &theme.tones);
                    badge(ui, &theme, status, tone);
                    let (name, _) = split_person(&mr.author);
                    avatar(ui, &theme, name, 20.0);
                    let mine = me
                        .as_deref()
                        .is_some_and(|u| matches_gitlab_username(&mr.author, u));
                    let mut text = RichText::new(name).font(fonts::medium(theme.text_size));
                    if mine {
                        text = text.color(theme.tones.blue.fg);
                    }
                    ui.label(text);
                    if mine {
                        ui.add(Chip::new(tl("you"), theme.tones.blue).pill());
                    }
                    let mut dates = Vec::new();
                    if let Some(at) = &mr.created_at {
                        dates.push(tf("opened {when}", &[("when", &format_relative_date(at))]));
                    }
                    if let Some(at) = &mr.updated_at {
                        dates.push(tf("updated {when}", &[("when", &format_relative_date(at))]));
                    }
                    ui.label(muted(&theme, format!("· {}", dates.join(" · "))));
                    branch_chip(ui, &theme, &mr.source_branch, false);
                    ui.label(muted(&theme, "→"));
                    branch_chip(ui, &theme, &mr.target_branch, true);
                    for label in &mr.labels {
                        if label_chip(ui, &g.app, label, &theme).clicked() {
                            label_clicked = Some(label.clone());
                        }
                    }
                });
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(
                ui,
                &theme,
                Some(Icon::ExternalLink),
                tl("Open in GitLab"),
                true,
            )
            .clicked()
            {
                pick = Some(Action::OpenSelectedMr);
            }
            // Log time is on the ticket tile.

            if button(ui, &theme, Some(Icon::Copy), tl("Copy git clone"), false)
                .on_hover_text(tl("git clone -b <source branch> <ssh url>"))
                .clicked()
            {
                pick = Some(Action::YankCloneCommand);
            }
        });
    });
    if let Some(action) = pick {
        g.pending.push(action);
    }
    if let Some(label) = label_clicked {
        g.filter_by_label(&label);
    }
}

fn branch_chip(ui: &mut Ui, theme: &Theme, name: &str, target: bool) {
    egui::Frame::new()
        .fill(theme.palette.input)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| {
            let color = if target {
                theme.tones.blue.fg
            } else {
                theme.palette.text_secondary
            };
            ui.label(mono(name, theme.text_size - 1.5).color(color));
        });
}

/// The figures that tell where the MR stands, side by side. "Behind target"
/// lists, on a click, what the MR lacks; returns the MR picked there.
fn facts(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) -> Option<String> {
    let theme = g.theme;
    let t = &theme.tones;
    let mut list: Vec<(&str, String, Option<Tone>, String)> = Vec::new();
    if let Some(p) = mr.pipelines.first() {
        let (label, tone) = tones::pipeline(p.status, t);
        let failed: Vec<&str> = p
            .jobs
            .iter()
            .filter(|j| j.status == "failed")
            .map(|j| j.name.as_str())
            .collect();
        let caption = if failed.is_empty() {
            format!("#{}", p.id)
        } else {
            format!("#{} · {}", p.id, failed.join(", "))
        };
        list.push((tl("Pipeline"), label.to_string(), Some(tone), caption));
    }
    let open_threads = unresolved(mr, data);
    list.push((
        tl("Threads"),
        if open_threads == 0 {
            tl("All resolved").to_string()
        } else {
            tn(open_threads as u64, "{n} open", "{n} open")
        },
        (open_threads > 0).then_some(t.amber),
        data.and_then(|d| d.threads.as_ref().ok())
            .map(|threads| {
                let all = tn(
                    threads.iter().filter(|t| t.resolvable).count() as u64,
                    "{n} thread in all",
                    "{n} threads in all",
                );
                // Plain comments are never open, but may still ask for something.
                match threads.iter().filter(|t| !t.resolvable).count() {
                    0 => all,
                    n => format!("{all} · {}", tn(n as u64, "{n} comment", "{n} comments")),
                }
            })
            .unwrap_or_default(),
    ));
    let (approvals, approval_caption) = match data.and_then(|d| d.rules.as_ref()) {
        Some(rules) => (
            format!(
                "{} / {}",
                rules.iter().filter(|r| r.approved).count(),
                rules.len()
            ),
            tl("rules met").to_string(),
        ),
        None => (
            mr.approved_by.len().to_string(),
            tl("approvals").to_string(),
        ),
    };
    list.push((tl("Approvals"), approvals, None, approval_caption));
    if let Some(stats) = &mr.diff_stats {
        list.push((
            tl("Size"),
            format!("+{} −{}", stats.additions, stats.deletions),
            None,
            format!(
                "{} · {}",
                tn(stats.files_changed as u64, "{n} file", "{n} files"),
                tn(stats.commits_count as u64, "{n} commit", "{n} commits")
            ),
        ));
        let score = stats.difficulty(&g.app.config.complexity_profile);
        let (label, tone) = tones::effort(Effort::from_score(score), t);
        list.push((
            tl("Effort"),
            label.to_string(),
            Some(tone),
            g.app.config.complexity_profile.name.clone(),
        ));
    }
    let behind = data
        .and_then(|d| d.overview.as_ref())
        .and_then(|o| o.diverged_commits_count)
        .or(mr.diff_stats.as_ref().and_then(|s| s.commits_behind));
    if let (Some(n), GitlabMrState::Opened) = (behind, mr.state) {
        let up_to_date = n == 0 && mr.mergeability == MergeabilityStatus::Mergeable;
        list.push((
            tl("Behind target"),
            if up_to_date {
                tl("Up to date").to_string()
            } else {
                tn(n as u64, "{n} commit", "{n} commits")
            },
            if up_to_date {
                Some(t.green)
            } else {
                (n > 0).then_some(t.amber)
            },
            mr.target_branch.clone(),
        ));
    }
    if let Some(ticket) = &mr.linked_ticket {
        list.push((
            tl("Ticket"),
            format!("#{}", ticket.id),
            Some(t.blue),
            ticket.status.clone(),
        ));
    }

    let behind_name = (behind.is_some_and(|n| n > 0) && mr.state == GitlabMrState::Opened)
        .then(|| tl("Behind target"));
    let mut open = None;
    let columns = list.len().max(1);
    let gap = 1.0;
    let width = ((ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32).max(120.0);
    egui::Frame::new()
        .fill(theme.palette.border_soft)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(gap);
                for (name, value, tone, caption) in &list {
                    let link = behind_name == Some(*name);
                    let frame = egui::Frame::new()
                        .fill(theme.palette.panel)
                        .inner_margin(Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.set_width(width - 28.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(
                                    muted(&theme, name.to_uppercase()).size(theme.text_size - 2.5),
                                );
                                let font = fonts::semibold(theme.text_size + 2.0);
                                match mr.diff_stats.as_ref().filter(|_| *name == tl("Size")) {
                                    // The lines added in green, the lines removed in red.
                                    Some(stats) => {
                                        let mut job = egui::text::LayoutJob::default();
                                        job.append(
                                            &format!("+{}", stats.additions),
                                            0.0,
                                            egui::TextFormat::simple(font.clone(), t.green.fg),
                                        );
                                        job.append(
                                            &format!("−{}", stats.deletions),
                                            6.0,
                                            egui::TextFormat::simple(font, t.red.fg),
                                        );
                                        ui.label(job);
                                    }
                                    None => {
                                        let color = tone.map_or(theme.palette.text, |t| t.fg);
                                        ui.label(RichText::new(value).font(font).color(color));
                                    }
                                }
                                let caption = if link {
                                    format!("{caption} · {}", tl("see what it lacks"))
                                } else {
                                    caption.clone()
                                };
                                ui.add(
                                    egui::Label::new(
                                        muted(&theme, caption).size(theme.text_size - 1.5),
                                    )
                                    .truncate()
                                    .selectable(!link),
                                );
                            });
                        });
                    if link {
                        let response = frame
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.hovered() {
                            ui.painter().rect_stroke(
                                response.rect,
                                CornerRadius::ZERO,
                                egui::Stroke::new(1.0, t.amber.fg.gamma_multiply(0.5)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if let Some(id) = behind_popup(g, &response, mr, data) {
                            open = Some(id);
                        }
                    }
                }
            });
        });
    open
}

// ── Behind the target ────────────────────────────────────────────────────────

/// What the target branch has that the MR lacks, in a popup under `response`
/// (opened by a click on it): the merges and direct pushes on the target, the
/// tracked MRs among them opening their page. Returns the MR picked.
pub fn behind_popup(
    g: &GuiApp,
    response: &egui::Response,
    mr: &TrackedMr,
    data: Option<&PageData>,
) -> Option<String> {
    let theme = g.theme;
    let mut open = None;
    egui::Popup::menu(response).show(|ui| {
        const WIDTH: f32 = 540.0;
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 2.0;
        let count = data
            .and_then(|d| d.overview.as_ref())
            .and_then(|o| o.diverged_commits_count)
            .or(mr.diff_stats.as_ref().and_then(|s| s.commits_behind))
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(semibold(
                tn(
                    count as u64,
                    "{n} commit behind",
                    "{n} commits behind",
                ),
                theme.text_size,
            ));
            ui.add(
                Chip::new(&mr.target_branch, theme.tones.amber)
                    .font(egui::FontId::monospace(theme.text_size - 2.5)),
            );
        });
        ui.label(
            muted(
                &theme,
                tl("On the target, not in this branch yet: rebase it, or merge the target into it."),
            )
            .size(theme.text_size - 1.5),
        );
        ui.add_space(6.0);
        match data.map(|d| d.behind.as_deref()) {
            None => {
                ui.horizontal(|ui| {
                    spinner(ui, 12.0, theme.palette.accent);
                    ui.label(muted(&theme, tl("Asking GitLab…")));
                });
            }
            Some(None) => {
                ui.label(muted(&theme, tl("GitLab did not list them.")));
            }
            Some(Some(commits)) => {
                for commit in commits {
                    let tracked = g.app.mrs.iter().find(|other| {
                        commit.mr_iid.as_deref() == Some(other.id.as_str())
                            || other.sha.as_deref().is_some_and(|sha| {
                                !sha.is_empty()
                                    && (commit.sha.starts_with(sha) || sha.starts_with(&commit.sha))
                            })
                    });
                    let picked = match tracked {
                        Some(other) => {
                            super::sidebar::behind_row(g, ui, other, &mr.source_branch, WIDTH)
                        }
                        None => {
                            commit_row(g, ui, commit, WIDTH);
                            None
                        }
                    };
                    if let Some(id) = picked {
                        open = Some(id);
                        ui.close();
                    }
                }
                // The first parents only: the commits a merge brought are in
                // the count, not in the list.
                if commits.len() < count as usize {
                    ui.add_space(4.0);
                    ui.label(
                        muted(
                            &theme,
                            tl("Each merge once: the commits it brought are counted, not listed."),
                        )
                        .size(theme.text_size - 2.5),
                    );
                }
            }
        }
        let Some(project) = mr.web_url.split_once("/-/merge_requests").map(|(p, _)| p) else {
            return;
        };
        ui.add_space(6.0);
        super::widgets::rule(ui, &theme);
        ui.add_space(6.0);
        let url = format!(
            "{project}/-/compare/{}...{}",
            mr.source_branch, mr.target_branch
        );
        let text = tf(
            "Compare with {source} on GitLab",
            &[("source", &mr.target_branch)],
        );
        if button(ui, &theme, Some(Icon::ExternalLink), &text, false)
            .on_hover_text(&url)
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            ui.close();
        }
    });
    open
}

/// A commit of the target the tracking does not know: its author, `!iid` (or
/// short SHA) and title, who and when. A click opens it on GitLab.
fn commit_row(g: &GuiApp, ui: &mut Ui, commit: &crate::gitlab_detail::BehindCommit, width: f32) {
    let theme = g.theme;
    let p = &theme.palette;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open in GitLab"));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme.radii.medium), p.raised);
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(8.0, 5.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    avatar(ui, &theme, &commit.author, 26.0);
    ui.add_space(4.0);
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let reference = match &commit.mr_iid {
                Some(iid) => format!("!{iid}"),
                None => commit.sha.get(..8).unwrap_or(&commit.sha).to_string(),
            };
            ui.label(mono(reference, theme.text_size - 1.5).color(p.text_muted));
            ui.add(
                egui::Label::new(
                    RichText::new(&commit.title)
                        .font(fonts::medium(theme.text_size - 1.0))
                        .color(p.text),
                )
                .truncate()
                .selectable(false),
            );
        });
        let detail = if commit.mr_iid.is_some() {
            tf(
                "merged by {who} {when}, not tracked",
                &[
                    ("who", &commit.author),
                    ("when", &format_relative_date(&commit.created_at)),
                ],
            )
        } else {
            tf(
                "pushed by {who} {when}",
                &[
                    ("who", &commit.author),
                    ("when", &format_relative_date(&commit.created_at)),
                ],
            )
        };
        ui.add(
            egui::Label::new(muted(&theme, detail).size(theme.text_size - 2.5))
                .truncate()
                .selectable(false),
        );
    });
    if response.clicked() && !commit.web_url.is_empty() {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab(commit.web_url.clone()));
        ui.close();
    }
}

/// Open threads: the page's count once read, else the list's.
fn unresolved(mr: &TrackedMr, data: Option<&PageData>) -> usize {
    match data.and_then(|d| d.threads.as_ref().ok()) {
        Some(threads) => threads.iter().filter(|t| t.is_open()).count(),
        None => mr.user_notes_count as usize,
    }
}

// ── Why it can't merge ───────────────────────────────────────────────────────

/// A merge check: met, failed, or a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Met,
    Blocks,
    Warns,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub verdict: Verdict,
    pub what: String,
    pub why: String,
    /// GitLab logins shown after `what`, by name with their avatar.
    pub who: Vec<String>,
    /// The files in conflict, under the conflict.
    pub files: Vec<ConflictFile>,
    /// "N commits behind": a click lists them.
    pub behind: bool,
}

/// What stands between the MR and its merge, worst first, then what is met.
pub fn checks(
    mr: &TrackedMr,
    overview: Option<&Overview>,
    rules: Option<&[Rule]>,
    threads: Option<&[Thread]>,
    conflicts: Option<&[ConflictFile]>,
) -> Vec<Check> {
    let status = overview.map_or("", |o| o.detailed_merge_status.as_str());
    let mut list = Vec::new();
    let mut push = |verdict, what: String, why: String| {
        list.push(Check {
            verdict,
            what,
            why,
            who: Vec::new(),
            files: Vec::new(),
            behind: false,
        })
    };

    let draft = overview.is_some_and(|o| o.draft) || mr.mergeability == MergeabilityStatus::Draft;
    if draft {
        push(
            Verdict::Blocks,
            tl("Still a draft").into(),
            tl("Mark it as ready in GitLab once it is.").into(),
        );
    }
    let conflict = overview.is_some_and(|o| o.has_conflicts)
        || mr.mergeability == MergeabilityStatus::Conflict;
    let conflict_title = tf("Conflicts with {target}", &[("target", &mr.target_branch)]);
    if conflict {
        push(
            Verdict::Blocks,
            conflict_title.clone(),
            tf(
                "Rebase {source} on {target}, or merge {target} into it, and resolve them.",
                &[("source", &mr.source_branch), ("target", &mr.target_branch)],
            ),
        );
    }
    if mr.mergeability == MergeabilityStatus::RequestedChanges || status == "requested_changes" {
        push(
            Verdict::Blocks,
            tl("Changes requested").into(),
            tl("A reviewer asked for changes: their review must be withdrawn or redone.").into(),
        );
    }
    match mr.pipelines.first() {
        Some(p) if p.status == PipelineState::Failed => {
            let failed: Vec<&str> = p
                .jobs
                .iter()
                .filter(|j| j.status == "failed")
                .map(|j| j.name.as_str())
                .collect();
            let why = if failed.is_empty() {
                tf("Pipeline #{id}.", &[("id", &p.id.to_string())])
            } else {
                tf(
                    "Pipeline #{id}: {jobs}. Its log is in the Pipelines tab.",
                    &[("id", &p.id.to_string()), ("jobs", &failed.join(", "))],
                )
            };
            push(Verdict::Blocks, tl("Pipeline failed").into(), why);
        }
        Some(p) if matches!(p.status, PipelineState::Running | PipelineState::Pending) => {
            push(
                Verdict::Warns,
                tl("Pipeline still running").into(),
                tf("Pipeline #{id}.", &[("id", &p.id.to_string())]),
            );
        }
        Some(p) if p.status == PipelineState::Success => {
            push(Verdict::Met, tl("Pipeline passed").into(), String::new());
        }
        _ => {}
    }
    let open = match threads {
        Some(threads) => threads.iter().filter(|t| t.is_open()).count(),
        None => mr.user_notes_count as usize,
    };
    if open > 0 {
        let blocking = overview.is_some_and(|o| !o.blocking_discussions_resolved)
            || status == "discussions_not_resolved";
        push(
            if blocking {
                Verdict::Blocks
            } else {
                Verdict::Warns
            },
            tn(
                open as u64,
                "{n} unresolved thread",
                "{n} unresolved threads",
            ),
            if blocking {
                tl("The project requires every thread resolved before merging.").into()
            } else {
                tl("They do not block the merge here.").into()
            },
        );
    } else {
        push(
            Verdict::Met,
            tl("Every thread resolved").into(),
            String::new(),
        );
    }
    // Plain comments cannot be resolved and never block, but one the author
    // has not answered yet may still ask for something.
    let unanswered = threads.map_or(0, |threads| {
        threads
            .iter()
            .filter(|t| !t.resolvable)
            .filter(|t| {
                t.notes.last().is_some_and(|last| {
                    !crate::utils::matches_gitlab_username(&mr.author, &last.username)
                })
            })
            .count()
    });
    if unanswered > 0 {
        push(
            Verdict::Warns,
            tn(
                unanswered as u64,
                "{n} general comment unanswered",
                "{n} general comments unanswered",
            ),
            tl("They do not block the merge, but may ask for something.").into(),
        );
    }
    match rules {
        Some(rules) => {
            let met = rules.iter().filter(|r| r.approved).count();
            if met == rules.len() {
                push(
                    Verdict::Met,
                    tl("Every approval rule met").into(),
                    String::new(),
                );
            } else {
                let waiting: Vec<String> = rules
                    .iter()
                    .filter(|r| !r.approved)
                    .map(|r| {
                        let who: Vec<&str> = r
                            .eligible_approvers
                            .iter()
                            .map(|u| u.name.as_str())
                            .take(3)
                            .collect();
                        if who.is_empty() {
                            format!("{} ({})", r.name, r.approvals_required)
                        } else {
                            format!("{} ({}: {})", r.name, r.approvals_required, who.join(", "))
                        }
                    })
                    .collect();
                push(
                    Verdict::Warns,
                    tf(
                        "Approvals: {met} of {total} rules met",
                        &[
                            ("met", &met.to_string()),
                            ("total", &rules.len().to_string()),
                        ],
                    ),
                    waiting.join(" · "),
                );
            }
        }
        None if !mr.approved_by.is_empty() => {
            push(Verdict::Met, tl("Approved by").into(), String::new());
        }
        None if status == "not_approved" || mr.mergeability == MergeabilityStatus::NotApproved => {
            push(Verdict::Warns, tl("Not approved yet").into(), String::new());
        }
        None => {}
    }
    let behind = overview
        .and_then(|o| o.diverged_commits_count)
        .or(mr.diff_stats.as_ref().and_then(|s| s.commits_behind));
    let mut behind_title = None;
    if let Some(n) = behind.filter(|n| *n > 0) {
        let title = tn(
            n as u64,
            "{n} commit behind {target}",
            "{n} commits behind {target}",
        )
        .replace("{target}", &mr.target_branch);
        behind_title = Some(title.clone());
        let rebase = status == "need_rebase" || mr.mergeability == MergeabilityStatus::NeedsRebase;
        push(
            if rebase {
                Verdict::Blocks
            } else {
                Verdict::Warns
            },
            title,
            if rebase {
                tl("The project merges fast-forward only: rebase first.").into()
            } else {
                tl("It merges anyway; a rebase tests it against the latest code.").into()
            },
        );
    }
    if !draft {
        push(Verdict::Met, tl("Not a draft").into(), String::new());
    }
    if overview.and_then(|o| o.force_remove_source_branch) == Some(true) {
        push(
            Verdict::Met,
            tl("The source branch is deleted after the merge").into(),
            String::new(),
        );
    }
    if let Some(check) = list
        .iter_mut()
        .find(|c| Some(&c.what) == behind_title.as_ref())
    {
        check.behind = true;
    }
    if let Some(check) = list.iter_mut().find(|c| c.what == conflict_title) {
        check.files = conflicts.unwrap_or_default().to_vec();
    }
    // The approvers by name, next to "Approved by".
    if let Some(check) = list.iter_mut().find(|c| c.what == tl("Approved by")) {
        check.who = mr.approved_by.clone();
    }
    list.sort_by_key(|c| match c.verdict {
        Verdict::Blocks => 0,
        Verdict::Warns => 1,
        Verdict::Met => 2,
    });
    list
}

/// The checks, worst first; the file in conflict picked, to show in Changes,
/// and the MR picked in the "behind" popup, to open.
fn blockers(
    g: &GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    data: Option<&PageData>,
) -> (Option<String>, Option<String>) {
    let theme = g.theme;
    let list = checks(
        mr,
        data.and_then(|d| d.overview.as_ref()),
        data.and_then(|d| d.rules.as_deref()),
        data.and_then(|d| d.threads.as_deref().ok()),
        data.and_then(|d| d.conflicts.as_deref()),
    );
    let mut picked = None;
    let mut open = None;
    let ready = !list.iter().any(|c| c.verdict != Verdict::Met);
    let title = if ready {
        tl("Nothing blocks the merge")
    } else {
        tl("Why it can't merge")
    };
    let stroke = if ready {
        theme.tones.green.fg
    } else {
        theme.tones.red.fg
    };
    theme
        .card_frame()
        .stroke(egui::Stroke::new(1.0, stroke.gamma_multiply(0.45)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(semibold(title, theme.text_size));
                if data.is_none() {
                    spinner(ui, 12.0, theme.palette.accent);
                    ui.label(muted(&theme, tl("Asking GitLab…")));
                }
            });
            ui.add_space(6.0);
            for (i, check) in list.iter().enumerate() {
                if i > 0 {
                    super::widgets::rule(ui, &theme);
                }
                let (glyph, color) = match check.verdict {
                    Verdict::Met => ("✓", theme.tones.green.fg),
                    Verdict::Blocks => ("✕", theme.tones.red.fg),
                    Verdict::Warns => ("!", theme.tones.amber.fg),
                };
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add_sized(
                        [14.0, 18.0],
                        egui::Label::new(
                            RichText::new(glyph)
                                .font(fonts::semibold(theme.text_size))
                                .color(color),
                        ),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let what = RichText::new(&check.what).font(fonts::medium(theme.text_size));
                        if check.behind {
                            // A link: the commits it lacks, in a popup.
                            let response = ui
                                .add(
                                    egui::Label::new(what.underline())
                                        .sense(Sense::click())
                                        .selectable(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(tl("See what it lacks"));
                            if let Some(id) = behind_popup(g, &response, mr, data) {
                                open = Some(id);
                            }
                        } else if check.who.is_empty() {
                            ui.label(what);
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                // As tall as a text line, like the checks around.
                                ui.spacing_mut().interact_size.y = 18.0;
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(what);
                                for login in &check.who {
                                    person_chip(ui, &theme, &display_of(mr, data, login));
                                }
                            });
                        }
                        if !check.why.is_empty() {
                            ui.label(muted(&theme, &check.why));
                        }
                        if !check.files.is_empty() {
                            picked = conflict_files(ui, &theme, &mr.id, &check.files);
                        }
                    });
                });
            }
        });
    (picked, open)
}

/// How many files in conflict show at once; the box scrolls past them.
const CONFLICTS_SHOWN: usize = 10;

/// The files in conflict, all of them, in a box that scrolls past
/// [`CONFLICTS_SHOWN`]; the files changed on both sides that merge anyway, on
/// one line. Returns the file clicked.
fn conflict_files(
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    files: &[ConflictFile],
) -> Option<String> {
    let (clashes, others): (Vec<&ConflictFile>, Vec<&ConflictFile>) =
        files.iter().partition(|f| f.clash);
    let mut picked = None;
    ui.add_space(6.0);
    if !clashes.is_empty() {
        ui.label(
            RichText::new(tn(
                clashes.len() as u64,
                "{n} file in conflict",
                "{n} files in conflict",
            ))
            .font(fonts::medium(theme.text_size - 1.0))
            .color(theme.palette.text_secondary),
        )
        .on_hover_text(tl(
            "Worked out from what each side changed since they parted: the same or \
             adjacent lines. GitLab lists them exactly when you resolve the conflicts.",
        ));
        ui.add_space(2.0);
        let row = ui
            .painter()
            .layout_no_wrap("x".into(), conflict_font(theme), egui::Color32::WHITE)
            .size()
            .y
            + ui.spacing().item_spacing.y;
        egui::Frame::new()
            .fill(theme.palette.raised)
            .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
            .corner_radius(CornerRadius::same(theme.radii.small))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt(("mr page conflicts", mr_id))
                    .max_height(CONFLICTS_SHOWN as f32 * row - ui.spacing().item_spacing.y)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // Rows as tall as their text, not as a control.
                        ui.spacing_mut().interact_size.y = 0.0;
                        let tree = Folder::of(clashes.iter().map(|f| f.path.as_str()));
                        picked = tree_rows(ui, theme, mr_id, &tree, "", 0);
                    });
            });
    }
    if !others.is_empty() {
        ui.add_space(4.0);
        let names: Vec<&str> = others.iter().map(|f| file_name(&f.path)).collect();
        ui.add(
            egui::Label::new(muted(
                theme,
                format!(
                    "{} {}",
                    tn(
                        others.len() as u64,
                        "Also changed on both sides, likely merging on its own ({n}):",
                        "Also changed on both sides, likely merging on their own ({n}):",
                    ),
                    names.join(", ")
                ),
            ))
            .truncate(),
        )
        .on_hover_text(
            others
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    picked
}

fn conflict_font(theme: &Theme) -> egui::FontId {
    egui::FontId::monospace(theme.text_size - 1.5)
}

/// A file's last path segment.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Indentation of one tree level.
const TREE_INDENT: f32 = 16.0;

/// The rows of `folder` (at `path`, `depth` deep): each sub-folder (a chain of
/// folders that hold only one folder on one row, as `src/api/handlers`), its
/// content under it unless folded, then the files. Returns the file clicked.
fn tree_rows(
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    folder: &Folder,
    path: &str,
    depth: usize,
) -> Option<String> {
    let mut picked = None;
    for (name, mut sub) in &folder.folders {
        let mut label = (*name).to_string();
        while sub.files.is_empty() && sub.folders.len() == 1 {
            let (next, deeper) = sub.folders.iter().next().expect("one folder");
            label = format!("{label}/{next}");
            sub = deeper;
        }
        let full = format!("{path}{label}/");
        let id = egui::Id::unique(("mr page conflict folder", mr_id, &full));
        let folded = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let chevron = if folded { "▸" } else { "▾" };
        let response = tree_row(
            ui,
            theme,
            depth,
            &format!("{chevron} {label}/"),
            theme.palette.text_muted,
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            ui.data_mut(|d| d.insert_temp(id, !folded));
        }
        if !folded {
            picked = tree_rows(ui, theme, mr_id, sub, &full, depth + 1).or(picked);
        }
    }
    for path in &folder.files {
        let response = tree_row(ui, theme, depth, file_name(path), theme.tones.red.fg)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{path}\n{}", tl("Show it in Changes")));
        if response.clicked() {
            picked = Some((*path).to_string());
        }
    }
    picked
}

/// One row of the tree, indented, with a guide for each level above it.
fn tree_row(
    ui: &mut Ui,
    theme: &Theme,
    depth: usize,
    text: &str,
    color: egui::Color32,
) -> egui::Response {
    let indent = depth as f32 * TREE_INDENT;
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(conflict_font(theme), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(ui.available_width() - indent);
    let response = ui
        .horizontal(|ui| {
            ui.add_space(indent);
            ui.add(egui::Label::new(job).sense(Sense::click()))
        })
        .inner;
    // The guides, through the item spacing so that they join from row to row.
    let gap = ui.spacing().item_spacing.y;
    let y = egui::Rangef::new(
        response.rect.top() - gap / 2.0,
        response.rect.bottom() + gap / 2.0,
    );
    for level in 0..depth {
        let x = response.rect.left() - indent + level as f32 * TREE_INDENT + 4.0;
        ui.painter()
            .vline(x, y, egui::Stroke::new(1.0, theme.palette.border));
    }
    response
}

// ── Left: the work ───────────────────────────────────────────────────────────

fn work(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    if mr.state == GitlabMrState::Opened {
        let (path, open) = blockers(g, ui, mr, data);
        if let Some(path) = path {
            g.mr_tab = Tab::Changes;
            g.details.open_file(&mr.id, &path);
        }
        if let Some(id) = open {
            g.open_mr(&id);
        }
        ui.add_space(16.0);
    }
    let threads = data.and_then(|d| d.threads.as_ref().ok());
    let changed_files = g.details.diff(&mr.id).map(<[_]>::len);
    let tabs = [
        (
            Tab::Discussions,
            tl("Discussions"),
            threads.map_or(mr.user_notes_count as usize, |t| t.len()),
        ),
        (
            Tab::Changes,
            tl("Changes"),
            // The diff read, else GitLab's count (the demo has none).
            changed_files.unwrap_or_else(|| {
                mr.diff_stats
                    .as_ref()
                    .map_or(0, |s| s.files_changed as usize)
            }),
        ),
        (Tab::Commits, tl("Commits"), mr.commits.len()),
        (Tab::Pipelines, tl("Pipelines"), mr.pipelines.len()),
    ];
    theme.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, label, n) in tabs {
                let on = g.mr_tab == tab;
                let text = RichText::new(format!("{label} · {n}"))
                    .font(if on {
                        fonts::medium(theme.text_size)
                    } else {
                        egui::FontId::proportional(theme.text_size)
                    })
                    .color(if on {
                        theme.palette.text
                    } else {
                        theme.palette.text_secondary
                    });
                let response = ui
                    .add(egui::Label::new(text).sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if on {
                    let r = response.rect;
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 6.0,
                        egui::Stroke::new(2.0, theme.palette.accent),
                    );
                }
                if response.clicked() {
                    g.mr_tab = tab;
                }
                ui.add_space(14.0);
            }
        });
        ui.add_space(6.0);
        super::widgets::rule(ui, &theme);
        ui.add_space(10.0);
        match g.mr_tab {
            Tab::Discussions => discussions(g, ui, mr, data),
            Tab::Changes => {
                g.details.open_first_file(&mr.id);
                details::changes(
                    &mut g.details,
                    &g.app,
                    ui,
                    &theme,
                    &mr.id,
                    &mr.web_url,
                    true,
                )
            }
            Tab::Commits => commits(g, ui, mr),
            Tab::Pipelines => super::mr_pipelines::tab(g, ui, mr),
        }
    });
}

fn discussions(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    let threads = match data.map(|d| &d.threads) {
        None => {
            ui.horizontal(|ui| {
                spinner(ui, 12.0, theme.palette.accent);
                ui.label(muted(&theme, tl("Reading the threads…")));
            });
            return;
        }
        Some(Err(error)) => {
            ui.label(RichText::new(format!("⚠ {error}")).color(theme.tones.red.fg));
            return;
        }
        Some(Ok(threads)) => threads,
    };
    let open = threads.iter().filter(|t| t.is_open()).count();
    let resolved = threads.iter().filter(|t| t.resolved).count();
    let comments = threads.iter().filter(|t| !t.resolvable).count();
    ui.horizontal(|ui| {
        let mut summary = tf(
            "{open} open · {resolved} resolved",
            &[
                ("open", &open.to_string()),
                ("resolved", &resolved.to_string()),
            ],
        );
        if comments > 0 {
            summary = format!(
                "{summary} · {}",
                tn(comments as u64, "{n} comment", "{n} comments")
            );
        }
        ui.label(muted(&theme, summary));
        if resolved > 0 {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if super::widgets::switch(ui, &theme, g.show_resolved, tl("Show resolved"))
                    .clicked()
                {
                    g.show_resolved = !g.show_resolved;
                }
            });
        }
    });
    ui.add_space(8.0);
    if threads.is_empty() {
        ui.label(muted(&theme, tl("No review thread on this MR.")));
        return;
    }
    let me = g.app.config.gitlab_username.clone();
    // The threads waiting for me first, then the other open ones, then the rest.
    let waits_for_me = |t: &Thread| {
        t.is_open()
            && t.notes
                .last()
                .is_some_and(|last| me.as_deref().is_some_and(|u| last.username != u))
    };
    let mut shown: Vec<&Thread> = threads
        .iter()
        .filter(|t| g.show_resolved || !t.resolved)
        .collect();
    shown.sort_by_key(|t| (!waits_for_me(t), t.resolved, !t.resolvable));
    for thread in shown {
        // The code it is about, from the diff once read.
        let code: Vec<String> = match (&thread.path, thread.line, g.details.diff(&mr.id)) {
            (Some(path), Some(line), Some(files)) => files
                .iter()
                .find(|f| &f.new_path == path || &f.old_path == path)
                .map(|f| {
                    details::snippet(&f.diff, line, 3)
                        .into_iter()
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        thread_card(ui, &theme, thread, &code, me.as_deref());
        ui.add_space(10.0);
    }
}

fn thread_card(ui: &mut Ui, theme: &Theme, thread: &Thread, code: &[String], me: Option<&str>) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .fill(theme.palette.raised)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Whose turn first, on the right; the path takes what is
                    // left and is shortened when too long.
                    let last = thread.notes.last();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(last) = last {
                                let waiting =
                                    thread.is_open() && me.is_some_and(|u| last.username != u);
                                let when = format_relative_date(&last.created_at);
                                if waiting {
                                    ui.label(muted(theme, when));
                                    ui.add(Chip::new(tl("Your turn"), theme.tones.orange))
                                        .on_hover_text(tl("The last note is someone else's"));
                                } else {
                                    ui.label(muted(theme, when));
                                }
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                let (label, tone) = if !thread.resolvable {
                                    (tl("Comment"), theme.tones.gray)
                                } else if thread.resolved {
                                    (tl("Resolved"), theme.tones.green)
                                } else {
                                    (tl("Unresolved"), theme.tones.amber)
                                };
                                ui.add(Chip::new(label, tone));
                                match (&thread.path, thread.line) {
                                    (Some(path), line) => {
                                        let text = match line {
                                            Some(n) => format!(
                                                "{path} · {}",
                                                tf("line {n}", &[("n", &n.to_string())])
                                            ),
                                            None => path.clone(),
                                        };
                                        ui.add(
                                            egui::Label::new(mono(text, theme.text_size - 1.5))
                                                .truncate(),
                                        )
                                        .on_hover_text(path);
                                    }
                                    _ => {
                                        ui.label(muted(theme, tl("General")));
                                    }
                                }
                            });
                        });
                    });
                });
            if !code.is_empty() {
                let lines: Vec<&str> = code.iter().map(String::as_str).collect();
                details::code(ui, theme, &lines, true);
            }
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 10,
                    right: 10,
                    top: 6,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for note in &thread.notes {
                        ui.add_space(6.0);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            avatar(ui, theme, &note.author, 20.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&note.author)
                                            .font(fonts::medium(theme.text_size)),
                                    );
                                    ui.label(
                                        muted(theme, format_relative_date(&note.created_at))
                                            .size(theme.text_size - 1.5),
                                    );
                                });
                                ui.label(
                                    RichText::new(note.body.trim())
                                        .color(theme.palette.text_secondary),
                                );
                            });
                        });
                    }
                });
        });
}

/// The MR's commits, and where each one is.
fn commits(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    if mr.commits.is_empty() {
        ui.label(muted(
            &theme,
            tl("The commits are read with the next refresh."),
        ));
        return;
    }
    let branches = g.app.branches.clone();
    // Fixed columns, the title taking the rest (never under TITLE_MIN: the
    // table scrolls sideways instead).
    const SHA: f32 = 76.0;
    const DATE_MIN: f32 = 104.0;
    const GAP: f32 = 16.0;
    const TITLE_MIN: f32 = 220.0;
    const ROW: f32 = 32.0;
    let branch_font = egui::FontId::monospace(theme.text_size - 2.0);
    let branch_widths: Vec<f32> = branches
        .iter()
        .map(|b| text_width(ui, b, branch_font.clone()).max(36.0))
        .collect();
    // The dates as shown: the column is as wide as the longest ("about a month
    // ago"), so that it never pushes the branches' columns of its row.
    let dates: Vec<String> = mr
        .commits
        .iter()
        .map(|commit| {
            chrono::DateTime::from_timestamp(commit.fingerprint.authored_at, 0)
                .map(|d| format_relative_date(&d.to_rfc3339()))
                .unwrap_or_default()
        })
        .collect();
    let date_font = egui::FontId::proportional(theme.text_size - 1.0);
    let date_width = dates
        .iter()
        .map(|d| text_width(ui, d, date_font.clone()).ceil() + 2.0)
        .fold(DATE_MIN, f32::max);
    let fixed_width =
        SHA + date_width + branch_widths.iter().sum::<f32>() + GAP * (2 + branches.len()) as f32;
    let title = (ui.available_width() - fixed_width).max(TITLE_MIN);
    let total = fixed_width + title;

    egui::ScrollArea::horizontal()
        .id_salt(("mr_commits", &mr.id))
        .show(ui, |ui| {
            ui.set_width(total);
            ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.0);
            // Header.
            ui.horizontal(|ui| {
                ui.set_height(24.0);
                let head = |ui: &mut Ui, width: f32, text: &str| {
                    fixed(ui, width, |ui| {
                        ui.label(muted(&theme, text.to_uppercase()).size(theme.text_size - 2.5));
                    });
                };
                head(ui, SHA, "SHA");
                head(ui, title, tl("Title"));
                head(ui, date_width, tl("Authored"));
                for (b, w) in branches.iter().zip(&branch_widths) {
                    centered(ui, *w, |ui| {
                        ui.label(
                            RichText::new(b)
                                .font(branch_font.clone())
                                .color(theme.palette.text_muted),
                        );
                    });
                }
            });
            super::widgets::rule(ui, &theme);
            for (commit, date) in mr.commits.iter().zip(&dates) {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(total, ROW), Sense::hover());
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
                }
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                row.spacing_mut().item_spacing.x = GAP;
                fixed(&mut row, SHA, |ui| {
                    ui.label(
                        mono(
                            commit.sha.get(..8).unwrap_or(&commit.sha),
                            theme.text_size - 1.5,
                        )
                        .color(theme.palette.text_secondary),
                    );
                });
                fixed(&mut row, title, |ui| {
                    ui.add(egui::Label::new(&commit.fingerprint.title).truncate())
                        .on_hover_text(format!(
                            "{}\n{}",
                            commit.fingerprint.title, commit.fingerprint.author_email
                        ));
                });
                fixed(&mut row, date_width, |ui| {
                    ui.label(muted(&theme, date.as_str()));
                });
                for (b, w) in branches.iter().zip(&branch_widths) {
                    let presence = commit_presence(&g.app, mr, b, commit);
                    let (glyph, label, tone) = match presence {
                        Some(p) => tones::presence(Some(p), false, &theme.tones),
                        None => ("…", tl("Not checked yet"), theme.tones.gray),
                    };
                    centered(&mut row, *w, |ui| {
                        ui.label(RichText::new(glyph).color(tone.fg))
                            .on_hover_text(format!("{b}: {label}"));
                    });
                }
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom(),
                    egui::Stroke::new(1.0, theme.palette.border_soft),
                );
            }
        });
    ui.add_space(10.0);
    ui.label(muted(&theme, tl("● same SHA · ◆ cherry-picked with -x · ≈ copied (same author, date and title) · ○ absent")));
}

/// A column `width` wide, its content centered.
fn centered(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 20.0),
        Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

/// Where one of the MR's commits is on `branch` (`None`: not scanned yet).
fn commit_presence(
    app: &App,
    mr: &TrackedMr,
    branch: &str,
    commit: &crate::models::MrCommit,
) -> Option<BranchPresence> {
    if matches!(&mr.status, MrStatus::MergedIn(found) if found.contains(branch)) {
        return Some(BranchPresence::Present);
    }
    let index = app.branch_commits.get(branch)?;
    Some(if index.shas.contains(&commit.sha) {
        BranchPresence::Present
    } else if index.picked_from.contains_key(&commit.sha) {
        BranchPresence::CherryPicked
    } else if index.fingerprints.contains_key(&commit.fingerprint) {
        BranchPresence::Copied
    } else {
        BranchPresence::Absent
    })
}

// ── Context tiles ────────────────────────────────────────────────────────────

/// Where the MR is (branches and deployments), who is on it, and its ticket:
/// short states, side by side under the figures (stacked when narrow).
fn tiles(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>, width: f32) {
    let with_ticket = g.app.tracker.is_some() || mr.linked_ticket.is_some();
    let count = if with_ticket { 3 } else { 2 };
    let gap = 12.0;
    let in_a_row = width >= TILES_IN_A_ROW;
    let tile_width = if in_a_row {
        (width - gap * (count as f32 - 1.0)) / count as f32
    } else {
        width
    };
    // Tall enough for every tracked branch; the tiles of a row share it.
    let branches = g.app.branches.len().min(MAX_BRANCH_LINES) as f32;
    let tile_height = TILE_HEIGHT.max(52.0 + branches * BRANCH_LINE);
    let viewport = ui.clip_rect();
    let origin = ui.cursor().min;
    let mut bottom = origin.y;
    let mut pick = None;
    for index in 0..count {
        let (x, y) = if in_a_row {
            (origin.x + index as f32 * (tile_width + gap), origin.y)
        } else {
            (origin.x, origin.y + index as f32 * (tile_height + gap))
        };
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), Vec2::new(tile_width, tile_height));
        bottom = bottom.max(rect.bottom());
        let mut tile_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        tile_ui.set_clip_rect(rect.intersect(viewport));
        let theme = g.theme;
        egui::Frame::new()
            .fill(theme.palette.panel)
            .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
            .corner_radius(CornerRadius::same(theme.radii.large))
            .inner_margin(Margin::symmetric(14, 10))
            .show(&mut tile_ui, |ui| {
                ui.set_width(tile_width - 28.0);
                ui.set_height(tile_height - 20.0);
                match index {
                    0 => where_it_is(g, ui, mr, data),
                    1 => people(g, ui, mr, data),
                    _ => {
                        if let Some(action) = ticket(g, ui, mr) {
                            pick = Some(action);
                        }
                    }
                }
            });
    }
    ui.allocate_space(Vec2::new(width, bottom - origin.y));
    if let Some(action) = pick {
        g.pending.push(action);
    }
}

fn tile_title(ui: &mut Ui, theme: &Theme, title: &str) {
    ui.label(muted(theme, title.to_uppercase()).size(theme.text_size - 2.5));
    ui.add_space(6.0);
}

/// Each tracked branch: whether the MR is on it, and the environment deployed
/// from it, with whether that deployment includes the MR.
fn where_it_is(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    tile_title(ui, &theme, tl("Where it is"));
    let rows = g.app.flow_branch_rows(mr);
    let failed = matches!(mr.status, MrStatus::Error(_));
    let shown = MAX_BRANCH_LINES;
    let name_width = rows
        .iter()
        .take(shown)
        .map(|(r, _)| {
            text_width(
                ui,
                &r.branch,
                egui::FontId::monospace(theme.text_size - 1.5),
            )
        })
        .fold(0.0, f32::max)
        .min(ui.available_width() * 0.4);
    // Each row's chip, ahead: the widest sets the column the deployments
    // start at, so they all line up.
    let chips: Vec<(&str, String, Tone)> = rows
        .iter()
        .take(shown)
        .map(|(row, place)| {
            let (glyph, label, tone) =
                tones::flow_presence(row.presence, failed, *place, &theme.tones);
            let label = match row.presence {
                Some(BranchPresence::Partial) => {
                    tf("{n} missing", &[("n", &row.missing.len().to_string())])
                }
                _ => label.to_string(),
            };
            (glyph, label, tone)
        })
        .collect();
    let small = ui
        .style()
        .text_styles
        .get(&egui::TextStyle::Small)
        .map_or(11.0, |f| f.size);
    // As the chip draws it: "glyph label", padded 7 on each side.
    let chip_width = chips
        .iter()
        .map(|(glyph, label, _)| {
            text_width(
                ui,
                &format!("{glyph} {label}"),
                egui::FontId::proportional(small),
            ) + 14.0
        })
        .fold(0.0, f32::max);
    // Even lines, as tight as the chips allow.
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.spacing_mut().interact_size.y = BRANCH_LINE - 4.0;
    for ((row, _), (glyph, label, tone)) in rows.iter().zip(chips) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            fixed(ui, name_width, |ui| {
                ui.add(egui::Label::new(mono(&row.branch, theme.text_size - 1.5)).truncate())
                    .on_hover_text(&row.branch);
            });
            fixed(ui, chip_width, |ui| {
                ui.add(Chip::new(&label, tone).glyph(glyph));
            });
            let deployed =
                data.and_then(|d| d.deployments.iter().find(|dep| dep.reference == row.branch));
            if let Some(dep) = deployed {
                // Cut to the room left: a plain label in a row extends past it.
                let line = deployment_line(&theme, dep, ui.available_width());
                ui.add(egui::Label::new(line).truncate());
            }
        });
    }
    if rows.len() > shown {
        ui.label(muted(
            &theme,
            tn(
                (rows.len() - shown) as u64,
                "+{n} more branch",
                "+{n} more branches",
            ),
        ));
    }
}

/// A deployment in one line: whether it includes the MR (in green when it
/// does), then what was deployed and when, cut with an ellipsis to `width`.
pub(super) fn deployment_line(
    theme: &Theme,
    dep: &crate::gitlab_detail::Deployment,
    width: f32,
) -> egui::text::LayoutJob {
    let (text, color) = match dep.includes {
        Some(true) => (
            tf("deployed on {env}", &[("env", &dep.environment)]),
            theme.tones.green.fg,
        ),
        Some(false) => (
            tf("not yet on {env}", &[("env", &dep.environment)]),
            theme.palette.text_muted,
        ),
        None => (dep.environment.clone(), theme.palette.text_muted),
    };
    let mut job = egui::text::LayoutJob::default();
    let size = theme.text_size - 1.5;
    job.append(
        &text,
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(size), color),
    );
    job.append(
        dep.sha.get(..8).unwrap_or(&dep.sha),
        6.0,
        egui::TextFormat::simple(
            egui::FontId::monospace(size - 0.5),
            theme.palette.text_muted,
        ),
    );
    job.append(
        &format!("· {}", format_relative_date(&dep.created_at)),
        6.0,
        egui::TextFormat::simple(egui::FontId::proportional(size), theme.palette.text_muted),
    );
    job.wrap.max_rows = 1;
    job.wrap.max_width = width;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    job
}

/// Author, assignee, reviewers and approvers, me marked.
fn people(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    tile_title(ui, &theme, tl("People"));
    // Four rows in the tile: each as tall as its avatar, not a control.
    ui.spacing_mut().interact_size.y = 20.0;
    let me = g.app.config.gitlab_username.clone();
    let person = |ui: &mut Ui, display: &str| {
        let (name, handle) = split_person(display);
        let mine = me
            .as_deref()
            .is_some_and(|u| matches_gitlab_username(display, u));
        let response = avatar(ui, &theme, name, 18.0);
        let mut text = RichText::new(name).font(fonts::medium(theme.text_size - 0.5));
        if mine {
            text = text.color(theme.tones.blue.fg);
        }
        let response = response | ui.add(egui::Label::new(text).truncate());
        if let Some(handle) = handle {
            response.on_hover_text(format!("@{handle}"));
        }
    };
    let caption_width = [
        tl("Author"),
        tl("Assignee"),
        tl("Reviewers"),
        tl("Approved by"),
    ]
    .iter()
    .map(|c| text_width(ui, c, egui::FontId::proportional(theme.text_size)))
    .fold(0.0, f32::max);
    let caption = |ui: &mut Ui, text: &str| {
        fixed(ui, caption_width, |ui| {
            ui.label(muted(&theme, text));
        });
    };
    ui.horizontal(|ui| {
        caption(ui, tl("Author"));
        person(ui, &mr.author);
    });
    ui.horizontal(|ui| {
        caption(ui, tl("Assignee"));
        match &mr.assignee {
            Some(a) => person(ui, a),
            None => {
                ui.label(muted(&theme, "—"));
            }
        }
    });
    ui.horizontal(|ui| {
        caption(ui, tl("Reviewers"));
        ui.spacing_mut().item_spacing.x = 4.0;
        if mr.reviewers.is_empty() {
            ui.label(muted(&theme, tl("None")));
        }
        // One reviewer by name; more as avatars, names on hover.
        if mr.reviewers.len() == 1 {
            person(ui, &mr.reviewers[0]);
        } else {
            for reviewer in &mr.reviewers {
                let (name, _) = split_person(reviewer);
                let approved = mr
                    .approved_by
                    .iter()
                    .any(|a| reviewer.contains(&format!("@{a}")));
                let hover = if approved {
                    tf("{who} approved", &[("who", name)])
                } else {
                    name.to_string()
                };
                avatar(ui, &theme, name, 20.0).on_hover_text(hover);
            }
        }
    });
    ui.horizontal(|ui| {
        caption(ui, tl("Approved by"));
        let approvers: Vec<String> = mr
            .approved_by
            .iter()
            .map(|login| display_of(mr, data, login))
            .collect();
        match approvers.as_slice() {
            [] => {
                ui.label(muted(&theme, tl("None")));
            }
            [one] => person(ui, one),
            several => {
                // Every avatar, then the names as far as they fit, all on hover.
                ui.spacing_mut().item_spacing.x = 4.0;
                let names: Vec<&str> = several.iter().map(|d| split_person(d).0).collect();
                let mut response = None::<egui::Response>;
                for name in &names {
                    let r = avatar(ui, &theme, name, 18.0);
                    response = Some(match response {
                        Some(prev) => prev | r,
                        None => r,
                    });
                }
                ui.add_space(2.0);
                let label = ui.add(
                    egui::Label::new(
                        RichText::new(names.join(", ")).font(fonts::medium(theme.text_size - 0.5)),
                    )
                    .truncate(),
                );
                let response = response.map_or(label.clone(), |r| r | label);
                response.on_hover_text(several.join("\n"));
            }
        }
    });
}

/// The width `text` takes in `font`.
fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font, egui::Color32::WHITE)
        .size()
        .x
}

/// A column `width` wide, its content left-aligned and kept to it.
fn fixed(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 20.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

/// The linked ticket in short: status, time spent against the estimate, and its
/// actions. Returns the action asked for.
fn ticket(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) -> Option<Action> {
    let theme = g.theme;
    let mut pick = None;
    tile_title(ui, &theme, tl("Ticket"));
    let Some(ticket) = &mr.linked_ticket else {
        ui.label(muted(
            &theme,
            tl("No ticket linked: name it in the MR's title or description, e.g. #1234."),
        ));
        return None;
    };
    // The status on the right; the subject takes what is left.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(Chip::new(&ticket.status, theme.tones.blue));
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let id = ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!("#{}", ticket.id))
                                .color(theme.palette.accent)
                                .font(fonts::medium(theme.text_size)),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tl("Open the ticket"));
                if id.clicked() {
                    pick = Some(Action::OpenSelectedTicket);
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(&ticket.subject).font(fonts::medium(theme.text_size)),
                    )
                    .truncate(),
                )
                .on_hover_text(&ticket.subject);
            });
        });
    });
    ui.add_space(6.0);
    match (ticket.time_spent, ticket.time_estimate) {
        (spent, Some(estimate)) if estimate > 0 => {
            let spent = spent.unwrap_or(0);
            let fraction = spent as f32 / estimate as f32;
            let color = if fraction > 1.0 {
                theme.tones.red.fg
            } else {
                theme.palette.accent
            };
            super::widgets::bar(ui, &theme, fraction.min(1.0), color, 6.0);
            ui.add_space(3.0);
            ui.label(
                muted(
                    &theme,
                    tf(
                        "{spent} spent of {estimate}",
                        &[
                            ("spent", &inspector::duration(spent)),
                            ("estimate", &inspector::duration(estimate)),
                        ],
                    ),
                )
                .size(theme.text_size - 1.5),
            );
        }
        (Some(spent), _) => {
            ui.label(
                muted(
                    &theme,
                    tf(
                        "{spent} spent, no estimate",
                        &[("spent", &inspector::duration(spent))],
                    ),
                )
                .size(theme.text_size - 1.5),
            );
        }
        _ => {
            ui.label(muted(&theme, tl("No time logged yet")).size(theme.text_size - 1.5));
        }
    }
    if g.app.tracker.is_some() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(ui, &theme, Some(Icon::Clock), tl("Log time"), true)
                .on_hover_text(tl("Log time on this ticket (L)"))
                .clicked()
            {
                pick = Some(Action::OpenLogTime);
            }
            if button(
                ui,
                &theme,
                Some(Icon::ExternalLink),
                tl("Open the ticket"),
                false,
            )
            .on_hover_text(&ticket.url)
            .clicked()
            {
                pick = Some(Action::OpenSelectedTicket);
            }
        });
    }
    pick
}

// ── Right: the context ───────────────────────────────────────────────────────

fn context(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    side_card(ui, &theme, tl("Timeline"), |ui| {
        inspector::timeline(ui, &theme, &steps(g, mr, data))
    });
    approvals(g, ui, mr, data);
    related(g, ui, mr);
    compared(g, ui, mr);
    side_card(ui, &theme, tl("Planning"), |ui| {
        inspector::planning_body(ui, &theme, mr)
    });
    if !mr.description.trim().is_empty() {
        side_card(ui, &theme, tl("Description"), |ui| {
            g.description.show(ui, &theme, mr);
        });
    }
}

/// A block of the right column: a tile of its own, titled like the tiles above.
fn side_card(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            tile_title(ui, theme, title);
            body(ui);
        });
    ui.add_space(12.0);
}

/// The MR's life, newest first: the branches up to production, then (oldest
/// last) review requests and reviews, pushes, pipelines, approval and merge,
/// the branches its changes came down to branching off just above the merge.
fn steps(g: &GuiApp, mr: &TrackedMr, data: Option<&PageData>) -> Vec<Step> {
    let t = &g.theme.tones;
    let mut dated: Vec<(String, Step)> = Vec::new();
    let mut add = |at: Option<&str>, title: String, color: egui::Color32| {
        if let Some(at) = at {
            dated.push((
                at.to_string(),
                Step {
                    title,
                    detail: Some(format_relative_date(at)),
                    color,
                    pending: false,
                    side: false,
                },
            ));
        }
    };
    add(mr.created_at.as_deref(), tl("Created").into(), t.blue.fg);
    if let Some(review) = &mr.review {
        for (who, at) in &review.review_requested_at {
            let display = display_of(mr, data, who);
            let name = split_person(&display).0;
            add(
                Some(at),
                tf("Review requested from {who}", &[("who", name)]),
                t.gray.fg,
            );
        }
        add(
            review.first_review_at.as_deref(),
            tl("First review").into(),
            t.teal.fg,
        );
        add(
            review.last_push_at.as_deref(),
            tl("Last push").into(),
            t.blue.fg,
        );
        add(
            review.approved_at.as_deref(),
            tl("Approved").into(),
            t.green.fg,
        );
    }
    for p in &mr.pipelines {
        let (label, tone) = tones::pipeline(p.status, t);
        add(
            p.created_at.as_deref(),
            tf(
                "Pipeline #{id} · {status}",
                &[("id", &p.id.to_string()), ("status", label)],
            ),
            tone.fg,
        );
    }
    if mr.state == GitlabMrState::Merged {
        let by = mr.merged_by.as_deref().unwrap_or("?");
        add(
            mr.merged_at.as_deref(),
            tf(
                "Merged into {branch} by {who}",
                &[("branch", &mr.target_branch), ("who", by)],
            ),
            t.purple.fg,
        );
    }
    // Built oldest first, then turned over: the newest on top, as in the
    // History of the MR list's inspector, the steps still to come above all.
    dated.sort_by(|a, b| a.0.cmp(&b.0));
    if mr.state != GitlabMrState::Merged {
        return dated.into_iter().rev().map(|(_, s)| s).collect();
    }
    let (sync, promotion) = super::inspector::flow_steps(g, mr, format_relative_date);
    // The branches its changes come down to branch off right at the merge.
    let merged_at = mr.merged_at.clone().unwrap_or_default();
    let split = dated.partition_point(|(at, _)| *at <= merged_at);
    let after = dated.split_off(split);
    let mut steps: Vec<Step> = dated.into_iter().map(|(_, s)| s).collect();
    steps.extend(sync);
    steps.extend(after.into_iter().map(|(_, s)| s));
    steps.extend(promotion);
    steps.reverse();
    steps
}

fn approvals(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    let Some(rules) = data.and_then(|d| d.rules.as_ref()) else {
        return;
    };
    let met = rules.iter().filter(|r| r.approved).count();
    side_card(
        ui,
        &theme,
        &format!("{} · {met} / {}", tl("Approval rules"), rules.len()),
        |ui| {
            for rule in rules {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(&rule.name);
                    if rule.approved {
                        ui.add(Chip::new(tl("met"), theme.tones.green));
                    } else {
                        let missing = rule
                            .approvals_required
                            .saturating_sub(rule.approved_by.len() as u32);
                        ui.add(Chip::new(
                            &tn(missing as u64, "needs {n}", "needs {n}"),
                            theme.tones.amber,
                        ));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for user in rule.eligible_approvers.iter().take(5).rev() {
                            let approved =
                                rule.approved_by.iter().any(|a| a.username == user.username);
                            let response = avatar(ui, &theme, &user.name, 18.0);
                            let hover = if approved {
                                tf("{who} approved", &[("who", &user.name)])
                            } else {
                                user.name.clone()
                            };
                            response.on_hover_text(hover);
                        }
                    });
                });
                ui.add_space(4.0);
            }
            if !mr.approved_by.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(muted(&theme, tl("Approved by")));
                    for login in &mr.approved_by {
                        person_chip(ui, &theme, &display_of(mr, data, login));
                    }
                });
            }
        },
    );
}

/// "Full Name (@login)" for a GitLab login, from what the MR and the page know
/// (author, assignee, reviewers, approval rules, thread authors); the login alone
/// when nobody says.
pub fn display_of(mr: &TrackedMr, data: Option<&PageData>, login: &str) -> String {
    let tag = format!("(@{login})");
    let known = mr
        .approvers
        .iter()
        .chain(std::iter::once(&mr.author))
        .chain(mr.assignee.iter())
        .chain(mr.reviewers.iter())
        .find(|display| display.ends_with(&tag));
    if let Some(display) = known {
        return display.clone();
    }
    let from_page = data.and_then(|d| {
        let rules = d
            .rules
            .iter()
            .flatten()
            .flat_map(|r| r.approved_by.iter().chain(&r.eligible_approvers));
        rules
            .map(|u| (u.name.as_str(), u.username.as_str()))
            .chain(
                d.threads
                    .iter()
                    .flatten()
                    .flat_map(|t| &t.notes)
                    .map(|n| (n.author.as_str(), n.username.as_str())),
            )
            .find(|(name, username)| *username == login && !name.is_empty())
            .map(|(name, _)| format!("{name} {tag}"))
    });
    from_page.unwrap_or_else(|| login.to_string())
}

/// A person: initials avatar, full name, and the login muted.
fn person_chip(ui: &mut Ui, theme: &Theme, display: &str) {
    let (name, handle) = split_person(display);
    avatar(ui, theme, name, 18.0);
    ui.label(RichText::new(name).font(fonts::medium(theme.text_size)));
    if let Some(handle) = handle {
        ui.label(muted(theme, format!("(@{handle})")));
    }
}

/// The other tracked MRs this one goes with: the same ticket, the same source
/// branch (backports), or naming it.
pub fn related_mrs<'a>(app: &'a App, mr: &TrackedMr) -> Vec<(&'a TrackedMr, String)> {
    let ticket = mr.linked_ticket.as_ref().map(|t| t.id.as_str());
    let mention = format!("!{}", mr.id);
    app.mrs
        .iter()
        .filter(|other| other.id != mr.id && !other.title.is_empty())
        .filter_map(|other| {
            let why = if ticket.is_some()
                && other.linked_ticket.as_ref().map(|t| t.id.as_str()) == ticket
            {
                tf("same ticket #{id}", &[("id", ticket.unwrap_or_default())])
            } else if other.source_branch == mr.source_branch {
                tf(
                    "same branch, into {target}",
                    &[("target", &other.target_branch)],
                )
            } else if other.description.contains(&mention) || other.title.contains(&mention) {
                tf("mentions {mr}", &[("mr", &mention)])
            } else {
                return None;
            };
            Some((other, why))
        })
        .collect()
}

fn related(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let list = related_mrs(&g.app, mr);
    if list.is_empty() {
        return;
    }
    let mut pick = None;
    side_card(ui, &theme, tl("Related MRs"), |ui| {
        for (other, why) in &list {
            let response = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        mono(format!("!{}", other.id), theme.text_size - 1.0)
                            .color(theme.palette.text_secondary),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (status, tone) =
                            tones::status(other.state, &other.mergeability, &theme.tones);
                        badge(ui, &theme, status, tone);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(&other.title).truncate());
                        });
                    });
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            ui.label(muted(&theme, why).size(theme.text_size - 1.5));
            ui.add_space(4.0);
            if response.clicked() {
                pick = Some(other.id.clone());
            }
        }
    });
    if let Some(id) = pick {
        g.select_id = Some(id);
    }
}

/// Days between two GitLab timestamps.
fn days(from: Option<&str>, to: Option<&str>) -> Option<f64> {
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok();
    let from = parse(from?)?;
    let to = match to {
        Some(to) => parse(to)?,
        None => chrono::Utc::now().fixed_offset(),
    };
    Some((to - from).num_minutes() as f64 / 1440.0)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[values.len() / 2])
}

/// "2.1 d", or hours under a day.
fn span(days: f64) -> String {
    if days < 1.0 {
        format!("{:.0} h", days * 24.0)
    } else {
        format!("{days:.1} d")
    }
}

/// How this MR compares with the other tracked ones: its wait for a first review,
/// its age (or cycle time), its size.
fn compared(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let first_review = |m: &TrackedMr| {
        days(
            m.created_at.as_deref(),
            m.review.as_ref()?.first_review_at.as_deref(),
        )
    };
    let cycle = |m: &TrackedMr| match m.state {
        GitlabMrState::Merged => days(m.created_at.as_deref(), m.merged_at.as_deref()),
        _ => None,
    };
    let lines = |m: &TrackedMr| {
        m.diff_stats
            .as_ref()
            .map(|s| (s.additions + s.deletions) as f64)
    };
    let others: Vec<&TrackedMr> = g.app.mrs.iter().filter(|m| m.id != mr.id).collect();
    let mut rows: Vec<(String, String, Option<Tone>, String)> = Vec::new();
    if let (Some(mine), Some(med)) = (
        first_review(mr),
        median(others.iter().filter_map(|m| first_review(m)).collect()),
    ) {
        rows.push((
            tl("Wait for first review").into(),
            span(mine),
            (mine > med * 1.5).then_some(theme.tones.amber),
            tf("median {v}", &[("v", &span(med))]),
        ));
    }
    let age = match mr.state {
        GitlabMrState::Merged => cycle(mr),
        _ => days(mr.created_at.as_deref(), None),
    };
    if let (Some(mine), Some(med)) = (
        age,
        median(others.iter().filter_map(|m| cycle(m)).collect()),
    ) {
        let label = if mr.state == GitlabMrState::Merged {
            tl("Cycle time")
        } else {
            tl("Open so far")
        };
        rows.push((
            label.into(),
            span(mine),
            (mine > med * 1.5).then_some(theme.tones.amber),
            tf("median cycle {v}", &[("v", &span(med))]),
        ));
    }
    if let Some(mine) = lines(mr) {
        let sizes: Vec<f64> = others.iter().filter_map(|m| lines(m)).collect();
        if !sizes.is_empty() {
            let below = sizes.iter().filter(|s| **s < mine).count();
            let percent = below * 100 / sizes.len();
            rows.push((
                tl("Size").into(),
                tn(mine as u64, "{n} line", "{n} lines"),
                None,
                tf("larger than {p} %", &[("p", &percent.to_string())]),
            ));
        }
    }
    let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
    if matches!(
        step.actor,
        crate::next_step::Actor::Author
            | crate::next_step::Actor::Reviewers
            | crate::next_step::Actor::Merger
    ) {
        if let Some(waited) = step.waited_days(chrono::Utc::now()) {
            let usual = (step.actor == crate::next_step::Actor::Reviewers)
                .then(|| crate::next_step::usual_first_review_days(&g.app))
                .flatten();
            let who = if step.mine {
                tl("you").to_string()
            } else {
                step.who()
            };
            rows.insert(
                0,
                (
                    tf("Waiting on {who}", &[("who", &who)]),
                    span(waited),
                    usual
                        .map_or(waited > 7.0, |u| waited > u * 1.5)
                        .then_some(theme.tones.amber),
                    usual
                        .map(|u| tf("usually {v}", &[("v", &span(u))]))
                        .unwrap_or_else(|| tl("since the last move").to_string()),
                ),
            );
        }
    }
    if rows.is_empty() {
        return;
    }
    side_card(ui, &theme, tl("Compared to the tracked MRs"), |ui| {
        egui::Grid::new(("compare", &mr.id))
            .spacing(Vec2::new(12.0, 6.0))
            .show(ui, |ui| {
                for (name, value, tone, caption) in &rows {
                    ui.label(name);
                    ui.label(
                        RichText::new(value)
                            .font(fonts::semibold(theme.text_size))
                            .color(tone.map_or(theme.palette.text, |t| t.fg)),
                    );
                    ui.label(muted(&theme, caption));
                    ui.end_row();
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_checks_put_what_blocks_first() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let mr = app
            .mrs
            .iter()
            .find(|m| m.mergeability == MergeabilityStatus::Conflict)
            .expect("the demo has an MR in conflict");
        let data = crate::demo::demo_page_data(mr);
        let list = checks(
            mr,
            data.overview.as_ref(),
            data.rules.as_deref(),
            data.threads.as_deref().ok(),
            data.conflicts.as_deref(),
        );
        assert_eq!(list.first().map(|c| c.verdict), Some(Verdict::Blocks));
        assert!(list.iter().any(|c| c.what.contains("Conflicts")));
        // Sorted: nothing met before something that blocks or warns.
        let order: Vec<u8> = list
            .iter()
            .map(|c| match c.verdict {
                Verdict::Blocks => 0,
                Verdict::Warns => 1,
                Verdict::Met => 2,
            })
            .collect();
        assert!(order.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn medians_and_spans_read_as_days() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(Vec::new()), None);
        assert_eq!(span(0.5), "12 h");
        assert_eq!(span(2.06), "2.1 d");
    }
}
