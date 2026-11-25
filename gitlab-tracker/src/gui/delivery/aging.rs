//! Open MRs by step and age against the usual time, and the most common paths.

use super::*;

/// The tone of an open MR's age against the usual time to merge.
pub(super) fn age_tone(theme: &Theme, age: f64, p50: Option<f64>, p85: Option<f64>) -> Tone {
    match (p50, p85) {
        (_, Some(p85)) if age >= p85 => theme.tones.red,
        (Some(p50), _) if age >= p50 => theme.tones.amber,
        (Some(_), _) | (_, Some(_)) => theme.tones.green,
        _ => theme.tones.gray,
    }
}

/// The open MRs by step (columns) and age (height), with the P50 and P85 of
/// the time to merge across: a dot above P85 is late before being overdue.
pub(super) fn aging_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    selected: Option<&str>,
    picked: &mut Option<String>,
) {
    let late = d
        .open
        .iter()
        .filter(|a| d.p85.is_some_and(|p| a.age >= p))
        .count();
    let note = match d.p85 {
        Some(_) => tn(
            late as u64,
            "{n} past 85 % of the merged MRs",
            "{n} past 85 % of the merged MRs",
        ),
        None => tl("no merged MR to compare with yet").to_string(),
    };
    board::card(ui, theme, tl("Aging work in progress"), &note, 0.0, |ui| {
        if d.open.is_empty() {
            board::empty(ui, theme, tl("No open MR."));
            return;
        }
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 292.0), Sense::hover());
        // The column headers on top, then the plot, a margin under it so that
        // the youngest MRs do not sit on the edge.
        let header_h = 30.0;
        let area = Rect::from_min_max(
            egui::pos2(rect.left() + 44.0, rect.top()),
            egui::pos2(rect.right() - 70.0, rect.bottom()),
        );
        let plot = Rect::from_min_max(
            egui::pos2(area.left(), area.top() + header_h + 12.0),
            egui::pos2(area.right(), area.bottom() - 10.0),
        );
        // Up to four times the P85 (a week at least): an MR forgotten for
        // months sits on the top edge instead of flattening the others.
        let oldest = d.open.iter().map(|a| a.age).fold(1.0_f64, f64::max);
        let cap = d.p85.map_or(oldest, |p| (p * 4.0).max(7.0));
        let top_age = oldest.min(cap).max(d.p85.unwrap_or(0.0)).max(1.0) * 1.12;
        let y_of = |age: f64| {
            plot.bottom() - 8.0 - (age.min(top_age) / top_age) as f32 * (plot.height() - 8.0)
        };
        // The columns: a header each (name, count), bands every other one shaded.
        let col_w = plot.width() / Column::ALL.len() as f32;
        for (i, column) in Column::ALL.iter().enumerate() {
            let left = plot.left() + i as f32 * col_w;
            let band = Rect::from_min_max(
                egui::pos2(left, plot.top()),
                egui::pos2(left + col_w, plot.bottom()),
            );
            if i % 2 == 0 {
                ui.painter().rect_filled(
                    band,
                    CornerRadius::same(theme.radii.small),
                    theme.palette.raised.gamma_multiply(0.6),
                );
            }
            let header = Rect::from_min_size(
                egui::pos2(left + 3.0, area.top()),
                Vec2::new(col_w - 6.0, header_h),
            );
            ui.painter().rect(
                header,
                CornerRadius::same(theme.radii.medium),
                theme.palette.raised,
                Stroke::new(1.0, theme.palette.border_soft),
                egui::StrokeKind::Inside,
            );
            let n = d.open.iter().filter(|a| a.column == *column).count();
            let count = n.to_string();
            let count_w = board::one_line(
                ui,
                &count,
                fonts::medium(theme.text_size - 2.5),
                theme.palette.text,
                60.0,
            )
            .size()
            .x + 12.0;
            let name = board::one_line(
                ui,
                column.label(),
                fonts::semibold(theme.text_size - 0.5),
                theme.palette.text,
                header.width() - count_w - 20.0,
            );
            let total_w = name.size().x + 8.0 + count_w;
            let x = header.center().x - total_w / 2.0;
            let name_h = name.size().y;
            ui.painter().galley(
                egui::pos2(x, header.center().y - name_h / 2.0),
                name,
                theme.palette.text,
            );
            board::paint_tag(
                ui,
                theme,
                egui::pos2(x + total_w - count_w, header.center().y),
                &count,
                board::id_tone(theme),
                false,
            );
        }
        // The age scale on the left, on round steps.
        let step = [
            1.0 / 24.0,
            0.25,
            0.5,
            1.0,
            2.0,
            3.0,
            7.0,
            14.0,
            21.0,
            30.0,
            60.0,
            90.0,
            180.0,
            365.0,
        ]
        .into_iter()
        .find(|s| top_age / s <= 4.5)
        .unwrap_or(365.0);
        let mut age = 0.0;
        while age <= top_age {
            let y = y_of(age);
            ui.painter().hline(
                plot.x_range(),
                y,
                Stroke::new(1.0, theme.palette.border_soft.gamma_multiply(0.7)),
            );
            board::paint_text(
                ui,
                egui::pos2(plot.left() - 8.0, y),
                Align2::RIGHT_CENTER,
                &if age == 0.0 {
                    "0".to_string()
                } else {
                    board::duration_text(age)
                },
                FontId::proportional(theme.text_size - 2.5),
                theme.palette.text_muted,
                40.0,
            );
            age += step;
        }
        // The usual time to merge, across.
        let close = match (d.p50, d.p85) {
            (Some(a), Some(b)) => (y_of(a) - y_of(b)).abs() < 14.0,
            _ => false,
        };
        for (p, label, tone) in [
            (d.p50, "P50", theme.tones.amber),
            (d.p85, "P85", theme.tones.red),
        ] {
            let Some(p) = p else {
                continue;
            };
            let y = y_of(p);
            let label_y = if close && label == "P50" {
                y + 8.0
            } else if close {
                y - 8.0
            } else {
                y
            };
            super::super::milestones::dashed(
                ui,
                egui::pos2(plot.left(), y),
                egui::pos2(plot.right(), y),
                tone.fg.gamma_multiply(0.8),
            );
            board::paint_text(
                ui,
                egui::pos2(plot.right() + 6.0, label_y),
                Align2::LEFT_CENTER,
                &format!("{label} · {}", board::duration_text(p)),
                FontId::proportional(theme.text_size - 2.5),
                tone.fg,
                64.0,
            );
        }
        // The MRs, spread across their column, the oldest first.
        for (i, column) in Column::ALL.iter().enumerate() {
            let list: Vec<&Aging> = d.open.iter().filter(|a| a.column == *column).collect();
            let left = plot.left() + i as f32 * col_w;
            for (k, a) in list.iter().enumerate() {
                let x = left + col_w * (k as f32 + 1.0) / (list.len() as f32 + 1.0);
                let center = egui::pos2(x, y_of(a.age));
                // Off the scale: a mark on the top edge.
                if a.age > top_age {
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            center + Vec2::new(0.0, -14.0),
                            center + Vec2::new(-4.0, -9.0),
                            center + Vec2::new(4.0, -9.0),
                        ],
                        theme.tones.red.fg,
                        Stroke::NONE,
                    ));
                }
                let tone = age_tone(theme, a.age, d.p50, d.p85);
                let is_selected = selected == Some(a.id.as_str());
                let hit = Rect::from_center_size(center, Vec2::splat(16.0));
                let response =
                    ui.interact(hit, ui.scope_id().with(("aging", &a.id)), Sense::click());
                let radius = if response.hovered() || is_selected {
                    7.5
                } else {
                    6.0
                };
                ui.painter().circle(
                    center,
                    radius,
                    tone.fg,
                    Stroke::new(
                        if is_selected { 2.5 } else { 1.5 },
                        if is_selected {
                            theme.palette.text
                        } else {
                            theme.palette.panel
                        },
                    ),
                );
                let rounds = a
                    .rounds
                    .filter(|n| *n > 0)
                    .map(|n| format!(" · {}", tn(u64::from(n), "{n} round", "{n} rounds")))
                    .unwrap_or_default();
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .rich(Tip::new().mr(&a.id, &a.title).text(format!(
                        "{} · {}{rounds}",
                        split_person(&a.author).0,
                        tf("open for {time}", &[("time", &board::duration_text(a.age))]),
                    )));
                if response.clicked() {
                    *picked = Some(a.id.clone());
                }
            }
        }
    });
}

/// The MRs merged in the period, each as its steps end to end, the slowest
/// first.
pub(super) fn paths_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    selected: Option<&str>,
    now: DateTime<Utc>,
    picked: &mut Option<String>,
) {
    board::card(
        ui,
        theme,
        tl("Merged in the period"),
        tl("the slowest first"),
        0.0,
        |ui| {
            if d.merged.is_empty() {
                board::empty(ui, theme, tl("No MR merged in the period."));
                return;
            }
            let mut list: Vec<(&Path, [Option<f64>; 5], f64)> = d
                .merged
                .iter()
                .map(|p| {
                    let stages = p.stages(now);
                    let sum = stages.iter().flatten().sum();
                    (p, stages, sum)
                })
                .collect();
            list.sort_by(|a, b| b.2.total_cmp(&a.2));
            let longest = list.first().map_or(1.0, |l| l.2).max(0.01);
            for (path, stages, sum) in list.iter().take(12) {
                let (rect, response) =
                    board::row(ui, theme, 32.0, selected == Some(path.id.as_str()));
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    &format!("!{}", path.id),
                    board::id_tone(theme),
                    true,
                );
                let title_w = (rect.width() * 0.34).min(320.0);
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + w + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &path.title,
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text,
                    title_w - w - 14.0,
                );
                let total_w = 64.0;
                board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &board::duration_text(*sum),
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text_secondary,
                    total_w,
                );
                let room = rect.width() - title_w - total_w - 24.0;
                if room > 30.0 {
                    let bar = Rect::from_min_size(
                        egui::pos2(rect.left() + title_w + 8.0, rect.center().y - 5.0),
                        Vec2::new((room * (*sum / longest) as f32).max(4.0), 10.0),
                    );
                    let parts: Vec<(f32, Color32)> = Stage::ALL
                        .iter()
                        .map(|s| (stages[*s as usize].unwrap_or(0.0) as f32, s.tone(theme).fg))
                        .collect();
                    board::paint_stacked_bar(ui, theme, bar, &parts, 1.0);
                }
                if response.clicked() {
                    *picked = Some(path.id.clone());
                }
            }
            ui.add_space(6.0);
            let legend: Vec<(Color32, String)> = Stage::ALL
                .iter()
                .map(|s| (s.tone(theme).fg, s.label().to_string()))
                .collect();
            board::legend(ui, theme, &legend);
        },
    );
}
