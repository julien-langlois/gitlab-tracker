//! The four tiles over the cards (blocked, my reviews, next release, oldest open MR) and the grid of cards.

use super::*;

/// Four tiles over the cards: blocked MRs, my reviews, the next release, the
/// oldest open MR. Two per line on a narrow page.
pub(super) fn tiles(ui: &mut Ui, theme: &Theme, h: &Headline, counts: &Counts<'_>) -> Option<Pick> {
    let t = &theme.tones;
    let mut pick = None;
    let release = h
        .releases
        .iter()
        .find(|r| r.remaining() > 0)
        .or(h.releases.first());
    let release_tile = match release {
        Some(r) => {
            let (caption, tone) = due_caption(r, t);
            let left = tn(r.remaining() as u64, "{n} MR to merge", "{n} MRs to merge");
            (
                r.title.clone(),
                format!("{caption} · {left}"),
                tone.map_or(theme.palette.text, |t| t.fg),
                Some((format!("release-open:{}", r.title), r.title.clone())),
            )
        }
        None => (
            "—".into(),
            tl("No milestone on the listed MRs").into(),
            theme.palette.text_muted,
            None,
        ),
    };
    let oldest_color = match h.oldest_open_days {
        Some(d) if d >= h.old_open_alert_days => t.red.fg,
        Some(d) if d >= h.old_open_warning_days => t.amber.fg,
        Some(_) => t.green.fg,
        None => theme.palette.text_muted,
    };
    let review_caption = match h.oldest_review_wait_days {
        Some(d) if h.awaiting_my_review > 0 => {
            tf("the oldest waits for {n}d", &[("n", &d.to_string())])
        }
        _ => tl("nothing waits for you").into(),
    };
    let tiles: [Tile; 4] = [
        Tile {
            caption: tl("Blocked"),
            value: h.blocked.to_string(),
            detail: tf("of {n} open MRs", &[("n", &h.open.to_string())]),
            color: if h.blocked > 0 { t.red.fg } else { t.green.fg },
            focus: (h.blocked > 0).then(|| ("Blocked".into(), tl("Blocked").into())),
        },
        Tile {
            caption: tl("To review (me)"),
            value: h.awaiting_my_review.to_string(),
            detail: review_caption,
            color: if h.awaiting_my_review > 0 {
                t.amber.fg
            } else {
                theme.palette.text
            },
            focus: (h.awaiting_my_review > 0)
                .then(|| ("To review (me)".into(), tl("To review (me)").into())),
        },
        Tile {
            caption: tl("Next release"),
            value: release_tile.0,
            detail: release_tile.1,
            color: release_tile.2,
            focus: release_tile.3,
        },
        Tile {
            caption: tl("Oldest open"),
            value: h
                .oldest_open_days
                .map_or("—".into(), |d| tf("{n}d", &[("n", &d.to_string())])),
            detail: tl("since its creation").into(),
            color: oldest_color,
            focus: None,
        },
    ];

    let gap = 12.0;
    let per_line = if ui.available_width() < 760.0 { 2 } else { 4 };
    let width = (ui.available_width() - gap * (per_line - 1) as f32) / per_line as f32;
    for line in tiles.chunks(per_line) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for Tile {
                caption,
                value,
                detail,
                color,
                focus,
            } in line
            {
                let response = ui
                    .allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            kpi(
                                ui,
                                theme,
                                caption,
                                value,
                                *color,
                                detail,
                                theme.palette.text_muted,
                            );
                        },
                    )
                    .response;
                let Some((key, label)) = focus else { continue };
                let response = ui
                    .interact(
                        response.rect,
                        ui.scope_id().with(("tile", key)),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let response = match counts.members.get(key.as_str()) {
                    Some(ids) => response.rich(Tip::new().title(label.as_str()).hint(tn(
                        ids.len() as u64,
                        "Click to list {n} MR",
                        "Click to list {n} MRs",
                    ))),
                    None => response.rich(
                        Tip::new()
                            .title(label.as_str())
                            .hint(tl("Click to list these MRs")),
                    ),
                };
                if response.hovered() {
                    hover_ring(ui, theme, response.rect);
                }
                if response.clicked() {
                    pick = Some(Pick::Focus(key.clone(), label.clone()));
                }
            }
        });
        ui.add_space(gap);
    }
    pick
}

/// A headline tile: its figure, and the counter a click lists (key, label).
pub(super) struct Tile {
    pub(super) caption: &'static str,
    pub(super) value: String,
    pub(super) detail: String,
    pub(super) color: Color32,
    pub(super) focus: Option<(String, String)>,
}

pub(super) fn hover_ring(ui: &Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(theme.radii.large),
        Stroke::new(1.0, theme.palette.accent),
        egui::StrokeKind::Inside,
    );
}

/// "885 days late" in red, "in 3 days" in amber within a week, "no due date".
pub(super) fn due_caption(r: &ReleaseView, t: &bevel_theme::Tones) -> (String, Option<Tone>) {
    match r.days_left {
        None => (tl("no due date").into(), None),
        Some(0) => (tl("due today").into(), Some(t.amber)),
        Some(d) if d < 0 => (
            tn(d.unsigned_abs(), "{n} day late", "{n} days late"),
            Some(if r.remaining() > 0 { t.red } else { t.gray }),
        ),
        Some(d) => (
            tn(d as u64, "in {n} day", "in {n} days"),
            Some(if r.at_risk || d <= 7 {
                t.amber
            } else {
                t.green
            }),
        ),
    }
}

// ── Cards ────────────────────────────────────────────────────────────────────

/// The cards in as many columns as fit, all as tall as the tallest one. Returns
/// what was clicked and each card's height.
///
/// The tallest natural height (content alone) is measured each frame and applied
/// on the next one; the window redraws once when it changes.
pub(super) fn grid(
    ui: &mut Ui,
    theme: &Theme,
    cards: &[(&str, Vec<CockpitRow>)],
    releases: &[ReleaseView],
    counts: &Counts<'_>,
) -> (Option<Pick>, Vec<f32>) {
    let gap = 12.0;
    let per_row = ((ui.available_width() + gap) / (MIN_CARD_WIDTH + gap))
        .floor()
        .clamp(1.0, cards.len().max(1) as f32) as usize;
    let width = (ui.available_width() - gap * (per_row - 1) as f32) / per_row as f32;
    let id = ui.make_persistent_id("cockpit_card_height");
    let target = ui.data(|d| d.get_temp::<f32>(id));
    let mut tallest: f32 = 0.0;
    let mut heights = Vec::new();
    let mut pick = None;
    let releases_title = tl("Releases");
    for row in cards.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (title, rows) in row {
                let response = ui.allocate_ui(Vec2::new(width, 0.0), |ui| {
                    ui.set_width(width);
                    card(ui, theme, title, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if *title == releases_title && !releases.is_empty() {
                            for release in releases {
                                if let Some(p) = release_block(ui, theme, release, counts) {
                                    pick = Some(p);
                                }
                            }
                        } else {
                            let history = *title == tl("Flow");
                            for row in rows {
                                if let Some(p) = cockpit_row(ui, theme, row, counts, history) {
                                    pick = Some(p);
                                }
                            }
                        }
                        // Pad up to the tallest card (`set_min_height` would
                        // count from the cursor, below the content).
                        let natural = ui.min_rect().height();
                        tallest = tallest.max(natural);
                        if let Some(target) = target {
                            ui.add_space((target - natural).max(0.0));
                        }
                    });
                });
                heights.push(response.response.rect.height());
            }
        });
        ui.add_space(gap);
    }
    if target.is_none_or(|target| (target - tallest).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(id, tallest));
        ui.ctx().request_repaint();
    }
    (pick, heights)
}
