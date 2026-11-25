//! Who reviews whom: the matrix card.

use super::*;

/// Who reviews whom: authors in rows, reviewers in columns, the count of
/// requests in each cell, shaded by it.
pub(super) fn matrix_card(ui: &mut Ui, theme: &Theme, analysis: &Analysis, period: &str) {
    let note = tf(
        "{period} · rows: authors, columns: reviewers",
        &[("period", tl(period))],
    );
    board::card(ui, theme, tl("Who reviews whom"), &note, 250.0, |ui| {
        let mut totals: BTreeMap<&str, u32> = BTreeMap::new();
        for row in analysis.matrix.values() {
            for (reviewer, n) in row {
                *totals.entry(reviewer.as_str()).or_default() += n;
            }
        }
        if totals.is_empty() {
            board::empty(ui, theme, tl("No review request in the period."));
            return;
        }
        let mut reviewers: Vec<(&str, u32)> = totals.into_iter().collect();
        reviewers.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let label_w = 44.0;
        let total_w = 34.0;
        let room = ui.available_width() - label_w - total_w;
        let fit = ((room + 4.0) / 34.0).floor().max(1.0) as usize;
        reviewers.truncate(fit.min(8));
        let mut authors: Vec<(&String, u32)> = analysis
            .matrix
            .iter()
            .map(|(a, row)| (a, row.values().sum()))
            .collect();
        authors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        authors.truncate(8);
        let max = analysis
            .matrix
            .values()
            .flat_map(|r| r.values())
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        let cell_w =
            ((room - 4.0 * (reviewers.len() as f32 - 1.0)) / reviewers.len() as f32).min(56.0);
        // The totals' column right after the last reviewer's, not at the
        // card's edge (the cells stop growing on a wide card).
        let sum_right = label_w + reviewers.len() as f32 * (cell_w + 4.0) - 4.0 + 12.0 + total_w;
        let initials = |login: &str| -> String {
            let name = analysis
                .people
                .iter()
                .find(|p| p.login == login)
                .map_or(login, |p| p.name.as_str());
            let letters: String = name
                .split_whitespace()
                .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
                .take(2)
                .flat_map(char::to_uppercase)
                .collect();
            if letters.is_empty() {
                login.chars().take(2).collect::<String>().to_uppercase()
            } else {
                letters
            }
        };
        let name_of = |login: &str| -> String {
            analysis
                .people
                .iter()
                .find(|p| p.login == login)
                .map_or(login.to_string(), |p| p.name.clone())
        };
        // "Full Name (@login)", for the tooltips' avatars and handles.
        let display = |login: &str| -> String {
            match name_of(login) {
                name if name.is_empty() || name == login => login.to_string(),
                name => format!("{name} (@{login})"),
            }
        };
        let small = FontId::proportional(theme.text_size - 2.0);
        let strong = fonts::semibold(theme.text_size - 2.0);
        // Header: the reviewers.
        let (head, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
        for (i, (reviewer, _)) in reviewers.iter().enumerate() {
            let cx = head.left() + label_w + i as f32 * (cell_w + 4.0) + cell_w / 2.0;
            let r =
                Rect::from_center_size(egui::pos2(cx, head.center().y), Vec2::new(cell_w, 20.0));
            board::paint_text(
                ui,
                r.center(),
                Align2::CENTER_CENTER,
                &initials(reviewer),
                strong.clone(),
                theme.palette.text_secondary,
                cell_w,
            );
            Tip::new().person(&display(reviewer)).show(ui.interact(
                r,
                ui.scope_id().with(("rev", i)),
                Sense::hover(),
            ));
        }
        board::paint_text(
            ui,
            egui::pos2(head.left() + sum_right, head.center().y),
            Align2::RIGHT_CENTER,
            "Σ",
            small.clone(),
            theme.palette.text_muted,
            total_w,
        );
        ui.add_space(4.0);
        for (author, total) in &authors {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
            let label_rect = Rect::from_min_size(line.min, Vec2::new(label_w, 26.0));
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &initials(author),
                strong.clone(),
                theme.palette.text_secondary,
                label_w - 4.0,
            );
            Tip::new().person(&display(author)).show(ui.interact(
                label_rect,
                ui.scope_id().with(("author", author.as_str())),
                Sense::hover(),
            ));
            let row = analysis.matrix.get(*author);
            for (i, (reviewer, _)) in reviewers.iter().enumerate() {
                let n = row.and_then(|r| r.get(*reviewer)).copied().unwrap_or(0);
                let cell = Rect::from_min_size(
                    egui::pos2(
                        line.left() + label_w + i as f32 * (cell_w + 4.0),
                        line.top() + 1.0,
                    ),
                    Vec2::new(cell_w, 24.0),
                );
                let (fill, color) = if n == 0 {
                    (
                        theme.palette.raised.gamma_multiply(0.6),
                        theme.palette.text_muted.gamma_multiply(0.6),
                    )
                } else {
                    let strength = 0.18 + 0.6 * n as f32 / max as f32;
                    (
                        theme.palette.accent.gamma_multiply(strength),
                        theme.palette.text,
                    )
                };
                ui.painter().rect_filled(cell, CornerRadius::same(4), fill);
                let text = if n == 0 {
                    "·".to_string()
                } else {
                    n.to_string()
                };
                board::paint_text(
                    ui,
                    cell.center(),
                    Align2::CENTER_CENTER,
                    &text,
                    fonts::medium(theme.text_size - 2.0),
                    color,
                    cell_w,
                );
                let tip = if n == 0 {
                    tf(
                        "No request from {author} to {reviewer}",
                        &[
                            ("author", &name_of(author)),
                            ("reviewer", &name_of(reviewer)),
                        ],
                    )
                } else {
                    tn(
                        u64::from(n),
                        "{author} asked {reviewer} for {n} review",
                        "{author} asked {reviewer} for {n} reviews",
                    )
                    .replace("{author}", &name_of(author))
                    .replace("{reviewer}", &name_of(reviewer))
                };
                Tip::new()
                    .pair(&display(author), &display(reviewer))
                    .text(tip)
                    .show(ui.interact(
                        cell,
                        ui.scope_id().with(("cell", author.as_str(), i)),
                        Sense::hover(),
                    ));
            }
            board::paint_text(
                ui,
                egui::pos2(line.left() + sum_right, line.center().y),
                Align2::RIGHT_CENTER,
                &total.to_string(),
                small.clone(),
                theme.palette.text_muted,
                total_w,
            );
        }
        // Totals per reviewer.
        let (foot, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
        board::paint_text(
            ui,
            egui::pos2(foot.left(), foot.center().y),
            Align2::LEFT_CENTER,
            "Σ",
            small.clone(),
            theme.palette.text_muted,
            label_w,
        );
        let top = reviewers.first().map_or(0, |r| r.1);
        for (i, (_, total)) in reviewers.iter().enumerate() {
            let cx = foot.left() + label_w + i as f32 * (cell_w + 4.0) + cell_w / 2.0;
            let color = if *total == top && reviewers.len() > 1 {
                theme.tones.amber.fg
            } else {
                theme.palette.text_secondary
            };
            board::paint_text(
                ui,
                egui::pos2(cx, foot.center().y),
                Align2::CENTER_CENTER,
                &total.to_string(),
                fonts::semibold(theme.text_size - 2.0),
                color,
                cell_w,
            );
        }
    });
}
