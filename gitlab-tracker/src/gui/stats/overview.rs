//! The Overview tab: KPIs, MR sizes, the backlog.

use super::*;

/// The overview; returns the MRs a click on the size chart asked for (key, label).
pub(super) fn overview(
    ui: &mut Ui,
    theme: &Theme,
    report: &StatReport,
    live: &Live,
    days: Option<i64>,
) -> Option<(String, String)> {
    let a = &report.aggregated;
    let t = &theme.tones;
    let kpis: [(&str, String, String, Color32); 5] = [
        (
            tl("Merged"),
            a.merged_count.to_string(),
            tf(
                "{week} this week · {today} today",
                &[
                    ("week", &a.merged_this_week.to_string()),
                    ("today", &a.merged_today.to_string()),
                ],
            ),
            theme.palette.text_muted,
        ),
        (
            tl("Throughput"),
            a.throughput_per_week.map_or("—".into(), per_week),
            tf(
                "{n} in the last 30 days",
                &[("n", &a.merged_last_30_days.to_string())],
            ),
            theme.palette.text_muted,
        ),
        (
            tl("Median time to merge"),
            hours(a.cycle_time_median_hours),
            format!(
                "P75 {} · P90 {}",
                hours(a.cycle_time_p75_hours),
                hours(a.cycle_time_p90_hours)
            ),
            theme.palette.text_muted,
        ),
        // The time to merge without the development: an MR opened as a draft
        // when the work starts counts from its move to ready.
        (
            tl("Ready → merge"),
            hours(a.ready_to_merge_median_hours),
            tf(
                "median · P75 {p75} · {n} MRs",
                &[
                    ("p75", &hours(a.ready_to_merge_p75_hours)),
                    ("n", &a.ready_to_merge_sample_size.to_string()),
                ],
            ),
            theme.palette.text_muted,
        ),
        (
            tl("Pipeline fail rate"),
            percent(a.avg_pipeline_failure_rate),
            tf(
                "{percent} of MRs with pipeline data",
                &[(
                    "percent",
                    &format!("{:.0}%", a.pipeline_data_coverage * 100.0),
                )],
            ),
            match a.avg_pipeline_failure_rate {
                Some(rate) if rate > 0.15 => t.red.fg,
                Some(rate) if rate > 0.08 => t.amber.fg,
                Some(_) => t.green.fg,
                None => theme.palette.text_muted,
            },
        ),
    ];
    let b = report.baseline.as_ref();
    let trends = [
        trend(
            theme,
            Some(a.merged_count as f64),
            b.map(|b| b.merged_count as f64),
            (a.merged_count, b.map_or(0, |b| b.merged_count)),
            Change::Relative,
            Better::Higher,
            |n| format!("{n:.0}"),
        ),
        trend(
            theme,
            a.throughput_per_week,
            b.and_then(|b| b.throughput_per_week),
            (a.merged_count, b.map_or(0, |b| b.merged_count)),
            Change::Relative,
            Better::Higher,
            per_week,
        ),
        trend(
            theme,
            a.cycle_time_median_hours,
            b.and_then(|b| b.cycle_time_median_hours),
            (
                a.cycle_time_sample_size,
                b.map_or(0, |b| b.cycle_time_sample_size),
            ),
            Change::Relative,
            Better::Lower,
            |h| hours(Some(h)),
        ),
        trend(
            theme,
            a.ready_to_merge_median_hours,
            b.and_then(|b| b.ready_to_merge_median_hours),
            (
                a.ready_to_merge_sample_size,
                b.map_or(0, |b| b.ready_to_merge_sample_size),
            ),
            Change::Relative,
            Better::Lower,
            |h| hours(Some(h)),
        ),
        trend(
            theme,
            a.avg_pipeline_failure_rate,
            b.and_then(|b| b.avg_pipeline_failure_rate),
            (with_pipelines(a), b.map_or(0, with_pipelines)),
            Change::Points,
            Better::Lower,
            |v| percent(Some(v)),
        ),
    ];
    let mut trends = trends.into_iter();
    let cards: Vec<_> = kpis
        .into_iter()
        .map(|k| (k, trends.next().flatten()))
        .collect();
    // Five tiles in a row once each keeps its name whole; else three, then
    // two (never four and a lone one).
    let per_row = match fit(ui, 200.0, 5) {
        4 => 3,
        n => n,
    };
    for chunk in cards.chunks(per_row) {
        columns(ui, per_row, |ui, i| {
            if let Some(((label, value, caption, color), trend)) = chunk.get(i) {
                kpi(ui, theme, label, value, caption, *color, trend.as_ref());
            }
        });
    }
    if b.is_some() {
        let mut legend = tl(
            "▲ ▼ against the previous period of the same length · green: better · orange: worse · grey: too few to tell",
        )
        .to_string();
        if let Some(days) = days {
            let now = chrono::Utc::now();
            let day = |t: chrono::DateTime<chrono::Utc>| {
                t.with_timezone(&chrono::Local).format("%d/%m").to_string()
            };
            let start = now - chrono::Duration::days(days);
            let earlier = start - chrono::Duration::days(days);
            legend.push_str(" · ");
            legend.push_str(&tf(
                "{from} – {to} against {from_before} – {to_before}",
                &[
                    ("from", &day(start)),
                    ("to", &day(now)),
                    ("from_before", &day(earlier)),
                    ("to_before", &day(start)),
                ],
            ));
        }
        ui.label(muted(theme, legend));
    }

    let side_by_side = ui.available_width() > 760.0;
    let mut pick = None;
    let mut draw_sizes = |ui: &mut Ui| pick = size_chart(ui, theme, report);
    let draw_backlog = |ui: &mut Ui| backlog(ui, theme, report, live);
    if side_by_side {
        let width = ui.available_width();
        let left = (width - GAP) * 0.6;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(Vec2::new(left, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(left);
                draw_sizes(ui);
            });
            ui.allocate_ui_with_layout(
                Vec2::new(width - left - GAP, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(width - left - GAP);
                    draw_backlog(ui);
                },
            );
        });
    } else {
        draw_sizes(ui);
        draw_backlog(ui);
    }
    pick
}

/// Median time to merge per MR size, as vertical bars. A size reads in full on
/// hover; a click lists its merged MRs still tracked (key, label).
pub(super) fn size_chart(
    ui: &mut Ui,
    theme: &Theme,
    report: &StatReport,
) -> Option<(String, String)> {
    let buckets = &report.aggregated.size_buckets;
    let mut pick = None;
    card(ui, theme, tl("Time to merge by MR size"), |ui| {
        ui.horizontal(|ui| {
            dot(ui, theme.palette.accent, 8.0);
            ui.label(muted(theme, tl("Median time to merge")));
            ui.add_space(8.0);
            dot(ui, theme.tones.purple.fg, 8.0);
            ui.label(muted(theme, tl("Merged MRs")));
        });
        ui.add_space(4.0);
        if buckets.is_empty() {
            ui.label(muted(theme, tl("Not enough merged MRs yet.")));
            return;
        }
        let height = 180.0;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), height + 36.0),
            Sense::hover(),
        );
        let chart = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), height));
        ui.painter().hline(
            chart.x_range(),
            chart.bottom(),
            Stroke::new(1.0, theme.palette.border),
        );
        let max_time = buckets
            .iter()
            .filter_map(|b| b.cycle_time_median_hours)
            .fold(0.0, f64::max)
            .max(f64::EPSILON);
        let max_count = buckets
            .iter()
            .map(|b| b.merged_mrs)
            .max()
            .unwrap_or(1)
            .max(1);
        let slot = chart.width() / buckets.len() as f32;
        for (i, bucket) in buckets.iter().enumerate() {
            let center = chart.left() + slot * (i as f32 + 0.5);
            let column = egui::Rect::from_min_max(
                egui::pos2(chart.left() + slot * i as f32, chart.top()),
                egui::pos2(chart.left() + slot * (i as f32 + 1.0), rect.bottom()),
            );
            let response = ui.interact(column, ui.scope_id().with(("size", i)), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    column,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised.gamma_multiply(0.6),
                );
            }
            let range = match bucket.max_changed_lines {
                Some(max) => tf(
                    "{min}–{max} changed lines",
                    &[
                        ("min", &bucket.min_changed_lines.to_string()),
                        ("max", &max.to_string()),
                    ],
                ),
                None => tf(
                    "{min}+ changed lines",
                    &[("min", &bucket.min_changed_lines.to_string())],
                ),
            };
            let response = response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .rich(
                    Tip::new()
                        .title(format!("{} · {range}", tl(&bucket.label)))
                        .text(tf(
                            "Median time to merge: {time}",
                            &[("time", &hours(bucket.cycle_time_median_hours))],
                        ))
                        .text(tn(
                            bucket.merged_mrs as u64,
                            "{n} merged MR",
                            "{n} merged MRs",
                        ))
                        .hint(tl("Click to list the tracked ones")),
                );
            if response.clicked() {
                let key = format!(
                    "size:{}:{}",
                    bucket.min_changed_lines,
                    bucket
                        .max_changed_lines
                        .map_or(String::new(), |m| m.to_string())
                );
                pick = Some((key, format!("{} · {}", tl("Size"), tl(&bucket.label))));
            }
            let bar_width = (slot * 0.28).min(36.0);
            let time = bucket.cycle_time_median_hours.unwrap_or(0.0);
            let time_height = (time / max_time) as f32 * (height - 20.0);
            let count_height = bucket.merged_mrs as f32 / max_count as f32 * (height - 20.0);
            let time_rect = egui::Rect::from_min_max(
                egui::pos2(center - bar_width - 2.0, chart.bottom() - time_height),
                egui::pos2(center - 2.0, chart.bottom()),
            );
            let count_rect = egui::Rect::from_min_max(
                egui::pos2(center + 2.0, chart.bottom() - count_height),
                egui::pos2(center + bar_width + 2.0, chart.bottom()),
            );
            let radius = CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            };
            ui.painter()
                .rect_filled(time_rect, radius, theme.palette.accent);
            ui.painter()
                .rect_filled(count_rect, radius, theme.tones.purple.fg);
            ui.painter().text(
                egui::pos2(time_rect.center().x, time_rect.top() - 4.0),
                egui::Align2::CENTER_BOTTOM,
                hours(bucket.cycle_time_median_hours),
                egui::FontId::proportional(theme.text_size - 2.5),
                theme.palette.text_secondary,
            );
            ui.painter().text(
                egui::pos2(count_rect.center().x, count_rect.top() - 4.0),
                egui::Align2::CENTER_BOTTOM,
                bucket.merged_mrs.to_string(),
                egui::FontId::proportional(theme.text_size - 2.5),
                theme.palette.text_secondary,
            );
            ui.painter().text(
                egui::pos2(center, chart.bottom() + 8.0),
                egui::Align2::CENTER_TOP,
                tl(&bucket.label),
                egui::FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
            );
        }
    });
    pick
}

pub(super) fn backlog(ui: &mut Ui, theme: &Theme, report: &StatReport, live: &Live) {
    let t = &theme.tones;
    let ages = &live.ages;
    let open = live.open;
    let median = (!ages.is_empty()).then(|| ages[ages.len() / 2]);
    let oldest = ages.last().copied();
    let queue = &report.poisson.queue_insight;
    card(ui, theme, tl("Backlog health"), |ui| {
        ui.label(muted(
            theme,
            tl("The tracked MRs open now, as on the cockpit"),
        ));
        let scale = oldest.unwrap_or(1.0).max(30.0) as f32;
        meter(
            ui,
            theme,
            tl("Open MRs"),
            &open.to_string(),
            (open as f32 / 25.0).min(1.0),
            theme.palette.accent,
        );
        meter(
            ui,
            theme,
            tl("Median age"),
            &median.map_or("—".into(), |d| days(d, 1)),
            median.unwrap_or(0.0) as f32 / scale,
            t.green.fg,
        );
        meter(
            ui,
            theme,
            tl("Oldest"),
            &oldest.map_or("—".into(), |d| days(d, 0)),
            oldest.unwrap_or(0.0) as f32 / scale,
            t.red.fg,
        );
        let stale = format!("{} · {} · {}", live.stale[0], live.stale[1], live.stale[2]);
        meter(
            ui,
            theme,
            tl("Stale ≥ 7 · 14 · 30 days"),
            &stale,
            live.stale[0] as f32 / open.max(1) as f32,
            t.amber.fg,
        );
        let wait = match queue.status {
            QueueStatus::OverCapacity => tl("Over capacity").to_string(),
            QueueStatus::Stable => hours(queue.expected_wait_hours),
            QueueStatus::InsufficientData => "—".to_string(),
        };
        let fraction = queue.traffic_intensity.unwrap_or(0.0).min(1.0) as f32;
        meter(
            ui,
            theme,
            tl("Expected wait (queue model)"),
            &wait,
            fraction,
            t.amber.fg,
        );
    });
}

// ── Flow ─────────────────────────────────────────────────────────────────────
