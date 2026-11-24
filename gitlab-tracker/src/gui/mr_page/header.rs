//! The page's top: breadcrumbs, title, branch chips, facts and the "behind" popup.

use super::*;

/// Back to the list, where this MR is in it, and the keys to move.
pub(super) fn crumbs(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
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
            .rich(Tip::new().mr(id, title));
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

/// A key's cap, as the shortcuts show it.
pub(in crate::gui) fn key(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.label(mono(text, theme.text_size - 2.0).color(theme.palette.text_secondary));
        });
}

pub(super) fn header(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
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
                .rich(Tip::new().mono(tl("git clone -b <source branch> <ssh url>")))
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

pub(super) fn branch_chip(ui: &mut Ui, theme: &Theme, name: &str, target: bool) {
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
/// The MR's figures in a band. Returns the MR picked in the "behind" popup,
/// and the section of the page a figure leads to when clicked (the pipelines,
/// the threads, the approvals, the changes, the planning).
pub(super) fn facts(
    g: &GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    data: Option<&PageData>,
) -> (Option<String>, Option<&'static str>) {
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
            // The number of rules is above it: the word in its form.
            tn(rules.len() as u64, "rule met", "rules met"),
        ),
        None => (
            mr.approved_by.len().to_string(),
            // The number is above it: the word alone, in the number's form.
            tn(mr.approved_by.len() as u64, "approval", "approvals"),
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
    let has_rules = data.and_then(|d| d.rules.as_ref()).is_some();
    let target = |name: &str| -> Option<&'static str> {
        if name == tl("Pipeline") {
            Some(Tab::Pipelines.id())
        } else if name == tl("Threads") {
            Some(Tab::Discussions.id())
        } else if name == tl("Approvals") && has_rules {
            Some("approvals")
        } else if name == tl("Size") {
            Some(Tab::Changes.id())
        } else if name == tl("Effort") {
            Some("planning")
        } else {
            None
        }
    };
    let mut open = None;
    let mut jump = None;
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
                    let to = target(name);
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
                                    .selectable(!link && to.is_none()),
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
                    } else if let Some(to) = to {
                        // A shortcut to the section it sums up.
                        let response = frame
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.hovered() {
                            ui.painter().rect_stroke(
                                response.rect,
                                CornerRadius::ZERO,
                                egui::Stroke::new(1.0, theme.palette.accent.gamma_multiply(0.5)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            jump = Some(to);
                        }
                    }
                }
            });
        });
    (open, jump)
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
                            super::super::sidebar::behind_row(g, ui, other, &mr.source_branch, WIDTH)
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
        super::super::widgets::rule(ui, &theme);
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
            .rich(Tip::new().mono(&url))
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
pub(super) fn commit_row(
    g: &GuiApp,
    ui: &mut Ui,
    commit: &crate::gitlab_detail::BehindCommit,
    width: f32,
) {
    let theme = g.theme;
    let p = &theme.palette;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::click());
    let reference = match &commit.mr_iid {
        Some(iid) => Tip::new().mr(iid, &commit.title),
        None => Tip::new().item(
            commit.sha.get(..8).unwrap_or(&commit.sha).to_string(),
            &commit.title,
        ),
    };
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(reference.text(&commit.author).hint(tl("Open in GitLab")));
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
pub(super) fn unresolved(mr: &TrackedMr, data: Option<&PageData>) -> usize {
    match data.and_then(|d| d.threads.as_ref().ok()) {
        Some(threads) => threads.iter().filter(|t| t.is_open()).count(),
        None => mr.user_notes_count as usize,
    }
}

// ── Why it can't merge ───────────────────────────────────────────────────────
