//! The tracked branches: their roles, compare links, what each is behind.

use super::*;

/// The tracked branches (one column each in the table): remove one, or add one.
pub(super) fn branches(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    section_title(ui, &theme, tl("Tracked branches"));
    ui.add_space(4.0);
    let mut removed = None;
    let mut moved = None;
    let mut flow_change = None;
    let mut open = None;
    let count = g.app.branches.len();
    let flow = g.app.branch_flow();
    let project = project_url(g);
    for (i, branch) in g.app.branches.iter().enumerate() {
        let row = ui.horizontal(|ui| {
            ui.set_height(24.0);
            dot(ui, theme.tones.green.fg, 6.0);
            ui.label(mono(branch, theme.text_size - 1.0));
            if let Some(fb) = flow.get(branch) {
                flow_change = flow_change
                    .take()
                    .or_else(|| role_chip(g, ui, i, fb, &flow, project.as_deref()));
            }
        });
        // Under it, the merges it still has to take in, when there are some.
        open = open
            .take()
            .or_else(|| behind(g, ui, branch, project.as_deref()));
        // The controls show on hover: the order is the table's columns.
        if !ui.rect_contains_pointer(row.response.rect) {
            continue;
        }
        let mut controls = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row.response.rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        let small = |ui: &mut Ui, text: &str, hint: &str| {
            ui.add(egui::Button::new(muted(&theme, text)).frame(false))
                .tip(hint)
                .clicked()
        };
        if !g.app.read_only && small(&mut controls, "✕", tl("Stop tracking this branch")) {
            removed = Some(branch.clone());
        }
        if i + 1 < count && small(&mut controls, "↓", tl("Move right in the table")) {
            moved = Some((i, i + 1));
        }
        if i > 0 && small(&mut controls, "↑", tl("Move left in the table")) {
            moved = Some((i, i - 1));
        }
    }
    if let Some(branch) = removed {
        g.pending.push(Action::SubmitInput(format!("-{branch}")));
    }
    if let Some((from, to)) = moved {
        g.pending.push(Action::MoveBranch { from, to });
    }
    if let Some((branch, setting)) = flow_change {
        g.pending.push(Action::SetBranchFlow { branch, setting });
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
    if g.app.read_only {
        return;
    }
    ui.add_space(4.0);
    let id = ui.make_persistent_id("new_branch");
    let mut draft: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    let response = ui.add(
        egui::TextEdit::singleline(&mut draft)
            .hint_text(RichText::new(tl("Add a branch, then Enter")))
            .margin(Margin::symmetric(8, 5))
            .desired_width(f32::INFINITY),
    );
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let name = draft.trim().trim_start_matches('-').to_string();
        // A number would be read as an MR to track.
        if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
            g.pending.push(Action::SubmitInput(name));
        }
        draft.clear();
    }
    ui.data_mut(|d| d.insert_temp(id, draft));
}

/// What a tracked branch is for, as a chip: a click sets its role and, for a
/// feature branch, the branch it is fed by. A guessed role shows muted.
/// Returns the new setting of the branch when one was picked.
pub(super) fn role_chip(
    g: &GuiApp,
    ui: &mut Ui,
    index: usize,
    branch: &FlowBranch,
    flow: &BranchFlow,
    project: Option<&str>,
) -> Option<(String, BranchSetting)> {
    let theme = g.theme;
    let t = &theme.tones;
    let tone = match branch.role {
        BranchRole::Feature => t.teal,
        BranchRole::Integration => t.blue,
        BranchRole::Staging => t.amber,
        BranchRole::Production => t.purple,
    };
    let tone = if branch.guessed { t.gray } else { tone };
    let chip =
        ui.add(Chip::new(tl(branch.role.label()), tone).font(fonts::medium(theme.text_size - 3.0)));
    let response = ui
        .interact(
            chip.rect,
            ui.make_persistent_id(("branch_role", index)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hint = match (&branch.parent, branch.guessed) {
        (Some(parent), true) => tf(
            "Guessed from the name, fed by {parent}: click to set",
            &[("parent", parent)],
        ),
        (Some(parent), false) => tf("Fed by {parent}: click to change", &[("parent", parent)]),
        (None, true) => tl("Guessed from the name: click to set").to_string(),
        (None, false) => tl("Click to change").to_string(),
    };
    let response = response.tip(hint);
    if g.app.read_only {
        return None;
    }
    let saved = g
        .app
        .project_settings
        .branch_flow
        .as_ref()
        .and_then(|s| s.get(&branch.name))
        .cloned()
        .unwrap_or_default();
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(180.0);
        ui.label(muted(&theme, tl("Role in the flow")));
        for role in BranchRole::ALL {
            let on = !branch.guessed && branch.role == role;
            if ui.selectable_label(on, tl(role.label())).clicked() {
                picked = Some(BranchSetting {
                    role: Some(role),
                    ..saved.clone()
                });
            }
        }
        if ui
            .selectable_label(branch.guessed, tl("Guess from the name"))
            .clicked()
        {
            picked = Some(BranchSetting {
                role: None,
                ..saved.clone()
            });
        }
        if let (Some(parent), Some(project)) = (&branch.parent, project) {
            ui.separator();
            compare_link(ui, project, &branch.name, parent);
        }
        if branch.role != BranchRole::Feature {
            return;
        }
        ui.separator();
        ui.label(muted(&theme, tl("Fed by")));
        for other in flow.branches.iter().filter(|b| b.name != branch.name) {
            let on = branch.parent.as_deref() == Some(other.name.as_str());
            if ui
                .selectable_label(on, mono(&other.name, theme.text_size - 1.0))
                .clicked()
            {
                picked = Some(BranchSetting {
                    parent: Some(other.name.clone()),
                    ..saved.clone()
                });
            }
        }
    });
    picked.map(|setting| (branch.name.clone(), setting))
}

/// "https://host/group/project", from the web URL of a tracked MR.
pub(super) fn project_url(g: &GuiApp) -> Option<String> {
    g.app.mrs.iter().find_map(|mr| {
        mr.web_url
            .split_once("/-/merge_requests")
            .map(|(project, _)| project.to_string())
    })
}

/// GitLab's comparison of `branch` with `source`: the commits of `source`
/// not in `branch` yet, tracked MRs or not.
pub(super) fn compare_url(project: &str, branch: &str, source: &str) -> String {
    format!("{project}/-/compare/{branch}...{source}")
}

pub(super) fn compare_link(ui: &mut Ui, project: &str, branch: &str, source: &str) {
    let text = format!(
        "{} ↗",
        tf("Compare with {source} on GitLab", &[("source", source)])
    );
    if ui
        .button(text)
        .rich(Tip::new().mono(compare_url(project, branch, source)))
        .clicked()
    {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab(compare_url(project, branch, source)));
        ui.close();
    }
}

/// The tracked MRs merged upstream of `branch` and not in it yet, as a line
/// under it: a click lists them, each opening its page, with the comparison
/// on GitLab for what the tracking does not see. Returns the MR to open.
pub(super) fn behind(
    g: &GuiApp,
    ui: &mut Ui,
    branch: &str,
    project: Option<&str>,
) -> Option<String> {
    let theme = g.theme;
    let mrs = g.app.behind_on(branch);
    if mrs.is_empty() {
        return None;
    }
    let text = tn(
        mrs.len() as u64,
        "↓ {n} MR to bring in",
        "↓ {n} MRs to bring in",
    );
    let line = ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .size(theme.text_size - 2.0)
                    .color(theme.tones.amber.fg),
            )
            .sense(Sense::click()),
        )
    });
    let response = line
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tf(
            "Merged upstream, not in {branch} yet: rebase it, or merge into it",
            &[("branch", branch)],
        ));
    let mut open = None;
    egui::Popup::menu(&response).show(|ui| {
        const WIDTH: f32 = 400.0;
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 2.0;
        // Header: how many, into which branch, and what to do.
        ui.horizontal(|ui| {
            ui.label(semibold(
                tn(
                    mrs.len() as u64,
                    "{n} MR to bring in",
                    "{n} MRs to bring in",
                ),
                theme.text_size,
            ));
            ui.add(
                Chip::new(branch, theme.tones.amber)
                    .font(egui::FontId::monospace(theme.text_size - 2.5)),
            );
        });
        ui.label(
            muted(
                &theme,
                tl("Merged upstream, not in this branch yet: rebase it, or merge into it."),
            )
            .size(theme.text_size - 1.5),
        );
        ui.add_space(6.0);
        for mr in &mrs {
            if let Some(id) = behind_row(g, ui, mr, branch, WIDTH) {
                open = Some(id);
                ui.close();
            }
        }
        let Some(project) = project else {
            return;
        };
        ui.add_space(6.0);
        rule(ui, &theme);
        ui.add_space(6.0);
        let mut sources: Vec<&str> = mrs.iter().map(|mr| mr.target_branch.as_str()).collect();
        sources.sort_unstable();
        sources.dedup();
        ui.horizontal_wrapped(|ui| {
            for source in sources {
                let text = tf("Compare with {source} on GitLab", &[("source", source)]);
                let url = compare_url(project, branch, source);
                if button(ui, &theme, Some(Icon::ExternalLink), &text, false)
                    .rich(Tip::new().mono(&url))
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
        ui.label(
            muted(
                &theme,
                tl("Only the tracked MRs are listed; GitLab compares every commit."),
            )
            .size(theme.text_size - 2.5),
        );
    });
    open
}

/// One MR of the "to bring in" list: its author, `!iid` and title, where and
/// when it was merged, and how much of it `branch` lacks. A click opens its
/// page (returns its id); the arrow opens it on GitLab.
pub(in crate::gui) fn behind_row(
    g: &GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    branch: &str,
    width: f32,
) -> Option<String> {
    let theme = g.theme;
    let p = &theme.palette;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(Tip::new().mr(&mr.id, &mr.title).hint(tl("Open its page")));
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
    let name = split_person(&mr.author).0;
    avatar(ui, &theme, name, 26.0).rich(Tip::new().person(&mr.author));
    ui.add_space(4.0);
    // The arrow to GitLab on the right, the texts in what is left.
    let gitlab = ui
        .with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clicked = icon_button(
                ui,
                tl("Open in GitLab"),
                Icon::ExternalLink,
                &theme.icon_button(),
                24.0,
            )
            .clicked();
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        mono(format!("!{}", mr.id), theme.text_size - 1.5).color(p.text_muted),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&mr.title)
                                .font(fonts::medium(theme.text_size - 1.0))
                                .color(p.text),
                        )
                        .truncate()
                        .selectable(false),
                    );
                });
                let when = mr
                    .merged_at
                    .as_deref()
                    .map(format_relative_date)
                    .unwrap_or_default();
                let mut detail = tf(
                    "merged into {target} {when}",
                    &[("target", &mr.target_branch), ("when", &when)],
                );
                if g.app.branch_presence(mr, branch) == Some(BranchPresence::Partial) {
                    detail = format!("{detail} · {}", tl("partly there"));
                }
                ui.add(
                    egui::Label::new(muted(&theme, detail).size(theme.text_size - 2.5))
                        .truncate()
                        .selectable(false),
                );
            });
            clicked
        })
        .inner;
    if gitlab {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab(mr.web_url.clone()));
        ui.close();
        return None;
    }
    response.clicked().then(|| mr.id.clone())
}
