//! The summary insights beside the page.

use super::*;

/// Plain-language findings from the report, most telling first, each with the tab
/// that details it.
pub(super) fn insights(report: &StatReport, theme: &Theme) -> Vec<(String, Color32, StatsTab)> {
    let a = &report.aggregated;
    let t = &theme.tones;
    let mut out = Vec::new();
    let small = a
        .size_buckets
        .first()
        .and_then(|b| b.cycle_time_median_hours);
    let large = a
        .size_buckets
        .last()
        .and_then(|b| b.cycle_time_median_hours);
    if let (Some(small), Some(large), true) = (small, large, a.size_buckets.len() > 1) {
        if small > 0.0 && large / small >= 1.5 {
            out.push((
                tf(
                    "Large MRs ({large}) take {ratio}× longer to merge than small ones ({small}).",
                    &[
                        ("large", a.size_buckets.last().map_or("", |b| tl(&b.label))),
                        ("ratio", &format!("{:.1}", large / small)),
                        ("small", a.size_buckets.first().map_or("", |b| tl(&b.label))),
                    ],
                ),
                t.amber.fg,
                StatsTab::Quality,
            ));
        }
    }
    if let Some(last) = report
        .promotions
        .last()
        .filter(|p| p.arrivals > 0 && p.copied_share > 0.0)
    {
        out.push((
            tf(
                "{percent} of the MRs reaching {branch} arrive as copies (cherry-pick or rebase), not merges.",
                &[
                    ("percent", &format!("{:.0}%", last.copied_share * 100.0)),
                    ("branch", &last.branch),
                ],
            ),
            theme.palette.accent,
            StatsTab::Flow,
        ));
    }
    if let Some(c) = report
        .correlations
        .iter()
        .filter(|c| c.p_value < 0.05)
        .max_by(|x, y| x.rho.abs().total_cmp(&y.rho.abs()))
    {
        out.push((
            tf(
                "Strongest link: {pair} (ρ = {rho}, {strength}).",
                &[
                    ("pair", pair_label(&c.pair)),
                    ("rho", &format!("{:+.2}", c.rho)),
                    ("strength", strength(&c.interpretation)),
                ],
            ),
            t.purple.fg,
            StatsTab::Correlations,
        ));
    }
    // Only a figure compared with its own past has a "usual range"; pressure
    // signals (MRs left open) have their own line above.
    if let Some(signal) = report
        .poisson
        .anomalies
        .iter()
        .find(|s| s.is_anomalous() && s.baseline.is_some())
    {
        out.push((
            tf(
                "{metric} is out of its usual range.",
                &[("metric", &metric_name(&signal.metric))],
            ),
            t.red.fg,
            StatsTab::Forecasts,
        ));
    }
    match report.poisson.queue_insight.status {
        QueueStatus::OverCapacity => out.push((
            tl("MRs arrive faster than they are merged: the backlog grows.").into(),
            t.red.fg,
            StatsTab::Forecasts,
        )),
        QueueStatus::Stable => {
            if let Some(wait) = report.poisson.queue_insight.expected_wait_hours {
                out.push((
                    tf(
                        "A new MR waits about {time} before it is merged.",
                        &[("time", &hours(Some(wait)))],
                    ),
                    t.green.fg,
                    StatsTab::Forecasts,
                ));
            }
        }
        QueueStatus::InsufficientData => {}
    }
    if a.stale_open_mrs_14d > 0 {
        out.push((
            tn(
                a.stale_open_mrs_14d as u64,
                "{n} open MR has had no merge for 14 days or more; the oldest is {days} days old.",
                "{n} open MRs have had no merge for 14 days or more; the oldest is {days} days old.",
            )
            .replace(
                "{days}",
                &format!("{:.0}", a.open_mr_ages_days.last().copied().unwrap_or(0.0)),
            ),
            t.amber.fg,
            StatsTab::Overview,
        ));
    }
    out
}

/// The insights as cards; clicking one opens the tab that details it.
pub(super) fn insights_panel(ui: &mut Ui, theme: &Theme, report: &StatReport, tab: &mut StatsTab) {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, theme, tl("Summary insights"));
        });
    rule(ui, theme);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 10.0;
                    let found = insights(report, theme);
                    if found.is_empty() {
                        ui.label(muted(theme, tl("Nothing stands out in this window.")));
                    }
                    for (i, (text, color, target)) in found.into_iter().enumerate() {
                        let id = ui.scope_id().with(("insight", i));
                        // Last frame's hover: the frame is drawn before its response.
                        let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
                        let current = *tab == target;
                        let border = if hovered || current {
                            theme.palette.border
                        } else {
                            theme.palette.border_soft
                        };
                        let card = egui::Frame::new()
                            .fill(if hovered {
                                theme.palette.raised
                            } else {
                                theme.palette.content
                            })
                            .stroke(Stroke::new(1.0, border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    ui.add_space(0.0);
                                    ui.vertical(|ui| {
                                        ui.add_space(5.0);
                                        dot(ui, color, 6.0);
                                    });
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(text).line_height(Some(18.0)),
                                        )
                                        .wrap()
                                        .selectable(false),
                                    );
                                });
                                if current {
                                    return;
                                }
                                let link = tf("See {tab} →", &[("tab", target.label())]);
                                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                    let color = if hovered {
                                        theme.palette.accent
                                    } else {
                                        theme.palette.text_muted
                                    };
                                    ui.label(
                                        RichText::new(link)
                                            .size(theme.text_size - 2.0)
                                            .color(color),
                                    );
                                });
                            });
                        let response = ui.interact(card.response.rect, id, Sense::click());
                        if response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            *tab = target;
                        }
                    }
                });
        });
}
