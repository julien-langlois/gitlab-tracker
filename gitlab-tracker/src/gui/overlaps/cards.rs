//! The matrix of MRs against MRs, the hot spots and the files card.

use super::*;

/// The open MRs that share a file, in a lower triangle: the count of files
/// each pair shares, in the tone of how they meet.
pub(super) fn matrix_card(
    ui: &mut Ui,
    theme: &Theme,
    overlaps: &Overlaps,
    pair: Option<&(String, String)>,
    picked: &mut Option<(String, String)>,
) {
    board::card(ui, theme, tl("MR × MR"), tl("files in common"), 0.0, |ui| {
        let ids: Vec<&str> = overlaps.matrix.iter().map(String::as_str).collect();
        if ids.len() < 2 {
            board::empty(ui, theme, tl("No two open MRs change the same file."));
            return;
        }
        let label_w = 46.0;
        let n = ids.len() - 1;
        let cell = ((ui.available_width() - label_w - 4.0 * n as f32) / n as f32).clamp(22.0, 48.0);
        let font = egui::FontId::monospace(theme.text_size - 3.0);
        let (head, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        // An id alone says little: its title on hover.
        let with_mr = |tip: Tip, id: &str| tip.mr(id, overlaps.mr(id).map_or("", |m| &m.title));
        for (j, id) in ids.iter().enumerate().take(n) {
            let cx = head.left() + label_w + j as f32 * (cell + 4.0) + cell / 2.0;
            board::paint_text(
                ui,
                egui::pos2(cx, head.center().y),
                Align2::CENTER_CENTER,
                &format!("!{id}"),
                font.clone(),
                theme.palette.text_secondary,
                cell + 4.0,
            );
            ui.interact(
                Rect::from_center_size(
                    egui::pos2(cx, head.center().y),
                    Vec2::new(cell + 4.0, head.height()),
                ),
                ui.scope_id().with(("matrix_col", *id)),
                Sense::hover(),
            )
            .rich(with_mr(Tip::new(), id));
        }
        // A lower triangle: no first row, no last column.
        for (i, a) in ids.iter().enumerate().skip(1) {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), cell + 4.0), Sense::hover());
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &format!("!{a}"),
                font.clone(),
                theme.palette.text_secondary,
                label_w - 4.0,
            );
            ui.interact(
                Rect::from_min_size(line.min, Vec2::new(label_w - 4.0, line.height())),
                ui.scope_id().with(("matrix_row", *a)),
                Sense::hover(),
            )
            .rich(with_mr(Tip::new(), a));
            for (j, b) in ids.iter().enumerate().take(i) {
                let r = Rect::from_min_size(
                    egui::pos2(
                        line.left() + label_w + j as f32 * (cell + 4.0),
                        line.top() + 2.0,
                    ),
                    Vec2::splat(cell),
                );
                match overlaps.pair(a, b) {
                    Some(p) => {
                        let tone = p.level().tone(theme);
                        let response = ui
                            .interact(r, ui.scope_id().with(("pair", *a, *b)), Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .rich(
                                with_mr(with_mr(Tip::new(), a), b)
                                    .chip(tn(p.files.len() as u64, "{n} file", "{n} files"), tone),
                            );
                        let fill = if response.hovered() {
                            tone.bg.gamma_multiply(1.7)
                        } else {
                            tone.bg
                        };
                        ui.painter().rect_filled(r, CornerRadius::same(5), fill);
                        if pair.is_some_and(|(x, y)| (x == a && y == b) || (x == b && y == a)) {
                            ui.painter().rect_stroke(
                                r,
                                CornerRadius::same(5),
                                Stroke::new(2.0, theme.palette.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        board::paint_text(
                            ui,
                            r.center(),
                            Align2::CENTER_CENTER,
                            &p.files.len().to_string(),
                            fonts::semibold(theme.text_size - 1.0),
                            tone.fg,
                            cell,
                        );
                        if response.clicked() {
                            *picked = Some((p.a.clone(), p.b.clone()));
                        }
                    }
                    None => {
                        ui.painter().rect_filled(
                            r,
                            CornerRadius::same(5),
                            theme.palette.raised.gamma_multiply(0.55),
                        );
                        board::paint_text(
                            ui,
                            r.center(),
                            Align2::CENTER_CENTER,
                            "·",
                            FontId::proportional(theme.text_size - 1.0),
                            theme.palette.text_muted.gamma_multiply(0.6),
                            cell,
                        );
                    }
                }
            }
        }
        ui.add_space(8.0);
        board::legend(
            ui,
            theme,
            &[
                (theme.tones.red.fg, Level::SameLines.label().to_string()),
                (theme.tones.orange.fg, Level::Near.label().to_string()),
                (theme.tones.amber.fg, Level::SameFile.label().to_string()),
            ],
        );
    });
}

pub(super) fn hot_card(ui: &mut Ui, theme: &Theme, hot: &[(String, usize)]) {
    board::card(
        ui,
        theme,
        tl("Hot spots"),
        tl("MRs in 30 days"),
        0.0,
        |ui| {
            if hot.is_empty() {
                board::empty(ui, theme, tl("No file changed by several recent MRs."));
                return;
            }
            let max = hot.first().map_or(1, |h| h.1).max(1);
            for (path, n) in hot {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                let count_w = 26.0;
                let bar_w = (line.width() * 0.28).clamp(50.0, 110.0);
                board::paint_text(
                    ui,
                    egui::pos2(line.right(), line.center().y),
                    Align2::RIGHT_CENTER,
                    &n.to_string(),
                    fonts::medium(theme.text_size - 1.5),
                    theme.palette.text_muted,
                    count_w,
                );
                let track = Rect::from_min_size(
                    egui::pos2(line.right() - count_w - bar_w - 6.0, line.center().y - 3.0),
                    Vec2::new(bar_w, 6.0),
                );
                ui.painter()
                    .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
                let mut fill = track;
                fill.set_width(track.width() * *n as f32 / max as f32);
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(3), theme.tones.orange.fg);
                path_text(
                    ui,
                    theme,
                    egui::pos2(line.left(), line.center().y),
                    path,
                    track.left() - 10.0 - line.left(),
                );
            }
        },
    );
}

/// A path: its folder muted, its name bright, cut from the left when long.
pub(in crate::gui) fn path_text(
    ui: &Ui,
    theme: &Theme,
    pos: egui::Pos2,
    path: &str,
    max_width: f32,
) -> f32 {
    let (dir, name) = match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    };
    let font = egui::FontId::monospace(theme.text_size - 1.5);
    let name_galley = board::one_line(ui, name, font.clone(), theme.palette.text, max_width);
    let name_w = name_galley.size().x;
    let room = max_width - name_w;
    let mut x = pos.x;
    if !dir.is_empty() && room > 20.0 {
        // The folder, cut from its start: "…api/graphql/".
        let mut shown = dir.to_string();
        let mut galley = board::one_line(
            ui,
            &shown,
            font.clone(),
            theme.palette.text_muted,
            f32::INFINITY,
        );
        while galley.size().x > room && shown.chars().count() > 2 {
            shown = format!("…{}", shown.chars().skip(2).collect::<String>());
            galley = board::one_line(
                ui,
                &shown,
                font.clone(),
                theme.palette.text_muted,
                f32::INFINITY,
            );
        }
        if galley.size().x <= room {
            let w = galley.size().x;
            ui.painter().galley(
                egui::pos2(x, pos.y - galley.size().y / 2.0),
                galley,
                theme.palette.text_muted,
            );
            x += w;
        }
    }
    ui.painter().galley(
        egui::pos2(x, pos.y - name_galley.size().y / 2.0),
        name_galley,
        theme.palette.text,
    );
    x + name_w - pos.x
}

#[allow(clippy::too_many_arguments)]
pub(super) fn files_card(
    ui: &mut Ui,
    theme: &Theme,
    overlaps: &Overlaps,
    folder_filter: Option<&str>,
    pair: Option<&(String, String)>,
    picked: &mut Option<(String, String)>,
    open: &mut Option<String>,
) {
    board::card(
        ui,
        theme,
        tl("Shared files"),
        tl("the riskiest first"),
        0.0,
        |ui| {
            let files: Vec<&Shared> = overlaps
                .files
                .iter()
                .filter(|f| folder_filter.is_none_or(|d| folder(&f.path) == d))
                .collect();
            if files.is_empty() {
                board::empty(ui, theme, tl("No file changed by two open MRs here."));
                return;
            }
            let width = ui.available_width();
            let level_w = 92.0;
            let lines_w = 76.0;
            let mrs_w = (width * 0.28).clamp(96.0, 170.0);
            let (head, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
            let small = FontId::proportional(theme.text_size - 1.5);
            board::paint_text(
                ui,
                egui::pos2(head.left() + 8.0, head.center().y),
                Align2::LEFT_CENTER,
                tl("File"),
                small.clone(),
                theme.palette.text_muted,
                80.0,
            );
            let mrs_x = head.right() - 8.0 - lines_w - 12.0 - level_w - 12.0 - mrs_w;
            board::paint_text(
                ui,
                egui::pos2(mrs_x, head.center().y),
                Align2::LEFT_CENTER,
                tl("MRs"),
                small.clone(),
                theme.palette.text_muted,
                mrs_w,
            );
            board::paint_text(
                ui,
                egui::pos2(
                    head.right() - 8.0 - lines_w - 12.0 - level_w,
                    head.center().y,
                ),
                Align2::LEFT_CENTER,
                tl("Risk"),
                small.clone(),
                theme.palette.text_muted,
                level_w,
            );
            board::paint_text(
                ui,
                egui::pos2(head.right() - 8.0, head.center().y),
                Align2::RIGHT_CENTER,
                tl("Lines"),
                small,
                theme.palette.text_muted,
                lines_w,
            );
            for f in files {
                let selected = pair.is_some_and(|(a, b)| {
                    f.mrs.iter().any(|m| &m.0 == a) && f.mrs.iter().any(|m| &m.0 == b)
                });
                let (rect, response) = board::row(ui, theme, 38.0, selected);
                // Only the rows in view are drawn.
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                if !selected {
                    board::row_rule(ui, theme, rect);
                }
                let cy = rect.center().y;
                let mrs_x = rect.right() - 8.0 - lines_w - 12.0 - level_w - 12.0 - mrs_w;
                path_text(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 8.0, cy),
                    &f.path,
                    mrs_x - 12.0 - rect.left() - 8.0,
                );
                // The MRs, as many as fit.
                let mut x = mrs_x;
                for (k, (id, _)) in f.mrs.iter().enumerate() {
                    let text = format!("!{id}");
                    let galley = ui.painter().layout_no_wrap(
                        text.clone(),
                        egui::FontId::monospace(theme.text_size - 2.0),
                        theme.palette.text,
                    );
                    let w = galley.size().x + 12.0;
                    if x + w > mrs_x + mrs_w - if k + 1 < f.mrs.len() { 24.0 } else { 0.0 } {
                        board::paint_text(
                            ui,
                            egui::pos2(x, cy),
                            Align2::LEFT_CENTER,
                            &format!("+{}", f.mrs.len() - k),
                            fonts::medium(theme.text_size - 2.0),
                            theme.palette.text_muted,
                            30.0,
                        );
                        break;
                    }
                    let r = Rect::from_min_size(egui::pos2(x, cy - 10.0), Vec2::new(w, 20.0));
                    let hit = ui
                        .interact(
                            r,
                            ui.scope_id().with(("file_mr", f.path.as_str(), k)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .rich(Tip::new().mr(id, overlaps.mr(id).map_or("", |m| &m.title)));
                    let fill = if hit.hovered() {
                        theme.palette.border
                    } else {
                        theme.palette.raised
                    };
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(theme.radii.small), fill);
                    ui.painter().galley(
                        r.center() - galley.size() / 2.0,
                        galley,
                        theme.palette.text,
                    );
                    if hit.clicked() {
                        *open = Some(id.clone());
                    }
                    x += w + 5.0;
                }
                let level_x = rect.right() - 8.0 - lines_w - 12.0 - level_w;
                board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(level_x, cy),
                    f.level.label(),
                    f.level.tone(theme),
                    false,
                );
                let added = format!("+{}", f.added);
                let deleted = format!("−{}", f.deleted);
                let font = egui::FontId::monospace(theme.text_size - 2.0);
                let dw = board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 8.0, cy),
                    Align2::RIGHT_CENTER,
                    &deleted,
                    font.clone(),
                    theme.tones.red.fg,
                    lines_w / 2.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 8.0 - dw - 6.0, cy),
                    Align2::RIGHT_CENTER,
                    &added,
                    font,
                    theme.tones.green.fg,
                    lines_w / 2.0,
                );
                if response.clicked() {
                    // The worst pair on this file.
                    let best = overlaps
                        .pairs
                        .iter()
                        .filter(|p| p.files.iter().any(|(path, _)| path == &f.path))
                        .min_by_key(|p| {
                            p.files
                                .iter()
                                .find(|(path, _)| path == &f.path)
                                .map(|x| x.1)
                        });
                    if let Some(p) = best {
                        *picked = Some((p.a.clone(), p.b.clone()));
                    }
                }
            }
        },
    );
}
