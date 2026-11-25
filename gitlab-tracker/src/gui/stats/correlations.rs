//! The Correlations tab: Spearman correlations between MR traits.

use super::*;

pub(super) fn correlations(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    card(ui, theme, tl("Spearman correlations"), |ui| {
        if report.correlations.is_empty() {
            ui.label(muted(theme, tl("Not enough MRs to correlate yet.")));
            return;
        }
        ui.spacing_mut().item_spacing.y = 12.0;
        for c in &report.correlations {
            let significant = c.p_value < 0.05;
            let row = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let label_width = (ui.available_width() * 0.32).clamp(180.0, 280.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(label_width, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        // Same width whatever the label: the bars line up.
                        ui.set_min_width(label_width);
                        let color = if significant {
                            theme.palette.text
                        } else {
                            theme.palette.text_muted
                        };
                        ui.add(
                            egui::Label::new(RichText::new(pair_label(&c.pair)).color(color))
                                .truncate(),
                        );
                    },
                );
                // A diverging bar from the middle: right for ρ > 0, left for ρ < 0.
                let width = (ui.available_width() - 250.0).max(80.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 10.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(5), theme.palette.content);
                let mid = rect.center().x;
                let reach = (c.rho.clamp(-1.0, 1.0) as f32) * rect.width() / 2.0;
                let filled = egui::Rect::from_x_y_ranges(
                    mid.min(mid + reach)..=mid.max(mid + reach),
                    rect.y_range(),
                );
                let color = if significant {
                    theme.tones.purple.fg
                } else {
                    theme.palette.text_muted.gamma_multiply(0.6)
                };
                ui.painter()
                    .rect_filled(filled, CornerRadius::same(5), color);
                ui.painter()
                    .vline(mid, rect.y_range(), Stroke::new(1.0, theme.palette.border));
                let text = format!("ρ = {:+.2} · {}", c.rho, strength(&c.interpretation));
                let color = if significant {
                    theme.palette.text
                } else {
                    theme.palette.text_muted
                };
                ui.label(mono(text, theme.text_size - 1.0).color(color));
            });
            let mut tip = Tip::new()
                .fact("p", format!("{:.3}", c.p_value))
                .fact("n", c.sample_size.to_string());
            if !significant {
                tip = tip.chip(tl("not significant"), theme.tones.gray);
            }
            row.response.rich(tip);
        }
        ui.label(muted(
            theme,
            tl("Grayed out: not significant (p ≥ 0.05). Hover a row for its p-value and sample size."),
        ));
    });
}

// ── Insights ─────────────────────────────────────────────────────────────────
