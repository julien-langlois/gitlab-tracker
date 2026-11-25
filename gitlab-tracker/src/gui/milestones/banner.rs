//! The projection banner and the landing tiles.

use super::*;

pub(super) fn projection_banner(
    ui: &mut Ui,
    theme: &Theme,
    m: &Milestone,
    weekly: &[u32],
    flow_last: &str,
    now: DateTime<Utc>,
) {
    let flow_last = flow_last.to_string();
    let f = forecast(m, weekly, now);
    let eta = f.p85;
    let pace = pace_of(weekly);
    let left = left_to_merge(m);
    let held: Vec<&Item> = m
        .items
        .iter()
        .filter(|i| i.stand == Stand::OnAuthor)
        .collect();
    // The sentence says when; the tile beside it says how far from the date.
    let (tone, headline) = match (eta, m.due) {
        (None, _) => (
            theme.tones.gray,
            if weekly.iter().any(|n| *n > 0) {
                tn(
                    weekly.len() as u64,
                    "Only {n} week of merges known: a forecast needs three at least.",
                    "Only {n} weeks of merges known: a forecast needs three at least.",
                )
            } else {
                tl("No tracked MR merged in the last eight weeks: nothing to forecast from.").to_string()
            },
        ),
        (Some(eta), due) => (
            match (due, f.chance) {
                (Some(due), _) if eta <= due => theme.tones.green,
                (Some(_), Some(c)) if c >= 0.5 => theme.tones.amber,
                (Some(_), _) => theme.tones.red,
                (None, _) => theme.tones.blue,
            },
            match (f.p50, f.chance) {
                (Some(p50), Some(chance)) => tf(
                    "{chance} chance that every MR is merged by the due date: half the simulations are done by {p50}, 85 % by {eta}.",
                    &[
                        ("chance", &board::percent(chance)),
                        ("p50", &p50.format("%Y-%m-%d").to_string()),
                        ("eta", &eta.format("%Y-%m-%d").to_string()),
                    ],
                ),
                (Some(p50), None) => tf(
                    "Half the simulations have every MR merged by {p50}, 85 % by {eta}.",
                    &[
                        ("p50", &p50.format("%Y-%m-%d").to_string()),
                        ("eta", &eta.format("%Y-%m-%d").to_string()),
                    ],
                ),
                _ => tf(
                    "Every MR is merged around {eta}.",
                    &[("eta", &eta.format("%Y-%m-%d").to_string())],
                ),
            },
        ),
    };
    let method = tf(
        "{runs} simulations drawing from {weeks} weeks of merges ({list}) · {pace} a week lately · then the release to {branch}.",
        &[
            ("runs", &RUNS.to_string()),
            ("weeks", &weekly.len().to_string()),
            ("branch", &flow_last),
            (
                "list",
                &weekly
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
            ("pace", &format!("{pace:.1}")),
        ],
    );
    let detail = if held.is_empty() {
        format!(
            "{} {method}",
            tn(
                left as u64,
                "{n} MR left to merge.",
                "{n} MRs left to merge.",
            )
        )
    } else {
        let ids = held
            .iter()
            .map(|i| format!("!{}", i.id))
            .collect::<Vec<_>>()
            .join(", ");
        let held = tf(
            "{left} · back on their author, the first to unblock: {ids}",
            &[
                (
                    "left",
                    &tn(left as u64, "{n} MR left to merge", "{n} MRs left to merge"),
                ),
                ("ids", &ids),
            ],
        );
        format!("{held}\n{method}")
    };
    // The banner (two thirds) and the tile (one third) side by side, as tall
    // as the banner's text needs, the tile's figure at least; one above the
    // other on a narrow page.
    let width = ui.available_width();
    let side_by_side = width >= 560.0;
    let banner_w = if side_by_side {
        ((width - GAP) * 2.0 / 3.0).floor()
    } else {
        width
    };
    let text_left = 16.0 + 18.0 + 12.0;
    let wrap = banner_w - text_left - 16.0;
    let layout = |text: &str, font: FontId, color: Color32| {
        ui.painter().layout(text.to_owned(), font, color, wrap)
    };
    let headline = layout(
        &headline,
        fonts::semibold(theme.text_size - 0.5),
        theme.palette.text,
    );
    let detail = layout(
        &detail,
        FontId::proportional(theme.text_size - 1.0),
        theme.palette.text_secondary,
    );
    let text_h = headline.size().y + 6.0 + detail.size().y;
    let tile_min = 104.0;
    let banner_h = (text_h + 28.0).max(if side_by_side { tile_min } else { 0.0 });
    let (row, _) = ui.allocate_exact_size(
        Vec2::new(
            width,
            if side_by_side {
                banner_h
            } else {
                banner_h + GAP + tile_min
            },
        ),
        Sense::hover(),
    );
    let banner = Rect::from_min_size(row.min, Vec2::new(banner_w, banner_h));
    ui.painter().rect(
        banner,
        CornerRadius::same(theme.radii.large),
        tone.bg.gamma_multiply(0.55),
        Stroke::new(1.0, tone.fg.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    // The text block centered in the banner's height.
    let top = banner.top() + (banner_h - text_h) / 2.0;
    let icon = Rect::from_min_size(egui::pos2(banner.left() + 16.0, top), Vec2::splat(18.0));
    super::super::icons::paint(ui.painter(), icon, super::super::icons::Icon::Flag, tone.fg);
    let x = banner.left() + text_left;
    let headline_h = headline.size().y;
    ui.painter()
        .galley(egui::pos2(x, top), headline, theme.palette.text);
    ui.painter().galley(
        egui::pos2(x, top + headline_h + 6.0),
        detail,
        theme.palette.text_secondary,
    );
    // The MRs named by their id only: their titles on hover.
    if !held.is_empty() {
        let titles = held
            .iter()
            .fold(Tip::new(), |tip, i| tip.mr(&i.id, &i.title));
        ui.interact(banner, ui.scope_id().with("held_titles"), Sense::hover())
            .rich(titles);
    }
    let tile = if side_by_side {
        Rect::from_min_max(egui::pos2(banner.right() + GAP, row.top()), row.max)
    } else {
        Rect::from_min_max(egui::pos2(row.left(), banner.bottom() + GAP), row.max)
    };
    landing_tile(ui, theme, tile, eta, m.due);
}

/// How far from its due date the milestone lands, large: `34` days late in
/// red, `12` days early in green, `0` on the date; the projected date under
/// it; painted in `rect`.
pub(super) fn landing_tile(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    eta: Option<NaiveDate>,
    due: Option<NaiveDate>,
) {
    let response = ui.interact(rect, ui.scope_id().with("landing_tile"), Sense::hover());
    let t = &theme.tones;
    let (value, caption, tone) = match (eta, due) {
        (None, _) => (
            board::dash().to_string(),
            tl("no pace yet").to_string(),
            t.gray,
        ),
        (Some(_), None) => (
            board::dash().to_string(),
            tl("no due date").to_string(),
            t.gray,
        ),
        (Some(eta), Some(due)) => {
            let days = (eta - due).num_days();
            match days {
                d if d > 0 => (d.to_string(), tn(d as u64, "day late", "days late"), t.red),
                d if d < 0 => (
                    (-d).to_string(),
                    tn((-d) as u64, "day early", "days early"),
                    t.green,
                ),
                _ => ("0".to_string(), tl("on the due date").to_string(), t.green),
            }
        }
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(theme.radii.large),
        tone.bg.gamma_multiply(0.7),
        Stroke::new(1.0, tone.fg.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink2(Vec2::new(14.0, 0.0));
    // The figure and its caption, as a block centered in the tile.
    let figure = board::one_line(ui, &value, fonts::semibold(36.0), tone.fg, inner.width());
    let under = board::one_line(
        ui,
        &caption,
        fonts::medium(theme.text_size),
        tone.fg,
        inner.width(),
    );
    let landing = eta.map(|e| {
        tf(
            "85 % sure by {date}",
            &[("date", &e.format("%Y-%m-%d").to_string())],
        )
    });
    let small = landing.as_ref().map(|text| {
        board::one_line(
            ui,
            text,
            FontId::proportional(theme.text_size - 2.0),
            theme.palette.text_secondary,
            inner.width(),
        )
    });
    let block =
        figure.size().y + 2.0 + under.size().y + small.as_ref().map_or(0.0, |g| 6.0 + g.size().y);
    let mut y = rect.center().y - block / 2.0;
    let center = rect.center().x;
    let fh = figure.size();
    ui.painter()
        .galley(egui::pos2(center - fh.x / 2.0, y), figure, tone.fg);
    y += fh.y + 2.0;
    let uh = under.size();
    ui.painter()
        .galley(egui::pos2(center - uh.x / 2.0, y), under, tone.fg);
    y += uh.y + 6.0;
    if let Some(small) = small {
        let sw = small.size().x;
        ui.painter().galley(
            egui::pos2(center - sw / 2.0, y),
            small,
            theme.palette.text_secondary,
        );
    }
    if let (Some(eta), Some(due)) = (eta, due) {
        response.tip(tf(
            "Due {due}, projected {eta}",
            &[
                ("due", &due.format("%Y-%m-%d").to_string()),
                ("eta", &eta.format("%Y-%m-%d").to_string()),
            ],
        ));
    }
}
