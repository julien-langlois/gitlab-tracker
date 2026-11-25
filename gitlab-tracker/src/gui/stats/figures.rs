//! The page's building blocks: number formats, meters, trends against the period before, figure rows, bars, KPIs.

use super::*;

/// `count` equal columns `GAP` apart, each drawn by `cell(ui, index)`.
pub(super) fn columns(ui: &mut Ui, count: usize, mut cell: impl FnMut(&mut Ui, usize)) {
    let width = (ui.available_width() - GAP * (count - 1) as f32) / count as f32;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for i in 0..count {
            ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                cell(ui, i);
            });
        }
    });
}

/// How many columns of at least `min` points fit.
pub(super) fn fit(ui: &Ui, min: f32, most: usize) -> usize {
    (((ui.available_width() + GAP) / (min + GAP)).floor() as usize).clamp(1, most)
}

pub(super) fn hours(value: Option<f64>) -> String {
    match value {
        Some(h) if h >= 24.0 => days(h / 24.0, 1),
        Some(h) => tf("{n} h", &[("n", &format!("{h:.1}"))]),
        None => "—".into(),
    }
}

/// "3.5 d": `value` days with `decimals` decimals.
pub(super) fn days(value: f64, decimals: usize) -> String {
    tf("{n} d", &[("n", &format!("{value:.decimals$}"))])
}

/// "2.5 / week".
pub(super) fn per_week(value: f64) -> String {
    tf("{n} / week", &[("n", &format!("{value:.1}"))])
}

pub(super) fn percent(value: Option<f64>) -> String {
    value.map_or("—".into(), |v| format!("{:.1}%", v * 100.0))
}

/// A labelled horizontal bar: caption and value above, the bar below.
pub(super) fn meter(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    value: &str,
    fraction: f32,
    color: Color32,
) {
    ui.spacing_mut().item_spacing.y = 5.0;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme.palette.text_secondary));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(semibold(value, theme.text_size));
        });
    });
    super::super::widgets::bar(ui, theme, fraction, color, 6.0);
    ui.add_space(6.0);
}

/// Which way a figure is good news.
#[derive(Clone, Copy)]
pub(super) enum Better {
    Lower,
    Higher,
    Neither,
}

/// How a figure compares with the previous window: shares (0..1) by points,
/// everything else by relative change.
#[derive(Clone, Copy)]
pub(super) enum Change {
    Relative,
    Points,
}

/// A figure's move since the previous window: "▲ 12%", colored by whether it is
/// good news, with the previous value for the tooltip.
pub(super) struct Trend {
    pub(super) text: String,
    pub(super) color: Color32,
    pub(super) previous: String,
}

/// The MRs that had pipeline data, of `s`'s.
pub(super) fn with_pipelines(s: &gitlab_tracker_stats::AggregatedStats) -> usize {
    (s.pipeline_data_coverage * s.total_mrs as f64).round() as usize
}

/// `samples`: the cases each window's figure stands on; below
/// [`super::super::delivery::MIN_SAMPLE`] on either side, the move is greyed: too few
/// to tell, as on the Delivery page.
pub(super) fn trend(
    theme: &Theme,
    current: Option<f64>,
    previous: Option<f64>,
    samples: (usize, usize),
    change: Change,
    better: Better,
    format: impl Fn(f64) -> String,
) -> Option<Trend> {
    let (current, previous) = (current?, previous?);
    let delta = match change {
        Change::Relative if previous.abs() < f64::EPSILON => return None,
        Change::Relative => (current - previous) / previous * 100.0,
        Change::Points => (current - previous) * 100.0,
    };
    let amount = match change {
        Change::Relative => format!("{:.0}%", delta.abs()),
        Change::Points => tf("{n} pt", &[("n", &format!("{:.1}", delta.abs()))]),
    };
    let steady = delta.abs() < 0.5;
    let up = delta > 0.0;
    let few = samples.0 < super::super::delivery::MIN_SAMPLE
        || samples.1 < super::super::delivery::MIN_SAMPLE;
    let color = match better {
        _ if steady || few => theme.palette.text_muted,
        Better::Neither => theme.palette.text_secondary,
        Better::Higher if up => theme.tones.green.fg,
        Better::Lower if !up => theme.tones.green.fg,
        _ => theme.tones.orange.fg,
    };
    let text = if steady {
        "≈".to_string()
    } else {
        format!("{} {amount}", if up { "▲" } else { "▼" })
    };
    Some(Trend {
        text,
        color,
        previous: if few && !steady {
            format!(
                "{} · {}",
                tf("Previous period: {value}", &[("value", &format(previous))]),
                tl("too few to tell")
            )
        } else {
            tf("Previous period: {value}", &[("value", &format(previous))])
        },
    })
}

/// How much a severity weighs, to keep the worst of several signals.
pub(super) fn rank(severity: &AnomalySeverity) -> u8 {
    match severity {
        AnomalySeverity::Normal => 0,
        AnomalySeverity::Elevated => 1,
        AnomalySeverity::Warning => 2,
        AnomalySeverity::Critical => 3,
    }
}

/// The worst anomaly raised on a metric: the signals named `metric`, `metric_spike`…
pub(super) fn flagged<'a>(report: &'a StatReport, metric: &str) -> Option<&'a AnomalySeverity> {
    report
        .poisson
        .anomalies
        .iter()
        .filter(|s| s.is_anomalous() && s.metric.starts_with(metric))
        .map(|s| &s.severity)
        .max_by_key(|s| rank(s))
}

/// One row of [`figures`].
pub(super) struct Figure<'a> {
    pub(super) caption: &'a str,
    pub(super) value: String,
    /// What the figure means, on hovering its caption.
    pub(super) hint: &'a str,
    pub(super) severity: Option<&'a AnomalySeverity>,
    pub(super) trend: Option<Trend>,
}

impl<'a> Figure<'a> {
    pub(super) fn new(caption: &'a str, value: String, hint: &'a str) -> Self {
        Self {
            caption,
            value,
            hint,
            severity: None,
            trend: None,
        }
    }

    pub(super) fn flag(mut self, severity: Option<&'a AnomalySeverity>) -> Self {
        self.severity = severity;
        self
    }

    pub(super) fn trend(mut self, trend: Option<Trend>) -> Self {
        self.trend = trend;
        self
    }
}

/// A list of figures: caption on the left, value pushed to the right edge, a hairline
/// between rows, so each value reads against its caption across the card's width.
/// A flagged figure gets its signal's badge and color; a trend sits left of its value.
pub(super) fn figures(ui: &mut Ui, theme: &Theme, rows: Vec<Figure<'_>>) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, row) in rows.into_iter().enumerate() {
            if i > 0 {
                let x = ui.max_rect().x_range();
                let y = ui.cursor().top();
                ui.painter()
                    .hline(x, y, Stroke::new(1.0, theme.palette.border_soft));
            }
            let flag = row
                .severity
                .filter(|s| s.is_anomalous())
                .map(|s| severity(s, theme));
            ui.horizontal(|ui| {
                ui.set_height(30.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(row.caption).color(theme.palette.text_secondary))
                    .on_hover_cursor(egui::CursorIcon::Help)
                    .tip(row.hint);
                if let Some((label, tone)) = flag {
                    badge(ui, theme, label, tone);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let color = flag.map_or(theme.palette.text, |(_, tone)| tone.fg);
                    ui.label(semibold(&row.value, theme.text_size).color(color));
                    if let Some(trend) = row.trend {
                        ui.label(
                            RichText::new(trend.text)
                                .size(theme.text_size - 1.5)
                                .color(trend.color),
                        )
                        .tip(trend.previous);
                    }
                });
            });
        }
    });
}

/// One row of a bar list: label on the left, bar in the middle, value on the right.
pub(super) fn bar_row(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    fraction: f32,
    color: Color32,
    value: &str,
) {
    ui.horizontal(|ui| {
        ui.set_height(22.0);
        let label_width = (ui.available_width() * 0.36).clamp(120.0, 260.0);
        ui.allocate_ui_with_layout(
            Vec2::new(label_width, 22.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                // Keeps the column's width whatever the label's: the bars of a list line up.
                ui.set_min_width(label_width);
                ui.add(
                    egui::Label::new(RichText::new(label).color(theme.palette.text_secondary))
                        .truncate(),
                );
            },
        );
        let value_width = 90.0;
        let bar_width = (ui.available_width() - value_width - 12.0).max(40.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, 10.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), theme.palette.content);
        let mut filled = rect;
        filled.set_width(rect.width() * fraction.clamp(0.0, 1.0));
        if filled.width() > 0.5 {
            ui.painter()
                .rect_filled(filled, CornerRadius::same(5), color);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(mono(value, theme.text_size - 1.0));
        });
    });
}

/// The top `limit` entries of a map, largest value first.
pub(super) fn top(
    map: &std::collections::HashMap<String, f64>,
    limit: usize,
) -> Vec<(String, f64)> {
    let mut entries: Vec<_> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    entries.sort_by(|a, b| b.1.total_cmp(&a.1));
    entries.truncate(limit);
    entries
}

pub(super) fn bar_list(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    entries: &[(String, f64)],
    color: Color32,
    format: impl Fn(f64) -> String,
) {
    card(ui, theme, title, |ui| {
        if entries.is_empty() {
            ui.label(muted(theme, tl("Not enough data yet.")));
            return;
        }
        let max = entries
            .iter()
            .map(|e| e.1)
            .fold(0.0, f64::max)
            .max(f64::EPSILON);
        for (label, value) in entries {
            bar_row(
                ui,
                theme,
                label,
                (*value / max) as f32,
                color,
                &format(*value),
            );
        }
    });
}

// ── Overview ─────────────────────────────────────────────────────────────────

/// [`super::super::widgets::kpi`] with the figure's trend on the label's line.
pub(super) fn kpi(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    value: &str,
    caption: &str,
    caption_color: Color32,
    trend: Option<&Trend>,
) {
    theme
        .card_frame()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            // The trend first, from the right: the label truncates in what is left.
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(trend) = trend {
                        ui.label(
                            RichText::new(&trend.text)
                                .size(theme.text_size - 1.5)
                                .color(trend.color),
                        )
                        .tip(&trend.previous);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(muted(theme, label))
                                .truncate()
                                .show_tooltip_when_elided(false),
                        )
                        .rich(Tip::new().title(label).text(kpi_hint(label)));
                    });
                });
            });
            ui.label(semibold(value, 24.0).color(theme.palette.text));
            // One line, so the tiles of a row keep the same height.
            ui.add(
                egui::Label::new(
                    RichText::new(caption)
                        .size(theme.text_size - 1.5)
                        .color(caption_color),
                )
                .truncate()
                .show_tooltip_when_elided(false),
            )
            .tip(caption);
        });
}

/// What a tile measures, on hovering its name; empty when the name says it.
pub(super) fn kpi_hint(label: &str) -> &'static str {
    if label == tl("Median time to merge") {
        tl("From the opening to the merge: the whole cycle. An MR opened as a draft when the work starts has its development in it.")
    } else if label == tl("Ready → merge") {
        tl("From up for review to the merge: out of draft for an MR opened as a draft, else its first review request. The time to merge without the development. MRs recorded before this was known are not in it.")
    } else {
        ""
    }
}
