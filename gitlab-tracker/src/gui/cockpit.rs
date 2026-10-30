//! The cockpit page: the TUI's cockpit cards (flow, attention, delivery health,
//! quality, releases) as cards of counters, in as many columns as the width allows.

use bevel_theme::Theme;
use egui::{Align, Layout, RichText, Ui, Vec2};

use super::widgets::{card, page_title, semibold};
use super::{colors, GuiApp};
use crate::i18n::{tf, tl};
use crate::ui::cockpit::CockpitRow;

/// Cards narrower than this wrap onto another row.
const MIN_CARD_WIDTH: f32 = 260.0;
const ROW_HEIGHT: f32 = 24.0;

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    page_title(
        ui,
        &theme,
        tl("Cockpit"),
        tf(
            "{n} tracked merge requests · live from the list",
            &[("n", &g.app.mrs.len().to_string())],
        ),
    );
    ui.add_space(16.0);

    let cards = crate::ui::cockpit::cockpit_cards(&g.app);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| grid(ui, &theme, &cards));
}

/// The cards in as many columns as fit, all as tall as the tallest one. Returns
/// each card's height.
///
/// The tallest natural height (content alone) is measured each frame and applied
/// on the next one; the window redraws once when it changes.
fn grid(ui: &mut Ui, theme: &Theme, cards: &[(&str, Vec<CockpitRow>)]) -> Vec<f32> {
    let gap = 12.0;
    let per_row = ((ui.available_width() + gap) / (MIN_CARD_WIDTH + gap))
        .floor()
        .clamp(1.0, cards.len().max(1) as f32) as usize;
    let width = (ui.available_width() - gap * (per_row - 1) as f32) / per_row as f32;
    let id = ui.make_persistent_id("cockpit_card_height");
    let target = ui.data(|d| d.get_temp::<f32>(id));
    let mut tallest: f32 = 0.0;
    let mut heights = Vec::new();
    for row in cards.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (title, rows) in row {
                let response = ui.allocate_ui(Vec2::new(width, 0.0), |ui| {
                    ui.set_width(width);
                    card(ui, theme, title, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for row in rows {
                            cockpit_row(ui, theme, row);
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
    heights
}

fn cockpit_row(ui: &mut Ui, theme: &Theme, row: &CockpitRow) {
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
        }
        CockpitRow::Metric {
            label,
            value,
            color,
            nested,
        } => {
            let value_color = colors::foreground(*color, theme).unwrap_or(theme.palette.text);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ROW_HEIGHT),
                Layout::left_to_right(Align::Center),
                |ui| {
                    if *nested {
                        ui.add_space(12.0);
                    }
                    ui.label(RichText::new(label).color(theme.palette.text_secondary));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(semibold(value, theme.text_size).color(value_color));
                    });
                },
            );
        }
    }
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
        (0..frames)
            .map(|_| {
                let mut out = Vec::new();
                let output = ctx.run_ui(input(), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| out = grid(ui, &theme, cards));
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
}
