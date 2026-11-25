//! The Forecasts tab: signals, throughput forecasts, the review queue.

use super::*;

pub(super) fn forecasts(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    let poisson = &report.poisson;
    card(ui, theme, tl("Signals"), |ui| {
        if poisson.anomalies.is_empty() {
            ui.label(muted(
                theme,
                tl("No signal: every tracked metric is within its usual range."),
            ));
        }
        for signal in &poisson.anomalies {
            egui::Frame::new()
                .fill(theme.palette.content)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let (label, tone) = severity(&signal.severity, theme);
                        ui.allocate_ui_with_layout(
                            Vec2::new(84.0, 20.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                badge(ui, theme, label, tone);
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(200.0, 20.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.label(
                                    RichText::new(metric_name(&signal.metric))
                                        .font(bevel_theme::fonts::medium(theme.text_size)),
                                );
                            },
                        );
                        let detail = match (signal.baseline, signal.p_value, signal.ratio) {
                            (Some(expected), Some(p), _) => tf(
                                "Observed {observed}, expected {expected} (p = {p})",
                                &[
                                    ("observed", &number(signal.observed)),
                                    ("expected", &format!("{expected:.1}")),
                                    ("p", &format!("{p:.3}")),
                                ],
                            ),
                            (Some(baseline), None, Some(ratio)) => tf(
                                "Observed {observed}, baseline {baseline} (×{ratio})",
                                &[
                                    ("observed", &number(signal.observed)),
                                    ("baseline", &number(baseline)),
                                    ("ratio", &format!("{ratio:.1}")),
                                ],
                            ),
                            _ => tf(
                                "Value {observed}: pressure signal",
                                &[("observed", &number(signal.observed))],
                            ),
                        };
                        ui.label(RichText::new(detail).color(theme.palette.text_secondary));
                    });
                });
        }
    });
    let side_by_side = ui.available_width() > 760.0;
    let throughput = |ui: &mut Ui| {
        card(ui, theme, tl("Throughput forecasts"), |ui| {
            if poisson.throughput_forecasts.is_empty() {
                ui.label(muted(theme, tl("Not enough merged MRs to forecast.")));
            }
            for f in &poisson.throughput_forecasts {
                let label = tf(
                    "≥ {merges} in {weeks}",
                    &[
                        (
                            "merges",
                            &tn(f.target_merges.into(), "{n} merge", "{n} merges"),
                        ),
                        (
                            "weeks",
                            &tn(f.forecast_weeks.into(), "{n} week", "{n} weeks"),
                        ),
                    ],
                );
                let color = if f.probability >= 0.8 {
                    theme.tones.green.fg
                } else if f.probability >= 0.5 {
                    theme.tones.amber.fg
                } else {
                    theme.tones.red.fg
                };
                meter(
                    ui,
                    theme,
                    &label,
                    &format!("{:.0}%", f.probability * 100.0),
                    f.probability as f32,
                    color,
                );
            }
            if let Some(f) = poisson.throughput_forecasts.first() {
                ui.label(muted(
                    theme,
                    tf(
                        "λ = {n} merges per week",
                        &[("n", &format!("{:.1}", f.lambda_per_week))],
                    ),
                ));
            }
        });
    };
    let queue = |ui: &mut Ui| {
        let q = &poisson.queue_insight;
        card(ui, theme, tl("Review queue"), |ui| {
            let (label, tone) = match q.status {
                QueueStatus::OverCapacity => (tl("OVER CAPACITY"), theme.tones.red),
                QueueStatus::Stable => (tl("STABLE"), theme.tones.green),
                QueueStatus::InsufficientData => (tl("NOT ENOUGH DATA"), theme.tones.gray),
            };
            badge(ui, theme, label, tone);
            ui.add_space(8.0);
            if let Some(load) = q.traffic_intensity {
                let color = if load < 0.7 {
                    theme.tones.green.fg
                } else if load < 0.9 {
                    theme.tones.amber.fg
                } else {
                    theme.tones.red.fg
                };
                ui.scope(|ui| {
                    meter(
                        ui,
                        theme,
                        tl("Load"),
                        &format!("{:.0}%", load * 100.0),
                        load as f32,
                        color,
                    );
                })
                .response
                .tip(tl(
                    "How busy the review flow is: arrivals ÷ capacity. Above 100%, the backlog grows.",
                ));
            }
            // Demand against capacity, on one scale: the gap is the slack.
            if let (Some(arrivals), Some(service)) =
                (q.arrival_rate_per_week, q.service_rate_per_week)
            {
                let max = arrivals.max(service).max(f64::EPSILON);
                let capacity = if service > arrivals {
                    theme.tones.green.fg
                } else {
                    theme.tones.red.fg
                };
                let bars = [
                    (
                        tl("Arrivals"),
                        arrivals,
                        theme.palette.accent,
                        tl("MRs merged per week: the flow entering the queue."),
                    ),
                    (
                        tl("Service"),
                        service,
                        capacity,
                        tl("MRs per week the team can get through, from the median time to merge."),
                    ),
                ];
                for (caption, rate, color, hint) in bars {
                    ui.scope(|ui| {
                        bar_row(
                            ui,
                            theme,
                            caption,
                            (rate / max) as f32,
                            color,
                            &per_week(rate),
                        );
                    })
                    .response
                    .tip(hint);
                }
                ui.add_space(6.0);
            }
            figures(
                ui,
                theme,
                vec![
                    Figure::new(
                        tl("In the queue"),
                        q.expected_mrs_in_system.map_or("—".into(), |l| {
                            tf("{n} MRs", &[("n", &format!("{l:.1}"))])
                        }),
                        tl("MRs expected to be waiting or in review at any time."),
                    ),
                    Figure::new(
                        tl("Expected wait"),
                        hours(q.expected_wait_hours),
                        tl("How long a new MR should wait before it is merged."),
                    ),
                ],
            );
        });
    };
    if side_by_side {
        columns(
            ui,
            2,
            |ui, i| if i == 0 { throughput(ui) } else { queue(ui) },
        );
    } else {
        throughput(ui);
        queue(ui);
    }
}

// ── Correlations ─────────────────────────────────────────────────────────────

pub(super) fn pair_label(pair: &MetricPair) -> &'static str {
    tl(match pair {
        MetricPair::DiffSizeVsCycleTime => "Diff size ↔ time to merge",
        MetricPair::DiffSizeVsComments => "Diff size ↔ comments",
        MetricPair::CommentsVsCycleTime => "Comments ↔ time to merge",
        MetricPair::PipelineFailuresVsCycleTime => "Pipeline failures ↔ time to merge",
        MetricPair::CommitsCountVsCycleTime => "Commits ↔ time to merge",
        MetricPair::DiffDifficultyVsCycleTime => "Review effort ↔ time to merge",
    })
}

pub(super) fn strength(s: &CorrelationStrength) -> &'static str {
    tl(match s {
        CorrelationStrength::Negligible => "negligible",
        CorrelationStrength::Weak => "weak",
        CorrelationStrength::Moderate => "moderate",
        CorrelationStrength::Strong => "strong",
        CorrelationStrength::VeryStrong => "very strong",
    })
}
