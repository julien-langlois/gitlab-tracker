//! The cockpit page: headline tiles, the TUI's cockpit cards (flow, attention,
//! delivery health, quality, releases) in as many columns as the width allows,
//! and the MRs that need attention first. Every count of MRs is a link: a click
//! lists them on the merge requests page.

use std::collections::HashMap;

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, Vec2};

use super::widgets::{
    avatar, badge, card, kpi, mono, muted, page_title, semibold, split_person, switch,
};
use super::{colors, tones, GuiApp};
use crate::i18n::{tf, tl, tn};
use crate::ui::cockpit::{CockpitRow, Headline, ReleaseView};

/// Cards narrower than this wrap onto another row.
const MIN_CARD_WIDTH: f32 = 260.0;
const ROW_HEIGHT: f32 = 24.0;
/// MRs in the "Needs attention" list.
const URGENT_COUNT: usize = 6;

/// What a click on the page asked for.
enum Pick {
    /// List the MRs behind this counter (key, label).
    Focus(String, String),
    /// Select this MR on the merge requests page.
    Mr(String),
}

/// What the cards need besides their rows: the MRs behind each counter, and
/// how each counter moved since the list was first complete.
struct Counts<'a> {
    members: &'a HashMap<String, Vec<String>>,
    baseline: Option<&'a HashMap<String, usize>>,
    since: Option<&'a str>,
    hide_zeros: bool,
}

impl Counts<'_> {
    /// The change of `key` since the baseline (0 without one).
    fn delta(&self, key: &str) -> i64 {
        let Some(baseline) = self.baseline else {
            return 0;
        };
        let now = self.members.get(key).map_or(0, Vec::len) as i64;
        now - baseline.get(key).copied().unwrap_or(0) as i64
    }
}

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let headline = crate::ui::cockpit::headline(&g.app);
    let cards = crate::ui::cockpit::cockpit_cards(&g.app);
    let urgent = urgent(g);
    let mut pick = None;

    ui.horizontal(|ui| {
        page_title(
            ui,
            &theme,
            tl("Cockpit"),
            tf(
                "{n} tracked merge requests · live from the list",
                &[("n", &g.app.mrs.len().to_string())],
            ),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if switch(ui, &theme, g.hide_zeros, tl("Hide zeros"))
                .on_hover_text(tl("Hide the counters at zero"))
                .clicked()
            {
                g.hide_zeros = !g.hide_zeros;
            }
        });
    });
    ui.add_space(16.0);

    let mut close_digest = false;
    let counts = Counts {
        members: &g.members,
        baseline: g.baseline.as_ref().map(|b| &b.counts),
        since: g.baseline.as_ref().map(|b| b.at.as_str()),
        hide_zeros: g.hide_zeros,
    };
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if let Some(digest) = g.app.last_digest.as_deref().filter(|_| !g.digest_closed) {
                if since_last_session(ui, &theme, digest) {
                    close_digest = true;
                }
                ui.add_space(12.0);
            }
            let from_tiles = tiles(ui, &theme, &headline, &counts);
            ui.add_space(12.0);
            let (from_cards, _) = grid(ui, &theme, &cards, &headline.releases, &counts);
            let from_list = needs_attention(ui, &theme, &urgent);
            pick = from_tiles.or(from_cards).or(from_list);
        });

    if close_digest {
        g.digest_closed = true;
    }
    match pick {
        Some(Pick::Focus(key, label)) => g.focus_on(&key, label),
        Some(Pick::Mr(id)) => g.show_mr(&id),
        None => {}
    }
}

/// What changed while the app was closed, from the startup load. Returns
/// whether it was closed.
fn since_last_session(ui: &mut Ui, theme: &Theme, digest: &str) -> bool {
    let mut closed = false;
    egui::Frame::new()
        .fill(theme.tones.blue.bg)
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                // The digest names itself ("Since last session: …").
                ui.label(RichText::new("ⓘ").color(theme.tones.blue.fg));
                ui.add(egui::Label::new(RichText::new(digest).color(theme.palette.text)).wrap());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    closed = ui
                        .add(egui::Button::new(muted(theme, "✕")).frame(false))
                        .on_hover_text(tl("Close"))
                        .clicked();
                });
            });
        });
    closed
}

// ── Headline tiles ───────────────────────────────────────────────────────────

/// Four tiles over the cards: blocked MRs, my reviews, the next release, the
/// oldest open MR. Two per line on a narrow page.
fn tiles(ui: &mut Ui, theme: &Theme, h: &Headline, counts: &Counts<'_>) -> Option<Pick> {
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
                    Some(ids) => response.on_hover_text(tn(
                        ids.len() as u64,
                        "Click to list {n} MR",
                        "Click to list {n} MRs",
                    )),
                    None => response.on_hover_text(tl("Click to list these MRs")),
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
struct Tile {
    caption: &'static str,
    value: String,
    detail: String,
    color: Color32,
    focus: Option<(String, String)>,
}

fn hover_ring(ui: &Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(theme.radii.large),
        Stroke::new(1.0, theme.palette.accent),
        egui::StrokeKind::Inside,
    );
}

/// "885 days late" in red, "in 3 days" in amber within a week, "no due date".
fn due_caption(r: &ReleaseView, t: &bevel_theme::Tones) -> (String, Option<Tone>) {
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
fn grid(
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

/// One line of a card. In the Flow card (`history`), the figures without a key
/// come from the stats' history, not the listed MRs: a clock says so.
fn cockpit_row(
    ui: &mut Ui,
    theme: &Theme,
    row: &CockpitRow,
    counts: &Counts<'_>,
    history: bool,
) -> Option<Pick> {
    match row {
        CockpitRow::Heading { text, color, alert } => {
            let color = colors::foreground(*color, theme).unwrap_or(theme.palette.text);
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ROW_HEIGHT),
                Layout::left_to_right(Align::Center),
                |ui| {
                    let text = if *alert {
                        format!("⚠ {text}")
                    } else {
                        text.clone()
                    };
                    ui.add(
                        egui::Label::new(semibold(text, theme.text_size).color(color)).truncate(),
                    );
                },
            );
            None
        }
        CockpitRow::Metric {
            label,
            value,
            color,
            nested,
            key,
        } => {
            let zero = value == "0";
            if zero && counts.hide_zeros && key.is_some() {
                return None;
            }
            let value_color = if zero {
                theme.palette.text_muted
            } else {
                colors::foreground(*color, theme).unwrap_or(theme.palette.text)
            };
            let label_color = if zero {
                theme.palette.text_muted
            } else {
                theme.palette.text_secondary
            };
            let clickable = key.as_ref().is_some_and(|k| counts.members.contains_key(k));
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), ROW_HEIGHT),
                if clickable {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            if clickable && response.hovered() {
                ui.painter().rect_filled(
                    rect.expand2(Vec2::new(6.0, 0.0)),
                    CornerRadius::same(theme.radii.small),
                    theme.palette.raised,
                );
            }
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if *nested {
                inner.add_space(12.0);
            }
            if history && key.is_none() {
                let (clock, response) =
                    inner.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                super::icons::paint(
                    inner.painter(),
                    clock,
                    super::icons::Icon::Clock,
                    theme.palette.text_muted,
                );
                response.on_hover_text(tl(
                    "From the stats history since recording started, not the listed MRs",
                ));
            }
            inner.label(RichText::new(label).color(label_color));
            inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(semibold(value, theme.text_size).color(value_color));
                if let Some(key) = key {
                    trend(ui, theme, counts, key);
                }
            });
            let hint = key.as_deref().and_then(explain);
            let response = match (hint, clickable) {
                (Some(hint), true) => response
                    .on_hover_text(format!("{}\n{}", tl(hint), tl("Click to list them")))
                    .on_hover_cursor(egui::CursorIcon::PointingHand),
                (Some(hint), false) => response.on_hover_text(tl(hint)),
                (None, true) => response
                    .on_hover_text(tl("Click to list them"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand),
                (None, false) => response,
            };
            (clickable && response.clicked())
                .then(|| Pick::Focus(key.clone().unwrap_or_default(), label.clone()))
        }
    }
}

/// "▲2" or "▼1" before a counter that moved since the list was first complete:
/// green when it moved the right way, amber otherwise.
fn trend(ui: &mut Ui, theme: &Theme, counts: &Counts<'_>, key: &str) {
    let delta = counts.delta(key);
    if delta == 0 || key.starts_with("release:") {
        return;
    }
    let good = matches!(key, "Mergeable" | "Recently updated" | "With ticket") == (delta > 0);
    let tone = if good {
        theme.tones.green
    } else {
        theme.tones.amber
    };
    let arrow = if delta > 0 { "▲" } else { "▼" };
    let since = counts.since.unwrap_or("");
    ui.label(
        RichText::new(format!("{arrow}{}", delta.abs()))
            .size(theme.text_size - 2.5)
            .color(tone.fg),
    )
    .on_hover_text(format!(
        "{}\n{}",
        tf(
            "{delta} since {time}",
            &[("delta", &format!("{delta:+}")), ("time", since)],
        ),
        tl("Green: good news · amber: to watch")
    ));
}

/// A release: its title and due date, a bar of merged / in progress / review /
/// blocked MRs, and the four counts, each a link to its MRs.
fn release_block(ui: &mut Ui, theme: &Theme, r: &ReleaseView, counts: &Counts<'_>) -> Option<Pick> {
    let t = &theme.tones;
    let mut pick = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let color = if r.at_risk {
            t.red.fg
        } else {
            theme.palette.accent
        };
        let title = if r.at_risk {
            format!("⚠ {}", r.title)
        } else {
            r.title.clone()
        };
        ui.label(semibold(title, theme.text_size).color(color));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (caption, tone) = due_caption(r, t);
            ui.add(Chip::new(&caption, tone.unwrap_or(t.gray)).pill());
        });
    });
    ui.add_space(6.0);
    let parts = [
        ("Merged", r.merged, t.purple),
        ("WIP", r.in_progress, t.blue),
        ("Review", r.waiting_review, t.amber),
        ("Blocked", r.blocked, t.red),
    ];
    stacked_bar(ui, theme, &parts);
    ui.add_space(6.0);
    // Each entry's width (dot, name, count), to start a new line before one
    // that would not fit: a nested `horizontal` only knows its width once drawn.
    let entry_width = |bucket: &str, count: usize| {
        let font = egui::FontId::proportional(theme.text_size - 1.0);
        let text = |s: String| {
            ui.painter()
                .layout_no_wrap(s, font.clone(), Color32::WHITE)
                .size()
                .x
        };
        8.0 + 4.0 + text(tl(bucket).to_string()) + 4.0 + text(count.to_string()) + 2.0
    };
    let widths: Vec<f32> = parts.iter().map(|(b, c, _)| entry_width(b, *c)).collect();
    let max_x = ui.max_rect().right();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for ((bucket, count, tone), width) in parts.into_iter().zip(widths) {
            if ui.cursor().left() + width > max_x {
                ui.end_row();
            }
            let key = crate::ui::cockpit::release_metric_key(&r.title, bucket);
            let clickable = counts.members.contains_key(&key);
            let color = if count == 0 {
                theme.palette.text_muted.gamma_multiply(0.7)
            } else {
                theme.palette.text_secondary
            };
            let response = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, tone.fg);
                    ui.label(
                        RichText::new(tl(bucket))
                            .size(theme.text_size - 1.0)
                            .color(color),
                    );
                    ui.label(
                        semibold(count.to_string(), theme.text_size - 1.0).color(if count == 0 {
                            color
                        } else {
                            tone.fg
                        }),
                    );
                })
                .response;
            if clickable {
                let response = ui
                    .interact(response.rect, ui.scope_id().with(&key), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tl("Click to list them"));
                if response.hovered() {
                    ui.painter().hline(
                        response.rect.x_range(),
                        response.rect.bottom(),
                        Stroke::new(1.0, tone.fg),
                    );
                }
                if response.clicked() {
                    pick = Some(Pick::Focus(key, format!("{} · {}", r.title, tl(bucket))));
                }
            }
        }
    });
    ui.add_space(10.0);
    pick
}

/// One bar split by `parts`, in their tones; an empty track without MRs.
fn stacked_bar(ui: &mut Ui, theme: &Theme, parts: &[(&str, usize, Tone)]) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    let radius = CornerRadius::same(4);
    ui.painter()
        .rect_filled(rect, radius, theme.palette.border_soft);
    let total: usize = parts.iter().map(|p| p.1).sum();
    if total == 0 {
        return;
    }
    let mut x = rect.left();
    let painter = ui.painter().with_clip_rect(rect);
    for (_, count, tone) in parts {
        if *count == 0 {
            continue;
        }
        let w = rect.width() * *count as f32 / total as f32;
        let part =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w, rect.height()));
        painter.rect_filled(part, CornerRadius::ZERO, tone.fg);
        x += w;
    }
    // Rounded ends over the square parts.
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.5, theme.palette.panel),
        egui::StrokeKind::Outside,
    );
    let summary = parts
        .iter()
        .map(|(name, count, _)| format!("{} {count}", tl(name)))
        .collect::<Vec<_>>()
        .join(" · ");
    response.on_hover_text(summary);
}

/// What a counter means, for its tooltip.
fn explain(key: &str) -> Option<&'static str> {
    Some(match key {
        "Open" => "Open merge requests in the list",
        "Mergeable" => "Open MRs GitLab could merge now",
        "Draft" => "Open MRs still marked as draft",
        "Blocked" => "Conflicts, rebase needed, failing CI, open discussions or changes requested",
        "Conflicts" => "Open MRs in conflict with their target branch",
        "Needs rebase" => "Open MRs GitLab asks to rebase before merging",
        "CI failing" => "Open MRs whose last pipeline failed",
        "Discussions" => "Open MRs held back by unresolved discussions",
        "Changes req." => "Open MRs whose reviewers asked for changes",
        "Needs review" => "Open MRs still missing their approvals",
        "To review (me)" => "Open MRs waiting for your review",
        "No reviewer" => "Open MRs nobody was asked to review",
        "No assignee" => "Open MRs assigned to nobody",
        "Flagged" => "MRs you starred",
        "Stale" => "Open MRs nobody touched for a while",
        "No milestone" => "Open MRs planned in no milestone",
        "Due soon" => "Open MRs whose milestone is due within the next days",
        "Overdue" => "Open MRs whose milestone is past due",
        "Behind target" => "Open MRs whose branch lags behind its target",
        "Hot threads" => "Open MRs with many comment threads",
        "With ticket" => "MRs linked to a tracker ticket",
        "No ticket" => "MRs linked to no tracker ticket",
        "Over estimate" => "MRs whose ticket took longer than estimated",
        "Complex" => "Open MRs with a high review effort",
        "Large diff" => "Open MRs with many changed lines",
        "Many commits" => "Open MRs with many commits",
        "Many files" => "Open MRs touching many files",
        "No diff stats" => "Open MRs whose diff is not loaded yet",
        "Pipeline unknown" => "Open MRs without a known pipeline",
        "CI skipped" => "Open MRs whose last pipeline was skipped",
        "Recently updated" => "MRs changed at the last refresh",
        _ => return None,
    })
}

// ── Needs attention ──────────────────────────────────────────────────────────

/// How much each counter weighs in the "Needs attention" ranking.
const URGENCY: &[(&str, u32)] = &[
    ("Conflicts", 5),
    ("Changes req.", 5),
    ("CI failing", 4),
    ("Overdue", 3),
    ("To review (me)", 3),
    ("Needs rebase", 3),
    ("Over estimate", 2),
    ("Discussions", 2),
    ("Stale", 2),
    ("Hot threads", 1),
    ("Flagged", 1),
];

/// An MR of the "Needs attention" list.
struct Urgent {
    id: String,
    title: String,
    author: String,
    status: (&'static str, Tone),
    /// The counters it is behind, heaviest first.
    reasons: Vec<(&'static str, u32)>,
}

/// The listed MRs that weigh most in [`URGENCY`], heaviest first.
fn urgent(g: &GuiApp) -> Vec<Urgent> {
    let mut by_mr: HashMap<&str, Vec<(&'static str, u32)>> = HashMap::new();
    for (key, weight) in URGENCY {
        for id in g.members.get(*key).into_iter().flatten() {
            by_mr.entry(id.as_str()).or_default().push((key, *weight));
        }
    }
    let mut list: Vec<Urgent> = g
        .app
        .visible_mrs()
        .filter_map(|mr| {
            let reasons = by_mr.get(mr.id.as_str())?.clone();
            Some(Urgent {
                id: mr.id.clone(),
                title: mr.title.clone(),
                author: mr.author.clone(),
                status: tones::status(mr.state, &mr.mergeability, &g.theme.tones),
                reasons,
            })
        })
        .collect();
    list.sort_by_key(|u| std::cmp::Reverse(u.reasons.iter().map(|r| r.1).sum::<u32>()));
    list.truncate(URGENT_COUNT);
    list
}

fn needs_attention(ui: &mut Ui, theme: &Theme, urgent: &[Urgent]) -> Option<Pick> {
    let mut pick = None;
    card(ui, theme, tl("Needs attention"), |ui| {
        if urgent.is_empty() {
            ui.label(muted(
                theme,
                tl("Nothing urgent: no MR is blocked, late or waiting for you."),
            ));
            return;
        }
        ui.label(muted(
            theme,
            tl("The MRs that are blocked, late or waiting for you, the most urgent first."),
        ));
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for u in urgent {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            let heaviest = u.reasons.iter().map(|r| r.1).max().unwrap_or(0);
            let stripe = if heaviest >= 4 {
                theme.tones.red
            } else {
                theme.tones.amber
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 6.0), Vec2::new(3.0, 24.0)),
                CornerRadius::same(2),
                stripe.fg,
            );
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(Vec2::new(12.0, 0.0)))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            row.spacing_mut().item_spacing.x = 8.0;
            row.label(
                mono(format!("!{}", u.id), theme.text_size - 1.0)
                    .color(theme.palette.text_secondary),
            );
            let (name, _) = split_person(&u.author);
            avatar(&mut row, theme, name, 18.0).on_hover_text(name);
            row.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                badge(ui, theme, u.status.0, u.status.1);
                let mut reasons = u.reasons.clone();
                reasons.sort_by_key(|r| std::cmp::Reverse(r.1));
                for (key, weight) in reasons.iter().take(3).rev() {
                    let tone = if *weight >= 4 {
                        theme.tones.red
                    } else {
                        theme.tones.amber
                    };
                    ui.add(Chip::new(tl(key), tone).font(fonts::medium(theme.text_size - 2.5)));
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&u.title).font(fonts::medium(theme.text_size)),
                        )
                        .truncate(),
                    );
                });
            });
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tl("Open it on the merge requests page"))
                .clicked()
            {
                pick = Some(Pick::Mr(u.id.clone()));
            }
        }
    });
    pick
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit::CockpitRow;
    use ratatui::style::Color;

    fn metric(label: &str) -> CockpitRow {
        CockpitRow::Metric {
            label: label.into(),
            value: "3".into(),
            color: Color::Reset,
            nested: false,
            key: None,
        }
    }

    fn heading(text: &str) -> CockpitRow {
        CockpitRow::Heading {
            text: text.into(),
            color: Color::Reset,
            alert: false,
        }
    }

    /// The heights of the cards on each of `frames` frames, in a themed window
    /// `width` points wide.
    fn heights(cards: &[(&str, Vec<CockpitRow>)], width: f32, frames: usize) -> Vec<Vec<f32>> {
        let theme = Theme::dark();
        let ctx = egui::Context::default();
        ctx.global_style_mut(|style| theme.apply_to_style(style));
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 900.0),
            )),
            ..Default::default()
        };
        let members = HashMap::new();
        let counts = Counts {
            members: &members,
            baseline: None,
            since: None,
            hide_zeros: false,
        };
        (0..frames)
            .map(|_| {
                let mut out = Vec::new();
                let output = ctx.run_ui(input(), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| out = grid(ui, &theme, cards, &[], &counts).1);
                });
                let mut output = output;
                output.textures_delta.clear();
                out
            })
            .collect()
    }

    #[test]
    fn every_card_takes_the_height_of_the_tallest() {
        let cards = vec![
            ("Flow", vec![metric("a"), metric("b"), metric("c")]),
            ("Attention", vec![metric("a"), metric("b")]),
            (
                "Delivery health",
                vec![metric("a"), metric("b"), metric("c")],
            ),
            ("Quality", vec![metric("a")]),
            (
                "Releases",
                vec![heading("v1"), metric("a"), heading("v2"), metric("b")],
            ),
        ];
        for width in [1400.0, 900.0, 400.0] {
            let frames = heights(&cards, width, 4);
            let last = frames.last().unwrap();
            let tallest = last.iter().cloned().fold(0.0, f32::max);
            for h in last {
                assert!((h - tallest).abs() < 0.5, "{width}: {last:?}");
            }
            // Stable: the same heights once settled.
            assert_eq!(frames[2], frames[3], "{width}: {frames:?}");
        }
    }

    #[test]
    fn every_counter_with_a_key_is_explained() {
        let members = crate::ui::cockpit::metric_members(&crate::app::tests::test_app());
        assert!(members.is_empty());
        for key in [
            "Blocked",
            "Conflicts",
            "Stale",
            "Due soon",
            "To review (me)",
            "No ticket",
        ] {
            assert!(explain(key).is_some(), "{key}");
        }
        for (key, _) in URGENCY {
            assert!(explain(key).is_some(), "{key}");
        }
    }
}
