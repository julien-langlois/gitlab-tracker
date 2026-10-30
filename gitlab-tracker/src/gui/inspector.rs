//! The inspector: everything about the selected MR in one scrolling column —
//! review, effort, branches, labels, linked ticket, threads, pipelines, people,
//! planning and history.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use super::widgets::{
    activity, avatar, badge, bar, button, dot, icon_button, key_values, label_chip, mono, muted,
    section, semibold, spinner, split_person,
};
use super::{tones, GuiApp, InspectorSection};
use crate::actions::Action;
use crate::app::{TimeLogState, TrackerView};
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{BranchPresence, Effort, GitlabMrState, MrStatus, PipelineState, TrackedMr};
use crate::utils::{
    awaiting_review_by, format_relative_date, matches_gitlab_username, review_waiting_since,
};

pub fn panel(g: &mut GuiApp, ui: &mut Ui, width: f32) {
    let theme = g.theme;
    egui::Panel::right("inspector")
        .exact_size(width)
        .resizable(false)
        .frame(theme.panel_frame())
        .show(ui, |ui| {
            let edge = ui.max_rect();
            ui.painter().vline(
                edge.left() + 0.5,
                edge.y_range(),
                Stroke::new(1.0, theme.palette.border),
            );
            // Content wider than the panel would push the panel off the window:
            // keep it to the panel's width (long text wraps).
            ui.set_max_width(ui.available_width());
            let Some(mr) = g.app.selected_mr().cloned() else {
                ui.add_space(48.0);
                ui.vertical_centered(|ui| ui.label(muted(&theme, tl("Select a merge request"))));
                return;
            };
            header(g, ui, &mr);
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    body(g, ui, &mr);
                });
        });
}

fn header(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let tones = &theme.tones;
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    mono(format!("!{}", mr.id), theme.text_size - 1.0)
                        .color(theme.palette.text_secondary),
                );
                let (label, tone) = tones::status(mr.state, &mr.mergeability, tones);
                badge(ui, &theme, label, tone);
                let (label, tone) = activity(&g.app, mr.updated_at.as_deref(), &theme);
                ui.spacing_mut().item_spacing.x = 5.0;
                dot(ui, tone.fg, 6.0);
                ui.label(
                    RichText::new(label)
                        .size(theme.text_size - 1.5)
                        .color(tone.fg),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let style = theme.icon_button();
                    let (icon, hint) = if mr.flagged {
                        (Icon::Star, tl("Unflag (Space)"))
                    } else {
                        (Icon::StarOutline, tl("Flag (Space)"))
                    };
                    let response =
                        bevel_widgets::buttons::IconButton::new(hint, Vec2::splat(24.0), &style)
                            .show(ui, |ui, rect, tint| {
                                let color = if mr.flagged { tones.amber.fg } else { tint };
                                icons::paint(ui.painter(), rect.shrink(4.0), icon, color);
                            })
                            .on_hover_text(hint);
                    if response.clicked() {
                        g.pending.push(Action::ToggleFlagSelected);
                    }
                });
            });
            ui.label(semibold(&mr.title, theme.text_size + 2.0));
            byline(ui, &theme, mr);
            branch_route(ui, &theme, &mr.source_branch, &mr.target_branch);
            if let MrStatus::Error(message) = &mr.status {
                ui.label(
                    RichText::new(format!("{} — {message}", tl("Last fetch failed")))
                        .color(tones.red.fg),
                );
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let clone = crate::utils::git_clone_command(&mr.web_url, &mr.source_branch);
                let outlined = theme.outlined_icon_button();
                let height = theme.spacing.control_height + 2.0;
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(
                        ui,
                        tl("Copy the git clone command (Y)"),
                        Icon::Copy,
                        &outlined,
                        height,
                    )
                    .on_hover_text(clone)
                    .clicked()
                    {
                        g.pending.push(Action::YankCloneCommand);
                    }
                    ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                        let response = full_width_button(ui, &theme, tl("Open in GitLab"));
                        if response
                            .on_hover_text(tl("Open in the browser (O)"))
                            .clicked()
                        {
                            g.pending.push(Action::OpenSelectedMr);
                        }
                    });
                });
            });
        });
}

/// The author's avatar and name, then when the MR last moved.
fn byline(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let size = theme.text_size - 1.0;
        let (name, handle) = split_person(&mr.author);
        let response = avatar(ui, theme, name, 20.0);
        if let Some(handle) = handle {
            response.on_hover_text(format!("@{handle}"));
        }
        ui.label(
            RichText::new(name)
                .font(fonts::medium(size))
                .color(theme.palette.text_secondary),
        );
        let Some(updated) = mr.updated_at.as_deref() else {
            return;
        };
        ui.label(
            RichText::new("·")
                .size(size)
                .color(theme.palette.text_muted),
        );
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
        icons::paint(ui.painter(), rect, Icon::Clock, theme.palette.text_muted);
        ui.label(
            RichText::new(tf(
                "updated {when}",
                &[("when", &format_relative_date(updated))],
            ))
            .size(size)
            .color(theme.palette.text_muted),
        );
    });
}

/// Where the MR goes: the source branch, an arrow, the target branch (accented),
/// in a soft box. Each name is shortened with an ellipsis to fit, in full on hover.
fn branch_route(ui: &mut Ui, theme: &Theme, source: &str, target: &str) {
    let font = egui::FontId::monospace(theme.text_size - 1.5);
    let pad = 8.0;
    let pill_height = 22.0;
    let arrow_width = 26.0;
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(egui::CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (rect, response) = ui
                .allocate_exact_size(Vec2::new(ui.available_width(), pill_height), Sense::hover());
            let painter = ui.painter();
            let elided = |text: &str, max: f32, color: egui::Color32| {
                let mut job =
                    egui::text::LayoutJob::simple_singleline(text.to_owned(), font.clone(), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(max.max(0.0));
                painter.layout_job(job)
            };
            // The target is usually short: it keeps its width up to half the row.
            let room = rect.width() - arrow_width - 4.0 * pad;
            let target_galley = elided(target, room / 2.0, theme.tones.blue.fg);
            let source_galley = elided(
                source,
                room - target_galley.size().x,
                theme.palette.text_secondary,
            );
            let pill = |left: f32, galley: std::sync::Arc<egui::Galley>, fill, stroke| {
                let pill = egui::Rect::from_min_size(
                    egui::pos2(left, rect.top()),
                    Vec2::new(galley.size().x + 2.0 * pad, pill_height),
                );
                painter.rect(
                    pill,
                    egui::CornerRadius::same(theme.radii.small),
                    fill,
                    Stroke::new(1.0, stroke),
                    egui::StrokeKind::Inside,
                );
                let text_pos =
                    egui::pos2(pill.left() + pad, pill.center().y - galley.size().y / 2.0);
                painter.galley(text_pos, galley, theme.palette.text);
                pill
            };
            let source_pill = pill(
                rect.left(),
                source_galley,
                theme.palette.panel,
                theme.palette.border_soft,
            );
            let arrow = egui::Rect::from_min_size(
                egui::pos2(source_pill.right(), rect.top()),
                Vec2::new(arrow_width, pill_height),
            );
            let (y, x0, x1) = (arrow.center().y, arrow.left() + 7.0, arrow.right() - 7.0);
            let stroke = Stroke::new(1.5, theme.palette.text_muted);
            painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
            painter.line_segment([egui::pos2(x1 - 4.0, y - 4.0), egui::pos2(x1, y)], stroke);
            painter.line_segment([egui::pos2(x1 - 4.0, y + 4.0), egui::pos2(x1, y)], stroke);
            pill(
                arrow.right(),
                target_galley,
                theme.tones.blue.bg,
                theme.tones.blue.fg.gamma_multiply(0.35),
            );
            response.on_hover_text(format!("{source} → {target}"));
        });
}

/// The primary button stretched to the width left.
fn full_width_button(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    let width = ui.available_width();
    let height = theme.spacing.control_height + 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let fill = if response.hovered() {
        theme.palette.accent_fill.gamma_multiply(1.15)
    } else {
        theme.palette.accent_fill
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(theme.radii.medium), fill);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        fonts::medium(theme.text_size - 0.5),
        egui::Color32::WHITE,
    );
    let total = 13.0 + 6.0 + galley.size().x;
    let left = rect.center().x - total / 2.0;
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(egui::pos2(left + 6.5, rect.center().y), Vec2::splat(13.0)),
        Icon::ExternalLink,
        egui::Color32::WHITE,
    );
    ui.painter().galley(
        egui::pos2(left + 19.0, rect.center().y - galley.size().y / 2.0),
        galley,
        egui::Color32::WHITE,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn body(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    people(g, ui, mr);
    planning(ui, &theme, mr);
    history(ui, &theme, mr);
    review(g, ui, mr);
    if let Some(stats) = &mr.diff_stats {
        let score = stats.difficulty(&g.app.config.complexity_profile);
        let (label, tone) = tones::effort(Effort::from_score(score), &theme.tones);
        let profile = g.app.config.complexity_profile.name.clone();
        section(ui, &theme, tl("Review effort"), |ui| {
            ui.horizontal(|ui| {
                ui.label(muted(&theme, tf("Profile: {name}", &[("name", &profile)])));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.add(Chip::new(label, tone).glyph("●").pill());
                });
            });
            effort_blocks(ui, &theme, score, tone);
            ui.label(muted(
                &theme,
                format!(
                    "{} · +{} −{} · {}",
                    tn(stats.files_changed as u64, "{n} file", "{n} files"),
                    stats.additions,
                    stats.deletions,
                    tn(stats.commits_count as u64, "{n} commit", "{n} commits"),
                ),
            ));
        });
    }
    branches(g, ui, mr);
    if !mr.labels.is_empty() {
        section(ui, &theme, tl("Labels"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                for label in &mr.labels {
                    label_chip(ui, &g.app, label, &theme);
                }
            });
        });
    }
    if g.app.tracker.is_some() || mr.linked_ticket.is_some() {
        if g.scroll_to == Some(InspectorSection::Ticket) {
            ui.scroll_to_cursor(Some(Align::TOP));
            g.scroll_to = None;
        }
        ticket(g, ui, mr);
    }
    threads(ui, &theme, mr);
    if g.scroll_to == Some(InspectorSection::Pipelines) {
        ui.scroll_to_cursor(Some(Align::TOP));
        g.scroll_to = None;
    }
    pipelines(ui, &theme, mr);
    if !mr.description.trim().is_empty() {
        section(ui, &theme, tl("Description"), |ui| {
            ui.label(RichText::new(mr.description.trim()).color(theme.palette.text_secondary));
        });
    }
    ui.add_space(16.0);
}

fn review(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let tones = &theme.tones;
    let me = g.app.config.gitlab_username.clone();
    section(ui, &theme, tl("Review"), |ui| {
        key_values(ui, &theme, |rows| {
            rows.row(tl("Status"), |ui| {
                let waiting = me.as_deref().filter(|me| {
                    awaiting_review_by(
                        mr.state,
                        mr.mergeability,
                        &mr.reviewers,
                        &mr.approved_by,
                        me,
                    )
                });
                if let Some(username) = waiting {
                    let since = review_waiting_since(
                        mr.review.as_ref(),
                        mr.created_at.as_deref(),
                        username,
                    )
                    .map(|at| {
                        let when = format_relative_date(&at.to_rfc3339());
                        format!(" · {}", tf("requested {when}", &[("when", &when)]))
                    })
                    .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{}{since}", tl("Waiting for you")))
                            .color(tones.amber.fg),
                    );
                } else if !mr.approved_by.is_empty() {
                    let names: Vec<String> =
                        mr.approved_by.iter().map(|u| format!("@{u}")).collect();
                    ui.label(
                        RichText::new(tf("Approved by {names}", &[("names", &names.join(", "))]))
                            .color(tones.green.fg),
                    );
                } else if mr.reviewers.is_empty() {
                    ui.label(muted(&theme, tl("No reviewer")));
                } else {
                    let names: Vec<String> = mr.reviewers.iter().map(|u| format!("@{u}")).collect();
                    ui.label(tf(
                        "Requested from {names}",
                        &[("names", &names.join(", "))],
                    ));
                }
            });
            rows.row(tl("Pipeline"), |ui| match mr.pipelines.first() {
                Some(pipeline) => {
                    let (label, tone) = tones::pipeline(pipeline.status, tones);
                    let detail = pipeline_detail(pipeline);
                    ui.label(
                        RichText::new(format!(
                            "{} {label}{detail}",
                            pipeline_glyph(pipeline.status)
                        ))
                        .color(tone.fg),
                    );
                }
                None => {
                    ui.label(muted(&theme, tl("None")));
                }
            });
            rows.row(tl("Behind"), |ui| {
                match mr.diff_stats.as_ref().and_then(|s| s.commits_behind) {
                    Some(0) => {
                        ui.label(tl("Up to date"));
                    }
                    Some(n) => {
                        let tone = if n >= 10 { tones.red } else { tones.amber };
                        let behind = tn(
                            n as u64,
                            "{n} commit behind {branch}",
                            "{n} commits behind {branch}",
                        );
                        let behind = behind.replace("{branch}", &mr.target_branch);
                        ui.label(RichText::new(behind).color(tone.fg));
                    }
                    None => {
                        ui.label(muted(&theme, "—"));
                    }
                }
            });
            rows.row(tl("Threads"), |ui| {
                if mr.user_notes_count == 0 {
                    ui.label(muted(&theme, tl("None")));
                } else {
                    ui.label(
                        RichText::new(tf(
                            "{n} unresolved",
                            &[("n", &mr.user_notes_count.to_string())],
                        ))
                        .color(tones.amber.fg),
                    );
                }
            });
        });
    });
}

fn pipeline_glyph(state: PipelineState) -> &'static str {
    match state {
        PipelineState::Success => "●",
        PipelineState::Failed => "✕",
        PipelineState::Running | PipelineState::Pending | PipelineState::Created => "◌",
        _ => "–",
    }
}

/// " · test:integration" for a failed pipeline, " · 4 of 7 jobs" while it runs.
fn pipeline_detail(pipeline: &crate::models::Pipeline) -> String {
    match pipeline.status {
        PipelineState::Failed => pipeline
            .jobs
            .iter()
            .find(|j| j.status == "failed")
            .map(|j| format!(" · {}", j.name))
            .unwrap_or_default(),
        PipelineState::Running if !pipeline.jobs.is_empty() => {
            let done = pipeline
                .jobs
                .iter()
                .filter(|j| matches!(j.status.as_str(), "success" | "failed" | "skipped"))
                .count();
            let jobs = tf(
                "{done} of {total} jobs",
                &[
                    ("done", &done.to_string()),
                    ("total", &pipeline.jobs.len().to_string()),
                ],
            );
            format!(" · {jobs}")
        }
        _ => String::new(),
    }
}

/// Ten blocks, lit up to the effort score.
fn effort_blocks(ui: &mut Ui, theme: &Theme, score: f64, tone: Tone) {
    let lit = ((score.clamp(0.0, 1.0) * 10.0).ceil() as usize).max(1);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let gap = 3.0;
    let width = (rect.width() - gap * 9.0) / 10.0;
    for i in 0..10 {
        let block = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * (width + gap), rect.top()),
            Vec2::new(width, rect.height()),
        );
        let color = if i < lit {
            tone.fg
        } else {
            theme.palette.border_soft
        };
        ui.painter()
            .rect_filled(block, egui::CornerRadius::same(2), color);
    }
}

fn branches(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let rows = g.app.branch_rows(mr);
    let failed = matches!(mr.status, MrStatus::Error(_));
    let commits = mr.commits.len();
    section(ui, &theme, tl("Branches"), |ui| {
        for row in &rows {
            let (glyph, label, tone) = tones::presence(row.presence, failed, &theme.tones);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (name_rect, _) = ui.allocate_exact_size(Vec2::new(82.0, 18.0), Sense::hover());
                ui.painter().with_clip_rect(name_rect).text(
                    name_rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &row.branch,
                    egui::FontId::monospace(theme.text_size - 1.0),
                    theme.palette.text,
                );
                ui.add(Chip::new(label, tone).glyph(glyph));
                let detail = match row.presence {
                    Some(BranchPresence::Partial) if commits > 0 => tf(
                        "{n} of {total} commits missing",
                        &[
                            ("n", &row.missing.len().to_string()),
                            ("total", &commits.to_string()),
                        ],
                    ),
                    Some(BranchPresence::CherryPicked) => "cherry-pick -x".into(),
                    Some(BranchPresence::Copied) => tl("same author, date and title").into(),
                    _ => String::new(),
                };
                if !detail.is_empty() {
                    ui.add(egui::Label::new(muted(&theme, detail)).truncate());
                }
            });
            if row.presence == Some(BranchPresence::Partial) && !row.missing.is_empty() {
                egui::CollapsingHeader::new(muted(&theme, tl("Missing commits")))
                    .id_salt(("missing", &row.branch))
                    .show(ui, |ui| {
                        for commit in &row.missing {
                            ui.label(muted(&theme, format!("• {}", commit.fingerprint.title)));
                        }
                    });
            }
        }
    });
}

fn ticket(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let provider = g.app.tracker.as_ref().map(|t| t.name());
    super::widgets::rule(ui, &theme);
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                super::widgets::section_title(ui, &theme, tl("Tracker"));
                if let Some(provider) = provider {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(muted(&theme, provider));
                    });
                }
            });
            let Some(ticket) = &mr.linked_ticket else {
                ui.label(muted(
                    &theme,
                    tl("No ticket found in the MR's title, branch or description."),
                ));
                return;
            };
            ticket_card(ui, &theme, ticket);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if g.app.tracker.is_some()
                    && button(ui, &theme, Some(Icon::Plus), tl("Log time…"), true)
                        .on_hover_text(tl("Log time on this ticket (L)"))
                        .clicked()
                {
                    g.pending.push(Action::OpenLogTime);
                }
                if button(
                    ui,
                    &theme,
                    Some(Icon::ExternalLink),
                    tl("Open ticket"),
                    false,
                )
                .on_hover_text(tl("Open the ticket in the browser (T)"))
                .clicked()
                {
                    g.pending.push(Action::OpenSelectedTicket);
                }
            });
            if g.app.tracker.is_some() {
                let open = egui::CollapsingHeader::new(muted(&theme, tl("Time log")))
                    .id_salt("time_log")
                    .show(ui, |ui| time_log(g, ui, &ticket.id))
                    .fully_open();
                // The entries are fetched while the log is open, as in the TUI.
                g.app.tracker_view = if open {
                    TrackerView::TimeLog
                } else {
                    TrackerView::TicketInfo
                };
            }
        });
}

/// The ticket as a card: id and status, subject, progress and time, details.
pub fn ticket_card(ui: &mut Ui, theme: &Theme, ticket: &gitlab_tracker_core::LinkedTicket) {
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    mono(format!("#{}", ticket.id), theme.text_size - 1.0)
                        .color(theme.palette.accent),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.add(Chip::new(
                        &ticket.status,
                        tones::ticket_status(&ticket.status, &theme.tones),
                    ));
                    if let Some(kind) = &ticket.tracker_type {
                        ui.add(Chip::new(kind, theme.tones.gray));
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(semibold(&ticket.subject, theme.text_size));
            let fraction = match (ticket.done_ratio, ticket.time_spent, ticket.time_estimate) {
                (Some(done), _, _) => Some(done as f32 / 100.0),
                (None, Some(spent), Some(estimate)) if estimate > 0 => {
                    Some(spent as f32 / estimate as f32)
                }
                _ => None,
            };
            let time = match (ticket.time_spent, ticket.time_estimate) {
                (None, None) => None,
                (spent, estimate) => Some(format!(
                    "{} / {}",
                    spent.map_or("—".into(), duration),
                    estimate.map_or("—".into(), duration)
                )),
            };
            if fraction.is_some() || time.is_some() {
                ui.add_space(12.0);
                let over = matches!(
                    (ticket.time_spent, ticket.time_estimate),
                    (Some(s), Some(e)) if e > 0 && s > e
                );
                let color = if over {
                    theme.tones.red.fg
                } else {
                    theme.palette.accent
                };
                ui.horizontal(|ui| {
                    let size = theme.text_size - 1.5;
                    if let Some(time) = &time {
                        ui.label(muted(theme, tl("Spent / estimated")).size(size));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(time)
                                    .font(fonts::medium(size))
                                    .color(if over { color } else { theme.palette.text }),
                            );
                        });
                    } else if let Some(fraction) = fraction {
                        ui.label(muted(theme, tl("Done")).size(size));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{:.0}%", fraction * 100.0))
                                    .font(fonts::medium(size)),
                            );
                        });
                    }
                });
                if let Some(fraction) = fraction {
                    ui.add_space(6.0);
                    bar(ui, theme, fraction, color, 5.0);
                }
            }
            let details: Vec<(&str, &String)> = [
                ("Priority", ticket.priority.as_ref()),
                ("Version", ticket.version.as_ref()),
                ("Assignee", ticket.assignee.as_ref()),
            ]
            .into_iter()
            .filter_map(|(caption, value)| value.map(|v| (caption, v)))
            .collect();
            if !details.is_empty() {
                ui.add_space(12.0);
                super::widgets::rule(ui, theme);
                ui.add_space(10.0);
                key_values(ui, theme, |rows| {
                    for (caption, value) in details {
                        rows.row(tl(caption), |ui| {
                            if caption == "Assignee" {
                                ui.spacing_mut().interact_size.y = 18.0;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    avatar(ui, theme, value, 18.0);
                                    ui.label(value);
                                });
                            } else {
                                ui.label(value);
                            }
                        });
                    }
                });
            }
        });
}

/// "1h 30m" for a number of seconds.
pub fn duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

fn time_log(g: &GuiApp, ui: &mut Ui, ticket_id: &str) {
    let theme = g.theme;
    match g.app.time_entries.get(ticket_id) {
        None | Some(TimeLogState::Loading) => {
            ui.horizontal(|ui| {
                spinner(
                    ui,
                    ui.spacing().interact_size.y,
                    ui.visuals().strong_text_color(),
                );
                ui.label(muted(&theme, tl("Loading the time entries…")));
            });
        }
        Some(TimeLogState::Failed(error)) => {
            ui.label(RichText::new(error).color(theme.tones.red.fg));
        }
        Some(TimeLogState::Loaded(entries)) if entries.is_empty() => {
            ui.label(muted(&theme, tl("No time logged yet.")));
        }
        Some(TimeLogState::Loaded(entries)) => {
            let total: f32 = entries.iter().map(|e| e.hours).sum();
            ui.label(muted(
                &theme,
                format!(
                    "{total:.2} h · {}",
                    tn(entries.len() as u64, "{n} entry", "{n} entries")
                ),
            ));
            for entry in entries {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        mono(&entry.spent_on, theme.text_size - 1.5)
                            .color(theme.palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!("{:.2} h", entry.hours))
                            .font(fonts::medium(theme.text_size - 1.0)),
                    );
                    ui.label(muted(
                        &theme,
                        format!("{} · {}", entry.user, entry.activity.name),
                    ));
                });
                if !entry.comment.trim().is_empty() {
                    ui.label(
                        RichText::new(entry.comment.trim()).color(theme.palette.text_secondary),
                    );
                }
            }
        }
    }
}

/// The unresolved threads; a click opens one in GitLab.
fn threads(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    let open: Vec<_> = mr.threads.iter().filter(|t| !t.resolved).collect();
    if open.is_empty() {
        return;
    }
    section(ui, theme, tl("Unresolved threads"), |ui| {
        for thread in open {
            let response = egui::Frame::new()
                .fill(theme.palette.content)
                .stroke(Stroke::new(1.0, theme.palette.border_soft))
                .corner_radius(egui::CornerRadius::same(theme.radii.medium))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        icons::paint(ui.painter(), rect, Icon::Comment, theme.tones.amber.fg);
                        ui.label(
                            RichText::new(format!("@{}", thread.author))
                                .font(fonts::medium(theme.text_size - 1.0)),
                        );
                        ui.label(muted(
                            theme,
                            format!(
                                "{} · {}",
                                tn(thread.notes as u64, "{n} note", "{n} notes"),
                                format_relative_date(&thread.last_note_at)
                            ),
                        ));
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(&thread.excerpt).color(theme.palette.text_secondary),
                        )
                        .truncate(),
                    );
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tl("Open the thread in GitLab"));
            if response.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                    "{}#note_{}",
                    mr.web_url, thread.note_id
                )));
            }
        }
    });
}

/// The latest pipelines; the jobs of the newest one by stage.
fn pipelines(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    section(ui, theme, tl("Pipelines"), |ui| {
        if mr.pipelines.is_empty() {
            ui.label(muted(theme, tl("No pipeline yet.")));
            return;
        }
        for pipeline in mr.pipelines.iter().take(5) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    mono(format!("#{}", pipeline.id), theme.text_size - 1.5)
                        .color(theme.palette.text_secondary),
                );
                let (label, tone) = tones::pipeline(pipeline.status, &theme.tones);
                ui.add(Chip::new(label, tone));
                if let Some(at) = &pipeline.created_at {
                    ui.label(muted(theme, format_relative_date(at)));
                }
            });
        }
        let Some(latest) = mr.pipelines.first() else {
            return;
        };
        if latest.jobs.is_empty() {
            return;
        }
        ui.add_space(4.0);
        let mut stages: Vec<&str> = Vec::new();
        for job in &latest.jobs {
            if !stages.contains(&job.stage.as_str()) {
                stages.push(&job.stage);
            }
        }
        for stage in stages {
            ui.label(semibold(stage, theme.text_size - 2.0).color(theme.palette.text_muted));
            for job in latest.jobs.iter().filter(|j| j.stage == stage) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    dot(ui, job_color(theme, &job.status), 6.0);
                    ui.add(
                        egui::Label::new(RichText::new(&job.name).size(theme.text_size - 1.0))
                            .truncate(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let text = match job.duration {
                            Some(secs) => duration(secs.round() as u32).replace("0m", "<1m"),
                            None => job.status.clone(),
                        };
                        ui.label(muted(theme, text));
                    });
                });
            }
        }
    });
}

fn job_color(theme: &Theme, status: &str) -> egui::Color32 {
    let t = &theme.tones;
    match status {
        "success" => t.green.fg,
        "failed" => t.red.fg,
        "running" => t.orange.fg,
        "pending" | "created" | "manual" => t.amber.fg,
        _ => theme.palette.text_muted,
    }
}

fn people(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let me = g.app.config.gitlab_username.clone();
    // An initials avatar and the name, the handle on hover; me stands out, as
    // in the TUI.
    let person = |ui: &mut Ui, display: &str| {
        let is_me = me
            .as_deref()
            .is_some_and(|username| matches_gitlab_username(display, username));
        let (name, handle) = split_person(display);
        // `horizontal` is at least `interact_size.y` tall (the theme's 28-point
        // control height): keep it to the avatar's, as the text rows around.
        ui.spacing_mut().interact_size.y = 18.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let response = avatar(ui, &theme, name, 18.0);
            let mut text = RichText::new(name).font(fonts::medium(theme.text_size - 0.5));
            if is_me {
                text = text.color(theme.tones.blue.fg);
            }
            let response = response | ui.label(text);
            if is_me {
                ui.add(Chip::new(tl("you"), theme.tones.blue).pill());
            }
            if let Some(handle) = handle {
                response.on_hover_text(format!("@{handle}"));
            }
        });
    };
    section(ui, &theme, tl("People"), |ui| {
        key_values(ui, &theme, |rows| {
            rows.row(tl("Author"), |ui| person(ui, &mr.author));
            rows.row(tl("Assignee"), |ui| match &mr.assignee {
                Some(assignee) => person(ui, assignee),
                None => {
                    ui.label(muted(&theme, "—"));
                }
            });
            rows.row(tl("Reviewers"), |ui| {
                if mr.reviewers.is_empty() {
                    ui.label(muted(&theme, tl("None")));
                    return;
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for reviewer in &mr.reviewers {
                        person(ui, reviewer);
                    }
                });
            });
        });
    });
}

fn planning(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    let tones = &theme.tones;
    section(ui, theme, tl("Planning"), |ui| {
        key_values(ui, theme, |rows| {
            rows.row(tl("Milestone"), |ui| match &mr.milestone {
                Some(milestone) => {
                    ui.label(RichText::new(milestone).color(theme.palette.accent));
                }
                None => {
                    ui.label(muted(theme, "—"));
                }
            });
            rows.row(tl("Due"), |ui| match mr.milestone_due_date.as_deref() {
                None | Some("") => {
                    ui.label(muted(theme, tl("Not set")));
                }
                Some(date) => {
                    // Red when past, amber within a week (YYYY-MM-DD compares as text).
                    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let soon = (chrono::Utc::now() + chrono::Duration::days(7))
                        .format("%Y-%m-%d")
                        .to_string();
                    let color = if date < today.as_str() {
                        tones.red.fg
                    } else if date <= soon.as_str() {
                        tones.amber.fg
                    } else {
                        tones.green.fg
                    };
                    let relative = format_relative_date(&format!("{date}T00:00:00Z"));
                    ui.label(RichText::new(format!("{date} ({relative})")).color(color));
                }
            });
        });
        if let Some(description) = mr
            .milestone_description
            .as_deref()
            .filter(|d| !d.is_empty())
        {
            ui.label(muted(theme, description));
        }
    });
}

/// "date time (relative)" for an ISO timestamp.
fn when(iso: Option<&str>) -> String {
    match iso {
        Some(s) => format!(
            "{} ({})",
            s.get(..16).unwrap_or(s).replace('T', " "),
            format_relative_date(s)
        ),
        None => tl("Unknown").into(),
    }
}

fn history(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    section(ui, theme, tl("History"), |ui| {
        key_values(ui, theme, |rows| {
            rows.row(tl("Created"), |ui| {
                ui.label(when(mr.created_at.as_deref()));
            });
            rows.row(tl("Updated"), |ui| {
                ui.label(when(mr.updated_at.as_deref()));
            });
            if mr.state == GitlabMrState::Merged {
                rows.row(tc("date", "Merged"), |ui| {
                    let by = mr.merged_by.as_deref().unwrap_or(tl("Unknown"));
                    let at = when(mr.merged_at.as_deref());
                    ui.label(tf("{when} by {name}", &[("when", &at), ("name", by)]));
                });
            }
            rows.row(tl("Commit"), |ui| {
                let sha = mr
                    .sha
                    .as_deref()
                    .map(|s| s.get(..8).unwrap_or(s))
                    .unwrap_or("—");
                ui.label(mono(sha, theme.text_size - 1.0));
            });
            rows.row(tl("Branch"), |ui| {
                ui.label(mono(&mr.source_branch, theme.text_size - 1.5));
            });
        });
    });
}
