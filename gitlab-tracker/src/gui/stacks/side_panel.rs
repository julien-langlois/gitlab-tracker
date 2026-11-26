//! The panel beside the page (the MR picked: where it sits, what it waits on, the
//! merge order), and the rail's badge.

use super::*;

pub(super) fn inspector(g: &mut GuiApp, ui: &mut Ui, all: &[Stack]) {
    let theme = g.theme;
    let found = g.stacks.selected.as_deref().and_then(|id| {
        all.iter()
            .find_map(|s| s.nodes.iter().find(|n| n.id == id).map(|n| (s, n)))
    });
    let Some((stack, node)) = found else {
        board::inspector_empty(
            ui,
            &theme,
            tl("Pick an MR of a stack: where it sits, what it waits on, and the order to merge the stack."),
        );
        return;
    };
    if board::inspector_title(ui, &theme, &format!("!{}", node.id)) {
        g.stacks.selected = None;
        return;
    }
    let mut open = false;
    board::section(ui, &theme, "", "", |ui| {
        ui.add(egui::Label::new(semibold(&node.title, theme.text_size)).wrap());
        ui.add_space(4.0);
        ui.label(muted(&theme, split_person(&node.author).0));
        ui.add_space(8.0);
        open = button(ui, &theme, None, tl("Open the MR"), false).clicked();
    });
    board::section(ui, &theme, tl("Where it sits"), "", |ui| {
        let open_nodes: Vec<&Node> = stack
            .nodes
            .iter()
            .filter(|n| n.state == GitlabMrState::Opened)
            .collect();
        let position = open_nodes
            .iter()
            .position(|n| n.id == node.id)
            .map(|i| {
                tf(
                    "{i} of {n}",
                    &[
                        ("i", &(i + 1).to_string()),
                        ("n", &open_nodes.len().to_string()),
                    ],
                )
            })
            .unwrap_or_else(|| tl("merged").to_string());
        board::facts(
            ui,
            &theme,
            &[
                (tl("Branch"), node.source.clone()),
                (tl("Targets"), node.target.clone()),
                (tl("Stack ends in"), stack.base.clone()),
                (tl("Merge order"), position),
                (
                    tl("Sits on"),
                    node.parent
                        .as_ref()
                        .map_or_else(|| stack.base.clone(), |p| stack.named(p)),
                ),
            ],
        );
    });
    if !node.problems.is_empty() {
        board::section(ui, &theme, tl("To look at"), "", |ui| {
            for problem in &node.problems {
                let tone = problem.tone(&theme);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    super::super::widgets::dot(ui, tone.fg, 8.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(problem.text())
                                .size(theme.text_size - 1.0)
                                .color(if problem.is_action() {
                                    theme.palette.text
                                } else {
                                    theme.palette.text_secondary
                                }),
                        )
                        .wrap(),
                    )
                    .rich(stack.named_tip(problem.parent()));
                });
                ui.add_space(4.0);
            }
        });
    }
    board::section(
        ui,
        &theme,
        tl("Merge order"),
        tl("each after the one it sits on"),
        |ui| {
            let mut i = 0;
            for n in &stack.nodes {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                let merged = n.state == GitlabMrState::Merged;
                let label = if merged {
                    "✓".to_string()
                } else {
                    i += 1;
                    format!("{i}.")
                };
                let color = if n.id == node.id {
                    theme.palette.text
                } else {
                    theme.palette.text_secondary
                };
                board::paint_text(
                    ui,
                    egui::pos2(line.left(), line.center().y),
                    Align2::LEFT_CENTER,
                    &label,
                    fonts::semibold(theme.text_size - 1.5),
                    if merged {
                        theme.tones.purple.fg
                    } else {
                        theme.palette.text_muted
                    },
                    22.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(line.left() + 26.0, line.center().y),
                    Align2::LEFT_CENTER,
                    &format!("!{} {}", n.id, n.title),
                    FontId::proportional(theme.text_size - 1.0),
                    color,
                    line.width() - 26.0,
                );
            }
        },
    );
    if open {
        let id = node.id.clone();
        g.open_mr(&id);
    }
}

// ── The rail's badge ─────────────────────────────────────────────────────────

/// The MRs of a stack to retarget (red) or rebase (orange).
pub fn badge(g: &GuiApp) -> Option<Badge> {
    let all = g.memo.stacks.get(&g.app.project_id, (), || stacks(&g.app));
    let nodes: Vec<&Node> = all
        .iter()
        .flat_map(|s| &s.nodes)
        .filter(|n| n.problems.iter().any(Problem::is_action))
        .collect();
    if nodes.is_empty() {
        return None;
    }
    let retarget = nodes.iter().any(|n| {
        n.problems
            .iter()
            .any(|p| matches!(p, Problem::Retarget { .. }))
    });
    let ids: Vec<String> = nodes.iter().map(|n| format!("!{}", n.id)).collect();
    Some(Badge {
        n: nodes.len(),
        tone: if retarget {
            g.theme.tones.red
        } else {
            g.theme.tones.orange
        },
        hint: rail_badges::listed(
            tn(
                nodes.len() as u64,
                "{n} stacked MR to retarget or rebase",
                "{n} stacked MRs to retarget or rebase",
            ),
            rail_badges::names(&ids, 4),
        ),
    })
}
