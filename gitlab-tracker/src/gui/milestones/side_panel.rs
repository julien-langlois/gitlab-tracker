//! The panel beside the page: the burn-up chart and the release notes.

use super::*;

pub(super) fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    m: Option<&Milestone>,
    weekly: &[u32],
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    let Some(m) = m else {
        board::inspector_empty(ui, &theme, tl("Pick a milestone."));
        return;
    };
    board::section(
        ui,
        &theme,
        tl("Burn-up"),
        tl("delivered against the scope"),
        |ui| {
            burn_up(ui, &theme, m, weekly, now);
        },
    );
    if !m.title.is_empty() && !m.done() {
        forecast_section(ui, &theme, m, weekly, now);
    }
    let tickets: Vec<&(String, Option<u32>, Option<u32>)> =
        m.items.iter().filter_map(|i| i.ticket.as_ref()).collect();
    if !tickets.is_empty() {
        board::section(
            ui,
            &theme,
            tl("Tickets"),
            &tn(tickets.len() as u64, "{n} linked", "{n} linked"),
            |ui| {
                let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
                for t in &tickets {
                    *by_status.entry(t.0.as_str()).or_default() += 1;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    for (status, n) in &by_status {
                        board::chip(
                            ui,
                            &theme,
                            &format!("{n} · {status}"),
                            super::super::tones::ticket_status(status, &theme.tones),
                        );
                    }
                });
                let spent: u32 = tickets.iter().filter_map(|t| t.1).sum();
                let estimated: u32 = tickets.iter().filter_map(|t| t.2).sum();
                if estimated > 0 || spent > 0 {
                    ui.add_space(10.0);
                    let hours = |s: u32| format!("{:.0} h", f64::from(s) / 3600.0);
                    ui.horizontal(|ui| {
                        ui.label(muted(&theme, tl("Time spent")));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(muted(
                                &theme,
                                tf("/ {h} estimated", &[("h", &hours(estimated))]),
                            ));
                            ui.label(semibold(hours(spent), theme.text_size));
                        });
                    });
                    let fraction = if estimated > 0 {
                        spent as f32 / estimated as f32
                    } else {
                        0.0
                    };
                    let color = if fraction > 1.0 {
                        theme.tones.red.fg
                    } else {
                        theme.palette.accent
                    };
                    super::super::widgets::bar(ui, &theme, fraction.min(1.0), color, 6.0);
                }
            },
        );
    }
    board::section(ui, &theme, tl("People"), "", |ui| {
        let mut by_author: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for i in &m.items {
            let entry = by_author
                .entry(split_person(&i.author).0.to_string())
                .or_default();
            entry.0 += 1;
            if i.stand == Stand::Delivered {
                entry.1 += 1;
            }
        }
        let mut list: Vec<(String, (usize, usize))> = by_author.into_iter().collect();
        list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        for (name, (n, done)) in list {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                avatar(ui, &theme, &name, 22.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(&name).size(theme.text_size - 0.5))
                        .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(muted(
                        &theme,
                        tf(
                            "{done} / {n}",
                            &[("done", &done.to_string()), ("n", &n.to_string())],
                        ),
                    ));
                });
            });
        }
    });
}

/// The scope (MRs created so far) and the MRs delivered, day by day, from the
/// first MR to the due date (or the projection), today and the due date marked.
pub(super) fn burn_up(
    ui: &mut Ui,
    theme: &Theme,
    m: &Milestone,
    weekly: &[u32],
    now: DateTime<Utc>,
) {
    let today = now.date_naive();
    // From the first MR, two months back at most, a week at least.
    let start = m
        .items
        .iter()
        .filter_map(|i| i.created.map(|c| c.date_naive()))
        .min()
        .unwrap_or(today - Duration::days(14))
        .min(today - Duration::days(7))
        .max(today - Duration::days(60));
    let eta = projection(m, weekly, now);
    let end = [Some(today + Duration::days(3)), m.due, eta]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(today);
    let end = end.min(today + Duration::days(120));
    let span = (end - start).num_days().max(1) as f32;
    let total = m.items.len().max(1);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 150.0), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(26.0, 8.0),
        rect.max - Vec2::new(6.0, 18.0),
    );
    let x_of = |d: NaiveDate| plot.left() + plot.width() * (d - start).num_days() as f32 / span;
    let top = (total as f32 * 1.15).ceil().max(2.0);
    let y_of = |n: f32| plot.bottom() - plot.height() * n / top;
    // Grid and its labels.
    for k in 0..=3 {
        let n = top * k as f32 / 3.0;
        let y = y_of(n);
        ui.painter().hline(
            plot.x_range(),
            y,
            Stroke::new(1.0, theme.palette.border_soft),
        );
        board::paint_text(
            ui,
            egui::pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            &format!("{:.0}", n),
            FontId::proportional(theme.text_size - 3.0),
            theme.palette.text_muted,
            22.0,
        );
    }
    let mut scope = Vec::new();
    let mut done = Vec::new();
    let mut day = start;
    while day <= today.min(end) {
        let created = m
            .items
            .iter()
            .filter(|i| i.created.is_none_or(|c| c.date_naive() <= day))
            .count();
        let delivered = m
            .items
            .iter()
            .filter(|i| i.delivered.is_some_and(|d| d.date_naive() <= day))
            .count();
        scope.push(egui::pos2(x_of(day), y_of(created as f32)));
        done.push(egui::pos2(x_of(day), y_of(delivered as f32)));
        day += Duration::days(1);
    }
    let muted_line = theme.palette.text_secondary;
    if scope.len() >= 2 {
        ui.painter().add(egui::Shape::line(
            scope.clone(),
            Stroke::new(2.0, muted_line),
        ));
        ui.painter().add(egui::Shape::line(
            done.clone(),
            Stroke::new(2.5, theme.tones.green.fg),
        ));
    }
    if let (Some(last_scope), Some(last_done)) = (scope.last().copied(), done.last().copied()) {
        // The scope ahead, dashed; the projection to it.
        dashed(
            ui,
            last_scope,
            egui::pos2(x_of(end), last_scope.y),
            muted_line,
        );
        if let Some(eta) = eta.filter(|e| *e <= end) {
            let target = egui::pos2(x_of(eta), last_scope.y);
            dashed(ui, last_done, target, theme.tones.green.fg);
            ui.painter()
                .circle_stroke(target, 3.5, Stroke::new(1.5, theme.tones.green.fg));
        }
        ui.painter()
            .circle_filled(last_done, 3.5, theme.tones.green.fg);
    }
    // Today and the due date; a label too close to another is left out.
    let today_x = x_of(today);
    let due_x = m.due.filter(|d| *d >= start && *d <= end).map(x_of);
    ui.painter().vline(
        today_x,
        plot.y_range(),
        Stroke::new(1.0, theme.palette.border),
    );
    let small = FontId::proportional(theme.text_size - 3.0);
    let start_w = board::paint_text(
        ui,
        egui::pos2(plot.left(), rect.bottom() - 7.0),
        Align2::LEFT_CENTER,
        &start.format("%d/%m").to_string(),
        small.clone(),
        theme.palette.text_muted,
        40.0,
    );
    let clear_of_due = due_x.is_none_or(|x| (x - today_x).abs() >= 44.0);
    if clear_of_due && today_x - plot.left() >= start_w + 24.0 {
        board::paint_text(
            ui,
            egui::pos2(today_x, rect.bottom() - 7.0),
            Align2::CENTER_CENTER,
            tl("today"),
            small,
            theme.palette.text_muted,
            50.0,
        );
    }
    if let (Some(due), Some(x)) = (m.due, due_x) {
        let mut y = plot.top();
        while y < plot.bottom() {
            ui.painter().vline(
                x,
                y..=(y + 4.0).min(plot.bottom()),
                Stroke::new(1.5, theme.tones.amber.fg),
            );
            y += 7.0;
        }
        let align = if x > plot.right() - 30.0 {
            Align2::RIGHT_CENTER
        } else {
            Align2::CENTER_CENTER
        };
        board::paint_text(
            ui,
            egui::pos2(x, rect.bottom() - 7.0),
            align,
            &due.format("%d/%m").to_string(),
            fonts::medium(theme.text_size - 3.0),
            theme.tones.amber.fg,
            50.0,
        );
    }
    ui.add_space(6.0);
    board::legend(
        ui,
        theme,
        &[
            (muted_line, tl("Scope").to_string()),
            (theme.tones.green.fg, tl("Delivered").to_string()),
            (theme.tones.amber.fg, tl("Due date").to_string()),
        ],
    );
}

pub(in crate::gui) fn dashed(ui: &Ui, from: egui::Pos2, to: egui::Pos2, color: Color32) {
    let length = from.distance(to);
    if length < 1.0 {
        return;
    }
    let dir = (to - from) / length;
    let mut t = 0.0;
    while t < length {
        let a = from + dir * t;
        let b = from + dir * (t + 4.0).min(length);
        ui.painter().line_segment([a, b], Stroke::new(1.5, color));
        t += 8.0;
    }
}

/// The milestone's MRs as Markdown release notes, delivered first.
pub(super) fn release_notes(m: &Milestone, last: &str) -> String {
    let mut out = format!("## {}\n\n", m.name());
    for stand in Stand::ALL {
        let items: Vec<&Item> = m.items.iter().filter(|i| i.stand == stand).collect();
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("### {}\n\n", stand.label(last)));
        for i in items {
            out.push_str(&format!(
                "- !{} {} ({})\n",
                i.id,
                i.title,
                split_person(&i.author).0
            ));
        }
        out.push('\n');
    }
    out
}

/// The rail's badge: the MRs left in the next milestone with a due date, in the
/// tone of its outlook: red when past due or projected late, orange when due
/// within 3 days with MRs back on their author or in draft, amber when the
/// projection lands 2 days or less before the date (or cannot be made), green
/// when on schedule.
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let all = g
        .memo
        .milestones
        .get(&g.app.project_id, (), || milestones(&g.app));
    // Sorted by due date: the first undone with a date is the next.
    let m = all
        .iter()
        .find(|m| !m.title.is_empty() && !m.done() && m.due.is_some())?;
    let due = m.due?;
    let left = m.items.len() - m.delivered();
    let today = now.date_naive();
    let days = (due - today).num_days();
    let slip = projection(
        m,
        &weekly_merges(&g.app, g.details.project_ready(), now),
        now,
    )
    .map(|p| (p - due).num_days());
    let stuck = m
        .items
        .iter()
        .filter(|i| matches!(i.stand, Stand::OnAuthor | Stand::Draft))
        .count();
    let tones = &g.theme.tones;
    let tone = if days < 0 || slip.is_some_and(|s| s > 0) {
        tones.red
    } else if days <= 3 && stuck > 0 {
        tones.orange
    } else if slip.is_none_or(|s| s >= -2) {
        tones.amber
    } else {
        tones.green
    };
    let (due_text, _) = due_text(m.due, now, false, &g.theme);
    let mut hint = format!(
        "{} · {} · {}",
        m.title,
        tn(left as u64, "{n} MR left", "{n} MRs left"),
        due_text
    );
    hint.push('\n');
    hint.push_str(&match slip {
        None => tl("Not enough merge history yet: no forecast").to_string(),
        Some(s) if s > 0 => tn(
            s as u64,
            "Projected {n} day late",
            "Projected {n} days late",
        ),
        Some(0) => tl("Projected on its due date").to_string(),
        Some(s) => tn(
            (-s) as u64,
            "Projected {n} day early",
            "Projected {n} days early",
        ),
    });
    if stuck > 0 {
        hint.push('\n');
        hint.push_str(&tn(
            stuck as u64,
            "{n} MR back on its author or in draft",
            "{n} MRs back on their author or in draft",
        ));
    }
    Some(Badge {
        n: left,
        tone,
        hint,
    })
}
