//! Signals about the team's load: overloaded reviewers, lone owners, long waits.

use super::*;

/// A line of the Signals card: its dot, its sentence, and the MR it names
/// (id, title), shown on hover and opened on a click.
pub(super) struct Signal {
    pub(super) color: Color32,
    pub(super) text: String,
    pub(super) mr: Option<(String, String)>,
}

/// What stands out, in sentences: late reviews, an unbalanced load, someone
/// free, the trend.
pub(super) fn signals(
    analysis: &Analysis,
    now: DateTime<Utc>,
    reminder: f64,
    theme: &Theme,
) -> Vec<Signal> {
    let line = |color: Color32, text: String| Signal {
        color,
        text,
        mr: None,
    };
    let t = &theme.tones;
    let mut out = Vec::new();
    let oldest = analysis
        .people
        .iter()
        .flat_map(|p| p.reviews.iter().map(move |r| (p, r)))
        .filter_map(|(p, r)| r.since.map(|s| (p, r, board::days_between(s, now))))
        .max_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((p, r, days)) = oldest.filter(|o| o.2 >= reminder) {
        out.push(Signal {
            mr: Some((r.id.clone(), r.title.clone())),
            ..line(
                t.red.fg,
                tf(
                    "!{id} has waited for {name}'s review for {time}, past the reminder delay.",
                    &[
                        ("id", &r.id),
                        ("name", board::first_name(&p.name)),
                        ("time", &board::duration_text(days)),
                    ],
                ),
            )
        });
    }
    if let Some((who, share)) = busiest(&analysis.matrix).filter(|b| b.1 >= 0.4) {
        let name = analysis
            .people
            .iter()
            .find(|p| p.login == who)
            .map_or(who.clone(), |p| p.name.clone());
        out.push(line(
            t.amber.fg,
            tf(
                "{name} gets {share} of the review requests.",
                &[
                    ("name", board::first_name(&name)),
                    ("share", &board::percent(share)),
                ],
            ),
        ));
    }
    if let Some(free) = analysis
        .people
        .iter()
        .filter(|p| p.level == Level::Available)
        .min_by(|a, b| a.load.total_cmp(&b.load))
    {
        out.push(line(
            t.green.fg,
            tf(
                "{name} has no review waiting: they can take the next one.",
                &[("name", board::first_name(&free.name))],
            ),
        ));
    }
    if let Some(slow) = analysis
        .people
        .iter()
        .filter(|p| p.level != Level::Bot)
        .filter_map(|p| p.response.map(|r| (p, r)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, r)| analysis.response.is_some_and(|m| *r > m * 1.5))
    {
        out.push(line(
            t.blue.fg,
            tf(
                "{name} answers a review request in {time}, the slowest of the team.",
                &[
                    ("name", board::first_name(&slow.0.name)),
                    ("time", &board::duration_text(slow.1)),
                ],
            ),
        ));
    }
    if out.is_empty() {
        out.push(line(
            t.green.fg,
            tl("Nothing stands out: the reviews flow.").to_string(),
        ));
    }
    out
}

pub(super) fn signals_card(
    ui: &mut Ui,
    theme: &Theme,
    analysis: &Analysis,
    now: DateTime<Utc>,
    reminder: f64,
    open_mr: &mut Option<String>,
) {
    board::card(ui, theme, tl("Signals"), "", 250.0, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        for (index, Signal { color, text, mr }) in signals(analysis, now, reminder, theme)
            .into_iter()
            .enumerate()
        {
            let frame = egui::Frame::new()
                .fill(theme.palette.raised.gamma_multiply(0.7))
                .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
                .corner_radius(CornerRadius::same(theme.radii.medium))
                .inner_margin(egui::Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.vertical(|ui| {
                            ui.add_space(5.0);
                            super::super::widgets::dot(ui, color, 7.0);
                        });
                        ui.add(
                            egui::Label::new(RichText::new(text).size(theme.text_size - 0.5))
                                .wrap()
                                .selectable(false),
                        );
                    });
                });
            let Some((id, title)) = mr else {
                continue;
            };
            let rect = frame.response.rect;
            let response = ui
                .interact(rect, ui.scope_id().with(("signal", index)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let response = Tip::new()
                .mr(&id, &title)
                .hint(tl("Open its page"))
                .show(response);
            if response.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    egui::Stroke::new(1.0, theme.palette.text_muted),
                    egui::StrokeKind::Inside,
                );
            }
            if response.clicked() {
                *open_mr = Some(id);
            }
        }
    });
}

// ── Inspector ────────────────────────────────────────────────────────────────
