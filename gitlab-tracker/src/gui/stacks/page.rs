//! The page itself: the stacks in the side list, then each stack as a tree.

use super::*;

pub(super) fn nav(g: &mut GuiApp, ui: &mut Ui, all: &[Stack]) {
    let theme = g.theme;
    let mrs: usize = all.iter().map(Stack::open).sum();
    board::nav_header(
        ui,
        &theme,
        tl("Stacked MRs"),
        &format!(
            "{} · {}",
            tn(all.len() as u64, "{n} stack", "{n} stacks"),
            tn(mrs as u64, "{n} open MR", "{n} open MRs")
        ),
    );
    let mut picked = None;
    board::nav_group(ui, &theme, tl("Stacks"), |ui| {
        if board::nav_item(
            ui,
            &theme,
            tl("All the stacks"),
            &all.len().to_string(),
            None,
            g.stacks.stack.is_none(),
        )
        .clicked()
        {
            picked = Some(None);
        }
        for stack in all {
            let root = stack.root();
            let label = format!("!{} {}", root.id, root.title);
            if board::nav_item(
                ui,
                &theme,
                &label,
                &stack.nodes.len().to_string(),
                Some(stack_tone(stack, &theme)),
                g.stacks.stack.as_deref() == Some(root.id.as_str()),
            )
            .clicked()
            {
                picked = Some(Some(root.id.clone()));
            }
        }
    });
    if let Some(stack) = picked {
        g.stacks.stack = stack;
    }
    board::nav_note(
        ui,
        &theme,
        tl("A stack is an MR opened on another MR's branch, to split a change into steps that review well. They merge from the top: each one after the one it sits on."),
    );
}

pub(super) fn main(g: &mut GuiApp, ui: &mut Ui, all: &[Stack]) {
    let theme = g.theme;
    board::heading(
        ui,
        &theme,
        tl("Stacked MRs"),
        tl("MRs built on each other, the order to merge them, and what is out of step"),
        |_| {},
    );
    ui.add_space(14.0);
    let count = |pred: fn(&Problem) -> bool| {
        all.iter()
            .flat_map(|s| &s.nodes)
            .filter(|n| n.problems.iter().any(pred))
            .count()
    };
    let retarget = count(|p| matches!(p, Problem::Retarget { .. }));
    let rebase = count(|p| matches!(p, Problem::Rebase { .. }));
    let deepest = all
        .iter()
        .map(|s| s.nodes.iter().map(|n| n.depth + 1).max().unwrap_or(0))
        .max()
        .unwrap_or(0);
    let t = &theme.tones;
    let figures = [
        Figure {
            label: tl("Stacks").into(),
            value: all.len().to_string(),
            value_color: None,
            caption: tn(
                deepest as u64,
                "{n} MR deep at most",
                "{n} MRs deep at most",
            ),
            caption_color: None,
        },
        Figure {
            label: tl("MRs in a stack").into(),
            value: all.iter().map(Stack::open).sum::<usize>().to_string(),
            value_color: None,
            caption: tl("open").to_string(),
            caption_color: None,
        },
        Figure {
            label: tl("To retarget").into(),
            value: retarget.to_string(),
            value_color: (retarget > 0).then_some(t.red.fg),
            caption: tl("their parent is merged").to_string(),
            caption_color: None,
        },
        Figure {
            label: tl("To rebase").into(),
            value: rebase.to_string(),
            value_color: (rebase > 0).then_some(t.orange.fg),
            caption: tl("their parent changed since").to_string(),
            caption_color: None,
        },
    ];
    board::figures(ui, &theme, &figures);
    if all.is_empty() {
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("No tracked MR targets another MR's branch: no stack to follow."),
            );
        });
        return;
    }
    let shown: Vec<&Stack> = all
        .iter()
        .filter(|s| g.stacks.stack.as_ref().is_none_or(|id| &s.root().id == id))
        .collect();
    let mut picked = None;
    let mut open = None;
    let mut copied = false;
    for stack in shown {
        let root = stack.root();
        board::card(
            ui,
            &theme,
            &format!("!{} {}", root.id, root.title),
            &tf(
                "into {base} · {n}",
                &[
                    ("base", &stack.base),
                    ("n", &tn(stack.nodes.len() as u64, "{n} MR", "{n} MRs")),
                ],
            ),
            0.0,
            |ui| {
                stack_body(
                    ui,
                    &theme,
                    stack,
                    g.stacks.selected.as_deref(),
                    &mut picked,
                    &mut open,
                );
                if let Some(commands) = commands(stack) {
                    ui.add_space(12.0);
                    ui.label(semibold(
                        tl("Bring the stack back in step"),
                        theme.text_size,
                    ));
                    ui.label(muted(
                        &theme,
                        tl("Each MR to move, then those stacked on it, in merge order. The app only reads: it hands you the commands, you run them."),
                    ));
                    copied |= super::super::mr_page::command_box(ui, &theme, &commands);
                }
            },
        );
        ui.add_space(GAP);
    }
    if let Some(id) = picked {
        g.stacks.selected = Some(id);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
    if copied {
        g.app.flash(tl("Commands copied"), false);
    }
}

/// Width of a level of the tree.
const INDENT: f32 = 26.0;
/// The author's avatar, right of a stack's row.
const AUTHOR_AVATAR: f32 = 22.0;

/// The base branch, then each MR under the one it sits on, joined by lines.
pub(super) fn stack_body(
    ui: &mut Ui,
    theme: &Theme,
    stack: &Stack,
    selected: Option<&str>,
    picked: &mut Option<String>,
    open: &mut Option<String>,
) {
    // The base branch.
    let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let w = board::paint_tag(
        ui,
        theme,
        egui::pos2(line.left(), line.center().y),
        &stack.base,
        theme.tones.green,
        true,
    );
    board::paint_text(
        ui,
        egui::pos2(line.left() + w + 10.0, line.center().y),
        Align2::LEFT_CENTER,
        tl("where the stack ends"),
        FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
        line.width() - w - 10.0,
    );
    let mut anchors: HashMap<String, egui::Pos2> = HashMap::new();
    let base_anchor = egui::pos2(line.left() + 10.0, line.bottom());
    let mut order = 0;
    for node in &stack.nodes {
        let height = if node.problems.is_empty() { 50.0 } else { 70.0 };
        let (rect, response) = board::row(ui, theme, height, selected == Some(node.id.as_str()));
        let x = rect.left() + 10.0 + node.depth as f32 * INDENT;
        let dot = egui::pos2(x, rect.top() + 17.0);
        // The line from the parent (or the base) to this MR.
        let from = node
            .parent
            .as_ref()
            .and_then(|p| anchors.get(p).copied())
            .unwrap_or(base_anchor);
        let line_color = theme.palette.border;
        ui.painter().line_segment(
            [from, egui::pos2(from.x, dot.y)],
            Stroke::new(1.5, line_color),
        );
        if (from.x - dot.x).abs() > 0.5 {
            ui.painter().line_segment(
                [egui::pos2(from.x, dot.y), dot],
                Stroke::new(1.5, line_color),
            );
        }
        anchors.insert(node.id.clone(), egui::pos2(dot.x, dot.y + 6.0));
        let merged = node.state == GitlabMrState::Merged;
        let dot_color = if merged {
            theme.tones.purple.fg
        } else if let Some(p) = node.problems.first() {
            p.tone(theme).fg
        } else if node.ready() {
            theme.tones.green.fg
        } else {
            theme.tones.blue.fg
        };
        ui.painter()
            .circle(dot, 6.0, dot_color, Stroke::new(2.0, theme.palette.panel));
        // First line: the merge order, the id, the title, the author.
        let left = x + 16.0;
        let order_text = if merged {
            tl("merged").to_string()
        } else {
            order += 1;
            format!("{order}.")
        };
        let ow = board::paint_text(
            ui,
            egui::pos2(left, dot.y),
            Align2::LEFT_CENTER,
            &order_text,
            fonts::semibold(theme.text_size - 1.5),
            theme.palette.text_muted,
            60.0,
        );
        let iw = board::paint_tag(
            ui,
            theme,
            egui::pos2(left + ow + 8.0, dot.y),
            &format!("!{}", node.id),
            board::id_tone(theme),
            true,
        );
        // The author, avatar and name, centred on the row's height.
        let author = split_person(&node.author).0;
        let aw = if author.is_empty() {
            0.0
        } else {
            let nw = board::paint_text(
                ui,
                egui::pos2(rect.right() - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                author,
                FontId::proportional(theme.text_size - 1.5),
                theme.palette.text_muted,
                140.0,
            );
            let cell = egui::Rect::from_center_size(
                egui::pos2(
                    rect.right() - 10.0 - nw - 8.0 - AUTHOR_AVATAR / 2.0,
                    rect.center().y,
                ),
                Vec2::splat(AUTHOR_AVATAR),
            );
            let mut cell = board::inside(ui, cell);
            super::super::widgets::avatar(&mut cell, theme, author, AUTHOR_AVATAR)
                .rich(Tip::new().person(&node.author));
            nw + 8.0 + AUTHOR_AVATAR
        };
        // What the lines of the row may use, left of the author.
        let right = rect.right() - 10.0 - if aw > 0.0 { aw + 12.0 } else { 0.0 };
        let title_x = left + ow + iw + 16.0;
        board::paint_text(
            ui,
            egui::pos2(title_x, dot.y),
            Align2::LEFT_CENTER,
            &node.title,
            FontId::proportional(theme.text_size - 0.5),
            if merged {
                theme.palette.text_muted
            } else {
                theme.palette.text
            },
            right - title_x,
        );
        // Second line: the branch, then its state.
        let y2 = rect.top() + 38.0;
        let mut cx = left;
        cx += board::paint_text(
            ui,
            egui::pos2(cx, y2),
            Align2::LEFT_CENTER,
            &node.source,
            FontId::monospace(theme.text_size - 2.5),
            theme.palette.text_muted,
            (right - cx) * 0.4,
        ) + 10.0;
        for (text, tone) in state_tags(node, theme) {
            if cx > right - 50.0 {
                break;
            }
            cx += board::paint_tag_within(
                ui,
                theme,
                egui::pos2(cx, y2),
                &text,
                tone,
                false,
                right - cx,
            ) + 6.0;
        }
        // Third line: the worst problem.
        if let Some(problem) = node.problems.first() {
            let y3 = rect.top() + 58.0;
            let extra = node.problems.len() - 1;
            let text = if extra > 0 {
                format!("{} (+{extra})", problem.text())
            } else {
                problem.text()
            };
            let w = board::paint_text(
                ui,
                egui::pos2(left, y3),
                Align2::LEFT_CENTER,
                &text,
                fonts::medium(theme.text_size - 1.5),
                problem.tone(theme).fg,
                right - left,
            );
            // The parent it names, by its title.
            ui.interact(
                egui::Rect::from_min_size(egui::pos2(left, y3 - 9.0), Vec2::new(w, 18.0)),
                ui.scope_id().with(("problem", node.id.as_str())),
                Sense::hover(),
            )
            .rich(stack.named_tip(problem.parent()));
        }
        if super::super::widgets::double_clicked(ui, &response, ("stack", &node.id)) {
            *open = Some(node.id.clone());
        } else if response.clicked() {
            *picked = Some(node.id.clone());
        }
    }
}

/// The short tags of an MR's state: approved, CI, conflicts, behind, threads.
pub(super) fn state_tags(node: &Node, theme: &Theme) -> Vec<(String, Tone)> {
    let t = &theme.tones;
    let mut out = Vec::new();
    if node.state == GitlabMrState::Merged {
        out.push((tc("state", "Merged").to_string(), t.purple));
        return out;
    }
    match node.mergeability {
        MergeabilityStatus::Draft => out.push((tc("state", "Draft").to_string(), t.gray)),
        MergeabilityStatus::Conflict => out.push((tl("Conflict").to_string(), t.red)),
        _ => {}
    }
    if node.ci_failed {
        out.push((tl("CI failed").to_string(), t.red));
    }
    if node.approved {
        out.push((tl("Approved").to_string(), t.green));
    }
    if let Some(n) = node.behind.filter(|n| *n > 0) {
        out.push((tn(u64::from(n), "{n} behind", "{n} behind"), t.amber));
    }
    if node.threads > 0 {
        out.push((
            tn(u64::from(node.threads), "{n} thread", "{n} threads"),
            t.amber,
        ));
    }
    out
}
