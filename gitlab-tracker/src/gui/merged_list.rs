//! The merged MRs of a period on the merge requests page, as the cockpit's Flow
//! counts and Today's "Merged today" open them: the view and the "Merged
//! between" days set, the latest merge first, a heading per day.
//!
//! The Flow counts come from the stats' history: every merge of the project
//! since recording started, tracked or not. The page lists the tracked MRs only,
//! so a banner says how many of the count it shows, while the list is still the
//! one the count opened.

use bevel_theme::fonts;
use egui::{Align, CornerRadius, Layout, Margin, RichText, Ui};

use super::table::GroupBy;
use super::{GuiApp, Page};
use crate::app::{ActiveFilter, App, SortColumn, SortOrder};
use crate::i18n::{tf, tl};
use crate::models::GitlabMrState;
use crate::ui::cockpit::History;

/// A count of the history the page was opened on: the banner's figures.
#[derive(Debug, Clone)]
pub struct Gap {
    pub history: History,
    /// What the cockpit counted.
    pub counted: usize,
    /// The count's name ("Merged this week").
    pub label: String,
    /// The filters it set, to know when the list is no longer that one.
    filters: (ActiveFilter, Vec<ActiveFilter>),
}

fn index_of(app: &App, id: &str) -> Option<usize> {
    app.filter_defs.iter().position(|d| d.id == id)
}

/// The view `view` with `stacked` on top, alone: no focus, no search, no status
/// picked; then the merge requests page, the latest merge first by day when
/// they are merged MRs.
pub fn show(
    g: &mut GuiApp,
    view: &str,
    stacked: &[(&str, String)],
    by_merge: bool,
) -> Option<(ActiveFilter, Vec<ActiveFilter>)> {
    let view = ActiveFilter {
        index: index_of(&g.app, view)?,
        query: String::new(),
    };
    let stacked: Vec<ActiveFilter> = stacked
        .iter()
        .filter_map(|(id, query)| {
            Some(ActiveFilter {
                index: index_of(&g.app, id)?,
                query: query.clone(),
            })
        })
        .collect();
    if g.app.focus.is_some() {
        g.next_focus = Some(None);
    }
    g.app.input.clear();
    g.app.milestone_suggestions.clear();
    // Set now, not as pending actions: the banner compares them next frame.
    g.app.set_status_pick(None);
    g.app.set_filters(stacked.clone());
    g.app.set_filter(view.index, String::new());
    if by_merge {
        g.app.sort_column = SortColumn::MergedAt;
        g.app.sort_order = SortOrder::Descending;
        g.app.sort_mrs();
        g.group_by = GroupBy::MergedDay;
        g.folded.clear();
    }
    g.views.active = None;
    g.merged_gap = None;
    g.open_page(Page::MergeRequests);
    Some((view, stacked))
}

/// The MRs behind a Flow count of the history, with the banner when fewer are
/// tracked than counted.
pub fn show_history(g: &mut GuiApp, history: History, counted: usize, label: String) {
    let today = chrono::Local::now().date_naive();
    let (view, stacked) = match history {
        History::Merged(period) => (
            "state_merged",
            period
                .map(|p| vec![("merged_between", p.range(today).query())])
                .unwrap_or_default(),
        ),
        History::Closed => ("state_closed", Vec::new()),
    };
    let by_merge = matches!(history, History::Merged(_));
    if let Some(filters) = show(g, view, &stacked, by_merge) {
        g.merged_gap = Some(Gap {
            history,
            counted,
            label,
            filters,
        });
    }
}

/// The tracked MRs `history` counts.
pub fn tracked(app: &App, history: History) -> usize {
    let today = chrono::Local::now().date_naive();
    app.mrs
        .iter()
        .filter(|mr| match history {
            History::Merged(period) => {
                mr.state == GitlabMrState::Merged
                    && period.is_none_or(|p| p.range(today).contains_at(mr.merged_at.as_deref()))
            }
            History::Closed => mr.state == GitlabMrState::Closed,
        })
        .count()
}

/// The banner over the table: how many of the count the list shows, and why
/// the others are not there. Gone with a click on ✕, or once the filters change.
pub fn banner(g: &mut GuiApp, ui: &mut Ui) {
    let Some(gap) = &g.merged_gap else {
        return;
    };
    if (&g.app.active_filter, &g.app.filters) != (&gap.filters.0, &gap.filters.1) {
        g.merged_gap = None;
        return;
    }
    let shown = tracked(&g.app, gap.history);
    if shown >= gap.counted {
        return;
    }
    let theme = g.theme;
    let tone = theme.tones.blue;
    let mut close = false;
    egui::Frame::new()
        .fill(tone.bg)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 8))
        .outer_margin(Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new("ℹ").color(tone.fg));
                ui.label(
                    RichText::new(tf(
                        "{label}: {shown} of the {counted} MRs are tracked here",
                        &[
                            ("label", &gap.label),
                            ("shown", &shown.to_string()),
                            ("counted", &gap.counted.to_string()),
                        ],
                    ))
                    .font(fonts::semibold(theme.text_size))
                    .color(tone.fg),
                );
                ui.add(
                    egui::Label::new(super::widgets::muted(
                        &theme,
                        tf(
                            "the stats count {n} more, not in your tracked MRs",
                            &[("n", &(gap.counted - shown).to_string())],
                        ),
                    ))
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui
                        .add(egui::Button::new(RichText::new("✕").color(tone.fg)).frame(false))
                        .clicked();
                });
            });
        });
    if close {
        g.merged_gap = None;
    }
}

/// The tooltip of a Flow count of the history: how many of them the list shows.
pub fn tip(app: &App, history: History, counted: usize) -> String {
    let shown = tracked(app, history);
    if shown >= counted {
        tl("All of them are tracked: the list shows them").to_string()
    } else {
        tf(
            "{shown} of them are tracked: the list shows those",
            &[("shown", &shown.to_string())],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merged_range::Period;

    #[tokio::test]
    async fn the_gap_counts_the_tracked_mrs_of_the_period() {
        let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        for mr in &mut app.mrs {
            mr.data.state = GitlabMrState::Opened;
            mr.data.merged_at = None;
        }
        let now = chrono::Local::now();
        let at = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339();
        for (i, days) in [0, 0, 3, 40].into_iter().enumerate() {
            app.mrs[i].data.state = GitlabMrState::Merged;
            app.mrs[i].data.merged_at = Some(at(days));
        }
        app.mrs[4].data.state = GitlabMrState::Closed;
        assert_eq!(tracked(&app, History::Merged(Some(Period::Today))), 2);
        assert_eq!(tracked(&app, History::Merged(Some(Period::Last7Days))), 3);
        assert_eq!(tracked(&app, History::Merged(None)), 4);
        assert_eq!(tracked(&app, History::Closed), 1);
        assert!(tip(&app, History::Merged(None), 4).starts_with("All"));
        assert!(tip(&app, History::Merged(None), 9).starts_with('4'));
    }
}
