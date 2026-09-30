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
}

impl StatsViewState {
    /// Starts a new report request and returns its generation.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Applies a finished report request, unless a newer one was started since
    /// (e.g. the window changed with `[W]` while the old one was computing): a late
    /// response must not replace the report of the window shown in the title.
    pub fn apply_result(
        &mut self,
        generation: u64,
        result: Result<Box<gitlab_tracker_stats::StatReport>, String>,
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
            Err(error) => self.error = Some(error),
        }
    }
}

impl Default for StatsViewState {
    fn default() -> Self {
        Self {
            report: None,
            scroll: 0,
            loading: false,
            error: None,
            window: StatsWindow::default(),
            tab: StatsTab::default(),
            sprint_weeks: 2,
            generation: 0,
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
        match self {
            Self::Overview => "Overview",
            Self::Flow => "Flow",
            Self::Quality => "Quality",
            Self::Forecasts => "Forecasts",
            Self::Correlations => "Correlations",
        }
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
        match self {
            Self::Last30Days => "Last 30 days",
            Self::Last90Days => "Last 90 days",
            Self::Last365Days => "Last 365 days",
            Self::AllTime => "All time",
        }
    }
}
