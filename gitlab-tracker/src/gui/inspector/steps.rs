//! Where a merged MR went after its merge, and the timeline drawn down a line.

use super::*;

/// One step of the history timeline.
pub(in crate::gui) struct Step {
    pub title: String,
    pub detail: Option<String>,
    pub color: egui::Color32,
    /// Not reached yet: a hollow node, muted text.
    pub pending: bool,
    /// Off the main line: a branch the changes come down to (sync), drawn as
    /// a branch-off of the line.
    pub side: bool,
    /// When it happened: the exact time shows on hovering the detail.
    pub at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Where a merged MR went after its merge, nearest first: the branches fed by
/// its target (`sync`, side steps) and those up to production (`promotion`).
/// When it reached a branch is dated as the timeline dates its steps.
pub(in crate::gui) fn flow_steps(g: &GuiApp, mr: &TrackedMr) -> (Vec<Step>, Vec<Step>) {
    let tones = &g.theme.tones;
    let path = g.app.branch_flow().path(&mr.target_branch);
    let arrival = |branch: &str| {
        g.app
            .branch_arrivals
            .get(&(mr.id.clone(), branch.to_string()))
            .copied()
    };
    let detail = |branch: &str, label: &str| {
        let arrived = arrival(branch).map(|at| at.with_timezone(&chrono::Local).to_rfc3339());
        match arrived {
            Some(at) => format!("{label} · {}", format_relative_date(&at)),
            None => label.to_string(),
        }
    };
    let sync = path
        .sync
        .iter()
        .map(|branch| {
            let presence = g.app.branch_presence(mr, branch);
            let (_, label, tone) = super::super::tones::presence(presence, false, tones);
            let reached = presence.is_some_and(|p| !p.is_missing());
            let vars = [
                ("branch", branch.as_str()),
                ("target", mr.target_branch.as_str()),
            ];
            match presence {
                // Behind its parent: a rebase (or merge) is due, not a failure.
                Some(BranchPresence::Absent) => Step {
                    title: tf("Not in {branch} yet", &vars),
                    detail: Some(tf(
                        "Rebase it on {target}, or merge {target} into it",
                        &vars,
                    )),
                    color: tones.amber.fg,
                    pending: true,
                    side: true,
                    at: None,
                },
                _ => Step {
                    title: tf("Synced into {branch}", &vars),
                    detail: Some(detail(branch, label)),
                    color: tone.fg,
                    pending: !reached,
                    side: true,
                    at: arrival(branch),
                },
            }
        })
        .collect();
    let promotion = path
        .promotion
        .iter()
        .map(|branch| {
            let presence = g.app.branch_presence(mr, branch);
            let (_, label, tone) = super::super::tones::presence(presence, false, tones);
            Step {
                title: tf("On {branch}", &[("branch", branch)]),
                detail: Some(detail(branch, label)),
                color: tone.fg,
                pending: !presence.is_some_and(|p| !p.is_missing()),
                side: false,
                at: arrival(branch),
            }
        })
        .collect();
    (sync, promotion)
}

/// The same steps as the MR page's Timeline (`mr_page::timeline_steps`), the
/// commit and the source branch under them.
pub(super) fn history(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = &g.theme;
    let steps = super::super::mr_page::timeline_steps(g, mr, None);
    section(ui, theme, tl("History"), |ui| {
        timeline(ui, theme, &steps);
        ui.add_space(4.0);
        key_values(ui, theme, |rows| {
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

/// The steps down a line: a node per step, its title and detail beside it.
/// A side step branches off the line, a little to the right.
pub(in crate::gui) fn timeline(ui: &mut Ui, theme: &Theme, steps: &[Step]) {
    /// How far a side step sits right of the line.
    const SIDE: f32 = 18.0;
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut nodes = Vec::new();
    for step in steps {
        let response = ui
            .horizontal_top(|ui| {
                ui.add_space(if step.side { 22.0 + SIDE } else { 22.0 });
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let title_color = if step.pending {
                        theme.palette.text_muted
                    } else {
                        theme.palette.text
                    };
                    ui.label(
                        RichText::new(&step.title)
                            .font(fonts::medium(theme.text_size - 0.5))
                            .color(title_color),
                    );
                    if let Some(detail) = &step.detail {
                        let response = ui.label(muted(theme, detail).size(theme.text_size - 1.5));
                        if let Some(at) = step.at {
                            Tip::new()
                                .title(step.title.as_str())
                                .moment(at)
                                .show(response);
                        }
                    }
                    ui.add_space(10.0);
                });
            })
            .response;
        nodes.push((response.rect, step));
    }
    let painter = ui.painter();
    let x = |r: egui::Rect| r.left() + 7.0;
    let line = |pending: bool| {
        let color = if pending {
            theme.palette.border
        } else {
            theme.palette.text_muted.gamma_multiply(0.6)
        };
        egui::Stroke::new(1.5, color)
    };
    for pair in nodes.windows(2) {
        let (a, above) = pair[0];
        let (b, next) = pair[1];
        // Down to the next node, or to the branch-off of a side step.
        let end = if next.side {
            b.top() + 9.0
        } else {
            b.top() + 3.0
        };
        // Faint towards (or from) a step still to come, whichever way the
        // line reads; a side step's state is on its own branch-off.
        let pending = (above.pending && !above.side) || (next.pending && !next.side);
        painter.vline(x(a), (a.top() + 15.0)..=end, line(pending));
    }
    for (rect, step) in nodes {
        let mut center = egui::pos2(x(rect), rect.top() + 9.0);
        if step.side {
            center.x += SIDE;
            painter.hline((x(rect))..=(center.x - 5.0), center.y, line(step.pending));
        }
        if step.pending {
            painter.circle_filled(center, 5.0, theme.palette.panel);
            painter.circle_stroke(center, 4.5, egui::Stroke::new(1.5, step.color));
        } else {
            painter.circle_filled(center, 5.5, step.color);
        }
    }
}
