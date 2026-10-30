//! State of the Stats overlay: selected tab and window, last report, and the
//! generation guard against late responses. Split out of `app.rs`.

/// UI state for the Stats fullscreen overlay (only compiled with the `stats` feature).
///
/// Decoupled from `StatReport` so the overlay can render a "Loading…" state
/// while the async aggregation is in flight, and an "Insufficient data" state
/// when fewer than 3 snapshots are available.
#[derive(Debug)]
pub struct StatsViewState {
    /// The last successfully computed report, ready to render.
    pub report: Option<gitlab_tracker_stats::StatReport>,
    /// Aggregation over the whole retained history, for the cockpit (Flow counters,
    /// merged MRs per release). Unlike `report` it does not follow `window`: the
    /// cockpit must not change when `[W]` is pressed in the overlay.
    pub cockpit: Option<gitlab_tracker_stats::AggregatedStats>,
    /// Vertical scroll offset inside the stats overlay (in lines).
    pub scroll: u16,
    /// Whether an async aggregation is currently in flight.
    pub loading: bool,
    /// Human-readable error shown when aggregation fails.
    pub error: Option<String>,
    /// The time window currently selected by the user (cycles with `[W]`).
    pub window: StatsWindow,
    /// The currently selected stats dashboard tab.
    pub tab: StatsTab,
    /// Sprint duration in weeks for throughput forecasts — read from
    /// `stats_sprint_weeks` in `projects.toml`, defaults to 2.
    pub sprint_weeks: u32,
    /// Id of the latest report request; older responses are stale.
    pub generation: u64,
    /// Local calendar day the report was last known to be current for.
    pub report_day: chrono::NaiveDate,
}

impl StatsViewState {
    /// Starts a new report request and returns its generation.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Whether the local day changed since the last call (or since startup). The
    /// report's day-relative figures ("merged today", this week, open MR ages) are
    /// computed against "now", so it must be recomputed at midnight even when no
    /// snapshot was recorded.
    pub fn day_changed(&mut self, today: chrono::NaiveDate) -> bool {
        std::mem::replace(&mut self.report_day, today) != today
    }

    /// Applies a finished report request, unless a newer one was started since
    /// (e.g. the window changed with `[W]` while the old one was computing): a late
    /// response must not replace the report of the window shown in the title.
    pub fn apply_result(
        &mut self,
        generation: u64,
        result: Result<Box<gitlab_tracker_stats::StatReport>, gitlab_tracker_stats::StatsError>,
    ) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        match result {
            Ok(report) => {
                self.error = None;
                self.report = Some(*report);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Applies a finished cockpit aggregation, unless a newer one was started
    /// since. A failed one keeps the previous figures.
    pub fn apply_cockpit_result(
        &mut self,
        generation: u64,
        result: Result<
            Box<gitlab_tracker_stats::AggregatedStats>,
            gitlab_tracker_stats::StatsError,
        >,
    ) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(stats) => self.cockpit = Some(*stats),
            Err(error) => tracing::warn!(%error, "Failed to aggregate the cockpit stats"),
        }
    }
}

impl Default for StatsViewState {
    fn default() -> Self {
        Self {
            report: None,
            cockpit: None,
            scroll: 0,
            loading: false,
            error: None,
            window: StatsWindow::default(),
            tab: StatsTab::default(),
            sprint_weeks: 2,
            generation: 0,
            report_day: chrono::Local::now().date_naive(),
        }
    }
}

/// Stats overlay tab selector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum StatsTab {
    #[default]
    Overview,
    Flow,
    Quality,
    Forecasts,
    Correlations,
}

impl StatsTab {
    pub fn next(self) -> Self {
        match self {
            Self::Overview => Self::Flow,
            Self::Flow => Self::Quality,
            Self::Quality => Self::Forecasts,
            Self::Forecasts => Self::Correlations,
            Self::Correlations => Self::Overview,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Overview => Self::Correlations,
            Self::Flow => Self::Overview,
            Self::Quality => Self::Flow,
            Self::Forecasts => Self::Quality,
            Self::Correlations => Self::Forecasts,
        }
    }

    pub fn label(self) -> &'static str {
        crate::i18n::tl(match self {
            Self::Overview => "Overview",
            Self::Flow => "Flow",
            Self::Quality => "Quality",
            Self::Forecasts => "Forecasts",
            Self::Correlations => "Correlations",
        })
    }

    pub fn from_digit(c: char) -> Option<Self> {
        match c {
            '1' => Some(Self::Overview),
            '2' => Some(Self::Flow),
            '3' => Some(Self::Quality),
            '4' => Some(Self::Forecasts),
            '5' => Some(Self::Correlations),
            _ => None,
        }
    }
}

/// Time-window selector cycled by `[W]` inside the Stats overlay.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum StatsWindow {
    #[default]
    Last30Days,
    Last90Days,
    Last365Days,
    AllTime,
}

impl StatsWindow {
    /// Cycles to the next window in the sequence.
    pub fn next(self) -> Self {
        match self {
            Self::Last30Days => Self::Last90Days,
            Self::Last90Days => Self::Last365Days,
            Self::Last365Days => Self::AllTime,
            Self::AllTime => Self::Last30Days,
        }
    }

    /// Returns the corresponding [`gitlab_tracker_stats::TimeWindow`] for DB queries.
    pub fn to_query_window(self) -> Option<gitlab_tracker_stats::TimeWindow> {
        match self {
            Self::Last30Days => Some(gitlab_tracker_stats::TimeWindow::LastDays(30)),
            Self::Last90Days => Some(gitlab_tracker_stats::TimeWindow::LastDays(90)),
            Self::Last365Days => Some(gitlab_tracker_stats::TimeWindow::LastDays(365)),
            Self::AllTime => None,
        }
    }

    /// Short label shown in the overlay title bar.
    pub fn label(self) -> &'static str {
        crate::i18n::tl(match self {
            Self::Last30Days => "Last 30 days",
            Self::Last90Days => "Last 90 days",
            Self::Last365Days => "Last 365 days",
            Self::AllTime => "All time",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cockpit_stats_ignore_late_responses_and_survive_errors() {
        use gitlab_tracker_stats::{aggregator::aggregate, QueryFilter, SqliteStatsDb, StatsError};
        let db = SqliteStatsDb::open(":memory:").await.unwrap();
        let stats = || async { Box::new(aggregate(&db, &QueryFilter::default()).await.unwrap()) };

        let mut view = StatsViewState::default();
        let stale = view.next_generation();
        let current = view.next_generation();
        view.apply_cockpit_result(stale, Ok(stats().await));
        assert!(view.cockpit.is_none());
        view.apply_cockpit_result(current, Ok(stats().await));
        assert!(view.cockpit.is_some());
        view.apply_cockpit_result(current, Err(StatsError::InvalidData("boom".into())));
        assert!(view.cockpit.is_some());
    }

    #[test]
    fn day_change_is_reported_once() {
        let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let mut view = StatsViewState {
            report_day: day(9),
            ..StatsViewState::default()
        };
        assert!(!view.day_changed(day(9)));
        assert!(view.day_changed(day(10)));
        assert!(!view.day_changed(day(10)));
    }
}
