//! The cockpit page: headline tiles, the TUI's cockpit cards (flow, attention,
//! delivery health, quality, releases) in as many columns as the width allows,
//! and the MRs that need attention first. Every count of MRs is a link: a click
//! lists them on the merge requests page.

use std::collections::HashMap;

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, Vec2};

use super::tip::{Tip, TipExt};
use super::widgets::{
    avatar, badge, card, kpi, mono, muted, page_title, semibold, split_person, switch,
};
use super::{colors, tones, GuiApp};
use crate::i18n::{tf, tl, tn};
use crate::ui::cockpit::{CockpitRow, Headline, ReleaseView};

mod attention;
mod card_rows;
mod tiles_grid;

use attention::*;
use card_rows::*;
use tiles_grid::*;

/// Cards narrower than this wrap onto another row.
const MIN_CARD_WIDTH: f32 = 260.0;
const ROW_HEIGHT: f32 = 24.0;
/// MRs in the "Needs attention" list.
const URGENT_COUNT: usize = 6;

/// What a click on the page asked for.
enum Pick {
    /// List the MRs behind this counter (key, label).
    Focus(String, String),
    /// Open this MR's page.
    Mr(String),
    /// List the MRs of this release (milestone) on the merge requests page.
    Release(String),
    /// List the tracked MRs among a count of the stats' history (count, label).
    History(crate::ui::cockpit::History, usize, String),
}

/// What the cards need besides their rows: the MRs behind each counter, and
/// how each counter moved since the list was first complete.
struct Counts<'a> {
    /// The tracked MRs, for what the counts of the history leave out.
    app: Option<&'a crate::app::App>,
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
    // Each reads every tracked MR: kept between frames (see `memo.rs`).
    let app = &g.app;
    let read = g.memo.cockpit.get(&app.project_id, (), || {
        (
            crate::ui::cockpit::headline(app),
            crate::ui::cockpit::cockpit_cards(app),
        )
    });
    let (headline, cards) = (&read.0, &read.1);
    let urgent = urgent(g);
    let mut pick = None;
    let mut by_label = None;

    ui.horizontal(|ui| {
        page_title(
            ui,
            &theme,
            tl("Cockpit"),
            tf(
                "{n} tracked merge requests · trends against the {weeks} previous weeks",
                &[
                    ("n", &g.app.mrs.len().to_string()),
                    ("weeks", &g.trend_period.before().to_string()),
                ],
            ),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            if switch(ui, &theme, g.hide_zeros, tl("Hide zeros"))
                .tip(tl("Hide the counters at zero"))
                .clicked()
            {
                g.hide_zeros = !g.hide_zeros;
            }
            super::flow::period_toggle(ui, &theme, &mut g.trend_period);
        });
    });
    ui.add_space(16.0);

    let mut close_digest = false;
    let counts = Counts {
        app: Some(&g.app),
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
            if let Some((key, name)) = super::flow::flow_card(ui, &theme, &g.app) {
                pick = Some(Pick::Focus(key, name));
            }
            ui.add_space(12.0);
            // The counters, then the flow and attention cards, then the weekly
            // trends; the lists last: what needs attention, the labels.
            let from_tiles = tiles(ui, &theme, headline, &counts);
            ui.add_space(12.0);
            let (from_cards, _) = grid(ui, &theme, cards, &headline.releases, &counts);
            ui.add_space(12.0);
            super::flow::trend_tiles(
                ui,
                &theme,
                &super::flow::trends(&g.app, chrono::Utc::now(), g.trend_period),
            );
            ui.add_space(12.0);
            let from_list = needs_attention(ui, &theme, &urgent);
            ui.add_space(12.0);
            if let Some(click) = super::labels::card(ui, &theme, &g.app, &g.memo.labels) {
                by_label = Some(click);
            }
            pick = pick.take().or(from_tiles).or(from_cards).or(from_list);
        });

    if close_digest {
        g.digest_closed = true;
    }
    match pick {
        Some(Pick::Focus(key, label)) => g.focus_on(&key, label),
        Some(Pick::Mr(id)) => g.open_mr(&id),
        Some(Pick::Release(title)) => g.filter_by_milestone(&title),
        Some(Pick::History(history, counted, label)) => {
            super::merged_list::show_history(g, history, counted, label);
        }
        None => {}
    }
    match by_label {
        Some(super::labels::Click::Label(label)) => g.filter_by_label(&label),
        Some(super::labels::Click::Mr(id)) => g.open_mr(&id),
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
                ui.label(RichText::new("ℹ").color(theme.tones.blue.fg));
                ui.add(egui::Label::new(RichText::new(digest).color(theme.palette.text)).wrap());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    closed = ui
                        .add(egui::Button::new(muted(theme, "✕")).frame(false))
                        .tip(tl("Close"))
                        .clicked();
                });
            });
        });
    closed
}

#[cfg(test)]
mod tests;
