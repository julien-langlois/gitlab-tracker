//! Where the MR is: a chip per tracked branch.

use super::*;

/// Between a branch's name and its presence chip.
pub(super) const BRANCH_CHIP_GAP: f32 = 16.0;
/// Kept for the presence chip when a branch's name is long.
pub(super) const BRANCH_CHIP_ROOM: f32 = 90.0;

pub(in crate::gui) fn branches(
    g: &GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    data: Option<&crate::gitlab_detail::PageData>,
) {
    let theme = g.theme;
    let (rows, places): (Vec<_>, Vec<_>) = g.app.flow_branch_rows(mr).into_iter().unzip();
    let failed = matches!(mr.status, MrStatus::Error(_));
    let fetching = g.app.pending_initial_fetches.contains(&mr.id)
        || g.app.pending_refresh_fetches.contains(&mr.id);
    let commits = mr.commits.len();
    section(ui, &theme, tl("Branches"), |ui| {
        // One column for the names, as wide as the longest one, so the chips line up.
        // A name too long for it is shortened with an ellipsis, in full on hover.
        let max_name = (ui.available_width() - BRANCH_CHIP_ROOM - BRANCH_CHIP_GAP).max(0.0);
        let names: Vec<_> = rows
            .iter()
            .map(|row| {
                let mut job = egui::text::LayoutJob::simple_singleline(
                    row.branch.clone(),
                    egui::FontId::monospace(theme.text_size - 1.0),
                    theme.palette.text,
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(max_name);
                ui.painter().layout_job(job)
            })
            .collect();
        let name_width = names.iter().map(|name| name.size().x).fold(0.0, f32::max);
        // Lines as tall as the chips, so a deployment stays by its branch.
        ui.spacing_mut().interact_size.y = 20.0;
        for ((row, name), place) in rows.iter().zip(names).zip(places.iter()) {
            let cell = tones::flow_presence(row.presence, failed, *place, &theme.tones);
            let (glyph, label, tone) = match cell {
                // Not loading: nothing is on its way, the next refresh tells.
                _ if row.presence.is_none() && !failed && !fetching => {
                    ("—", tl("Not checked yet"), theme.tones.gray)
                }
                cell => cell,
            };
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
            let detail_width = egui::WidgetText::from(muted(&theme, detail.as_str()))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x;
            let mut detail_fits = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (name_rect, response) =
                    ui.allocate_exact_size(Vec2::new(name_width, 18.0), Sense::hover());
                let elided = name.elided;
                let pos = name_rect.left_center() - Vec2::new(0.0, name.size().y / 2.0);
                ui.painter().galley(pos, name, theme.palette.text);
                if elided {
                    response.rich(Tip::new().mono(&row.branch));
                }
                ui.add_space(BRANCH_CHIP_GAP - 8.0);
                ui.add(Chip::new(label, tone).glyph(glyph));
                detail_fits = detail_width <= ui.available_width();
                if detail_fits && !detail.is_empty() {
                    ui.label(muted(&theme, detail.as_str()));
                }
            });
            // Next to a long name, the detail goes on its own line.
            if !detail_fits && !detail.is_empty() {
                ui.label(muted(&theme, detail.as_str()));
            }
            // Under it, set off under the chip: the environment deployed from it.
            let deployed =
                data.and_then(|d| d.deployments.iter().find(|dep| dep.reference == row.branch));
            if let Some(dep) = deployed {
                // Close to its branch: the section's spacing goes between branches.
                ui.add_space(2.0 - ui.spacing().item_spacing.y);
                ui.horizontal(|ui| {
                    ui.add_space(name_width + BRANCH_CHIP_GAP);
                    // Cut to the room left: in a row, a plain label extends past
                    // the panel, which would push the inspector off the window.
                    let line =
                        super::super::mr_page::deployment_line(&theme, dep, ui.available_width());
                    ui.add(egui::Label::new(line).truncate());
                });
            }
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
