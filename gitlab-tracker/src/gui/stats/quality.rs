//! The Quality tab, and how signals are named and graded.

use super::*;

pub(super) fn quality(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    let a = &report.aggregated;
    let t = &theme.tones;
    let side_by_side = ui.available_width() > 760.0;
    let confidence = |ui: &mut Ui| {
        card(ui, theme, tl("Data confidence"), |ui| {
            let ratio = |n: usize| (n as f32 / SAMPLE_TARGET as f32).min(1.0);
            let sample = |n: usize| format!("{n} / {SAMPLE_TARGET}");
            meter(
                ui,
                theme,
                tl("Pipeline data coverage"),
                &format!("{:.0}%", a.pipeline_data_coverage * 100.0),
                a.pipeline_data_coverage as f32,
                t.green.fg,
            );
            meter(
                ui,
                theme,
                tl("Reviewer coverage"),
                &format!("{:.0}%", a.reviewer_coverage * 100.0),
                a.reviewer_coverage as f32,
                t.green.fg,
            );
            meter(
                ui,
                theme,
                tl("Milestone coverage"),
                &format!("{:.0}%", a.milestone_coverage * 100.0),
                a.milestone_coverage as f32,
                t.green.fg,
            );
            let samples = ui.scope(|ui| {
                meter(
                    ui,
                    theme,
                    tl("Merged MRs with timing"),
                    &sample(a.cycle_time_sample_size),
                    ratio(a.cycle_time_sample_size),
                    theme.palette.accent,
                );
                meter(
                    ui,
                    theme,
                    tl("MRs with pipelines"),
                    &sample(a.pipeline_sample_size),
                    ratio(a.pipeline_sample_size),
                    theme.palette.accent,
                );
            });
            samples.response.tip(tl(
                "Medians and correlations become reliable from about 30 MRs.",
            ));
        });
    };
    let review = |ui: &mut Ui| {
        card(ui, theme, tl("Review signals"), |ui| {
            let b = report.baseline.as_ref();
            let lines = |n: f64| tf("{n} lines", &[("n", &format!("{n:.0}"))]);
            let comments = |n: f64| tf("{n} per MR", &[("n", &format!("{n:.1}"))]);
            let density = |d: f64| tf("{n} per 100 lines", &[("n", &format!("{d:.1}"))]);
            let share = |v: f64| percent(Some(v));
            figures(
                ui,
                theme,
                vec![
                    Figure::new(
                        tl("Average diff"),
                        lines(a.avg_diff_size),
                        tl("Mean lines changed (added + removed) per MR."),
                    )
                    .trend(trend(
                        theme,
                        Some(a.avg_diff_size),
                        b.map(|b| b.avg_diff_size),
                        (a.total_mrs, b.map_or(0, |b| b.total_mrs)),
                        Change::Relative,
                        Better::Lower,
                        lines,
                    )),
                    Figure::new(
                        tl("Comments"),
                        comments(a.avg_comments),
                        tl("Mean number of comments per MR."),
                    )
                    .trend(trend(
                        theme,
                        Some(a.avg_comments),
                        b.map(|b| b.avg_comments),
                        (a.total_mrs, b.map_or(0, |b| b.total_mrs)),
                        Change::Relative,
                        Better::Neither,
                        comments,
                    )),
                    Figure::new(
                        tl("Density"),
                        a.avg_comment_density.map_or("—".into(), density),
                        tl("Comments per 100 changed lines: how much discussion each line draws."),
                    )
                    .flag(flagged(report, "comment_density"))
                    .trend(trend(
                        theme,
                        a.avg_comment_density,
                        b.and_then(|b| b.avg_comment_density),
                        (a.total_mrs, b.map_or(0, |b| b.total_mrs)),
                        Change::Relative,
                        Better::Neither,
                        density,
                    )),
                    Figure::new(
                        tl("Pipelines"),
                        tf(
                            "{percent} failing",
                            &[("percent", &percent(a.avg_pipeline_failure_rate))],
                        ),
                        tl("Mean share of failed pipelines, on the MRs that ran some."),
                    )
                    .flag(flagged(report, "pipeline_failure_rate"))
                    .trend(trend(
                        theme,
                        a.avg_pipeline_failure_rate,
                        b.and_then(|b| b.avg_pipeline_failure_rate),
                        (with_pipelines(a), b.map_or(0, with_pipelines)),
                        Change::Points,
                        Better::Lower,
                        share,
                    )),
                    Figure::new(
                        tl("Abandoned"),
                        tf(
                            "{percent} ({n} closed)",
                            &[
                                ("percent", &percent(a.abandon_rate)),
                                ("n", &a.closed_count.to_string()),
                            ],
                        ),
                        tl("Share of the finished MRs closed without being merged."),
                    )
                    .flag(flagged(report, "abandon_rate"))
                    .trend(trend(
                        theme,
                        a.abandon_rate,
                        b.and_then(|b| b.abandon_rate),
                        (
                            a.merged_count + a.closed_count,
                            b.map_or(0, |b| b.merged_count + b.closed_count),
                        ),
                        Change::Points,
                        Better::Lower,
                        share,
                    )),
                ],
            );
        });
    };
    if side_by_side {
        columns(
            ui,
            2,
            |ui, i| if i == 0 { confidence(ui) } else { review(ui) },
        );
    } else {
        confidence(ui);
        review(ui);
    }
    card(ui, theme, tl("MR size buckets"), |ui| {
        if a.size_buckets.is_empty() {
            ui.label(muted(theme, tl("Not enough data yet.")));
            return;
        }
        let total = a
            .size_buckets
            .iter()
            .map(|b| b.total_mrs)
            .sum::<usize>()
            .max(1);
        for bucket in &a.size_buckets {
            let share = bucket.total_mrs as f32 / total as f32;
            let value = format!(
                "{:.0}% · {}",
                share * 100.0,
                hours(bucket.cycle_time_median_hours)
            );
            bar_row(
                ui,
                theme,
                tl(&bucket.label),
                share,
                theme.palette.accent,
                &value,
            );
        }
        ui.label(muted(
            theme,
            tl("Share of the sample · median time to merge"),
        ));
    });
}

// ── Forecasts ────────────────────────────────────────────────────────────────

pub(super) fn severity(severity: &AnomalySeverity, theme: &Theme) -> (&'static str, Tone) {
    let t = &theme.tones;
    match severity {
        AnomalySeverity::Critical => (tl("CRITICAL"), t.red),
        AnomalySeverity::Warning => (tl("WARNING"), t.amber),
        AnomalySeverity::Elevated => (tl("ELEVATED"), t.blue),
        AnomalySeverity::Normal => (tl("OK"), t.green),
    }
}

/// `pipeline_failure_rate_spike` → "Pipeline failure rate", translated.
pub(super) fn metric_name(metric: &str) -> String {
    // Pressure signals on MRs left open: their id is not a readable name.
    match metric {
        "stale_open_mrs_30d" => return tl("MRs open for over 30 days").to_string(),
        "stale_open_mrs_14d" => return tl("MRs open for over 14 days").to_string(),
        "stale_open_mrs_7d" => return tl("MRs open for over 7 days").to_string(),
        _ => {}
    }
    let name = metric.trim_end_matches("_spike").replace('_', " ");
    let mut chars = name.chars();
    let name: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name,
    };
    tl(&name).to_string()
}

pub(super) fn number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}
