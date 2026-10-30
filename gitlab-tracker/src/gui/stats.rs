//! The statistics page: the tabs and the time window in a side panel, each tab
//! drawn natively (KPI cards, bars, signals), and summary insights on the right.

use bevel_theme::{Theme, Tone};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use gitlab_tracker_stats::poisson::{AnomalySeverity, QueueStatus};
use gitlab_tracker_stats::{CorrelationStrength, MetricPair, StatReport};

use super::shell::Sizes;
use super::widgets::{
    badge, card, dot, kpi, mono, muted, page_title, rule, section_title, semibold, spinner,
};
use super::GuiApp;
use crate::actions::Action;
use crate::app::{StatsTab, StatsWindow};
use crate::i18n::{tf, tl, tn};

const TABS: [StatsTab; 5] = [
    StatsTab::Overview,
    StatsTab::Flow,
    StatsTab::Quality,
    StatsTab::Forecasts,
    StatsTab::Correlations,
];

/// The time windows' short names (English: translated where drawn).
const WINDOWS: [(StatsWindow, &str); 4] = [
    (StatsWindow::Last30Days, "30 d"),
    (StatsWindow::Last90Days, "90 d"),
    (StatsWindow::Last365Days, "1 y"),
    (StatsWindow::AllTime, "All"),
];

const GAP: f32 = 12.0;

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let theme = g.theme;
    if let Some(width) = sizes.views {
        egui::Panel::left("stats_nav")
            .exact_size(width)
            .resizable(false)
            .frame(theme.panel_frame())
            .show(ui, |ui| {
                let edge = ui.max_rect();
                ui.painter().vline(
                    edge.right() - 0.5,
                    edge.y_range(),
                    Stroke::new(1.0, theme.palette.border),
                );
                ui.set_max_width(ui.available_width());
                nav(g, ui);
            });
    }
    let report = g.app.stats_view.report.clone();
    if let (Some(width), Some(report)) = (sizes.inspector, report.as_ref()) {
        egui::Panel::right("stats_insights")
            .exact_size(width)
            .resizable(false)
            .frame(theme.panel_frame())
            .show(ui, |ui| {
                let edge = ui.max_rect();
                ui.painter().vline(
                    edge.left() + 0.5,
                    edge.y_range(),
                    Stroke::new(1.0, theme.palette.border),
                );
                ui.set_max_width(ui.available_width());
                insights_panel(ui, &theme, report);
            });
    }
    egui::CentralPanel::default()
        .frame(
            theme
                .content_frame()
                .inner_margin(Margin::symmetric(24, 20)),
        )
        .show(ui, |ui| main(g, ui, sizes.views.is_none()));
}

// ── Side panel ───────────────────────────────────────────────────────────────

fn nav(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(semibold(tl("Statistics"), theme.text_size + 1.0));
            ui.label(muted(&theme, tl("Local SQLite snapshots")));
        });
    rule(ui, &theme);
    egui::Frame::new()
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, tab) in TABS.into_iter().enumerate() {
                let selected = g.app.stats_view.tab == tab;
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
                let fill = if selected {
                    theme.palette.selection
                } else if response.hovered() {
                    theme.palette.raised
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
                let (font, color) = if selected {
                    (
                        bevel_theme::fonts::medium(theme.text_size),
                        theme.palette.text,
                    )
                } else {
                    (
                        egui::FontId::proportional(theme.text_size),
                        theme.palette.text_secondary,
                    )
                };
                ui.painter().text(
                    egui::pos2(rect.left() + 10.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    tab.label(),
                    font,
                    color,
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    egui::FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    g.app.stats_view.tab = tab;
                }
            }
        });
    egui::Frame::new()
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(muted(&theme, tl("Period (W)")));
            period_selector(g, ui);
            ui.add_space(8.0);
            if super::widgets::button(
                ui,
                &theme,
                Some(super::icons::Icon::Refresh),
                tl("Recompute"),
                false,
            )
            .on_hover_text(tl("Recompute the report (R)"))
            .clicked()
            {
                g.pending.push(Action::RefreshStats);
            }
        });
}

/// The time windows as a segmented control.
fn period_selector(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    let radius = CornerRadius::same(theme.radii.medium);
    ui.painter().rect_filled(rect, radius, theme.palette.input);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme.palette.border),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(2.0);
    let width = inner.width() / WINDOWS.len() as f32;
    for (i, (window, label)) in WINDOWS.into_iter().enumerate() {
        let segment = egui::Rect::from_min_size(
            egui::pos2(inner.left() + i as f32 * width, inner.top()),
            Vec2::new(width, inner.height()),
        );
        let response = ui.interact(segment, ui.scope_id().with(("period", i)), Sense::click());
        let selected = g.app.stats_view.window == window;
        if selected {
            ui.painter()
                .rect_filled(segment, CornerRadius::same(4), theme.palette.raised);
        }
        let color = if selected || response.hovered() {
            theme.palette.text
        } else {
            theme.palette.text_muted
        };
        ui.painter().text(
            segment.center(),
            egui::Align2::CENTER_CENTER,
            tl(label),
            egui::FontId::proportional(theme.text_size - 1.0),
            color,
        );
        if response.on_hover_text(window.label()).clicked() && !selected {
            g.app.stats_view.window = window;
            g.pending.push(Action::RefreshStats);
        }
    }
}

// ── Main area ────────────────────────────────────────────────────────────────

fn main(g: &mut GuiApp, ui: &mut Ui, compact: bool) {
    let theme = g.theme;
    let view = &g.app.stats_view;
    let tab = view.tab;
    let detail = match view.report.as_ref() {
        Some(report) => format!(
            "{} · {}",
            view.window.label(),
            tn(
                report.aggregated.total_mrs as u64,
                "{n} MR in sample",
                "{n} MRs in sample"
            ),
        ),
        None => view.window.label().to_string(),
    };
    page_title(ui, &theme, tab.label(), detail);
    if compact {
        // No side panel: tabs and window inline.
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for tab in TABS {
                let selected = g.app.stats_view.tab == tab;
                if ui.selectable_label(selected, tab.label()).clicked() {
                    g.app.stats_view.tab = tab;
                }
            }
            ui.add_space(12.0);
            ui.allocate_ui(Vec2::new(220.0, 28.0), |ui| period_selector(g, ui));
        });
    }
    ui.add_space(16.0);

    let view = &g.app.stats_view;
    if view.loading {
        ui.horizontal(|ui| {
            spinner(
                ui,
                ui.spacing().interact_size.y,
                ui.visuals().strong_text_color(),
            );
            ui.label(muted(&theme, tl("Computing statistics…")));
        });
        return;
    }
    if let Some(error) = &view.error {
        ui.label(RichText::new(format!("✘ {error}")).color(theme.tones.red.fg));
        return;
    }
    let Some(report) = view.report.clone() else {
        ui.label(muted(
            &theme,
            tl(
                "No stats available yet. They are recorded as MRs are merged or refreshed: \
             come back after a few MR events.",
            ),
        ));
        return;
    };
    let window = view.window.label();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = GAP;
            match tab {
                StatsTab::Overview => overview(ui, &theme, &report),
                StatsTab::Flow => flow(ui, &theme, &report),
                StatsTab::Quality => quality(ui, &theme, &report),
                StatsTab::Forecasts => forecasts(ui, &theme, &report),
                StatsTab::Correlations => correlations(ui, &theme, &report),
            }
            ui.add_space(4.0);
            let generated = report
                .generated_at
                .get(..16)
                .unwrap_or(&report.generated_at)
                .replace('T', " ");
            ui.label(muted(
                &theme,
                tf(
                    "Generated {date} · {window}",
                    &[("date", &generated), ("window", window)],
                ),
            ));
        });
}

/// `count` equal columns `GAP` apart, each drawn by `cell(ui, index)`.
fn columns(ui: &mut Ui, count: usize, mut cell: impl FnMut(&mut Ui, usize)) {
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
fn fit(ui: &Ui, min: f32, most: usize) -> usize {
    (((ui.available_width() + GAP) / (min + GAP)).floor() as usize).clamp(1, most)
}

fn hours(value: Option<f64>) -> String {
    match value {
        Some(h) if h >= 24.0 => days(h / 24.0, 1),
        Some(h) => tf("{n} h", &[("n", &format!("{h:.1}"))]),
        None => "—".into(),
    }
}

/// "3.5 d": `value` days with `decimals` decimals.
fn days(value: f64, decimals: usize) -> String {
    tf("{n} d", &[("n", &format!("{value:.decimals$}"))])
}

/// "2.5 / week".
fn per_week(value: f64) -> String {
    tf("{n} / week", &[("n", &format!("{value:.1}"))])
}

fn percent(value: Option<f64>) -> String {
    value.map_or("—".into(), |v| format!("{:.1}%", v * 100.0))
}

/// A labelled horizontal bar: caption and value above, the bar below.
fn meter(ui: &mut Ui, theme: &Theme, label: &str, value: &str, fraction: f32, color: Color32) {
    ui.spacing_mut().item_spacing.y = 5.0;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme.palette.text_secondary));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(semibold(value, theme.text_size));
        });
    });
    super::widgets::bar(ui, theme, fraction, color, 6.0);
    ui.add_space(6.0);
}

/// One row of a bar list: label on the left, bar in the middle, value on the right.
fn bar_row(ui: &mut Ui, theme: &Theme, label: &str, fraction: f32, color: Color32, value: &str) {
    ui.horizontal(|ui| {
        ui.set_height(22.0);
        let label_width = (ui.available_width() * 0.36).clamp(120.0, 260.0);
        ui.allocate_ui_with_layout(
            Vec2::new(label_width, 22.0),
            Layout::left_to_right(Align::Center),
            |ui| {
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
fn top(map: &std::collections::HashMap<String, f64>, limit: usize) -> Vec<(String, f64)> {
    let mut entries: Vec<_> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    entries.sort_by(|a, b| b.1.total_cmp(&a.1));
    entries.truncate(limit);
    entries
}

fn bar_list(
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

fn overview(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    let a = &report.aggregated;
    let t = &theme.tones;
    let kpis: [(&str, String, String, Color32); 4] = [
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
    let per_row = fit(ui, 170.0, 4);
    for chunk in kpis.chunks(per_row) {
        columns(ui, per_row, |ui, i| {
            if let Some((label, value, caption, color)) = chunk.get(i) {
                kpi(ui, theme, label, value, theme.palette.text, caption, *color);
            }
        });
    }

    let side_by_side = ui.available_width() > 760.0;
    let draw_sizes = |ui: &mut Ui| size_chart(ui, theme, report);
    let draw_backlog = |ui: &mut Ui| backlog(ui, theme, report);
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
}

/// Median time to merge per MR size, as vertical bars.
fn size_chart(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    let buckets = &report.aggregated.size_buckets;
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
}

fn backlog(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    let a = &report.aggregated;
    let t = &theme.tones;
    let ages = &a.open_mr_ages_days;
    let open = ages.len();
    let median = (!ages.is_empty()).then(|| ages[ages.len() / 2]);
    let oldest = ages.last().copied();
    let queue = &report.poisson.queue_insight;
    card(ui, theme, tl("Backlog health"), |ui| {
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
        let stale = format!(
            "{} · {} · {}",
            a.stale_open_mrs_7d, a.stale_open_mrs_14d, a.stale_open_mrs_30d
        );
        meter(
            ui,
            theme,
            tl("Stale ≥ 7 · 14 · 30 days"),
            &stale,
            a.stale_open_mrs_7d as f32 / open.max(1) as f32,
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

fn flow(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    card(ui, theme, tl("Lead time from merge to branch"), |ui| {
        if report.promotions.is_empty() {
            ui.label(muted(
                theme,
                tl("No promotion to the other tracked branches recorded yet."),
            ));
            return;
        }
        let max = report
            .promotions
            .iter()
            .filter_map(|p| p.p90_hours.or(p.median_hours))
            .fold(0.0, f64::max)
            .max(f64::EPSILON);
        ui.spacing_mut().item_spacing.y = 10.0;
        for p in &report.promotions {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(100.0, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.label(mono(&p.branch, theme.text_size - 0.5));
                    },
                );
                let width = (ui.available_width() - 190.0).max(60.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
                let radius = CornerRadius::same(4);
                ui.painter()
                    .rect_filled(rect, radius, theme.palette.content);
                let mut p90 = rect;
                p90.set_width(rect.width() * (p.p90_hours.unwrap_or(0.0) / max) as f32);
                ui.painter()
                    .rect_filled(p90, radius, theme.palette.accent.gamma_multiply(0.3));
                let mut median = rect;
                median.set_width(rect.width() * (p.median_hours.unwrap_or(0.0) / max) as f32);
                ui.painter()
                    .rect_filled(median, radius, theme.palette.accent);
                ui.label(
                    RichText::new(tf(
                        "median {median} · P90 {p90}",
                        &[
                            ("median", &hours(p.median_hours)),
                            ("p90", &hours(p.p90_hours)),
                        ],
                    ))
                    .color(theme.palette.text_secondary),
                );
            });
        }
        ui.add_space(4.0);
        super::widgets::rule(ui, theme);
        egui::Grid::new("promotions")
            .num_columns(6)
            .spacing(Vec2::new(24.0, 8.0))
            .show(ui, |ui| {
                for title in ["Branch", "MRs", "Median", "P90", "Max", "Via copy"] {
                    let title = tl(title);
                    ui.label(muted(theme, title));
                }
                ui.end_row();
                for p in &report.promotions {
                    ui.label(mono(&p.branch, theme.text_size - 1.0));
                    ui.label(p.arrivals.to_string());
                    ui.label(hours(p.median_hours));
                    ui.label(hours(p.p90_hours));
                    ui.label(hours(p.max_hours));
                    ui.label(format!("{:.0}%", p.copied_share * 100.0));
                    ui.end_row();
                }
            });
    });
    let a = &report.aggregated;
    let per_row = fit(ui, 300.0, 3);
    let lists = [
        (
            tl("Time to merge by author"),
            top(&a.cycle_time_by_author, 8),
            theme.palette.accent,
        ),
        (
            tl("Time to merge by reviewer"),
            top(&a.cycle_time_by_reviewer, 8),
            theme.tones.teal.fg,
        ),
        (
            tl("Time to merge by milestone"),
            top(&a.cycle_time_by_milestone, 8),
            theme.tones.purple.fg,
        ),
    ];
    for chunk in lists.chunks(per_row) {
        columns(ui, per_row, |ui, i| {
            if let Some((title, entries, color)) = chunk.get(i) {
                bar_list(ui, theme, title, entries, *color, |h| hours(Some(h)));
            }
        });
    }
    let mut milestones: Vec<(String, f64)> = a
        .throughput_by_milestone
        .iter()
        .map(|(k, v)| (k.clone(), f64::from(*v)))
        .collect();
    milestones.sort_by(|x, y| y.1.total_cmp(&x.1));
    milestones.truncate(10);
    bar_list(
        ui,
        theme,
        tl("Merged MRs by milestone"),
        &milestones,
        theme.tones.green.fg,
        |v| format!("{v:.0}"),
    );
}

// ── Quality ──────────────────────────────────────────────────────────────────

fn quality(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    let a = &report.aggregated;
    let t = &theme.tones;
    let side_by_side = ui.available_width() > 760.0;
    let confidence = |ui: &mut Ui| {
        card(ui, theme, tl("Data confidence"), |ui| {
            let ratio = |n: usize, target: usize| (n as f32 / target as f32).min(1.0);
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
            meter(
                ui,
                theme,
                tl("Merged MRs with timing"),
                &a.cycle_time_sample_size.to_string(),
                ratio(a.cycle_time_sample_size, 30),
                theme.palette.accent,
            );
            meter(
                ui,
                theme,
                tl("MRs with pipelines"),
                &a.pipeline_sample_size.to_string(),
                ratio(a.pipeline_sample_size, 30),
                theme.palette.accent,
            );
        });
    };
    let review = |ui: &mut Ui| {
        card(ui, theme, tl("Review signals"), |ui| {
            super::widgets::key_values(ui, theme, |rows| {
                rows.row(tl("Average diff"), |ui| {
                    ui.label(tf(
                        "{n} lines",
                        &[("n", &format!("{:.0}", a.avg_diff_size))],
                    ));
                });
                rows.row(tl("Comments"), |ui| {
                    ui.label(tf(
                        "{n} per MR",
                        &[("n", &format!("{:.1}", a.avg_comments))],
                    ));
                });
                rows.row(tl("Density"), |ui| {
                    ui.label(a.avg_comment_density.map_or("—".into(), |d| {
                        tf("{n} per 100 lines", &[("n", &format!("{d:.1}"))])
                    }));
                });
                rows.row(tl("Pipelines"), |ui| {
                    ui.label(tf(
                        "{percent} failing",
                        &[("percent", &percent(a.avg_pipeline_failure_rate))],
                    ));
                });
                rows.row(tl("Abandoned"), |ui| {
                    ui.label(tf(
                        "{percent} ({n} closed)",
                        &[
                            ("percent", &percent(a.abandon_rate)),
                            ("n", &a.closed_count.to_string()),
                        ],
                    ));
                });
            });
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

fn severity(severity: &AnomalySeverity, theme: &Theme) -> (&'static str, Tone) {
    let t = &theme.tones;
    match severity {
        AnomalySeverity::Critical => (tl("CRITICAL"), t.red),
        AnomalySeverity::Warning => (tl("WARNING"), t.amber),
        AnomalySeverity::Elevated => (tl("ELEVATED"), t.blue),
        AnomalySeverity::Normal => (tl("OK"), t.green),
    }
}

/// `pipeline_failure_rate_spike` → "Pipeline failure rate", translated.
fn metric_name(metric: &str) -> String {
    let name = metric.trim_end_matches("_spike").replace('_', " ");
    let mut chars = name.chars();
    let name: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name,
    };
    tl(&name).to_string()
}

fn number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

fn forecasts(ui: &mut Ui, theme: &Theme, report: &StatReport) {
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
                    "≥ {n} merges in {weeks} weeks",
                    &[
                        ("n", &f.target_merges.to_string()),
                        ("weeks", &f.forecast_weeks.to_string()),
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
            ui.add_space(6.0);
            super::widgets::key_values(ui, theme, |rows| {
                let rate = |v: Option<f64>| v.map_or("—".into(), per_week);
                rows.row(tl("Arrivals"), |ui| {
                    ui.label(rate(q.arrival_rate_per_week));
                });
                rows.row(tl("Service"), |ui| {
                    ui.label(rate(q.service_rate_per_week));
                });
                rows.row(tl("Load"), |ui| {
                    ui.label(
                        q.traffic_intensity
                            .map_or("—".into(), |r| format!("{:.0}%", r * 100.0)),
                    );
                });
                rows.row(tl("In the queue"), |ui| {
                    ui.label(q.expected_mrs_in_system.map_or("—".into(), |l| {
                        tf("{n} MRs", &[("n", &format!("{l:.1}"))])
                    }));
                });
                rows.row(tl("Expected wait"), |ui| {
                    ui.label(hours(q.expected_wait_hours));
                });
            });
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

fn pair_label(pair: &MetricPair) -> &'static str {
    tl(match pair {
        MetricPair::DiffSizeVsCycleTime => "Diff size ↔ time to merge",
        MetricPair::DiffSizeVsComments => "Diff size ↔ comments",
        MetricPair::CommentsVsCycleTime => "Comments ↔ time to merge",
        MetricPair::PipelineFailuresVsCycleTime => "Pipeline failures ↔ time to merge",
        MetricPair::CommitsCountVsCycleTime => "Commits ↔ time to merge",
        MetricPair::DiffDifficultyVsCycleTime => "Review effort ↔ time to merge",
    })
}

fn strength(s: &CorrelationStrength) -> &'static str {
    tl(match s {
        CorrelationStrength::Negligible => "negligible",
        CorrelationStrength::Weak => "weak",
        CorrelationStrength::Moderate => "moderate",
        CorrelationStrength::Strong => "strong",
        CorrelationStrength::VeryStrong => "very strong",
    })
}

fn correlations(ui: &mut Ui, theme: &Theme, report: &StatReport) {
    card(ui, theme, tl("Spearman correlations"), |ui| {
        if report.correlations.is_empty() {
            ui.label(muted(theme, tl("Not enough MRs to correlate yet.")));
            return;
        }
        ui.spacing_mut().item_spacing.y = 10.0;
        for c in &report.correlations {
            let significant = c.p_value < 0.05;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let label_width = (ui.available_width() * 0.32).clamp(180.0, 280.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(label_width, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(pair_label(&c.pair))
                                    .color(theme.palette.text_secondary),
                            )
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
            ui.label(muted(
                theme,
                format!(
                    "p = {:.3} · n = {}{}",
                    c.p_value,
                    c.sample_size,
                    if significant {
                        String::new()
                    } else {
                        format!(" · {}", tl("not significant"))
                    }
                ),
            ));
        }
    });
}

// ── Insights ─────────────────────────────────────────────────────────────────

/// Plain-language findings from the report, most telling first.
fn insights(report: &StatReport, theme: &Theme) -> Vec<(String, Color32)> {
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
        if small <= 0.0 || large / small < 1.5 {
            return out;
        }
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
        ));
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
        ));
    }
    if let Some(signal) = report.poisson.anomalies.iter().find(|s| s.is_anomalous()) {
        out.push((
            tf(
                "{metric} is out of its usual range.",
                &[("metric", &metric_name(&signal.metric))],
            ),
            t.red.fg,
        ));
    }
    match report.poisson.queue_insight.status {
        QueueStatus::OverCapacity => out.push((
            tl("MRs arrive faster than they are merged: the backlog grows.").into(),
            t.red.fg,
        )),
        QueueStatus::Stable => {
            if let Some(wait) = report.poisson.queue_insight.expected_wait_hours {
                out.push((
                    tf(
                        "A new MR waits about {time} before it is merged.",
                        &[("time", &hours(Some(wait)))],
                    ),
                    t.green.fg,
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
        ));
    }
    out
}

fn insights_panel(ui: &mut Ui, theme: &Theme, report: &StatReport) {
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
                    for (text, color) in found {
                        egui::Frame::new()
                            .fill(theme.palette.content)
                            .stroke(Stroke::new(1.0, theme.palette.border_soft))
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
                                        .wrap(),
                                    );
                                });
                            });
                    }
                });
        });
}
