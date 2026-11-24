//! The right column's timeline, approvals, related MRs and how it compares.

use super::*;

/// The MR's life, newest first, shared by the MR page's Timeline and the MR
/// list's History: the steps still to come (the merge of an open MR, the
/// branches up to production), then (oldest last) review requests and
/// reviews, pushes, pipelines, milestone changes, approval and merge or
/// close, the branches its changes came down to branching off just above the
/// merge. `data` (the MR page's extra reads) only gives fuller names.
pub(in crate::gui) fn timeline_steps(
    g: &GuiApp,
    mr: &TrackedMr,
    data: Option<&PageData>,
) -> Vec<Step> {
    let t = &g.theme.tones;
    let mut dated: Vec<(String, Step)> = Vec::new();
    let mut add = |at: Option<&str>, title: String, by: Option<&str>, color: egui::Color32| {
        if let Some(at) = at {
            let when = format_relative_date(at);
            let moment = chrono::DateTime::parse_from_rfc3339(at)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc));
            let detail = match by {
                Some(name) => tf("{when} by {name}", &[("when", &when), ("name", name)]),
                None => when,
            };
            dated.push((
                at.to_string(),
                Step {
                    title,
                    detail: Some(detail),
                    color,
                    pending: false,
                    side: false,
                    at: moment,
                },
            ));
        }
    };
    add(
        mr.created_at.as_deref(),
        tl("Created").into(),
        None,
        t.blue.fg,
    );
    if let Some(review) = &mr.review {
        for (who, at) in &review.review_requested_at {
            let display = display_of(mr, data, who);
            let name = split_person(&display).0;
            add(
                Some(at),
                tf("Review requested from {who}", &[("who", name)]),
                None,
                t.gray.fg,
            );
        }
        add(
            review.ready_at.as_deref(),
            tl("Marked ready for review").into(),
            None,
            t.teal.fg,
        );
        add(
            review.first_review_at.as_deref(),
            tl("First review").into(),
            None,
            t.teal.fg,
        );
        add(
            review.last_push_at.as_deref(),
            tl("Last push").into(),
            None,
            t.blue.fg,
        );
        // One entry per approval still standing, by whom; without them (an
        // MR read before they were kept), the last approval alone.
        if review.approvals.is_empty() {
            let by = match mr.approved_by.as_slice() {
                [login] => Some(display_of(mr, data, login)),
                _ => None,
            };
            add(
                review.approved_at.as_deref(),
                tl("Approved").into(),
                by.as_deref().map(|d| split_person(d).0),
                t.green.fg,
            );
        }
        for (login, at) in &review.approvals {
            let display = display_of(mr, data, login);
            add(
                Some(at),
                tl("Approved").into(),
                Some(split_person(&display).0),
                t.green.fg,
            );
        }
    }
    for p in &mr.pipelines {
        let (label, tone) = tones::pipeline(p.status, t);
        add(
            p.created_at.as_deref(),
            tf(
                "Pipeline #{id} · {status}",
                &[("id", &p.id.to_string()), ("status", label)],
            ),
            None,
            tone.fg,
        );
    }
    match mr.state {
        GitlabMrState::Merged => {
            let by = mr.merged_by.as_deref().unwrap_or(tl("Unknown"));
            let title = match mr.target_branch.as_str() {
                "" | "unknown" => tf("Merged by {who}", &[("who", by)]),
                target => tf(
                    "Merged into {branch} by {who}",
                    &[("branch", target), ("who", by)],
                ),
            };
            add(mr.merged_at.as_deref(), title, None, t.purple.fg);
        }
        // GitLab's close date is not kept: the last update stands for it.
        GitlabMrState::Closed => add(
            mr.updated_at.as_deref(),
            tc("date", "Closed").into(),
            None,
            t.gray.fg,
        ),
        GitlabMrState::Opened => {}
    }
    for (at, change, by) in milestone_changes(mr.milestone_events.as_deref().unwrap_or_default()) {
        let title = match change {
            MilestoneChange::Set(m) => tf("Milestone set: {milestone}", &[("milestone", m)]),
            MilestoneChange::Changed(old, new) => tf(
                "Milestone changed: {old} → {new}",
                &[("old", old), ("new", new)],
            ),
            MilestoneChange::Removed(m) => {
                tf("Milestone removed: {milestone}", &[("milestone", m)])
            }
        };
        let name = by.map(|by| split_person(by).0);
        add(Some(at), title, name, t.orange.fg);
    }
    // Built oldest first, then turned over: the newest on top, the steps
    // still to come above all.
    dated.sort_by(|a, b| a.0.cmp(&b.0));
    if mr.state != GitlabMrState::Merged {
        let mut steps: Vec<Step> = dated.into_iter().map(|(_, s)| s).collect();
        if mr.state == GitlabMrState::Opened {
            steps.push(Step {
                title: tl("Merge").into(),
                detail: Some(tl("not merged yet").into()),
                color: t.gray.fg,
                pending: true,
                side: false,
                at: None,
            });
        }
        steps.reverse();
        return steps;
    }
    let (sync, promotion) = super::super::inspector::flow_steps(g, mr);
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

pub(super) fn approvals(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
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
                            let mut hover =
                                Tip::new().person(&format!("{} (@{})", user.name, user.username));
                            if approved {
                                hover = hover.dot(
                                    theme.tones.green.fg,
                                    tf("{who} approved", &[("who", &user.name)]),
                                );
                            }
                            response.rich(hover);
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
pub(super) fn person_chip(ui: &mut Ui, theme: &Theme, display: &str) {
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
    let mut list = app
        .mrs
        .iter()
        .filter(|other| other.id != mr.id && !other.title.is_empty())
        .filter_map(|other| {
            let reverts = |a: &TrackedMr, b: &TrackedMr| {
                super::super::delivery::reverted(app, a)
                    .is_some_and(|(undone, _)| undone.id == b.id)
            };
            let why = if reverts(other, mr) {
                tl("reverts this MR").to_string()
            } else if reverts(mr, other) {
                tl("reverted by this MR").to_string()
            } else if ticket.is_some()
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
        .collect::<Vec<_>>();
    // A revert first: it says the most about the MR.
    list.sort_by_key(|(_, why)| *why != tl("reverts this MR") && *why != tl("reverted by this MR"));
    list
}

pub(super) fn related(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
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
pub(super) fn days(from: Option<&str>, to: Option<&str>) -> Option<f64> {
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok();
    let from = parse(from?)?;
    let to = match to {
        Some(to) => parse(to)?,
        None => chrono::Utc::now().fixed_offset(),
    };
    Some((to - from).num_minutes() as f64 / 1440.0)
}

pub(super) fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[values.len() / 2])
}

/// "2.1 d", or hours under a day.
pub(super) fn span(days: f64) -> String {
    if days < 1.0 {
        format!("{:.0} h", days * 24.0)
    } else {
        format!("{days:.1} d")
    }
}

/// How this MR compares with the other tracked ones: its wait for a first review,
/// its age (or cycle time), its size.
pub(super) fn compared(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    // From up for review (out of draft, else the request): the writing of
    // an MR opened as a draft is not a wait.
    let first_review = |m: &TrackedMr| {
        let review = m.review.as_ref()?;
        days(
            review.up_for_review_at().or(m.created_at.as_deref()),
            review.first_review_at.as_deref(),
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
