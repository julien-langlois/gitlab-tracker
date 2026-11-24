//! Where the time goes: the steps card, drafts, review rounds.

use super::*;

/// The median path cut into its steps: a bar, then one line per step.
pub(super) fn steps_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
) {
    let mut note = tn(
        d.merged.len() as u64,
        "median of {n} merged MR",
        "median of {n} merged MRs",
    );
    if before.is_some() {
        note.push_str(" · ");
        note.push_str(&tf(
            "thin bar: {period}",
            &[("period", &period.before_label())],
        ));
    }
    board::card(ui, theme, tl("Where the time goes"), &note, 268.0, |ui| {
        let total: f64 = d.medians.iter().flatten().sum();
        if total <= 0.0 {
            board::empty(ui, theme, tl("No MR merged in the period."));
            return;
        }
        if let Some((stage, share)) = slowest(&d.medians) {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &tf(
                    "Most of the time: {stage}, {share} of the way",
                    &[
                        ("stage", &stage.label().to_lowercase()),
                        ("share", &board::percent(share)),
                    ],
                ),
                fonts::semibold(theme.text_size - 0.5),
                // A wait in its colour, the time to cut; work in the text's.
                if stage.is_wait() {
                    stage.tone(theme).fg
                } else {
                    theme.palette.text
                },
                line.width(),
            );
            ui.add_space(6.0);
        }
        // A step under 1 % of the way would be a sliver: it stays in the
        // lines below.
        let parts: Vec<(f32, Color32)> = Stage::ALL
            .iter()
            .map(|s| {
                let v = d.medians[*s as usize].unwrap_or(0.0);
                let v = if v / total < 0.01 { 0.0 } else { v };
                (v as f32, s.tone(theme).fg)
            })
            .collect();
        board::stacked_bar(ui, theme, &parts, 16.0, 2.0);
        ui.add_space(10.0);
        let then = before.map(|b| b.medians);
        let max = d
            .medians
            .iter()
            .chain(then.iter().flatten())
            .flatten()
            .fold(0.0_f64, |a, b| a.max(*b));
        let samples = (d.merged.len(), before.map_or(0, |b| b.merged.len()));
        for stage in Stage::ALL {
            let value = d.medians[stage as usize];
            let (line, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            let tone = stage.tone(theme);
            ui.painter().circle_filled(
                egui::pos2(line.left() + 4.0, line.center().y),
                4.0,
                tone.fg,
            );
            let label_w = (line.width() * 0.42).min(200.0);
            board::paint_text(
                ui,
                egui::pos2(line.left() + 14.0, line.center().y),
                Align2::LEFT_CENTER,
                stage.label(),
                FontId::proportional(theme.text_size - 0.5),
                theme.palette.text_secondary,
                label_w - 14.0,
            );
            let text = value.map_or_else(|| board::dash().to_string(), board::duration_text);
            let share = value
                .filter(|_| total > 0.0)
                .map(|v| board::percent(v / total))
                .unwrap_or_default();
            let value_w = 92.0;
            board::paint_text(
                ui,
                egui::pos2(line.right(), line.center().y),
                Align2::RIGHT_CENTER,
                &format!("{text}  {share}"),
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text,
                value_w,
            );
            let track = Rect::from_min_max(
                egui::pos2(line.left() + label_w + 6.0, line.center().y - 3.0),
                egui::pos2(line.right() - value_w - 10.0, line.center().y + 3.0),
            );
            if track.width() > 20.0 {
                ui.painter()
                    .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
                if let Some(v) = value.filter(|_| max > 0.0) {
                    let mut fill = track;
                    fill.set_width((track.width() * (v / max) as f32).max(3.0));
                    ui.painter()
                        .rect_filled(fill, CornerRadius::same(3), tone.fg);
                }
                // The period before: a thin bar under it.
                if let Some(v) = then.and_then(|m| m[stage as usize]).filter(|_| max > 0.0) {
                    let ghost = Rect::from_min_size(
                        egui::pos2(track.left(), track.bottom() + 3.0),
                        Vec2::new((track.width() * (v / max) as f32).max(2.0), 2.0),
                    );
                    ui.painter().rect_filled(
                        ghost,
                        CornerRadius::same(1),
                        theme.palette.text_muted.gamma_multiply(0.6),
                    );
                }
            }
            let mut tip = Tip::new().title(stage.label()).text(stage.hint());
            if let (Some(now), Some(was)) = (value, then.and_then(|m| m[stage as usize])) {
                // Every step is better shorter.
                let (text, color) = versus_note(
                    theme,
                    versus(now, was, samples, false, now.max(was) * 0.05),
                    &board::duration_text(was),
                    period,
                );
                // The arrow by the value, in its colour.
                if !text.starts_with('=') {
                    board::paint_text(
                        ui,
                        egui::pos2(line.right() - value_w - 2.0, line.center().y),
                        Align2::RIGHT_CENTER,
                        &text[..text.char_indices().nth(1).map_or(text.len(), |(i, _)| i)],
                        FontId::proportional(theme.text_size - 2.0),
                        color,
                        12.0,
                    );
                }
                tip = tip.dot(color, text);
            }
            response.rich(tip);
        }
        drafts_caption(ui, theme, d);
    });
}

/// "3 of 18 opened as drafts, 2 reviewed along the way".
pub(super) fn drafts_text(d: &Delivery) -> Option<String> {
    (d.drafted > 0).then(|| {
        tn(
            d.drafted as u64,
            "{n} of {total} opened as a draft, {along} reviewed along the way",
            "{n} of {total} opened as drafts, {along} reviewed along the way",
        )
        .replace("{total}", &(d.drafted + d.never_draft).to_string())
        .replace("{along}", &d.reviewed_in_draft.to_string())
    })
}

/// "15 never drafts: development counted up to the review request".
pub(super) fn never_draft_text(d: &Delivery) -> Option<String> {
    (d.never_draft > 0).then(|| {
        tn(
            d.never_draft as u64,
            "{n} never a draft: development counted up to the review request",
            "{n} never drafts: development counted up to the review request",
        )
    })
}

/// Under the steps: how many MRs were drafts reviewed along the way, and how
/// many never were, whose development cannot be told from their wait.
pub(super) fn drafts_caption(ui: &mut Ui, theme: &Theme, d: &Delivery) {
    let known = d.drafted + d.never_draft;
    if known == 0 {
        return;
    }
    ui.add_space(6.0);
    if let Some(text) = drafts_text(d) {
        ui.add(egui::Label::new(muted(theme, &text).size(theme.text_size - 1.5)).wrap())
            .rich(Tip::new().title(tl("Drafts")).text(tl(
                "An MR opened as a draft when the work starts is in development until it is marked ready. Comments while a draft are the review along the way: they count here, not as the first review nor as rounds.",
            )));
    }
    if let Some(text) = never_draft_text(d) {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .color(theme.tones.amber.fg)
                    .size(theme.text_size - 1.5),
            )
            .wrap(),
        )
        .rich(Tip::new().title(tl("Never a draft")).text(tl(
            "Nothing in GitLab tells when the work on these MRs ended. Their development ends at the first review request: when the reviewers were asked at the opening, the writing counts as waiting for a review. Open the MRs as drafts and mark them ready to split them.",
        )));
    }
}

pub(super) fn rounds_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
    picked: &mut Option<String>,
) {
    let r = &d.rounds;
    board::card(
        ui,
        theme,
        tl("Review rounds"),
        tl("pushes after the first review"),
        268.0,
        |ui| {
            if r.reviewed == 0 {
                board::empty(ui, theme, tl("No reviewed MR merged in the period."));
                return;
            }
            let share = r.first_time as f64 / r.reviewed as f64;
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
            let tone = if share >= 0.6 {
                theme.tones.green
            } else if share >= 0.35 {
                theme.tones.amber
            } else {
                theme.tones.red
            };
            let w = board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &board::percent(share),
                fonts::semibold(26.0),
                tone.fg,
                line.width() * 0.4,
            );
            board::paint_text(
                ui,
                egui::pos2(line.left() + w + 10.0, line.center().y),
                Align2::LEFT_CENTER,
                &tf(
                    "approved at the first round · median {n}",
                    &[(
                        "n",
                        &r.median
                            .map(|m| format!("{m:.1}").replace(".0", ""))
                            .unwrap_or_default(),
                    )],
                ),
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_secondary,
                line.width() - w - 10.0,
            );
            if let Some(b) = before.map(|b| &b.rounds).filter(|b| b.reviewed > 0) {
                let then = b.first_time as f64 / b.reviewed as f64;
                let (text, color) = versus_note(
                    theme,
                    versus(share, then, (r.reviewed, b.reviewed), true, 0.005),
                    &board::percent(then),
                    period,
                );
                ui.label(RichText::new(text).size(theme.text_size - 1.5).color(color));
            }
            ui.add_space(8.0);
            // One column per round count.
            let labels = [tl("0"), tl("1"), tl("2"), tl("3+")];
            let most = *r.histogram.iter().max().unwrap_or(&1).max(&1);
            let (area, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 86.0), Sense::hover());
            let col_w = area.width() / 4.0;
            let t = &theme.tones;
            let colors = [t.green.fg, t.teal.fg, t.amber.fg, t.red.fg];
            for (i, n) in r.histogram.iter().enumerate() {
                let x = area.left() + i as f32 * col_w;
                let bar_h = 56.0 * *n as f32 / most as f32;
                let bar = Rect::from_min_max(
                    egui::pos2(x + col_w * 0.22, area.top() + 14.0 + 56.0 - bar_h.max(2.0)),
                    egui::pos2(x + col_w * 0.78, area.top() + 70.0),
                );
                ui.painter().rect_filled(
                    bar,
                    CornerRadius::same(3),
                    colors[i].gamma_multiply(0.85),
                );
                board::paint_text(
                    ui,
                    egui::pos2(bar.center().x, bar.top() - 7.0),
                    Align2::CENTER_CENTER,
                    &n.to_string(),
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text,
                    col_w,
                );
                board::paint_text(
                    ui,
                    egui::pos2(bar.center().x, area.bottom() - 7.0),
                    Align2::CENTER_CENTER,
                    labels[i],
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    col_w,
                );
            }
            ui.add_space(6.0);
            if r.ping_pong.is_empty() {
                board::caption(ui, theme, tl("No open MR goes back and forth."));
                return;
            }
            board::caption(ui, theme, tl("Open MRs going back and forth:"));
            ui.add_space(2.0);
            for (id, title, n) in r.ping_pong.iter().take(3) {
                let (rect, response) = board::row(ui, theme, 26.0, false);
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 4.0, rect.center().y),
                    &format!("!{id}"),
                    board::id_tone(theme),
                    true,
                );
                let rounds = tn(u64::from(*n), "{n} round", "{n} rounds");
                let rw = board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 6.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &rounds,
                    FontId::proportional(theme.text_size - 1.5),
                    theme.tones.amber.fg,
                    90.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + w + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text,
                    rect.width() - w - rw - 26.0,
                );
                if response.clicked() {
                    *picked = Some(id.clone());
                }
            }
        },
    );
}
