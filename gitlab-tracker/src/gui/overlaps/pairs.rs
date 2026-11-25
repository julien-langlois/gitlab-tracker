//! The pairs card: two MRs that overlap, the order to merge them, their bands.

use super::*;

pub(super) fn pairs_card(
    ui: &mut Ui,
    theme: &Theme,
    overlaps: &Overlaps,
    list: &[&Pair],
    pair: Option<&(String, String)>,
    picked: &mut Option<(String, String)>,
) {
    board::card(
        ui,
        theme,
        tl("Pairs at risk"),
        tl("the riskiest first"),
        0.0,
        |ui| {
            if list.is_empty() {
                board::empty(ui, theme, tl("No pair of open MRs in this view."));
                return;
            }
            for p in list {
                let selected =
                    pair.is_some_and(|(a, b)| (a == &p.a && b == &p.b) || (a == &p.b && b == &p.a));
                let (rect, response) = board::row(ui, theme, 46.0, selected);
                // Only the rows in view are drawn: a busy project has thousands.
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                if !selected {
                    board::row_rule(ui, theme, rect);
                }
                let top = rect.top() + 14.0;
                let mut x = rect.left() + 8.0;
                x += board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(x, top),
                    &format!("!{}", p.a),
                    board::id_tone(theme),
                    true,
                ) + 6.0;
                x += board::paint_text(
                    ui,
                    egui::pos2(x, top),
                    Align2::LEFT_CENTER,
                    "×",
                    FontId::proportional(theme.text_size),
                    theme.palette.text_muted,
                    14.0,
                ) + 6.0;
                x += board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(x, top),
                    &format!("!{}", p.b),
                    board::id_tone(theme),
                    true,
                ) + 10.0;
                let level = p.level();
                let galley = ui.painter().layout_no_wrap(
                    level.label().to_string(),
                    fonts::medium(theme.text_size - 2.5),
                    level.tone(theme).fg,
                );
                let tag_w = galley.size().x + 12.0;
                board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.right() - 8.0 - tag_w, top),
                    level.label(),
                    level.tone(theme),
                    false,
                );
                let titles = format!(
                    "{} · {}",
                    overlaps.mr(&p.a).map_or("", |m| m.title.as_str()),
                    overlaps.mr(&p.b).map_or("", |m| m.title.as_str())
                );
                board::paint_text(
                    ui,
                    egui::pos2(x, top),
                    Align2::LEFT_CENTER,
                    &titles,
                    FontId::proportional(theme.text_size - 1.5),
                    theme.palette.text_muted,
                    rect.right() - 16.0 - tag_w - x,
                );
                let files = p
                    .files
                    .iter()
                    .map(|(f, _)| f.rsplit('/').next().unwrap_or(f))
                    .collect::<Vec<_>>()
                    .join(", ");
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 8.0, rect.top() + 33.0),
                    Align2::LEFT_CENTER,
                    &format!(
                        "{} · {files}",
                        tn(p.files.len() as u64, "{n} file", "{n} files")
                    ),
                    egui::FontId::monospace(theme.text_size - 2.5),
                    theme.palette.text_secondary,
                    rect.width() - 16.0,
                );
                if response.clicked() {
                    *picked = Some((p.a.clone(), p.b.clone()));
                }
            }
        },
    );
}

// ── Inspector ────────────────────────────────────────────────────────────────

/// Which of two MRs to merge first (approved and mergeable, then the
/// smaller), the other, and why.
pub(super) fn merge_order<'a>(ma: &'a Mr, mb: &'a Mr) -> (&'a Mr, &'a Mr, &'static str) {
    let score = |m: &Mr| (!(m.approved && m.mergeable), !m.approved, m.lines);
    let (first, second) = if score(ma) <= score(mb) {
        (ma, mb)
    } else {
        (mb, ma)
    };
    let why = if first.approved && !second.approved {
        tl("already approved")
    } else if first.lines < second.lines {
        tl("the smaller one")
    } else {
        tl("the closest to merging")
    };
    (first, second, why)
}

/// An MR of the pair: its colour down the left edge, its id and title, who,
/// its effort and whether it is approved. Clickable.
pub(super) fn pair_mr(ui: &mut Ui, theme: &Theme, m: &Mr, color: Color32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::click());
    let fill = if response.hovered() {
        theme.palette.raised
    } else {
        theme.palette.panel
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
        CornerRadius::same(2),
        color,
    );
    let inner = rect.shrink2(Vec2::new(12.0, 0.0));
    let w = board::paint_tag(
        ui,
        theme,
        egui::pos2(inner.left(), rect.top() + 16.0),
        &format!("!{}", m.id),
        board::id_tone(theme),
        true,
    );
    board::paint_text(
        ui,
        egui::pos2(inner.left() + w + 8.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        &m.title,
        fonts::medium(theme.text_size - 0.5),
        theme.palette.text,
        inner.width() - w - 8.0,
    );
    let effort = m
        .effort
        .map(|e| super::super::tones::effort(e, &theme.tones).0)
        .unwrap_or_default();
    let (state, state_color) = if m.approved {
        (tl("approved"), theme.tones.green.fg)
    } else {
        (tl("not approved"), theme.tones.amber.fg)
    };
    let who = format!("{} · {effort} · ", split_person(&m.author).0);
    let font = FontId::proportional(theme.text_size - 2.0);
    let left = board::paint_text(
        ui,
        egui::pos2(inner.left(), rect.top() + 36.0),
        Align2::LEFT_CENTER,
        &who,
        font.clone(),
        theme.palette.text_muted,
        inner.width(),
    );
    board::paint_text(
        ui,
        egui::pos2(inner.left() + left, rect.top() + 36.0),
        Align2::LEFT_CENTER,
        state,
        font,
        state_color,
        inner.width() - left,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl("Open its page"))
}

/// A step of the suggestion: its number in a round, then `text`.
pub(super) fn step(ui: &mut Ui, theme: &Theme, n: u32, first: bool, text: egui::text::LayoutJob) {
    // The text wraps beside the number, never under it.
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        if first {
            ui.painter()
                .circle_filled(rect.center(), 10.0, theme.palette.accent_fill);
        } else {
            ui.painter()
                .circle_stroke(rect.center(), 9.5, Stroke::new(1.0, theme.palette.border));
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            n.to_string(),
            fonts::semibold(theme.text_size - 2.0),
            if first {
                Color32::WHITE
            } else {
                theme.palette.text_secondary
            },
        );
        ui.vertical(|ui| {
            ui.add_space(1.0);
            ui.add(egui::Label::new(text).wrap());
        });
    });
}

/// The pair picked and what to do about it, over the page's width, in the
/// colour of how much they overlap: the two MRs, then the order to merge them
/// in, with the buttons to act on it. Nothing without a pair.
pub(super) fn pair_band(
    g: &mut GuiApp,
    ui: &mut Ui,
    overlaps: &Overlaps,
    pair: Option<&(String, String)>,
) {
    let theme = g.theme;
    let Some((a, b)) = pair else {
        return;
    };
    let (Some(ma), Some(mb), Some(p)) = (overlaps.mr(a), overlaps.mr(b), overlaps.pair(a, b))
    else {
        return;
    };
    let level = p.level();
    let tone = level.tone(&theme);
    let (first, second, why) = merge_order(ma, mb);
    let target = g
        .app
        .mrs
        .iter()
        .find(|m| m.id == second.id)
        .map(|m| m.target_branch.clone())
        .unwrap_or_default();
    let (ca, cb) = (theme.tones.blue.fg, theme.tones.purple.fg);
    let mut open = None;
    let mut diff = false;
    let mut message = false;
    ui.add_space(GAP);
    egui::Frame::new()
        .fill(tone.bg.gamma_multiply(0.45))
        .stroke(Stroke::new(1.0, tone.fg.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            board::split(
                ui,
                0.6,
                380.0,
                280.0,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            egui::RichText::new(tl("Pair picked").to_uppercase())
                                .font(fonts::medium(theme.text_size - 2.5))
                                .color(tone.fg),
                        );
                        board::chip(ui, &theme, level.label(), tone);
                        ui.label(muted(
                            &theme,
                            tn(
                                p.files.len() as u64,
                                "{n} file in common",
                                "{n} files in common",
                            ),
                        ));
                    });
                    ui.add_space(8.0);
                    board::columns(ui, 2, |ui, i| {
                        let (m, color) = if i == 0 { (ma, ca) } else { (mb, cb) };
                        if pair_mr(ui, &theme, m, color).clicked() {
                            open = Some(m.id.clone());
                        }
                    });
                },
                |ui| {
                    ui.label(
                        egui::RichText::new(tl("Suggestion").to_uppercase())
                            .font(fonts::medium(theme.text_size - 2.5))
                            .color(theme.palette.text_muted),
                    );
                    ui.add_space(8.0);
                    let text = theme.palette.text;
                    let quiet = theme.palette.text_muted;
                    let font = FontId::proportional(theme.text_size - 0.5);
                    let job = |parts: &[(&str, Color32, bool)]| {
                        let mut job = egui::text::LayoutJob::default();
                        for (part, color, mono) in parts {
                            let font = if *mono {
                                FontId::monospace(theme.text_size - 1.5)
                            } else {
                                font.clone()
                            };
                            job.append(part, 0.0, egui::TextFormat::simple(font, *color));
                        }
                        job
                    };
                    let one = tf("Merge !{id} first", &[("id", &first.id)]);
                    let why = format!(" — {why}");
                    step(
                        ui,
                        &theme,
                        1,
                        true,
                        job(&[(&one, text, false), (&why, quiet, false)]),
                    );
                    ui.add_space(4.0);
                    let two = tf("Then rebase !{id}", &[("id", &second.id)]);
                    let to = if target.is_empty() {
                        String::new()
                    } else {
                        format!(" → {target}")
                    };
                    let where_ = format!(" — {}", tl("where the files meet"));
                    step(
                        ui,
                        &theme,
                        2,
                        false,
                        job(&[
                            (&two, text, false),
                            (&to, theme.tones.blue.fg, true),
                            (&where_, quiet, false),
                        ]),
                    );
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                        diff = primary_button(ui, &theme, tl("See the changes")).clicked();
                        message = button(
                            ui,
                            &theme,
                            Some(super::super::icons::Icon::Copy),
                            tl("Copy a message"),
                            false,
                        )
                        .clicked();
                    });
                },
            );
        });
    if message {
        let files = p
            .files
            .iter()
            .map(|(f, l)| format!("- `{f}` ({})", l.label()))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!(
            "{}\n{files}\n{}",
            tf(
                "!{a} and !{b} change the same files:",
                &[("a", &ma.id), ("b", &mb.id)]
            ),
            tf(
                "Suggestion: merge !{first} first, then rebase !{second}.",
                &[("first", &first.id), ("second", &second.id)],
            ),
        );
        ui.ctx().copy_text(board::Links::of(&g.app).apply(&text));
        g.app.flash(tl("Message copied"), false);
    }
    if diff {
        g.open_mr(&first.id);
        g.mr_tab = super::super::mr_page::Tab::Changes;
        return;
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}
