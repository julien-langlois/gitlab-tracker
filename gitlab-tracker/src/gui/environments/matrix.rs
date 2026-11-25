//! Every pair of branches: the MRs on one that the other lacks.

use super::*;

/// Every pair of branches: the MRs on the row's branch the column's lacks.
pub(super) fn matrix_card(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    pair: Option<(usize, usize)>,
    now: DateTime<Utc>,
    picked: &mut Option<(usize, usize)>,
) {
    board::card(
        ui,
        theme,
        tl("Gaps between branches"),
        tl("on the row, not on the column"),
        0.0,
        |ui| {
            let n = model.branches.len();
            let label_w = 90.0;
            let gap = 6.0;
            let cell_w = ((ui.available_width() - label_w - gap * n as f32) / n as f32).max(40.0);
            let small = egui::FontId::monospace(theme.text_size - 2.0);
            let (head, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
            for (j, b) in model.branches.iter().enumerate() {
                let cx = head.left() + label_w + gap + j as f32 * (cell_w + gap) + cell_w / 2.0;
                board::paint_text(
                    ui,
                    egui::pos2(cx, head.center().y),
                    Align2::CENTER_CENTER,
                    &b.alias,
                    small.clone(),
                    theme.tones.blue.fg,
                    cell_w,
                );
            }
            ui.add_space(4.0);
            for (i, from) in model.branches.iter().enumerate() {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::hover());
                board::paint_text(
                    ui,
                    egui::pos2(line.left(), line.center().y),
                    Align2::LEFT_CENTER,
                    &from.alias,
                    small.clone(),
                    theme.tones.blue.fg,
                    label_w - 6.0,
                );
                for j in 0..n {
                    let cell = Rect::from_min_size(
                        egui::pos2(
                            line.left() + label_w + gap + j as f32 * (cell_w + gap),
                            line.top() + 2.0,
                        ),
                        Vec2::new(cell_w, 42.0),
                    );
                    if i == j {
                        ui.painter().rect_filled(
                            cell,
                            CornerRadius::same(theme.radii.medium),
                            theme.palette.raised.gamma_multiply(0.4),
                        );
                        board::paint_text(
                            ui,
                            cell.center(),
                            Align2::CENTER_CENTER,
                            board::dash(),
                            FontId::proportional(theme.text_size - 1.0),
                            theme.palette.text_muted,
                            cell_w,
                        );
                        continue;
                    }
                    let list = model.gap(i, j);
                    // Down the flow: a promotion to do; up it: a fix to bring back.
                    let up = j < i;
                    let tone = if list.is_empty() {
                        None
                    } else if up {
                        Some(theme.tones.orange)
                    } else if oldest(list).is_some_and(|o| board::days_between(o, now) >= LATE_DAYS)
                        || list.len() >= 6
                    {
                        Some(theme.tones.red)
                    } else {
                        Some(theme.tones.amber)
                    };
                    let response = ui
                        .interact(cell, ui.scope_id().with(("mx", i, j)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let (fill, fg) = match tone {
                        Some(t) => (
                            if response.hovered() {
                                t.bg.gamma_multiply(1.6)
                            } else {
                                t.bg
                            },
                            t.fg,
                        ),
                        None => (
                            theme.palette.raised.gamma_multiply(0.7),
                            theme.palette.text_muted,
                        ),
                    };
                    ui.painter()
                        .rect_filled(cell, CornerRadius::same(theme.radii.medium), fill);
                    if pair == Some((i, j)) {
                        ui.painter().rect_stroke(
                            cell,
                            CornerRadius::same(theme.radii.medium),
                            Stroke::new(2.0, theme.palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let caption = if list.is_empty() {
                        String::new()
                    } else if up {
                        tl("to bring back").to_string()
                    } else {
                        oldest(list).map_or_else(String::new, |o| {
                            board::duration_text(board::days_between(o, now))
                        })
                    };
                    let cy = if caption.is_empty() {
                        cell.center().y
                    } else {
                        cell.center().y - 6.0
                    };
                    board::paint_text(
                        ui,
                        egui::pos2(cell.center().x, cy),
                        Align2::CENTER_CENTER,
                        &list.len().to_string(),
                        fonts::semibold(theme.text_size + 1.0),
                        fg,
                        cell_w,
                    );
                    if !caption.is_empty() {
                        board::paint_text(
                            ui,
                            egui::pos2(cell.center().x, cell.center().y + 9.0),
                            Align2::CENTER_CENTER,
                            &caption,
                            FontId::proportional(theme.text_size - 3.0),
                            fg,
                            cell_w - 6.0,
                        );
                    }
                    let hover = tf(
                        "{n} on {from}, not on {to}",
                        &[
                            ("n", &list.len().to_string()),
                            ("from", &from.name),
                            ("to", &model.branches[j].name),
                        ],
                    );
                    if response.tip(hover).clicked() {
                        *picked = Some((i, j));
                    }
                }
            }
            // Fixes made on a branch further down the flow, missing upstream.
            let back: usize = (0..n)
                .flat_map(|i| (0..i).map(move |j| (i, j)))
                .map(|(i, j)| model.gap(i, j).len())
                .sum();
            if back > 0 {
                ui.add_space(8.0);
                board::caption(
                    ui,
                    theme,
                    &tn(
                        back as u64,
                        "{n} change further down the flow is missing upstream: bring it back.",
                        "{n} changes further down the flow are missing upstream: bring them back.",
                    ),
                );
            }
        },
    );
}
