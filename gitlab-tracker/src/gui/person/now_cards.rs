//! What waits now: the figures against the team, what waits on them, twelve weeks of reviews and merges.

use super::*;

/// The figures: what waits now, and how they compare with the team.
pub(super) fn figures(
    ui: &mut Ui,
    theme: &Theme,
    analysis: &Analysis,
    person: &Person,
    profile: &Profile,
    now: DateTime<Utc>,
    reminder: f64,
) {
    let t = &theme.tones;
    let oldest = person.oldest().map(|s| board::days_between(s, now));
    let on_them = person
        .mrs
        .iter()
        .filter(|m| m.step.actor == crate::next_step::Actor::Author)
        .count();
    let threads_mrs = person
        .mrs
        .iter()
        .filter(|m| {
            m.step
                .todos
                .iter()
                .any(|t| matches!(t, crate::next_step::Todo::AnswerThreads { .. }))
        })
        .count();
    let duration =
        |d: Option<f64>| d.map_or_else(|| board::dash().to_string(), board::duration_text);
    // Slower than the team by half again: amber; faster: green.
    let against = |mine: Option<f64>, team: Option<f64>| -> (String, Option<Color32>) {
        match (mine, team) {
            (_, Some(team_value)) => (
                tf("team: {d}", &[("d", &board::duration_text(team_value))]),
                mine.and_then(|m| {
                    if m > team_value * 1.5 {
                        Some(t.amber.fg)
                    } else if m < team_value {
                        Some(t.green.fg)
                    } else {
                        None
                    }
                }),
            ),
            _ => (tl("Nothing to compare with yet").to_string(), None),
        }
    };
    let (response_caption, response_color) = against(person.response, analysis.response);
    let (cycle_caption, cycle_color) = against(profile.cycle, profile.team_cycle);
    let figures = [
        Figure {
            label: tl("Reviews waiting on them").into(),
            value: person.reviews.len().to_string(),
            value_color: None,
            caption: match oldest {
                Some(d) => tf("the oldest: {time}", &[("time", &board::duration_text(d))]),
                None => tl("None").into(),
            },
            caption_color: oldest.filter(|d| *d >= reminder).map(|_| t.red.fg),
        },
        Figure {
            label: tl("Their open MRs").into(),
            value: person.mrs.len().to_string(),
            value_color: None,
            caption: tn(on_them as u64, "{n} waits on them", "{n} wait on them"),
            caption_color: (on_them > 0).then_some(t.amber.fg),
        },
        Figure {
            label: tl("Threads to answer").into(),
            value: person.threads.to_string(),
            value_color: None,
            caption: tn(threads_mrs as u64, "on {n} MR", "on {n} MRs"),
            caption_color: None,
        },
        Figure {
            label: tl("Review response").into(),
            value: duration(person.response),
            value_color: None,
            caption: response_caption,
            caption_color: response_color,
        },
        Figure {
            label: tl("Their MRs merged").into(),
            value: profile.merged.to_string(),
            value_color: None,
            caption: tf(
                "period before: {n}",
                &[("n", &profile.merged_before.to_string())],
            ),
            caption_color: None,
        },
        Figure {
            label: tl("Created → merged").into(),
            value: duration(profile.cycle),
            value_color: None,
            caption: cycle_caption,
            caption_color: cycle_color,
        },
    ];
    board::figures(ui, theme, &figures);
}

/// What waits on them, and what their MRs wait on.
pub(super) fn now_card(
    ui: &mut Ui,
    theme: &Theme,
    person: &Person,
    now: DateTime<Utc>,
    reminder: f64,
    go: &mut Go,
) {
    board::card(ui, theme, tl("Now"), "", 0.0, |ui| {
        let mut open = None;
        ui.label(semibold(tl("Waiting on them"), theme.text_size - 1.0));
        ui.add_space(4.0);
        team::waiting_list(ui, theme, person, now, reminder, &mut open);
        ui.add_space(10.0);
        ui.label(semibold(tl("Their MRs"), theme.text_size - 1.0));
        ui.add_space(4.0);
        if person.mrs.is_empty() {
            ui.label(muted(theme, tl("No open MR.")));
        }
        for m in &person.mrs {
            let caption = super::super::next::caption(&m.step);
            let summary = m.step.summary();
            let detail = if summary.is_empty() {
                caption
            } else {
                format!("{caption} · {summary}")
            };
            if team::line(
                ui,
                theme,
                &m.id,
                &m.title,
                &detail,
                Some(super::super::next::tone(&m.step, theme).fg),
            ) {
                open = Some(m.id.clone());
            }
        }
        if let Some(id) = open {
            *go = Go::Mr(id);
        }
    });
}

/// Twelve weeks: reviews given and MRs merged in bars, requests in a line.
pub(super) fn activity_card(ui: &mut Ui, theme: &Theme, profile: &Profile, now: DateTime<Utc>) {
    let reviews_color = theme.palette.accent;
    let merged_color = theme.tones.purple.fg;
    let asked_color = theme.tones.red.fg;
    board::card(ui, theme, tl("Activity"), tl("12 weeks"), 0.0, |ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), Sense::hover());
        let max = profile
            .weeks
            .iter()
            .flat_map(|w| [w.reviews, w.merged, w.asked])
            .max()
            .unwrap_or(0)
            .max(1) as f32;
        let painter = ui.painter();
        let plot = Rect::from_min_max(
            egui::pos2(rect.left() + 22.0, rect.top() + 6.0),
            egui::pos2(rect.right(), rect.bottom() - 2.0),
        );
        let y = |v: f32| plot.bottom() - plot.height() * v / max;
        // The grid: zero, and the top value.
        let grid = egui::Stroke::new(1.0, theme.palette.border_soft);
        painter.hline(plot.x_range(), plot.bottom(), grid);
        painter.hline(plot.x_range(), y(max), grid);
        let small = FontId::proportional(theme.text_size - 2.5);
        for v in [0.0, max] {
            painter.text(
                egui::pos2(plot.left() - 6.0, y(v)),
                Align2::RIGHT_CENTER,
                format!("{v:.0}"),
                small.clone(),
                theme.palette.text_muted,
            );
        }
        let slot = plot.width() / WEEKS as f32;
        let bar = ((slot - 6.0) / 2.0).clamp(3.0, 12.0);
        let mut points = Vec::with_capacity(WEEKS);
        for (i, w) in profile.weeks.iter().enumerate() {
            let cx = plot.left() + slot * (i as f32 + 0.5);
            for (n, color, dx) in [
                (w.reviews, reviews_color, -bar - 1.0),
                (w.merged, merged_color, 1.0),
            ] {
                if n == 0 {
                    continue;
                }
                let r = Rect::from_min_max(
                    egui::pos2(cx + dx, y(n as f32)),
                    egui::pos2(cx + dx + bar, plot.bottom()),
                );
                painter.rect_filled(
                    r,
                    CornerRadius {
                        nw: 2,
                        ne: 2,
                        sw: 0,
                        se: 0,
                    },
                    color,
                );
            }
            points.push(egui::pos2(cx, y(w.asked as f32)));
            let monday = now - Duration::weeks((WEEKS - 1 - i) as i64);
            ui.interact(
                Rect::from_center_size(
                    egui::pos2(cx, plot.center().y),
                    Vec2::new(slot, plot.height()),
                ),
                ui.scope_id().with(("week", i)),
                Sense::hover(),
            )
            .rich(
                Tip::new()
                    .title(tf(
                        "Week of {date}",
                        &[("date", &monday.format("%d/%m").to_string())],
                    ))
                    .dot(
                        reviews_color,
                        tn(
                            u64::from(w.reviews),
                            "{n} review given",
                            "{n} reviews given",
                        ),
                    )
                    .dot(
                        merged_color,
                        tn(
                            u64::from(w.merged),
                            "{n} MR of theirs merged",
                            "{n} MRs of theirs merged",
                        ),
                    )
                    .dot(
                        asked_color,
                        tn(
                            u64::from(w.asked),
                            "{n} review asked of them",
                            "{n} reviews asked of them",
                        ),
                    ),
            );
        }
        let painter = ui.painter();
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(2.0, asked_color),
        ));
        if let Some(last) = points.last() {
            painter.circle_filled(*last, 3.5, asked_color);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(muted(theme, tl("12 weeks ago")).size(theme.text_size - 2.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(muted(theme, tl("this week")).size(theme.text_size - 2.0));
            });
        });
        ui.add_space(4.0);
        board::legend(
            ui,
            theme,
            &[
                (reviews_color, tl("reviews given").to_string()),
                (merged_color, tl("their MRs merged").to_string()),
                (asked_color, tl("reviews asked of them").to_string()),
            ],
        );
        // The last four weeks: do the requests outrun the reviews?
        let last: Vec<&Week> = profile.weeks.iter().rev().take(4).collect();
        let asked: u32 = last.iter().map(|w| w.asked).sum();
        let given: u32 = last.iter().map(|w| w.reviews).sum();
        if asked > 0 || given > 0 {
            ui.add_space(6.0);
            let text = if asked >= 4 && f64::from(asked) > f64::from(given) * 1.3 {
                tf(
                    "Over the last 4 weeks, {asked} reviews were asked of them and they gave {given}: the queue grows.",
                    &[("asked", &asked.to_string()), ("given", &given.to_string())],
                )
            } else {
                tf(
                    "Over the last 4 weeks: {given} reviews given for {asked} asked.",
                    &[("asked", &asked.to_string()), ("given", &given.to_string())],
                )
            };
            board::caption(ui, theme, &text);
        }
    });
}

/// A horizontal bar: `label`, a track filled to `value / max`, `text` on the
/// right. Clickable when `link`.
#[allow(clippy::too_many_arguments)]
pub(super) fn bar(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    value: f32,
    max: f32,
    text: &str,
    color: Color32,
    link: bool,
) -> egui::Response {
    let sense = if link { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), sense);
    if link && response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(4.0, 0.0)),
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let label_w = (rect.width() * 0.36).clamp(90.0, 150.0);
    let text_w = 56.0;
    let cy = rect.center().y;
    board::paint_text(
        ui,
        egui::pos2(rect.left(), cy),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.text_size - 1.0),
        if link {
            theme.palette.text
        } else {
            theme.palette.text_secondary
        },
        label_w - 8.0,
    );
    let track = Rect::from_min_max(
        egui::pos2(rect.left() + label_w, cy - 4.0),
        egui::pos2(rect.right() - text_w, cy + 4.0),
    );
    ui.painter()
        .rect_filled(track, CornerRadius::same(4), theme.palette.border_soft);
    if value > 0.0 && max > 0.0 {
        let mut fill = track;
        fill.set_width((track.width() * (value / max).min(1.0)).max(4.0));
        ui.painter().rect_filled(fill, CornerRadius::same(4), color);
    }
    board::paint_text(
        ui,
        egui::pos2(rect.right(), cy),
        Align2::RIGHT_CENTER,
        text,
        fonts::medium(theme.text_size - 1.5),
        theme.palette.text_secondary,
        text_w - 6.0,
    );
    if link {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A muted heading over a group of bars.
pub(super) fn subheading(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(muted(theme, text).size(theme.text_size - 1.5));
    ui.add_space(2.0);
}
