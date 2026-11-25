//! The statistics page: the tabs and the time window in a side panel, each tab
//! drawn natively (KPI cards, bars, signals), and summary insights on the right.

use bevel_theme::{Theme, Tone};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use gitlab_tracker_stats::poisson::{AnomalySeverity, QueueStatus};
use gitlab_tracker_stats::{CorrelationStrength, MetricPair, StatReport};

use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{
    badge, card, dot, mono, muted, page_title, rule, section_title, semibold, spinner,
};
use super::GuiApp;
use crate::actions::Action;
use crate::app::{StatsTab, StatsWindow};
use crate::i18n::{tf, tl, tn};

mod correlations;
mod figures;
mod flow_tab;
mod forecasts;
mod insights;
mod overview;
mod quality;

use correlations::*;
use figures::*;
use flow_tab::*;
use forecasts::*;
use insights::*;
use overview::*;
use quality::*;

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

/// MRs from which the sample-based figures (medians, correlations) are reliable.
const SAMPLE_TARGET: usize = 30;

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
                insights_panel(ui, &theme, report, &mut g.app.stats_view.tab);
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
            .tip(tl("Recompute the report (R)"))
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
        if response.tip(window.label()).clicked() && !selected {
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
    let live = Live::of(&g.app);
    let mut pick = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = GAP;
            match tab {
                StatsTab::Overview => {
                    let days = match view.window.to_query_window() {
                        Some(gitlab_tracker_stats::TimeWindow::LastDays(n)) => Some(i64::from(n)),
                        _ => None,
                    };
                    pick = overview(ui, &theme, &report, &live, days);
                }
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
    if let Some((key, label)) = pick {
        g.focus_on(&key, label);
    }
}

/// What is open right now, from the tracked MRs: the backlog shows the same
/// MRs as the cockpit (the snapshots only know the MRs recorded in the window).
struct Live {
    /// Open MRs.
    open: usize,
    /// Ages in days of the open MRs (those with a creation date), ascending.
    ages: Vec<f64>,
    /// Open MRs untouched for 7, 14 and 30 days.
    stale: [usize; 3],
}

impl Live {
    fn of(app: &crate::app::App) -> Self {
        let now = chrono::Utc::now();
        let days = |at: Option<&str>| {
            at.and_then(|at| at.parse::<chrono::DateTime<chrono::Utc>>().ok())
                .map(|at| (now - at).num_hours() as f64 / 24.0)
        };
        let open: Vec<_> = app
            .mrs
            .iter()
            .filter(|mr| mr.state == crate::models::GitlabMrState::Opened)
            .collect();
        let mut ages: Vec<f64> = open
            .iter()
            .filter_map(|mr| days(mr.created_at.as_deref()))
            .collect();
        ages.sort_by(f64::total_cmp);
        let idle: Vec<f64> = open
            .iter()
            .filter_map(|mr| days(mr.updated_at.as_deref()))
            .collect();
        let stale = [7.0, 14.0, 30.0].map(|limit| idle.iter().filter(|d| **d >= limit).count());
        Self {
            open: open.len(),
            ages,
            stale,
        }
    }
}
