//! The branches as cards in flow order, after the open MRs' card; deploy freezes on them.

use super::*;

/// The branches as cards, in flow order, after a card for the open MRs; under
/// each, what it has that the next one lacks.
pub(super) fn flow_strip(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    pair: Option<(usize, usize)>,
    now: DateTime<Utc>,
    frozen: Frozen,
    picked: &mut Option<(usize, usize)>,
) {
    let n = model.branches.len() + 1;
    let arrow = 26.0;
    let width = ui.available_width();
    let card_w = ((width - arrow * (n - 1) as f32) / n as f32).floor();
    // Narrow: two per line, without the arrows.
    let per_line = if card_w >= 150.0 {
        n
    } else {
        2.max((width / 220.0).floor() as usize)
    };
    let card_w = if per_line == n {
        card_w
    } else {
        ((width - GAP * (per_line - 1) as f32) / per_line as f32).floor()
    };
    let height = 198.0;
    let mut index = 0;
    while index < n {
        let line = (n - index).min(per_line);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        for k in 0..line {
            let i = index + k;
            let gap = if per_line == n { arrow } else { GAP };
            let left = rect.left() + k as f32 * (card_w + gap);
            let card = Rect::from_min_size(egui::pos2(left, rect.top()), Vec2::new(card_w, height));
            if i == 0 {
                open_card(ui, theme, card, model);
            } else {
                branch_card(ui, theme, card, model, i - 1, pair, now, frozen, picked);
            }
            if per_line == n && k + 1 < line {
                let cx = card.right() + arrow / 2.0;
                let cy = card.top() + 60.0;
                let color = theme.palette.text_muted;
                ui.painter().line_segment(
                    [egui::pos2(cx - 7.0, cy), egui::pos2(cx + 6.0, cy)],
                    Stroke::new(1.5, color),
                );
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(cx + 1.0, cy - 5.0),
                        egui::pos2(cx + 6.0, cy),
                        egui::pos2(cx + 1.0, cy + 5.0),
                    ],
                    Stroke::new(1.5, color),
                ));
            }
        }
        index += line;
        if index < n {
            ui.add_space(GAP);
        }
    }
}

pub(super) fn card_ground(ui: &Ui, theme: &Theme, rect: Rect, accent: Color32, selected: bool) {
    let radius = CornerRadius::same(theme.radii.large);
    ui.painter().rect(
        rect,
        radius,
        theme.palette.panel,
        Stroke::new(
            1.0,
            if selected {
                theme.palette.accent.gamma_multiply(0.8)
            } else {
                theme.palette.border_soft
            },
        ),
        egui::StrokeKind::Inside,
    );
    let strip = Rect::from_min_size(
        rect.min + Vec2::new(1.0, 1.0),
        Vec2::new(rect.width() - 2.0, 3.0),
    );
    ui.painter().rect_filled(
        strip,
        CornerRadius {
            nw: theme.radii.large,
            ne: theme.radii.large,
            sw: 0,
            se: 0,
        },
        accent,
    );
}

/// A caption and its value on one line of a card: the value keeps its room,
/// the caption takes what is left.
pub(super) fn key_value(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    y: f32,
    key: &str,
    value: &str,
    color: Option<Color32>,
) {
    let font = FontId::proportional(theme.text_size - 1.5);
    let value_w = board::paint_text(
        ui,
        egui::pos2(rect.right(), y),
        Align2::RIGHT_CENTER,
        value,
        font.clone(),
        color.unwrap_or(theme.palette.text_secondary),
        rect.width() * 0.7,
    );
    board::paint_text(
        ui,
        egui::pos2(rect.left(), y),
        Align2::LEFT_CENTER,
        key,
        font,
        theme.palette.text_muted,
        rect.width() - value_w - 8.0,
    );
}

pub(super) fn open_card(ui: &Ui, theme: &Theme, rect: Rect, model: &Model) {
    card_ground(ui, theme, rect, theme.tones.gray.fg, false);
    let inner = rect.shrink2(Vec2::new(14.0, 14.0));
    let mut y = inner.top() + 10.0;
    board::paint_text(
        ui,
        egui::pos2(inner.left(), y),
        Align2::LEFT_CENTER,
        tl("Open MRs"),
        fonts::semibold(theme.text_size),
        theme.palette.text,
        inner.width(),
    );
    y += 30.0;
    let n = model.open.len();
    let w = board::paint_text(
        ui,
        egui::pos2(inner.left(), y),
        Align2::LEFT_CENTER,
        &n.to_string(),
        fonts::semibold(26.0),
        theme.palette.text,
        inner.width(),
    );
    board::paint_text(
        ui,
        egui::pos2(inner.left() + w + 8.0, y + 3.0),
        Align2::LEFT_CENTER,
        tl("in review or work"),
        FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
        inner.width() - w - 8.0,
    );
    y += 30.0;
    let ready = model.open.iter().filter(|m| m.ready).count();
    let blocked = model.open.iter().filter(|m| m.blocked).count();
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Ready to merge"),
        &ready.to_string(),
        (ready > 0).then_some(theme.tones.green.fg),
    );
    y += 22.0;
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Blocked"),
        &blocked.to_string(),
        (blocked > 0).then_some(theme.tones.red.fg),
    );
    y += 22.0;
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("In progress"),
        &(n - ready - blocked).to_string(),
        None,
    );
}

/// A deploy freeze holding now, until when (none: no end); `None`: no freeze.
pub(super) type Frozen = Option<Option<DateTime<Utc>>>;

#[allow(clippy::too_many_arguments)]
pub(super) fn branch_card(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    model: &Model,
    i: usize,
    pair: Option<(usize, usize)>,
    now: DateTime<Utc>,
    frozen: Frozen,
    picked: &mut Option<(usize, usize)>,
) {
    let b = &model.branches[i];
    let last = i + 1 == model.branches.len();
    // Production, frozen: tinted in blue, a snowflake in the corner, the
    // content still readable.
    let this_frozen = frozen.filter(|_| last);
    // The branch before it: what it would promote waits for the thaw.
    let next_frozen = frozen.is_some() && i + 2 == model.branches.len();
    let selected = pair.is_some_and(|(from, _)| from == i);
    let blue = theme.tones.blue;
    if this_frozen.is_some() {
        card_ground(ui, theme, rect, blue.fg, selected);
        let radius = CornerRadius::same(theme.radii.large);
        ui.painter().rect(
            rect.shrink(1.0),
            radius,
            blue.bg,
            Stroke::new(1.0, blue.fg.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
        let flake = Rect::from_center_size(
            rect.right_top() + Vec2::new(-30.0, 104.0),
            Vec2::splat(96.0),
        );
        super::super::icons::paint(
            &ui.painter().with_clip_rect(rect.shrink(2.0)),
            flake,
            super::super::icons::Icon::Snowflake,
            blue.fg.gamma_multiply(0.12),
        );
    } else {
        card_ground(ui, theme, rect, role_color(b.role, theme), selected);
    }
    let inner = rect.shrink2(Vec2::new(14.0, 14.0));
    let mut y = inner.top() + 10.0;
    let opacity = ui.opacity();
    // The name, the environment on the right ("frozen" in its place while
    // frozen: the tooltip of the banner names it).
    let frozen_tag = this_frozen.map(|_| tl("frozen").to_string());
    let env_w = frozen_tag
        .as_ref()
        .or(b.environments.first())
        .map_or(0.0, |env| {
            let galley = ui.painter().layout_no_wrap(
                env.clone(),
                fonts::medium(theme.text_size - 2.5),
                theme.tones.blue.fg,
            );
            let w = (galley.size().x + 12.0).min(inner.width() * 0.5);
            board::paint_tag(
                ui,
                theme,
                egui::pos2(inner.right() - w, y),
                env,
                theme.tones.blue,
                false,
            );
            w + 8.0
        });
    board::paint_text(
        ui,
        egui::pos2(inner.left(), y),
        Align2::LEFT_CENTER,
        &b.name,
        egui::FontId::monospace(theme.text_size),
        theme.palette.text,
        inner.width() - env_w,
    );
    // Under the freeze, what is deployed shows a little faded.
    if this_frozen.is_some() {
        ui.set_opacity(opacity * 0.65);
    }
    y += 30.0;
    // The state of what is deployed.
    let (state, tone) = match (&b.last, &b.pipeline) {
        (None, _) if b.environments.is_empty() => (tl("No environment"), theme.tones.gray),
        (None, _) => (tl("Never deployed"), theme.tones.gray),
        (Some(d), Some(p))
            if !p.sha.is_empty()
                && !d.sha.is_empty()
                && !p.sha.starts_with(&d.sha)
                && !d.sha.starts_with(&p.sha) =>
        {
            (tl("Newer commits not deployed"), theme.tones.amber)
        }
        (Some(_), _) => (tl("Up to date"), theme.tones.green),
    };
    board::paint_tag_within(
        ui,
        theme,
        egui::pos2(inner.left(), y),
        state,
        tone,
        false,
        inner.width(),
    );
    y += 28.0;
    let deployed = b
        .last
        .as_ref()
        .and_then(|d| board::parse_time(&d.created_at));
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Deployed"),
        &deployed.map_or_else(|| board::dash().to_string(), |d| board::ago(d, now)),
        None,
    );
    y += 22.0;
    let (pipeline, color) = match &b.pipeline {
        Some(p) => {
            let (label, tone) = super::super::tones::pipeline(p.status, &theme.tones);
            (
                format!("#{} · {}", p.id, label.to_lowercase()),
                Some(tone.fg),
            )
        }
        None => (board::dash().to_string(), None),
    };
    key_value(ui, theme, inner, y, tl("Pipeline"), &pipeline, color);
    y += 22.0;
    let sha = b
        .last
        .as_ref()
        .map(|d| d.sha.chars().take(8).collect::<String>());
    key_value(
        ui,
        theme,
        inner,
        y,
        tl("Commit"),
        &sha.unwrap_or_else(|| board::dash().to_string()),
        None,
    );
    y += 22.0;
    // The release it runs, and what reached it since.
    let version = match &b.version {
        Some((tag, 0)) => tag.clone(),
        Some((tag, n)) => tf(
            "{tag} + {mrs}",
            &[("tag", tag), ("mrs", &tn(*n as u64, "{n} MR", "{n} MRs"))],
        ),
        None => board::dash().to_string(),
    };
    key_value(ui, theme, inner, y, tl("Version"), &version, None);
    ui.set_opacity(opacity);
    // At the bottom: what waits for the next branch.
    let foot = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - 24.0), inner.max);
    if let Some(until) = this_frozen {
        // The time left is in the banner above: the card keeps the end.
        let text = match until {
            Some(until) => tf(
                "until {when}",
                &[(
                    "when",
                    &crate::snooze::when_text(until, &chrono::Local::now()),
                )],
            ),
            None => tl("frozen, with no end").to_string(),
        };
        ui.painter()
            .rect_filled(foot, CornerRadius::same(theme.radii.medium), blue.bg);
        board::paint_text(
            ui,
            foot.center(),
            Align2::CENTER_CENTER,
            &text,
            fonts::medium(theme.text_size - 2.0),
            blue.fg,
            foot.width() - 12.0,
        );
    } else if i + 1 < model.branches.len() {
        let list = model.gap(i, i + 1);
        let late = oldest(list).is_some_and(|o| board::days_between(o, now) >= LATE_DAYS);
        let (text, tone) = if list.is_empty() {
            (
                tf(
                    "In sync with {branch}",
                    &[("branch", &model.branches[i + 1].alias)],
                ),
                theme.tones.green,
            )
        } else {
            let wait = oldest(list).map(|o| board::duration_text(board::days_between(o, now)));
            let mut text = if next_frozen {
                tn(
                    list.len() as u64,
                    "{n} waits for the thaw",
                    "{n} wait for the thaw",
                )
            } else {
                tn(list.len() as u64, "{n} to promote", "{n} to promote")
            };
            if let Some(w) = wait {
                text = format!("{text} · {w}");
            }
            (
                text,
                if next_frozen {
                    blue
                } else if late {
                    theme.tones.red
                } else {
                    theme.tones.amber
                },
            )
        };
        let response = ui
            .interact(
                foot.expand2(Vec2::new(6.0, 4.0)),
                ui.scope_id().with(("gap", i)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .tip(tf(
                "What {from} has that {to} lacks",
                &[("from", &b.name), ("to", &model.branches[i + 1].name)],
            ));
        let fill = if response.hovered() {
            tone.bg.gamma_multiply(1.6)
        } else {
            tone.bg
        };
        ui.painter()
            .rect_filled(foot, CornerRadius::same(theme.radii.medium), fill);
        board::paint_text(
            ui,
            foot.center(),
            Align2::CENTER_CENTER,
            &text,
            fonts::medium(theme.text_size - 2.0),
            tone.fg,
            foot.width() - 12.0,
        );
        if response.clicked() {
            *picked = Some((i, i + 1));
        }
    } else {
        board::paint_text(
            ui,
            foot.center(),
            Align2::CENTER_CENTER,
            tl("End of the flow"),
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_muted,
            foot.width(),
        );
    }
}
