//! The right column: tiles, where it is, people, ticket, context, milestone changes.

use super::*;

/// Where the MR is (branches and deployments), who is on it, and its ticket:
/// short states, side by side under the figures (stacked when narrow).
pub(super) fn tiles(
    g: &mut GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    data: Option<&PageData>,
    width: f32,
) {
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

pub(super) fn tile_title(ui: &mut Ui, theme: &Theme, title: &str) {
    ui.label(muted(theme, title.to_uppercase()).size(theme.text_size - 2.5));
    ui.add_space(6.0);
}

/// Each tracked branch: whether the MR is on it, and the environment deployed
/// from it, with whether that deployment includes the MR.
pub(super) fn where_it_is(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
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
                ui.add(
                    egui::Label::new(mono(&row.branch, theme.text_size - 1.5))
                        .truncate()
                        .show_tooltip_when_elided(false),
                )
                .rich(Tip::new().mono(&row.branch));
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
pub(in crate::gui) fn deployment_line(
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
pub(super) fn people(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
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
        let response = response
            | ui.add(
                egui::Label::new(text)
                    .truncate()
                    .show_tooltip_when_elided(false),
            );
        if handle.is_some() {
            response.rich(Tip::new().person(display));
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
        if mr.reviewers.is_empty() {
            ui.label(muted(&theme, tl("None")));
        }
        // One reviewer by name, spaced as the rows around; more as avatars
        // drawn closer, names on hover.
        if mr.reviewers.len() == 1 {
            person(ui, &mr.reviewers[0]);
        } else {
            ui.spacing_mut().item_spacing.x = 4.0;
            for reviewer in &mr.reviewers {
                let (name, _) = split_person(reviewer);
                let approved = mr
                    .approved_by
                    .iter()
                    .any(|a| reviewer.contains(&format!("@{a}")));
                let mut hover = Tip::new().person(reviewer);
                if approved {
                    hover = hover.dot(theme.tones.green.fg, tf("{who} approved", &[("who", name)]));
                }
                avatar(ui, &theme, name, 20.0).rich(hover);
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
                    .truncate()
                    .show_tooltip_when_elided(false),
                );
                let response = response.map_or(label.clone(), |r| r | label);
                response.rich(several.iter().fold(Tip::new(), |tip, d| tip.person(d)));
            }
        }
    });
}

/// The width `text` takes in `font`.
pub(super) fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font, egui::Color32::WHITE)
        .size()
        .x
}

/// A column `width` wide, its content left-aligned and kept to it.
pub(super) fn fixed(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
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
pub(super) fn ticket(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) -> Option<Action> {
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
                    .tip(tl("Open the ticket"));
                if id.clicked() {
                    pick = Some(Action::OpenSelectedTicket);
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(&ticket.subject).font(fonts::medium(theme.text_size)),
                    )
                    .truncate()
                    .show_tooltip_when_elided(false),
                )
                .rich(
                    Tip::new()
                        .ticket(&ticket.id, &ticket.subject)
                        .chip(&ticket.status, theme.tones.blue),
                );
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
            super::super::widgets::bar(ui, &theme, fraction.min(1.0), color, 6.0);
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
                .tip(tl("Log time on this ticket (L)"))
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
            .rich(Tip::plain(tl("Open the ticket")).mono(&ticket.url))
            .clicked()
            {
                pick = Some(Action::OpenSelectedTicket);
            }
        });
    }
    pick
}

// ── Right: the context ───────────────────────────────────────────────────────

pub(super) fn context(
    g: &mut GuiApp,
    ui: &mut Ui,
    jumps: &mut super::super::jump::Jumps,
    mr: &TrackedMr,
    data: Option<&PageData>,
) {
    let theme = g.theme;
    // The gap under a card, when there is one: some have nothing to show.
    let gap = |ui: &mut Ui, drew: bool| {
        if drew {
            ui.add_space(12.0);
        }
    };
    jumps.section(ui, "timeline", |ui| {
        side_card(ui, &theme, tl("Timeline"), |ui| {
            inspector::timeline(ui, &theme, &timeline_steps(g, mr, data))
        })
    });
    gap(ui, jumps.drew());
    jumps.section(ui, "approvals", |ui| approvals(g, ui, mr, data));
    gap(ui, jumps.drew());
    jumps.section(ui, "related", |ui| related(g, ui, mr));
    gap(ui, jumps.drew());
    let drew = ui.scope(|ui| compared(g, ui, mr)).response.rect.height() > 0.5;
    gap(ui, drew);
    jumps.section(ui, "planning", |ui| {
        side_card(ui, &theme, tl("Planning"), |ui| {
            inspector::planning_body(ui, &theme, mr)
        })
    });
    ui.add_space(12.0);
}

/// A block of the right column: a tile of its own, titled like the tiles above.
pub(super) fn side_card(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui)) {
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
}

/// A milestone change as the timeline tells it, oldest first: when, what
/// (`set`, `changed from`, `removed`) and by whom. GitLab records a change as
/// the `add` of the new milestone, and some versions as a `remove` then an
/// `add` at the same moment: both read as one change.
#[derive(Debug, PartialEq)]
pub(in crate::gui) enum MilestoneChange<'a> {
    Set(&'a str),
    Changed(&'a str, &'a str),
    Removed(&'a str),
}

pub(in crate::gui) fn milestone_changes(
    events: &[MilestoneEvent],
) -> Vec<(&str, MilestoneChange<'_>, Option<&str>)> {
    const DELETED: &str = "?";
    let mut changes = Vec::new();
    let mut current: Option<&str> = None;
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        let title = e.milestone.as_deref().unwrap_or(DELETED);
        let next = events.get(i + 1);
        if !e.added {
            // A remove then an add at the same moment: one change.
            if let Some(n) = next.filter(|n| n.added && n.at == e.at) {
                let new = n.milestone.as_deref().unwrap_or(DELETED);
                changes.push((
                    n.at.as_str(),
                    MilestoneChange::Changed(title, new),
                    n.by.as_deref(),
                ));
                current = Some(new);
                i += 2;
                continue;
            }
            changes.push((
                e.at.as_str(),
                MilestoneChange::Removed(title),
                e.by.as_deref(),
            ));
            current = None;
        } else {
            let change = match current {
                Some(old) if old != title => MilestoneChange::Changed(old, title),
                _ => MilestoneChange::Set(title),
            };
            changes.push((e.at.as_str(), change, e.by.as_deref()));
            current = Some(title);
        }
        i += 1;
    }
    changes
}
