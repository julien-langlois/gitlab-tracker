use crate::config::AppConfig;
use crate::gitlab::{spawn_mr_fetch, CachePolicy, CachedMrData, CountApiCalls, FetchContext};
use crate::models::{
    AppEvent, GitLabMilestone, GitlabMrState, MergeabilityStatus, MrStatus, SavedMr, TrackedMr,
};
use crate::settings::SettingsEditorState;
use crate::storage::ProjectEntry;
use gitlab_tracker_core::{
    collect_all_columns, collect_all_filters, ColumnDef, DefaultMrEventPolicy, FilterDef,
    LinkedTicket, MrEventPolicy, MrLifecycleEvent, MrSnapshot,
};
use gitlab_tracker_notify as notify;
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Semaphore;

/// Shared handle to the active tracker provider (Redmine, Jira, Trello, …).
///
/// Wrapped in `Arc` so it can be cloned cheaply into spawned async tasks.
/// `None` when no provider is configured or the user skipped the token prompt.
pub type TrackerHandle = Arc<dyn gitlab_tracker_core::TrackerProvider>;

/// Shared handle to a tracker provider that supports status/workflow transitions.
///
/// Kept separate from `TrackerHandle` so read-only providers are not forced to expose
/// mutating operations. `None` when the active provider has no transition capability.
pub type TicketTransitionHandle = Arc<dyn gitlab_tracker_core::TicketTransitionProvider>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    UpdatedAt,
    Id,
    Milestone,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Controls whether keyboard input is routed to the text field or to shortcut bindings.
///
/// - `Normal`: shortcut keys (S, O, P, R, …) are active; the input field is passive.
/// - `Editing`: every printable key feeds the input field; shortcuts are suspended.
///   Enter `/` or `i` to enter Editing mode; press `Esc` to leave it.
/// - `ColumnPicker`: the column visibility popup is open; arrow keys and Space navigate/toggle.
/// - `Settings`: the project settings popup is open; arrow keys navigate, Space toggles booleans.
/// - `FilterPicker`: the filter picker popup is open — arrow keys navigate, Enter confirms,
///   typing feeds the text input for Milestone/Assignee entries.
/// - `LogTime`: the Log Time popup is open — Tab navigates fields, Enter submits.
///   Only reachable when a tracker provider is configured (`app.tracker.is_some()`).
/// - `Help`: the help popup is open — any key closes it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InputMode {
    /// Shortcut keys are active; the input field is passive.
    #[default]
    Normal,
    /// The input field has exclusive focus; shortcuts are suspended.
    Editing,
    /// The column-picker popup is open — arrow keys and Space toggle columns.
    ColumnPicker,
    /// The project settings popup is open — arrow keys navigate, Space toggles booleans.
    Settings,
    /// The filter picker popup is open — arrow keys navigate, Enter confirms.
    /// Typing feeds the text input for Milestone / Assignee entries.
    FilterPicker,
    /// The Log Time popup is open — Tab cycles fields, Enter submits.
    /// Only reachable when `app.tracker.is_some()`.
    LogTime,
    /// The help popup is open — lists all registered shortcuts by section.
    /// Any key press closes it and returns to Normal mode.
    Help,
    /// The Stats fullscreen overlay is open — displays MR analytics and correlations.
    /// Only compiled and reachable when the `stats` feature is enabled.
    /// [Esc] or [G] closes it and returns to Normal mode.
    #[cfg(feature = "stats")]
    Stats,
}

/// Which field is focused inside the Log Time popup.
///
/// Cycling order: Duration → Activity → Comment → (submit on Enter).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LogTimeField {
    #[default]
    Duration,
    Activity,
    Comment,
}

/// State held by the Log Time popup while it is open.
///
/// Reset every time the popup is opened so the user starts with a clean form.
#[derive(Debug, Clone, Default)]
pub struct LogTimeForm {
    /// Raw text typed by the user in the Duration field.
    pub duration_input: String,
    /// Index of the currently highlighted activity in the selector list.
    pub selected_activity_idx: usize,
    /// Raw text typed by the user in the Comment field.
    pub comment_input: String,
    /// Which field currently has focus inside the popup.
    pub focused_field: LogTimeField,
    /// Inline validation / submission error shown beneath the Duration field.
    /// `None` when no error is present.
    pub error: Option<String>,
    /// Whether a submission is in flight (disables the Submit button).
    pub submitting: bool,
}

/// Represents the currently focused pane in the TUI layout.
///
/// Adding a new pane only requires adding a variant here and handling it
/// in the relevant input/render logic — no structural change needed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ActivePane {
    /// The main MR list table (left pane).
    #[default]
    Dashboard,
    /// The MR detail side viewer — upper-right pane.
    Inspector,
    /// The tracker ticket pane — lower-right pane.
    /// Only reachable when a tracker provider is configured and a ticket is linked.
    Tracker,
}

impl ActivePane {
    /// Cycles to the next pane.
    /// When a tracker ticket is available the cycle is: Dashboard → Inspector → Tracker → Dashboard.
    /// Otherwise: Dashboard ↔ Inspector.
    pub fn next(self, has_tracker_ticket: bool) -> Self {
        match self {
            ActivePane::Dashboard => ActivePane::Inspector,
            ActivePane::Inspector => {
                if has_tracker_ticket {
                    ActivePane::Tracker
                } else {
                    ActivePane::Dashboard
                }
            }
            ActivePane::Tracker => ActivePane::Dashboard,
        }
    }
}

/// Controls which view is rendered inside the Inspector side panel.
///
/// Cycled with [P] — rotates between MrInfo and Pipelines only.
/// The TimeLog has moved to the dedicated Tracker pane.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InspectorView {
    /// Default: MR metadata, description, labels.
    #[default]
    MrInfo,
    /// Pipeline list for the selected MR.
    Pipelines,
}

impl InspectorView {
    /// Cycles between MrInfo and Pipelines.
    pub fn next(self) -> Self {
        match self {
            InspectorView::MrInfo => InspectorView::Pipelines,
            InspectorView::Pipelines => InspectorView::MrInfo,
        }
    }
}

/// Controls which view is rendered inside the Tracker pane (lower-right).
///
/// Cycled with [P] when the Tracker pane is focused.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TrackerView {
    /// Ticket details: type, priority, status, version, progress, time tracking.
    #[default]
    TicketInfo,
    /// Time entries logged on the linked ticket.
    TimeLog,
}

impl TrackerView {
    /// Cycles between TicketInfo and TimeLog.
    pub fn next(self) -> Self {
        match self {
            TrackerView::TicketInfo => TrackerView::TimeLog,
            TrackerView::TimeLog => TrackerView::TicketInfo,
        }
    }
}

/// Active filter state: which `FilterDef` is selected and the optional query string.
///
/// Replaces the old `FilterMode` enum — the predicate now lives inside `FilterDef::apply`
/// collected via `inventory`. The orchestrator only stores the index + query here.
#[derive(Debug, Clone, Default)]
pub struct ActiveFilter {
    /// Index into `App::filter_defs` of the currently active filter.
    /// Index 0 is always the "All" filter (priority 0, registered in `filters_core.rs`).
    pub index: usize,
    /// Free-text query for parametric filters (Milestone, Assignee).
    /// Empty string for non-parametric filters.
    pub query: String,
}

impl ActiveFilter {
    /// Returns the display label shown in the table header for the active filter.
    pub fn label(&self, filter_defs: &[&'static FilterDef]) -> String {
        let Some(def) = filter_defs.get(self.index) else {
            return "All".to_string();
        };
        if def.needs_text_input && !self.query.is_empty() {
            format!("{} {}", def.active_label, self.query)
        } else {
            def.active_label.to_string()
        }
    }
}

/// State held by the filter picker popup while it is open.
#[derive(Debug, Clone, Default)]
pub struct FilterPickerState {
    /// Index of the currently highlighted row (0-based).
    pub cursor: usize,
    /// Free-text input used for parametric filters (Milestone, Assignee, …).
    pub input: String,
}

/// UI state for the Stats fullscreen overlay (only compiled with the `stats` feature).
///
/// Decoupled from `StatReport` so the overlay can render a "Loading…" state
/// while the async aggregation is in flight, and an "Insufficient data" state
/// when fewer than 3 snapshots are available.
#[cfg(feature = "stats")]
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
    /// The time window currently selected by the user (cycles with [W]).
    pub window: StatsWindow,
    /// The currently selected stats dashboard tab.
    pub tab: StatsTab,
    /// Sprint duration in weeks for throughput forecasts — read from
    /// `stats_sprint_weeks` in `projects.toml`, defaults to 2.
    pub sprint_weeks: u32,
}

#[cfg(feature = "stats")]
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
        }
    }
}

/// Stats overlay tab selector.
#[cfg(feature = "stats")]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum StatsTab {
    #[default]
    Overview,
    Flow,
    Quality,
    Forecasts,
    Correlations,
}

#[cfg(feature = "stats")]
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

/// Time-window selector cycled by [W] inside the Stats overlay.
#[cfg(feature = "stats")]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum StatsWindow {
    #[default]
    Last30Days,
    Last90Days,
    Last365Days,
    AllTime,
}

#[cfg(feature = "stats")]
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

pub struct App {
    pub mrs: Vec<TrackedMr>,
    pub branches: Vec<String>,
    pub input: String,
    /// Whether the input field has exclusive keyboard focus.
    /// In `Editing` mode all printable keys feed the field; shortcuts are suspended.
    pub input_mode: InputMode,
    pub token: String,
    pub project_id: String,
    pub base_url: String,
    /// Optional human-readable alias for this project, as set in `projects.toml` (`name` field).
    /// Displayed in the table title alongside the URL when present.
    pub project_name: Option<String>,
    pub time_left: u64,
    pub refresh_interval_secs: u64,
    pub table_state: TableState,
    pub config: AppConfig,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    /// Which pane currently holds focus (drives keyboard & scroll routing).
    pub active_pane: ActivePane,
    /// Which view is rendered inside the Inspector panel ([P] toggles).
    pub inspector_view: InspectorView,
    /// Vertical scroll offset for the Inspector pane (in lines).
    pub inspector_scroll: u16,
    /// Total number of lines in the currently rendered Inspector content.
    /// Updated at each render frame — used to clamp scroll and avoid blank space.
    pub inspector_content_lines: u16,
    /// Height (in rows) of the Inspector pane area, updated at each render frame.
    pub inspector_pane_height: u16,
    /// Which view is rendered inside the Tracker pane ([P] toggles when focused).
    pub tracker_view: TrackerView,
    /// Vertical scroll offset for the Tracker pane (in lines).
    pub tracker_scroll: u16,
    /// Total number of lines in the currently rendered Tracker pane content.
    pub tracker_content_lines: u16,
    /// Height (in rows) of the Tracker pane area, updated at each render frame.
    pub tracker_pane_height: u16,
    /// Index of the currently highlighted row in the column-picker popup (0-based).
    pub column_picker_cursor: usize,
    /// Draft state for the project settings popup.
    pub settings_editor: SettingsEditorState,
    /// The active project entry as loaded from `projects.toml`.
    /// Kept as a TOML-backed settings source for plugin-provided settings.
    pub project_settings: ProjectEntry,
    /// Countdown (in ticks ~= seconds) during which recently-updated rows stay highlighted.
    /// Reset to `RECENT_UPDATE_FADE_TICKS` each time a MR update is detected.
    /// Decremented on every Tick; rows are highlighted while this is > 0.
    pub update_highlight_ticks: u64,
    /// List of active/upcoming milestones fetched from GitLab on startup.
    /// Used to power the milestone autocomplete in the input field.
    pub milestones: Vec<GitLabMilestone>,
    /// When the user types `@` followed by text in Editing mode, this holds the
    /// filtered list of milestone titles matching the current query.
    /// Empty when autocomplete is not active.
    pub milestone_suggestions: Vec<String>,
    /// Index of the currently highlighted suggestion in the autocomplete popup.
    pub milestone_suggestion_cursor: usize,
    /// Active filter applied to the MR table — selected via the [F] picker popup.
    pub active_filter: ActiveFilter,
    /// State of the filter picker popup (cursor position + text input).
    /// Reset each time the popup is opened.
    pub filter_picker: FilterPickerState,
    /// All registered filter definitions, collected at startup via `inventory`.
    /// Sorted by priority — index 0 is always "All".
    pub filter_defs: Vec<&'static FilterDef>,
    /// All registered column definitions, collected at startup via `inventory`.
    /// Sorted by priority — used by the column picker popup and `VisibleColumns`.
    pub column_defs: Vec<&'static ColumnDef>,
    /// Ids of MRs whose fetch from the initial startup load is still in flight.
    /// Change notifications (updated_at, mergeability, milestone) are suppressed
    /// until this is empty, preventing spurious toasts on first launch.
    ///
    /// Tracked by id (not a counter) so a failed fetch, an MR removed while its
    /// fetch was in flight, or an untracked fetch (MrAdded, milestone) can never
    /// leave the spinner stuck or release the fence early — see `complete_fetch`.
    pub pending_initial_fetches: HashSet<String>,
    /// Ids of MRs whose fetch from the current refresh cycle is still in flight.
    /// Drives the loading spinner in the status bar.
    pub pending_refresh_fetches: HashSet<String>,
    /// Estimated GitLab API HTTP calls for the *next* refresh cycle (GitLab only).
    /// Computed just before fetches are spawned so it reflects the real cache state.
    pub estimated_gitlab_calls: usize,
    /// Estimated tracker API calls for the *next* refresh cycle (Redmine, Jira, …).
    /// Kept separate from GitLab calls so the UI can display them independently.
    /// `0` when no tracker provider is configured.
    pub estimated_tracker_calls: usize,
    /// GitLab call estimate computed once from the saved state on the very first load.
    /// Shown in parentheses in the table title to give cold-start context.
    pub startup_gitlab_estimate: Option<usize>,
    /// Tracker call estimate computed once from the saved state on the very first load.
    pub startup_tracker_estimate: Option<usize>,
    /// Active tracker provider (Redmine, Jira, Trello, …), shared across async tasks via Arc.
    /// `None` when no provider is configured or the user skipped the token prompt.
    pub tracker: Option<TrackerHandle>,
    /// Optional status transition capability exposed by the active tracker provider.
    ///
    /// Stored separately from `tracker` to keep read-only providers compatible with the
    /// base `TrackerProvider` contract while allowing workflow automation when available.
    pub ticket_transitioner: Option<TicketTransitionHandle>,
    /// Colour maps for tracker badge labels (type and priority).
    /// Populated from the active tracker's config at startup and forwarded to the Tracker pane renderer.
    /// Defaults to empty maps (dark_gray / white fallback) when no provider is configured.
    pub tracker_colors: crate::ui::tracker::TrackerLabelColors,
    /// Activity categories fetched from the tracker at startup.
    /// Populated by `AppEvent::ActivitiesLoaded` and used to fill the Log Time popup.
    pub activities: Vec<gitlab_tracker_core::Activity>,
    /// Time entries per tracker ticket id, fetched on demand while the TimeLog view is
    /// shown (see `ensure_time_entries`). `None` = request in flight. Cleared at each
    /// refresh cycle so entries logged elsewhere show up within one cycle.
    pub time_entries: HashMap<String, Option<Vec<gitlab_tracker_core::TimeEntry>>>,
    /// State of the Log Time popup form. Reset each time the popup is opened.
    pub log_time_form: LogTimeForm,
    /// When `true`, the user has pressed Esc once and is being asked to confirm quitting.
    /// A second Esc (or `y`) confirms; any other key cancels.
    pub quit_confirm: bool,
    /// Monotonically incrementing counter bumped on every render frame (~20 fps).
    /// Used to animate the spinner independently of the 1-second tick timer.
    pub spinner_frame: usize,
    /// Set by `MrLoaded`; consumed once per event drain by [`App::flush_pending_sort`].
    pub needs_sort: bool,
    /// Per-render snapshot of `visible_mrs()` indices — see [`App::begin_render_cache`].
    visible_cache: Option<Vec<usize>>,
    /// Shortcut blocks collected at startup via `inventory` from every linked crate.
    ///
    /// Populated once by `gitlab_tracker_core::collect_all_blocks()` — no explicit
    /// provider registration needed in `main.rs`. The help popup iterates this list
    /// in collection order (link order: Core first, optional plugins after).
    pub shortcut_providers: Vec<gitlab_tracker_core::ShortcutBlock>,
    /// Policy that governs reactions to MR lifecycle events (refetch, remove, notify, persist).
    ///
    /// Injected at construction time so tests and future callers can swap in a
    /// custom policy without touching `apply_event`. Defaults to [`DefaultMrEventPolicy`].
    pub event_policy: Arc<dyn MrEventPolicy>,

    /// UI state for the Stats fullscreen overlay.
    ///
    /// Holds the last computed report and the scroll offset.
    /// `None` until the user opens the Stats view for the first time.
    #[cfg(feature = "stats")]
    pub stats_view: StatsViewState,

    /// Handle to the SQLite stats database, injected by `main.rs` after `SqliteStatsDb::open`.
    ///
    /// `None` when the `stats` feature is disabled or the DB failed to open at startup.
    /// All recording calls are guarded by `#[cfg(feature = "stats")]` — no runtime
    /// overhead when the feature is not compiled in.
    #[cfg(feature = "stats")]
    pub stats_db: Option<Arc<gitlab_tracker_stats::SqliteStatsDb>>,

    /// Tracks the calendar date of the last `on_refresh` snapshot per MR id.
    ///
    /// Prevents writing more than one `OnRefresh` snapshot per MR per calendar day,
    /// matching the DB `UNIQUE(mr_id, project_id, DATE(recorded_at), trigger)` constraint
    /// without a round-trip query on every `Tick`.
    #[cfg(feature = "stats")]
    pub stats_last_refresh_date: std::collections::HashMap<String, String>,
    /// Set when a snapshot was recorded; the report is recomputed at most once
    /// per `Tick` instead of once per recorded snapshot.
    #[cfg(feature = "stats")]
    pub stats_report_dirty: bool,
    /// Active colour palette — resolved once at startup from the terminal background
    /// colour (OSC 11 via `terminal-colorsaurus`). Falls back to the dark palette
    /// when the terminal does not respond. Forwarded to renderers that need it.
    pub theme: crate::ui::theme::Palette,

    /// When `true`, the discovery poller is active: at each refresh cycle the app
    /// queries `GET /projects/:id/merge_requests?state=all` and automatically
    /// adds any MR not yet in the tracking list.
    ///
    /// Set from `discover_new_mrs` in `[project.stats]` of `projects.toml`.
    /// Defaults to `false` — opt-in only.
    pub discovery_enabled: bool,
    /// Discovery anchor (RFC 3339), passed as `created_after` to the GitLab API.
    /// Set to "now" the first time the poller runs, so MRs created before the tool
    /// was started are never auto-added; then advanced after each complete poll
    /// (see `gitlab::next_discovery_anchor`) so polls don't re-paginate every MR
    /// since the first launch. `None` until the first poll; persisted in the state
    /// file (the field name is kept for state-file compatibility).
    pub discovery_started_at: Option<String>,
    /// MR IIDs manually removed from the dashboard during the current session.
    /// Discovery must ignore these ids so auto-polling does not immediately
    /// re-add items the user explicitly cleaned from the dashboard.
    pub dismissed_mr_ids: HashSet<String>,
}

/// Duration (in seconds) of the green highlight fade after a MR is updated.
pub const RECENT_UPDATE_FADE_TICKS: u64 = 10;

pub struct AppInit {
    pub token: String,
    pub project_id: String,
    pub base_url: String,
    pub project_name: Option<String>,
    pub refresh_interval_secs: u64,
    pub config: AppConfig,
    pub theme: crate::ui::theme::Palette,
    pub project_settings: ProjectEntry,
}

impl App {
    pub fn new(init: AppInit) -> Self {
        let AppInit {
            token,
            project_id,
            base_url,
            project_name,
            refresh_interval_secs,
            mut config,
            theme,
            project_settings,
        } = init;
        let mut table_state = TableState::default();
        table_state.select(None);

        // Collect columns first so we can seed `visible_columns` defaults before
        // moving `config` into the struct — avoids a borrow-after-move.
        let column_defs = collect_all_columns();
        config.visible_columns.apply_defaults(&column_defs);
        let settings_editor = SettingsEditorState::from_project_entry(&project_settings);

        Self {
            mrs: Vec::new(),
            branches: Vec::new(),
            input: String::new(),
            input_mode: InputMode::default(),
            token,
            project_id,
            base_url,
            project_name,
            refresh_interval_secs,
            time_left: refresh_interval_secs,
            table_state,
            config,
            sort_column: SortColumn::UpdatedAt,
            sort_order: SortOrder::Descending,
            active_pane: ActivePane::default(),
            inspector_view: InspectorView::default(),
            inspector_scroll: 0,
            inspector_content_lines: 0,
            inspector_pane_height: 0,
            tracker_view: TrackerView::default(),
            tracker_scroll: 0,
            tracker_content_lines: 0,
            tracker_pane_height: 0,
            column_picker_cursor: 0,
            settings_editor,
            project_settings,
            update_highlight_ticks: 0,
            milestones: Vec::new(),
            milestone_suggestions: Vec::new(),
            milestone_suggestion_cursor: 0,
            active_filter: ActiveFilter::default(),
            filter_picker: FilterPickerState::default(),
            filter_defs: collect_all_filters(),
            column_defs,
            // Initialised to 0 — main.rs sets this to the number of MRs loaded from state
            // before the first fetch cycle begins, then decrements it on each MrLoaded event.
            pending_initial_fetches: HashSet::new(),
            pending_refresh_fetches: HashSet::new(),
            estimated_gitlab_calls: 0,
            estimated_tracker_calls: 0,
            startup_gitlab_estimate: None,
            startup_tracker_estimate: None,
            // Initialised to None — main.rs injects the provider after keyring lookup.
            tracker: None,
            ticket_transitioner: None,
            tracker_colors: crate::ui::tracker::TrackerLabelColors::default(),
            activities: Vec::new(),
            time_entries: HashMap::new(),
            log_time_form: LogTimeForm::default(),
            quit_confirm: false,
            theme,
            spinner_frame: 0,
            needs_sort: false,
            visible_cache: None,
            // Populated at startup by main.rs — at least CoreShortcutProvider is always pushed.
            shortcut_providers: Vec::new(),
            event_policy: Arc::new(DefaultMrEventPolicy),
            #[cfg(feature = "stats")]
            stats_view: StatsViewState::default(),
            #[cfg(feature = "stats")]
            stats_db: None,
            #[cfg(feature = "stats")]
            stats_last_refresh_date: std::collections::HashMap::new(),
            #[cfg(feature = "stats")]
            stats_report_dirty: false,
            // Disabled by default — opt-in via `discover_new_mrs = true` in projects.toml.
            discovery_enabled: false,
            discovery_started_at: None,
            dismissed_mr_ids: HashSet::new(),
        }
    }

    /// Toggles the flagged state of the currently selected MR.
    ///
    /// Returns the MR id if a MR was toggled, `None` if no MR is selected.
    pub fn toggle_flag_selected(&mut self) -> Option<String> {
        let selected = self.table_state.selected()?;
        // When a filter is active the visible index differs from `self.mrs` index.
        let mr = self.visible_mrs_mut().nth(selected)?;
        mr.flagged = !mr.flagged;
        Some(mr.id.clone())
    }

    /// Opens the filter picker popup, pre-selecting the currently active filter row.
    ///
    /// The cursor is the position of the active filter **within the visible list**
    /// (not the full `filter_defs` index), so the highlighted row matches what the
    /// user sees on screen.
    pub fn open_filter_picker(&mut self) {
        // Find the visible-list position that corresponds to the active full-list index.
        let visible = self.visible_filter_defs();
        let cursor = visible
            .iter()
            .position(|(full_idx, _)| *full_idx == self.active_filter.index)
            .unwrap_or(0);
        self.filter_picker = FilterPickerState {
            cursor,
            input: self.active_filter.query.clone(),
        };
        self.input_mode = InputMode::FilterPicker;
    }

    /// Applies the filter picker selection and closes the popup.
    ///
    /// The picker cursor is a position in the **visible** filter list.
    /// We resolve it back to the full `filter_defs` index before storing it in
    /// `active_filter`, so `apply_filter` always finds the correct predicate.
    pub fn apply_filter_picker(&mut self) {
        let input = self.filter_picker.input.trim().to_string();
        let cursor = self.filter_picker.cursor;
        let visible = self.visible_filter_defs();

        // Resolve cursor → full-list index.  Fall back to 0 ("All") when out of range.
        let (final_idx, final_query) = if let Some((full_idx, def)) = visible.get(cursor) {
            if def.needs_text_input && input.is_empty() {
                (0, String::new())
            } else {
                (*full_idx, input)
            }
        } else {
            (0, String::new())
        };

        self.active_filter = ActiveFilter {
            index: final_idx,
            query: final_query,
        };

        self.input_mode = InputMode::Normal;
        // Reset selection so we never point past the end of the filtered list.
        if self.visible_mrs().next().is_some() {
            self.table_state.select(Some(0));
        } else {
            self.table_state.select(None);
        }
        self.reset_inspector_scroll();
    }

    /// Returns an iterator over the MRs that pass the current filter,
    /// ordered by fuzzy relevance when a search query is active in the input field.
    ///
    /// When `app.input` is non-empty (and not a `@milestone` autocomplete), each MR is
    /// scored against the query using [`crate::utils::fuzzy_score`] on its title, author,
    /// assignee and ID. MRs with no match are excluded; the rest are yielded in
    /// descending score order (most relevant first).
    ///
    /// When the input is empty the original insertion order is preserved so normal
    /// sort/filter behaviour is unaffected.
    pub fn visible_mrs(&self) -> impl Iterator<Item = &TrackedMr> {
        let computed;
        let indices: &[usize] = match &self.visible_cache {
            Some(cached) => cached,
            None => {
                computed = self.compute_visible_indices();
                &computed
            }
        };
        indices
            .iter()
            .map(|&i| &self.mrs[i])
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Freezes the visible list for the duration of one render: `render_ui` calls
    /// `visible_mrs()` from ~6 widgets, and each call would otherwise re-run the
    /// filters, difficulty scoring and fuzzy ranking. Rendering never mutates the
    /// MR list, input or filters, so the snapshot cannot go stale within a frame.
    pub fn begin_render_cache(&mut self) {
        self.visible_cache = Some(self.compute_visible_indices());
    }

    /// Drops the per-render snapshot; outside rendering `visible_mrs()` is live.
    pub fn end_render_cache(&mut self) {
        self.visible_cache = None;
    }

    /// Indices into `self.mrs` of the MRs that pass the active filter, fuzzy-ranked
    /// when a search query is active (see [`App::visible_mrs`]).
    fn compute_visible_indices(&self) -> Vec<usize> {
        use crate::utils::fuzzy_score;

        // Only apply fuzzy ranking when the user has typed a plain search query
        // (not a `@milestone` autocomplete prefix).
        let query = if !self.input.is_empty() && !self.input.starts_with('@') {
            Some(self.input.clone())
        } else {
            None
        };

        // Collect filtered MRs with their fuzzy score so we can sort them.
        // When no query is active we skip scoring entirely for efficiency.
        let mut scored: Vec<(f64, usize)> = self
            .mrs
            .iter()
            .enumerate()
            .filter(|(_, mr)| self.filter_passes(mr))
            .filter_map(|(i, mr)| {
                if let Some(ref q) = query {
                    // Score against the most user-visible fields — highest wins.
                    let best = [
                        fuzzy_score(q, &mr.title),
                        fuzzy_score(q, &mr.author),
                        fuzzy_score(q, &mr.assignee),
                        fuzzy_score(q, &mr.id),
                    ]
                    .into_iter()
                    .flatten()
                    .fold(f64::NEG_INFINITY, f64::max);

                    if best == f64::NEG_INFINITY {
                        None // No match on any field → exclude.
                    } else {
                        Some((best, i))
                    }
                } else {
                    // No query — include all, preserve order (score unused).
                    Some((0.0, i))
                }
            })
            .collect();

        // Sort by descending relevance only when a query is active.
        if query.is_some() {
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        }

        scored.into_iter().map(|(_, i)| i).collect()
    }

    /// Returns a mutable iterator over the MRs that pass the current filter.
    fn visible_mrs_mut(&mut self) -> impl Iterator<Item = &mut TrackedMr> {
        let filter_defs = self.filter_defs.clone();
        let active = self.active_filter.clone();
        let complexity_profile = self.config.complexity_profile.clone();
        let gitlab_username = self.config.gitlab_username.clone();
        self.mrs.iter_mut().filter(move |mr| {
            Self::apply_filter(
                &filter_defs,
                &active,
                mr,
                &complexity_profile,
                &gitlab_username,
            )
        })
    }

    /// Returns `true` when `mr` passes the currently active filter.
    fn filter_passes(&self, mr: &TrackedMr) -> bool {
        Self::apply_filter(
            &self.filter_defs,
            &self.active_filter,
            mr,
            &self.config.complexity_profile,
            &self.config.gitlab_username,
        )
    }

    /// Returns the subset of registered filter definitions visible in the picker,
    /// as `(original_index, def)` pairs where `original_index` is the position in
    /// `self.filter_defs` (the full list).
    ///
    /// The cursor in [`FilterPickerState`] and [`ActiveFilter::index`] always refer
    /// to indices in the **full** `filter_defs` list — never in the visible subset —
    /// so that `apply_filter` always resolves the correct predicate regardless of
    /// which filters are hidden.
    ///
    /// Filters that require a configured `gitlab_username` ("Assigned to me",
    /// "Reviewer: me") are excluded when that value is absent in `projects.toml`.
    pub fn visible_filter_defs(&self) -> Vec<(usize, &'static FilterDef)> {
        let has_username = self.config.gitlab_username.is_some();
        self.filter_defs
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, def)| {
                if matches!(def.id, "assigned_to_me" | "reviewer_me") {
                    has_username
                } else {
                    true
                }
            })
            .collect()
    }

    /// Pure predicate — does not borrow `self`, usable inside `iter_mut` closures.
    ///
    /// Builds a [`MrSnapshot`] from the tracked MR and delegates to the registered
    /// `FilterDef::apply` function — no match arm needed when a new filter is added.
    ///
    /// `complexity_profile` is used to compute the diff-difficulty score for the
    /// effort filters. `gitlab_username` is forwarded so "me" filters can match.
    fn apply_filter(
        filter_defs: &[&'static FilterDef],
        active: &ActiveFilter,
        mr: &TrackedMr,
        complexity_profile: &crate::models::DifficultyProfile,
        gitlab_username: &Option<String>,
    ) -> bool {
        let Some(def) = filter_defs.get(active.index) else {
            return true; // Unknown index → show all.
        };
        // Pre-compute the difficulty score using the project's complexity profile.
        // The filter predicates (effort_easy / effort_complex) only need relative
        // bands (< 0.33 / >= 0.66) — the profile is passed in from the caller.
        let diff_difficulty = mr
            .diff_stats
            .as_ref()
            .map(|s| s.difficulty(complexity_profile));
        let snapshot = MrSnapshot {
            flagged: mr.flagged,
            state: match &mr.state {
                crate::models::GitlabMrState::Opened => "opened",
                crate::models::GitlabMrState::Merged => "merged",
                crate::models::GitlabMrState::Closed => "closed",
            },
            mergeability: match &mr.mergeability {
                crate::models::MergeabilityStatus::Mergeable => "Mergeable",
                crate::models::MergeabilityStatus::Conflict => "Conflict",
                crate::models::MergeabilityStatus::NeedsRebase => "NeedsRebase",
                crate::models::MergeabilityStatus::NotApproved => "NotApproved",
                crate::models::MergeabilityStatus::RequestedChanges => "RequestedChanges",
                crate::models::MergeabilityStatus::Draft => "Draft",
                crate::models::MergeabilityStatus::DiscussionsNotResolved => {
                    "DiscussionsNotResolved"
                }
                crate::models::MergeabilityStatus::CiMustPass => "CiMustPass",
                crate::models::MergeabilityStatus::CiStillRunning => "CiStillRunning",
                crate::models::MergeabilityStatus::NotOpen => "NotOpen",
                crate::models::MergeabilityStatus::Retrying => "Retrying",
                crate::models::MergeabilityStatus::SyncFailed => "SyncFailed",
                crate::models::MergeabilityStatus::Unknown => "Unknown",
            },
            user_notes_count: mr.user_notes_count,
            milestone: &mr.milestone,
            assignee: &mr.assignee,
            linked_ticket: mr.linked_ticket.as_ref(),
            pipeline_status: mr.pipelines.first().map(|p| match &p.status {
                crate::models::PipelineState::Failed => "Failed",
                crate::models::PipelineState::Success => "Success",
                crate::models::PipelineState::Running => "Running",
                crate::models::PipelineState::Pending => "Pending",
                crate::models::PipelineState::Canceled => "Canceled",
                crate::models::PipelineState::Skipped => "Skipped",
                crate::models::PipelineState::Created => "Created",
                crate::models::PipelineState::Unknown => "Unknown",
            }),
            reviewers: &mr.reviewers,
            gitlab_username: gitlab_username.as_deref(),
            diff_difficulty,
            target_branch: &mr.target_branch,
        };
        (def.apply)(snapshot, &active.query)
    }

    /// Updates `milestone_suggestions` based on the current input query after `@`.
    ///
    /// Call this whenever the input changes in Editing mode. If the input does not
    /// contain `@`, suggestions are cleared. The query is case-insensitive.
    pub fn update_milestone_suggestions(&mut self) {
        if let Some(query) = self.input.strip_prefix('@') {
            let query_lower = query.to_lowercase();
            self.milestone_suggestions = self
                .milestones
                .iter()
                .map(|m| m.title.clone())
                .filter(|title| title.to_lowercase().contains(&query_lower))
                .collect();
            // Reset cursor to avoid out-of-bounds after list changes.
            self.milestone_suggestion_cursor = 0;
        } else {
            self.milestone_suggestions.clear();
            self.milestone_suggestion_cursor = 0;
        }
    }

    /// Moves the autocomplete cursor down (wraps around).
    pub fn milestone_suggestion_next(&mut self) {
        if !self.milestone_suggestions.is_empty() {
            self.milestone_suggestion_cursor =
                (self.milestone_suggestion_cursor + 1) % self.milestone_suggestions.len();
        }
    }

    /// Moves the autocomplete cursor up (wraps around).
    pub fn milestone_suggestion_prev(&mut self) {
        if !self.milestone_suggestions.is_empty() {
            let len = self.milestone_suggestions.len();
            self.milestone_suggestion_cursor = (self.milestone_suggestion_cursor + len - 1) % len;
        }
    }

    /// Confirms the currently highlighted suggestion, replacing the `@query` in the input.
    ///
    /// Returns the selected milestone title so the caller can trigger the bulk-add fetch.
    pub fn confirm_milestone_suggestion(&mut self) -> Option<String> {
        let selected = self
            .milestone_suggestions
            .get(self.milestone_suggestion_cursor)
            .cloned()?;
        // Replace the `@...` prefix with the confirmed milestone title (prefixed with `@`).
        self.input = format!("@{}", selected);
        self.milestone_suggestions.clear();
        Some(selected)
    }

    /// Scrolls the Inspector pane down by the given number of lines.
    ///
    /// Clamps the scroll so the user cannot scroll past the last line of content,
    /// preventing blank space from appearing at the bottom of the Inspector pane.
    pub fn inspector_scroll_down(&mut self, amount: u16) {
        let max_scroll = self
            .inspector_content_lines
            .saturating_sub(self.inspector_pane_height);
        self.inspector_scroll = self.inspector_scroll.saturating_add(amount).min(max_scroll);
    }

    /// Scrolls the Inspector pane up by the given number of lines.
    pub fn inspector_scroll_up(&mut self, amount: u16) {
        self.inspector_scroll = self.inspector_scroll.saturating_sub(amount);
    }

    /// Resets the Inspector scroll to the top (e.g. when selecting a new MR).
    pub fn reset_inspector_scroll(&mut self) {
        self.inspector_scroll = 0;
    }

    pub fn tracker_scroll_down(&mut self, amount: u16) {
        let max_scroll = self
            .tracker_content_lines
            .saturating_sub(self.tracker_pane_height);
        self.tracker_scroll = self.tracker_scroll.saturating_add(amount).min(max_scroll);
    }

    pub fn tracker_scroll_up(&mut self, amount: u16) {
        self.tracker_scroll = self.tracker_scroll.saturating_sub(amount);
    }

    pub fn reset_tracker_scroll(&mut self) {
        self.tracker_scroll = 0;
    }

    /// Returns true when the selected MR has a linked tracker ticket.
    pub fn has_tracker_ticket(&self) -> bool {
        self.table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
            .and_then(|mr| mr.linked_ticket.as_ref())
            .is_some()
    }

    /// Fetches the time entries of the selected MR's ticket when the TimeLog view is
    /// shown and they are neither cached nor in flight. Called once per main-loop
    /// iteration: navigation (keys, mouse, filters) never spawns a request itself,
    /// and a ticket is fetched at most once per refresh cycle.
    pub fn ensure_time_entries(&mut self, tx: &UnboundedSender<AppEvent>) {
        if self.tracker_view != TrackerView::TimeLog {
            return;
        }
        let Some(provider) = self.tracker.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(ticket_id) = self
            .table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
            .and_then(|mr| mr.linked_ticket.as_ref())
            .map(|t| t.id.clone())
        else {
            return;
        };
        if self.time_entries.contains_key(&ticket_id) {
            return;
        }
        self.time_entries.insert(ticket_id.clone(), None);
        let tx = tx.clone();
        tokio::spawn(async move {
            let entries = provider.fetch_time_entries(&ticket_id).await;
            let _ = tx.send(AppEvent::TimeEntriesLoaded { ticket_id, entries });
        });
    }

    /// Keeps the selection inside the visible list after it shrinks.
    fn clamp_selection(&mut self) {
        let count = self.visible_mrs().count();
        let selected = match self.table_state.selected() {
            _ if count == 0 => None,
            Some(i) => Some(i.min(count - 1)),
            None => None,
        };
        self.table_state.select(selected);
    }

    pub fn next_row(&mut self) {
        let count = self.visible_mrs().count();
        if count == 0 {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= count - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        // Reset inspector scroll when the selected MR changes.
        self.reset_inspector_scroll();
    }

    pub fn prev_row(&mut self) {
        let count = self.visible_mrs().count();
        if count == 0 {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        // Reset inspector scroll when the selected MR changes.
        self.reset_inspector_scroll();
    }

    pub fn cycle_sort_column(&mut self) {
        self.sort_column = match self.sort_column {
            SortColumn::UpdatedAt => SortColumn::Id,
            SortColumn::Id => SortColumn::Milestone,
            SortColumn::Milestone => SortColumn::Title,
            SortColumn::Title => SortColumn::UpdatedAt,
        };
        // Reset to a sensible default order when switching columns.
        self.sort_order = match self.sort_column {
            SortColumn::UpdatedAt => SortOrder::Descending,
            _ => SortOrder::Ascending,
        };
        self.sort_mrs();
    }

    pub fn toggle_sort_order(&mut self) {
        self.sort_order = match self.sort_order {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        };
        self.sort_mrs();
    }

    /// Marks the fetch of MR `id` as finished (loaded or failed).
    pub fn complete_fetch(&mut self, id: &str) -> FetchCompletion {
        let notify_allowed = self.pending_initial_fetches.is_empty();
        let initial_sync_just_completed =
            self.pending_initial_fetches.remove(id) && self.pending_initial_fetches.is_empty();
        FetchCompletion {
            notify_allowed,
            initial_sync_just_completed,
            from_refresh_cycle: self.pending_refresh_fetches.remove(id),
        }
    }

    /// Starts an MR discovery poll (auto-refresh cycle and manual `[R]`).
    ///
    /// On the very first poll the anchor is set to "now", so MRs that existed
    /// before the tool was started are never auto-added. Tracked and dismissed
    /// ids are passed as known so they are never re-added.
    pub fn spawn_discovery(&mut self, ctx: FetchContext, tx: &UnboundedSender<AppEvent>) {
        let anchor = self
            .discovery_started_at
            .get_or_insert_with(|| chrono::Utc::now().to_rfc3339())
            .clone();
        let known_ids = self
            .mrs
            .iter()
            .map(|m| m.id.clone())
            .chain(self.dismissed_mr_ids.iter().cloned())
            .collect();
        crate::gitlab::spawn_mrs_discovery(ctx, known_ids, anchor, tx.clone());
    }

    /// Applies a sort requested by `MrLoaded` during the last event drain.
    pub fn flush_pending_sort(&mut self) {
        if std::mem::take(&mut self.needs_sort) {
            self.sort_mrs();
        }
    }

    pub fn sort_mrs(&mut self) {
        let order = self.sort_order;
        let col = self.sort_column;

        // Preserve the currently selected MR id so the cursor can be restored
        // after the sort reorders the underlying vector.
        let selected_id: Option<String> = self
            .table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
            .map(|mr| mr.id.clone());

        // Keys that allocate (lowercased strings) are computed once per MR via
        // `sort_by_cached_key` instead of twice per comparison. `Reverse` keeps the
        // sort stable in descending order, so ties never swap between two sorts.
        fn sort_by_key_dir<K: Ord>(
            mrs: &mut [TrackedMr],
            descending: bool,
            key: impl Fn(&TrackedMr) -> K,
        ) {
            if descending {
                mrs.sort_by_cached_key(|mr| std::cmp::Reverse(key(mr)));
            } else {
                mrs.sort_by_cached_key(key);
            }
        }
        let descending = order == SortOrder::Descending;
        match col {
            // MRs without a timestamp sort first (`None < Some`). Borrowed compare:
            // no allocation needed, so no cached key.
            SortColumn::UpdatedAt if descending => {
                self.mrs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at))
            }
            SortColumn::UpdatedAt => self.mrs.sort_by(|a, b| a.updated_at.cmp(&b.updated_at)),
            SortColumn::Id => sort_by_key_dir(&mut self.mrs, descending, |mr| {
                mr.id.parse::<u64>().unwrap_or(0)
            }),
            // "None" milestones go last in ascending order.
            SortColumn::Milestone => sort_by_key_dir(&mut self.mrs, descending, |mr| {
                (mr.milestone == "None", mr.milestone.to_lowercase())
            }),
            SortColumn::Title => {
                sort_by_key_dir(&mut self.mrs, descending, |mr| mr.title.to_lowercase())
            }
        }

        // Restore the cursor on the same MR after the sort. If the previously
        // selected MR is no longer visible (e.g. filtered out), fall back to
        // position 0 so the selection is never left dangling.
        if let Some(id) = selected_id {
            let new_idx = self.visible_mrs().position(|mr| mr.id == id).unwrap_or(0);
            self.table_state.select(Some(new_idx));
        }
    }
}

/// What [`App::complete_fetch`] knows about the fetch that just finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchCompletion {
    /// Notifications are allowed: the startup load was already over before this event.
    pub notify_allowed: bool,
    /// This event completes the startup load.
    pub initial_sync_just_completed: bool,
    /// The fetch belonged to a refresh cycle (auto-refresh or `[R]`), which already
    /// re-fetched the linked tracker ticket unconditionally.
    pub from_refresh_cycle: bool,
}

pub trait TrackedMrExt {
    fn find_mut(&mut self, id: &str) -> Option<&mut TrackedMr>;
}

/// Branches an MR newly appeared on, for `mr_on_new_branch` notifications.
///
/// An MR with no persisted branch set (`previously_known == None`) seen during the
/// startup sync is a first sighting, not a change: its branches are recorded
/// silently instead of sending one notification per branch (first-launch avalanche).
/// Once known — even with an empty set, e.g. an open MR — every new branch notifies,
/// including changes that happened while the app was closed.
fn branches_to_notify<'a>(
    previously_known: Option<&HashSet<String>>,
    branches: &'a HashSet<String>,
    notify_allowed: bool,
) -> Vec<&'a String> {
    if previously_known.is_none() && !notify_allowed {
        return Vec::new();
    }
    branches
        .iter()
        .filter(|b| !previously_known.is_some_and(|known| known.contains(*b)))
        .collect()
}

impl TrackedMrExt for Vec<TrackedMr> {
    fn find_mut(&mut self, id: &str) -> Option<&mut TrackedMr> {
        self.iter_mut().find(|m| m.id == id)
    }
}

fn transition_target_for_state(project: &ProjectEntry, state: &GitlabMrState) -> Option<String> {
    let key = match state {
        GitlabMrState::Merged => "merged",
        GitlabMrState::Closed => "closed",
        GitlabMrState::Opened => return None,
    };

    project
        .tracker
        .as_ref()?
        .extra
        .get("status_transitions")?
        .as_table()?
        .get("gitlab_state")?
        .as_table()?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|target_id| !target_id.is_empty())
        .map(ToOwned::to_owned)
}

struct TicketTransitionRequest<'a> {
    tracker: Option<&'a TrackerHandle>,
    transitioner: Option<&'a TicketTransitionHandle>,
    tx: &'a UnboundedSender<AppEvent>,
    mr_id: &'a str,
    mr_title: &'a str,
    previous_state: &'a GitlabMrState,
    current_state: &'a GitlabMrState,
    previous_ticket: Option<LinkedTicket>,
    target_id: Option<String>,
}

fn spawn_ticket_transition_if_needed(request: TicketTransitionRequest<'_>) {
    if !matches!(
        (request.previous_state, request.current_state),
        (GitlabMrState::Opened, GitlabMrState::Merged)
            | (GitlabMrState::Opened, GitlabMrState::Closed)
    ) {
        return;
    }

    let (Some(provider), Some(transitioner), Some(previous_ticket), Some(target_id)) = (
        request.tracker,
        request.transitioner,
        request.previous_ticket,
        request.target_id,
    ) else {
        return;
    };

    let provider = Arc::clone(provider);
    let transitioner = Arc::clone(transitioner);
    let tx = request.tx.clone();
    let mr_id = request.mr_id.to_string();
    let mr_title = request.mr_title.to_string();
    let ticket_id = previous_ticket.id;
    let expected_status = previous_ticket.status;

    tokio::spawn(async move {
        let Some(upstream_ticket) = provider.fetch_ticket(&ticket_id).await else {
            tracing::warn!(ticket_id = %ticket_id, "Skipping tracker transition: ticket refetch failed");
            return;
        };

        if upstream_ticket.status != expected_status {
            tracing::info!(
                ticket_id = %ticket_id,
                expected = %expected_status,
                upstream = %upstream_ticket.status,
                "Skipping tracker transition: upstream status changed since last local snapshot",
            );
            let _ = tx.send(AppEvent::TrackerTicketLoaded {
                mr_id,
                ticket: Box::new(upstream_ticket),
            });
            return;
        }

        if let Err(error) = transitioner
            .transition_ticket_status(&ticket_id, &target_id)
            .await
        {
            tracing::warn!(
                ticket_id = %ticket_id,
                target_id = %target_id,
                error = %error,
                "Tracker transition failed",
            );
            let _ = tx.send(AppEvent::TrackerTicketLoaded {
                mr_id,
                ticket: Box::new(upstream_ticket),
            });
            return;
        }

        tracing::info!(
            ticket_id = %ticket_id,
            target_id = %target_id,
            "Tracker transition succeeded",
        );

        if let Some(refreshed_ticket) = provider.fetch_ticket(&ticket_id).await {
            notify::ticket_status_transitioned(
                &ticket_id,
                &mr_title,
                &expected_status,
                &refreshed_ticket.status,
                &refreshed_ticket.url,
            );
            let _ = tx.send(AppEvent::TrackerTicketLoaded {
                mr_id,
                ticket: Box::new(refreshed_ticket),
            });
        }
    });
}

impl App {
    /// Recomputes `estimated_gitlab_calls` and `estimated_tracker_calls` from the
    /// current MR list and tracker state.
    ///
    /// Must be called whenever the MR list changes (startup, add, remove) or just
    /// before a refresh cycle is triggered — this ensures the displayed counters are
    /// always up to date.
    ///
    /// # Breakdown
    /// For each MR that is *not* fully frozen (merged into all branches with a known SHA):
    ///   - GitLab calls: estimated via `CachedMrData::estimate()` (mirrors fetch logic exactly).
    ///   - Tracker calls: `estimate_calls_per_ticket()` per MR that already has a linked ticket.
    ///     New MRs (no linked ticket yet) cost 0 tracker calls — the ticket is only discovered
    ///     after `MrLoaded` fires and `detect_ticket_id` succeeds.
    ///
    /// The two counters are kept separate so the UI can display them independently.
    pub fn recompute_api_call_estimate(&mut self) {
        let tracker_calls_per_ticket = self
            .tracker
            .as_ref()
            .map(|p| p.estimate_calls_per_ticket())
            .unwrap_or(0);

        let mut gitlab_total: usize = 0;
        let mut tracker_total: usize = 0;

        for mr in &self.mrs {
            // Mirror the freeze guard from the Tick handler — skip fully-frozen MRs.
            if let MrStatus::MergedIn(ref found) = mr.status {
                if self.branches.iter().all(|b| found.contains(b))
                    && mr.sha.is_some()
                    && mr.state != GitlabMrState::Opened
                {
                    continue;
                }
            }

            let has_merge_sha = mr.sha.is_some() || mr.state == GitlabMrState::Merged;
            gitlab_total += mr.estimate(&mr.state, has_merge_sha).total();

            // Tracker calls only for MRs that already have a linked ticket resolved.
            if mr.linked_ticket.is_some() {
                tracker_total += tracker_calls_per_ticket;
            }
        }

        self.estimated_gitlab_calls = gitlab_total;
        self.estimated_tracker_calls = tracker_total;
    }
}

impl App {
    /// Builds a `FetchContext` from the current application state.
    ///
    /// Centralises the repeated construction of `FetchContext` that was
    /// previously scattered across `main.rs` and `events.rs`.
    pub fn fetch_context(&self) -> FetchContext {
        FetchContext {
            base_url: self.base_url.clone(),
            token: self.token.clone(),
            project_id: self.project_id.clone(),
            branches: self.branches.clone(),
        }
    }

    /// Restores tracked MRs from persisted state on startup.
    ///
    /// For each saved MR:
    /// - Reconstructs a `TrackedMr` with cached data (mergeability reset to Unknown).
    /// - If the MR is not already fully merged into all branches, spawns a background
    ///   fetch and increments `pending_initial_fetches` to suppress spurious notifications.
    pub fn restore_from_saved(
        &mut self,
        saved_mrs: Vec<SavedMr>,
        semaphore: Arc<Semaphore>,
        tx: UnboundedSender<AppEvent>,
    ) {
        let ctx = self.fetch_context();

        for saved in saved_mrs {
            let initial_status = if !saved.found_branches.is_empty()
                && self
                    .branches
                    .iter()
                    .all(|b| saved.found_branches.contains(b))
            {
                MrStatus::MergedIn(saved.found_branches.clone())
            } else {
                MrStatus::Loading
            };

            self.mrs.push(TrackedMr {
                id: saved.id.clone(),
                title: saved.title.clone(),
                status: initial_status.clone(),
                sha: saved.sha.clone(),
                description: saved
                    .description
                    .clone()
                    .unwrap_or_else(|| "No description cached.".to_string()),
                author: saved
                    .author
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                assignee: saved.assignee.clone().unwrap_or_else(|| "None".to_string()),
                reviewers: saved.reviewers.clone(),
                milestone: saved
                    .milestone
                    .clone()
                    .unwrap_or_else(|| "None".to_string()),
                milestone_due_date: saved.milestone_due_date.clone(),
                milestone_description: saved.milestone_description.clone(),
                web_url: saved.web_url.clone().unwrap_or_default(),
                labels: saved.labels.clone().unwrap_or_default(),
                updated_at: saved.updated_at.clone(),
                source_branch: saved
                    .source_branch
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                target_branch: saved
                    .target_branch
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                state: saved.state.clone(),
                merged_by: saved.merged_by.clone(),
                merged_at: saved.merged_at.clone(),
                // Mergeability is not persisted — reset to Unknown on restart and re-fetched live.
                mergeability: MergeabilityStatus::Unknown,
                // created_at is immutable — restored from the saved state when available,
                // falls back to None until the first successful fetch populates it.
                created_at: saved.created_at.clone(),
                // Restore persisted pipelines — refreshed on each MR fetch.
                pipelines: saved.pipelines.clone(),
                // On startup, no MR is considered recently updated.
                recently_updated: false,
                // Restore persisted notes count — refreshed on each MR fetch.
                user_notes_count: saved.user_notes_count,
                // Restore persisted flagged state.
                flagged: saved.flagged,
                // Restore persisted ticket — avoids a tracker request on every restart.
                linked_ticket: saved.linked_ticket,
                // Restore persisted diff stats — refreshed only when updated_at changes.
                diff_stats: saved.diff_stats.clone(),
            });

            if initial_status == MrStatus::Loading {
                let cached = CachedMrData {
                    title: Some(saved.title),
                    description: saved.description,
                    author: saved.author,
                    web_url: saved.web_url,
                    labels: saved.labels,
                    updated_at: saved.updated_at,
                    pipelines: saved.pipelines,
                    diff_stats: saved.diff_stats,
                    user_notes_count: saved.user_notes_count,
                    // Restore the persisted state so the notes cache is considered valid
                    // for already-merged MRs on restart — avoids a useless refetch.
                    cached_state: Some(saved.state),
                    cache_policy: CachePolicy::Normal,
                };

                // Count each pending fetch so we can suppress change notifications
                // until the initial sync is complete (avoids spurious toasts on launch).
                self.pending_initial_fetches.insert(saved.id.clone());

                spawn_mr_fetch(ctx.clone(), saved.id, cached, semaphore.clone(), tx.clone());
            }
        }

        if !self.mrs.is_empty() {
            self.table_state.select(Some(0));
        }

        // Compute the startup estimate now that all saved MRs have been pushed.
        // This is the cold-start baseline — shown in parentheses in the table title.
        // The tracker provider may not be injected yet at this point (main.rs injects
        // it after restore_from_saved), so tracker_total will be 0 here and corrected
        // on the first Tick once the provider is available.
        self.recompute_api_call_estimate();
        self.startup_gitlab_estimate = Some(self.estimated_gitlab_calls);
        self.startup_tracker_estimate = Some(self.estimated_tracker_calls);
    }

    /// Applies a single `AppEvent` to the application state.
    ///
    /// This is the central event dispatch extracted from `main.rs` to keep the
    /// event loop thin. Returns `true` if the state was mutated in a way that
    /// requires persisting (caller must then call `save_state_async`).
    ///
    /// Reactions to MR lifecycle transitions (refetch, remove, notify, persist) are
    /// governed by `self.event_policy` — no hardcoded booleans here.
    pub async fn apply_event(
        &mut self,
        event: AppEvent,
        semaphore: Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
        last_known_branches: &mut HashMap<String, HashSet<String>>,
    ) -> bool {
        // Resolve the lifecycle event once upfront so every arm can query the policy.
        let lifecycle: Option<MrLifecycleEvent> = event.as_lifecycle_event();
        let needs_persist = lifecycle
            .as_ref()
            .map(|l| self.event_policy.needs_persist(l))
            .unwrap_or(false);

        match event {
            // ── Tracker ticket resolved ───────────────────────────────────────
            AppEvent::TrackerTicketLoaded { mr_id, ticket } => {
                if let Some(mr) = self.mrs.find_mut(&mr_id) {
                    // Compute the diff between the cached ticket and the freshly fetched one.
                    // The diff logic lives entirely in `core` (LinkedTicket::diff) — this site
                    // only dispatches the resulting changes to the notification layer.
                    // Adding a new tracked field only requires touching `core::TicketChange`
                    // and `core::LinkedTicket::diff`; this match arm stays unchanged.
                    if let Some(old) = &mr.linked_ticket {
                        let mr_title = mr.title.clone();
                        let ticket_url = ticket.url.clone();
                        let ticket_id = ticket.id.clone();

                        for change in old.diff(&ticket) {
                            let (old_val, new_val) = change.before_after();
                            tracing::info!(
                                ticket_id = %ticket_id,
                                field = %change.field_label(),
                                old = %old_val,
                                new = %new_val,
                                "Tracker ticket field changed",
                            );
                            notify::ticket_field_changed(
                                &ticket_id,
                                &mr_title,
                                change.field_label(),
                                old_val,
                                new_val,
                                &ticket_url,
                            );
                        }
                    }

                    mr.linked_ticket = Some(*ticket);
                }
                // Ticket data is display-only — no state persist needed.
                false
            }

            // ── Activity categories loaded ────────────────────────────────────
            AppEvent::ActivitiesLoaded(activities) => {
                self.activities = activities;
                false
            }

            // ── Time entries loaded for a ticket ─────────────────────────────
            AppEvent::TimeEntriesLoaded { ticket_id, entries } => {
                self.time_entries.insert(ticket_id, Some(entries));
                false
            }

            // ── Time log submitted successfully ───────────────────────────────
            AppEvent::TimeLogSubmitted { mr_id, ticket_id } => {
                // Re-fetch both time entries (for the TimeLog view) and the full ticket
                // (so that spent_hours updates in the Inspector header and table column).
                // We now carry the mr_id so TrackerTicketLoaded routes to the right MR.
                if let Some(provider) = &self.tracker {
                    let provider = Arc::clone(provider);
                    let tx2 = tx.clone();
                    let tid = ticket_id.clone();
                    self.time_entries.insert(tid.clone(), None);
                    tokio::spawn(async move {
                        // Run both requests concurrently.
                        let (entries, ticket) = tokio::join!(
                            provider.fetch_time_entries(&tid),
                            provider.fetch_ticket(&tid),
                        );
                        let _ = tx2.send(AppEvent::TimeEntriesLoaded {
                            ticket_id: tid,
                            entries,
                        });
                        if let Some(ticket) = ticket {
                            let _ = tx2.send(AppEvent::TrackerTicketLoaded {
                                mr_id,
                                ticket: Box::new(ticket),
                            });
                        }
                    });
                }
                // Close the popup and reset the form.
                self.input_mode = InputMode::Normal;
                self.log_time_form = LogTimeForm::default();
                false
            }

            // ── Time log submission failed ────────────────────────────────────
            AppEvent::TimeLogFailed { error } => {
                self.log_time_form.submitting = false;
                self.log_time_form.error = Some(error);
                false
            }

            // ── MR lifecycle mutations ────────────────────────────────────────
            // All direct mutations of `app.mrs` must go through these events so
            // that `recompute_api_call_estimate` is always called exactly once,
            // in a single place, after the list changes.
            AppEvent::MrAdded(id) => {
                // Skip if already tracked.
                if self.mrs.iter().any(|m| m.id == id) {
                    return false;
                }
                // Manual re-add is an explicit opt-in: allow discovery to track it again.
                self.dismissed_mr_ids.remove(&id);
                self.mrs.push(TrackedMr::placeholder(
                    id.clone(),
                    "Loading...".to_string(),
                    "Loading".to_string(),
                ));
                // The selection indexes the visible (filtered) list, not `mrs`.
                let pos = self.visible_mrs().position(|m| m.id == id);
                if pos.is_some() {
                    self.table_state.select(pos);
                }
                // Spawn the fetch only if the policy confirms a refetch is needed for Added.
                if self.event_policy.needs_refetch(&MrLifecycleEvent::Added) {
                    spawn_mr_fetch(
                        self.fetch_context(),
                        id,
                        CachedMrData::default(),
                        semaphore.clone(),
                        tx.clone(),
                    );
                }
                self.recompute_api_call_estimate();
                needs_persist
            }

            AppEvent::MrRemovedById(id) => {
                let before = self.mrs.len();
                self.mrs.retain(|m| m.id != id);
                if self.mrs.len() == before {
                    return false; // Nothing removed — no state change.
                }
                self.dismissed_mr_ids.insert(id);
                self.clamp_selection();
                self.recompute_api_call_estimate();
                needs_persist
            }

            AppEvent::MrMergeabilityRetrying { id } => {
                if let Some(mr) = self.mrs.find_mut(&id) {
                    mr.mergeability = MergeabilityStatus::Retrying;
                }
                false
            }

            AppEvent::MrLoaded(data) => {
                // Release the fetch before any early return so the spinner and the
                // startup notification fence stay consistent.
                let completion = self.complete_fetch(&data.id);
                let notify_allowed = completion.notify_allowed;
                #[cfg(feature = "stats")]
                let initial_fetches_completed = completion.initial_sync_just_completed;

                let Some(mr) = self.mrs.find_mut(&data.id) else {
                    return false;
                };

                // Compare new branches against the last persisted state to avoid
                // re-notifying on restart or in-memory state that hasn't changed on disk.
                for b in branches_to_notify(
                    last_known_branches.get(&data.id),
                    &data.branches,
                    notify_allowed,
                ) {
                    notify::mr_on_new_branch(&data.id, &data.title, b, &data.web_url);
                }

                // Update the persisted reference so subsequent refreshes won't re-notify.
                last_known_branches.insert(data.id.clone(), data.branches.clone());

                // Detect whether this MR was actually updated since the last refresh.
                // We compare the old `updated_at` before overwriting it.
                let was_updated = mr.updated_at.is_some() && mr.updated_at != data.updated_at;
                let previous_state = mr.state.clone();
                let previous_ticket = mr.linked_ticket.clone();

                // Trace field-level changes so they are visible in the log file.
                // All comparisons happen before the fields are overwritten below.
                if was_updated {
                    tracing::info!(
                        mr_id = %data.id,
                        old = %mr.updated_at.as_deref().unwrap_or("none"),
                        new = %data.updated_at.as_deref().unwrap_or("none"),
                        "MR updated_at changed",
                    );
                    if notify_allowed {
                        notify::mr_updated(
                            &data.id,
                            &data.title,
                            data.updated_at.as_deref(),
                            &data.web_url,
                        );
                    }
                }
                if mr.mergeability != data.mergeability {
                    tracing::info!(
                        mr_id = %data.id,
                        old = ?mr.mergeability,
                        new = ?data.mergeability,
                        "MR mergeability changed",
                    );
                    if notify_allowed {
                        notify::mr_mergeability_changed(
                            &data.id,
                            &data.title,
                            &format!("{:?}", mr.mergeability),
                            &format!("{:?}", data.mergeability),
                            &data.web_url,
                        );
                    }
                }
                if mr.milestone != data.milestone {
                    tracing::info!(
                        mr_id = %data.id,
                        old = %mr.milestone,
                        new = %data.milestone,
                        "MR milestone changed",
                    );
                    if notify_allowed {
                        notify::mr_milestone_changed(
                            &data.id,
                            &data.title,
                            &mr.milestone,
                            &data.milestone,
                            &data.web_url,
                        );
                    }
                }

                mr.title = data.title;
                mr.sha = data.sha;
                mr.status = MrStatus::MergedIn(data.branches);
                mr.description = data.description;
                mr.author = data.author;
                mr.assignee = data.assignee;
                mr.reviewers = data.reviewers;
                mr.milestone = data.milestone;
                mr.milestone_due_date = data.milestone_due_date;
                mr.milestone_description = data.milestone_description;
                mr.web_url = data.web_url;
                mr.labels = data.labels;
                mr.updated_at = data.updated_at;
                mr.created_at = data.created_at;
                mr.source_branch = data.source_branch;
                mr.target_branch = data.target_branch;
                mr.state = data.state;
                mr.merged_by = data.merged_by;
                mr.merged_at = data.merged_at;
                mr.mergeability = data.mergeability;
                mr.pipelines = data.pipelines;
                mr.recently_updated = was_updated;
                mr.user_notes_count = data.user_notes_count;

                // Detect complexity category changes (EASY / MEDIUM / COMPLEX) before
                // overwriting the stored diff_stats. Only fires when both old and new
                // stats are available and the category boundary is actually crossed.
                {
                    let complexity_label = |score: f64| -> &'static str {
                        if score < 0.33 {
                            "🟢 EASY"
                        } else if score < 0.66 {
                            "🟡 MEDIUM"
                        } else {
                            "🔴 COMPLEX"
                        }
                    };
                    if let (Some(old_stats), Some(new_stats)) = (&mr.diff_stats, &data.diff_stats) {
                        let old_label =
                            complexity_label(old_stats.difficulty(&self.config.complexity_profile));
                        let new_label =
                            complexity_label(new_stats.difficulty(&self.config.complexity_profile));
                        if old_label != new_label {
                            notify::mr_complexity_changed(
                                &mr.id,
                                &mr.title,
                                old_label,
                                new_label,
                                &mr.web_url,
                            );
                        }
                    }
                }

                mr.diff_stats = data.diff_stats;

                let target_id = transition_target_for_state(&self.project_settings, &mr.state);
                spawn_ticket_transition_if_needed(TicketTransitionRequest {
                    tracker: self.tracker.as_ref(),
                    transitioner: self.ticket_transitioner.as_ref(),
                    tx,
                    mr_id: &mr.id,
                    mr_title: &mr.title,
                    previous_state: &previous_state,
                    current_state: &mr.state,
                    previous_ticket: previous_ticket.clone(),
                    target_id,
                });

                // Capture the MR id before the stats block — `data` is fully consumed above.
                #[cfg(feature = "stats")]
                let data_id_for_stats = mr.id.clone();

                // Arm (or re-arm) the global fade countdown.
                if was_updated {
                    self.update_highlight_ticks = RECENT_UPDATE_FADE_TICKS;
                }
                // If a tracker provider is active, re-fetch the linked ticket when:
                //   • the detected ticket ID is new or has changed (ID mismatch), OR
                //   • the MR was updated since the last refresh (was_updated), which
                //     implies that time entries or status may have changed on the
                //     tracker side (e.g. after a manual [R] refresh).
                if let Some(provider) = &self.tracker {
                    let detected_id = provider.detect_ticket_id(&mr.title, &mr.description);
                    let cached_id = mr.linked_ticket.as_ref().map(|t| t.id.clone());

                    // Determine the ticket id to fetch:
                    //   - If the detected id differs from the cache → use the new id.
                    //   - If they match but we want a forced refresh → reuse the cached id.
                    //   - If the cached ticket's schema is outdated → invalidate and re-fetch.
                    //   - If nothing is detected and nothing cached → nothing to do.
                    let cache_is_stale = mr.linked_ticket.as_ref().is_some_and(|t| {
                        t.schema_version < gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION
                    });

                    let fetch_id: Option<String> = if detected_id != cached_id {
                        // ID changed (or newly detected): always re-fetch.
                        detected_id.clone()
                    } else if detected_id.is_some() && was_updated && !completion.from_refresh_cycle
                    {
                        // Same ID but the MR was updated: refresh to pick up new spent hours.
                        // Skipped for refresh-cycle fetches: the cycle already re-fetched
                        // this ticket unconditionally, a second request would be a duplicate.
                        detected_id.clone()
                    } else if detected_id.is_some() && cache_is_stale {
                        // Same ID but the cached struct is from an older schema version:
                        // re-fetch silently to populate the new fields.
                        detected_id.clone()
                    } else {
                        // No change needed.
                        None
                    };

                    if let Some(raw_id) = fetch_id {
                        let provider = Arc::clone(provider);
                        let mr_id = mr.id.clone();
                        let tx2 = tx.clone();
                        tokio::spawn(async move {
                            if let Some(ticket) = provider.fetch_ticket(&raw_id).await {
                                let _ = tx2.send(AppEvent::TrackerTicketLoaded {
                                    mr_id,
                                    ticket: Box::new(ticket),
                                });
                            }
                        });
                    } else if detected_id.is_none() && cached_id.is_some() {
                        // Ticket reference was removed from the MR — clear the cache.
                        mr.linked_ticket = None;
                    }
                }

                // Sorting is deferred to the end of the event drain (see
                // `flush_pending_sort`) so a burst of N loads costs one sort, not N.
                self.needs_sort = true;

                // ── Stats recording ───────────────────────────────────────────
                // Record a snapshot into the stats DB when the feature is enabled.
                // Three triggers are handled here:
                //   • OnMerge  — when the MR just transitioned to Merged state.
                //   • OnClose  — when the MR just transitioned to Closed state.
                //   • OnRefresh — at most once per calendar day for open MRs.
                #[cfg(feature = "stats")]
                if let Some(db) = self.stats_db.clone() {
                    // Re-borrow after all mutations are applied so the snapshot
                    // captures the final state written to `mr` just above.
                    if let Some(mr) = self.mrs.find_mut(&data_id_for_stats) {
                        use gitlab_tracker_stats::snapshot::SnapshotTrigger;

                        let trigger = match &mr.state {
                            GitlabMrState::Merged => Some(SnapshotTrigger::OnMerge),
                            GitlabMrState::Closed => Some(SnapshotTrigger::OnClose),
                            GitlabMrState::Opened => {
                                // Record at most once per calendar day.
                                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                                let last = self
                                    .stats_last_refresh_date
                                    .get(&mr.id)
                                    .cloned()
                                    .unwrap_or_default();
                                if last == today {
                                    None
                                } else {
                                    self.stats_last_refresh_date.insert(mr.id.clone(), today);
                                    Some(SnapshotTrigger::OnRefresh)
                                }
                            }
                        };

                        if let Some(trigger) = trigger {
                            let snap = mr.stats_snapshot(
                                trigger,
                                &self.project_id,
                                &self.config.complexity_profile,
                            );

                            let project_id = self.project_id.clone();
                            let tx2 = if initial_fetches_completed {
                                Some(tx.clone())
                            } else {
                                None
                            };
                            tokio::spawn(async move {
                                use gitlab_tracker_stats::StatsDb;

                                if let Err(e) = db.upsert_snapshot(&snap).await {
                                    tracing::warn!(
                                        project_id = %project_id,
                                        error = %e,
                                        "Failed to record stats snapshot"
                                    );
                                    return;
                                }

                                // The report itself is recomputed on the next Tick
                                // (debounced), not once per recorded snapshot.
                                if let Some(tx2) = tx2 {
                                    let _ = tx2.send(AppEvent::StatsSnapshotRecorded);
                                }
                            });
                        }
                    }
                }

                // MrLoaded maps to MrLifecycleEvent::Refreshed — persist driven by policy.
                needs_persist
            }

            AppEvent::MrFailed { id, error } => {
                // A failed fetch is finished too — otherwise the spinner never stops.
                self.complete_fetch(&id);
                let Some(mr) = self.mrs.find_mut(&id) else {
                    return false;
                };
                mr.title = format!("⚠️ ERROR: {}", error);
                mr.status = MrStatus::Error;
                // FetchFailed → policy returns false (errors are not persisted to disk).
                needs_persist
            }

            // ── Stats async results ───────────────────────────────────────────
            #[cfg(feature = "stats")]
            AppEvent::StatsReportReady(report) => {
                self.stats_view.loading = false;
                self.stats_view.error = None;
                self.stats_view.report = Some(*report);
                false
            }

            #[cfg(feature = "stats")]
            AppEvent::StatsSnapshotRecorded => {
                self.stats_report_dirty = true;
                false
            }

            #[cfg(feature = "stats")]
            AppEvent::StatsReportFailed(err) => {
                self.stats_view.loading = false;
                self.stats_view.error = Some(err);
                false
            }

            AppEvent::GitlabLabelsLoaded(labels) => {
                // Store into config so it flows through everywhere config is passed.
                self.config.gitlab_label_colors = labels
                    .into_iter()
                    .map(|l| (l.name.to_lowercase(), l.color))
                    .collect();
                false
            }

            AppEvent::MilestonesLoaded(milestones) => {
                self.milestones = milestones;
                false
            }

            AppEvent::MilestoneMrsLoaded {
                milestone_title,
                mr_ids,
            } => {
                let ctx = self.fetch_context();
                let mut added = 0u32;

                for mr_id in mr_ids {
                    // Skip MRs already tracked to avoid duplicates.
                    if self.mrs.iter().any(|m| m.id == mr_id) {
                        continue;
                    }
                    self.mrs.push(TrackedMr::placeholder(
                        mr_id.clone(),
                        format!("Loading… ({})", milestone_title),
                        milestone_title.clone(),
                    ));
                    spawn_mr_fetch(
                        ctx.clone(),
                        mr_id,
                        CachedMrData::default(),
                        semaphore.clone(),
                        tx.clone(),
                    );
                    added += 1;
                }

                if added > 0 {
                    // Recompute the estimate now that new MRs have been added to the list.
                    self.recompute_api_call_estimate();
                    self.table_state.select(Some(0));
                    true
                } else {
                    false
                }
            }

            AppEvent::NewMrsDiscovered {
                mr_ids,
                next_anchor,
            } => {
                let anchor_moved =
                    self.discovery_started_at.as_deref() != Some(next_anchor.as_str());
                self.discovery_started_at = Some(next_anchor);
                let ctx = self.fetch_context();
                let mut added = 0u32;
                for mr_id in mr_ids {
                    // Guard against races: the MR may have been added between the
                    // discovery fetch and this event being processed.
                    if self.mrs.iter().any(|m| m.id == mr_id) {
                        continue;
                    }
                    self.mrs.push(TrackedMr::placeholder(
                        mr_id.clone(),
                        format!("Loading… ({})", mr_id),
                        String::new(),
                    ));
                    spawn_mr_fetch(
                        ctx.clone(),
                        mr_id,
                        CachedMrData::default(),
                        semaphore.clone(),
                        tx.clone(),
                    );
                    added += 1;
                }
                if added > 0 {
                    self.recompute_api_call_estimate();
                    true
                } else {
                    // Persist an advanced anchor even when nothing new was found.
                    anchor_moved
                }
            }

            AppEvent::Tick => {
                #[cfg(feature = "stats")]
                if std::mem::take(&mut self.stats_report_dirty) {
                    crate::ui::stats::trigger_background_stats_refresh(self, tx);
                }

                // Decrement the highlight fade countdown and clear flags when expired.
                if self.update_highlight_ticks > 0 {
                    self.update_highlight_ticks -= 1;
                    if self.update_highlight_ticks == 0 {
                        for mr in &mut self.mrs {
                            mr.recently_updated = false;
                        }
                    }
                }

                if self.time_left > 0 {
                    self.time_left -= 1;
                    return false;
                }

                // Timer elapsed — trigger a full refresh of all MRs.
                self.time_left = self.refresh_interval_secs;
                self.time_entries.clear();
                let ctx = self.fetch_context();

                // Discovery poller: find new MRs created by any team member since the
                // discovery anchor, regardless of their current state. This prevents losing
                // MRs opened and merged between two refresh cycles, which would otherwise
                // make stats incomplete. Only active when `discover_new_mrs = true` in
                // `[project.stats]` of `projects.toml`.
                if self.discovery_enabled {
                    self.spawn_discovery(ctx.clone(), tx);
                }

                // Recompute GitLab + tracker estimates *before* spawning fetches so the
                // counter reflects the actual cache state at trigger time.
                self.recompute_api_call_estimate();

                for mr in &mut self.mrs {
                    if let MrStatus::MergedIn(ref found) = mr.status {
                        // Skip refresh for MRs that are fully merged into all branches —
                        // state != Opened ensures we don't skip still-open MRs.
                        if self.branches.iter().all(|b| found.contains(b))
                            && mr.sha.is_some()
                            && mr.state != GitlabMrState::Opened
                        {
                            continue;
                        }
                    }

                    mr.status = MrStatus::Loading;
                    let cached = CachedMrData::from(&*mr);
                    spawn_mr_fetch(
                        ctx.clone(),
                        mr.id.clone(),
                        cached,
                        semaphore.clone(),
                        tx.clone(),
                    );
                    // Track pending auto-refresh fetches to drive the spinner.
                    self.pending_refresh_fetches.insert(mr.id.clone());

                    // Re-fetch the tracker ticket unconditionally on each auto-refresh cycle,
                    // mirroring the manual [R] refresh behaviour. The GitLab MR may not have
                    // changed (was_updated = false) while the tracker ticket status, spent time,
                    // or priority did — the conditional re-fetch inside MrLoaded would miss this.
                    if let Some(provider) = self.tracker.as_ref().map(Arc::clone) {
                        if let Some(ticket_id) = mr.linked_ticket.as_ref().map(|t| t.id.clone()) {
                            let mr_id = mr.id.clone();
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                if let Some(ticket) = provider.fetch_ticket(&ticket_id).await {
                                    let _ = tx2.send(AppEvent::TrackerTicketLoaded {
                                        mr_id,
                                        ticket: Box::new(ticket),
                                    });
                                }
                            });
                        }
                    }
                }

                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App::new(AppInit {
            token: "t".into(),
            project_id: "1".into(),
            base_url: "https://gitlab.example".into(),
            project_name: None,
            refresh_interval_secs: 900,
            config: AppConfig::default(),
            theme: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
            project_settings: ProjectEntry {
                name: None,
                gitlab_url: "https://gitlab.example".into(),
                project_id: "1".into(),
                active: true,
                default_branches: None,
                table_label_prefixes: None,
                complexity_profile: None,
                tracked_branches: None,
                refresh_interval_secs: None,
                activity_stale_days: None,
                activity_recent_days: None,
                show_cockpit: None,
                cockpit_thresholds: None,
                stats: None,
                visible_columns: None,
                label_colors: None,
                tracker: None,
                gitlab_username: None,
                discover_new_mrs: None,
            },
        })
    }

    #[tokio::test]
    async fn failed_fetch_of_untracked_mr_releases_spinner() {
        let mut app = test_app();
        app.pending_initial_fetches.insert("1".into());
        app.pending_refresh_fetches
            .extend(["1".to_string(), "2".to_string()]);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        // MR "1" is not in `app.mrs` (removed while its fetch was in flight):
        // the failure must still release it, and only it.
        app.apply_event(
            AppEvent::MrFailed {
                id: "1".into(),
                error: "boom".into(),
            },
            Arc::new(Semaphore::new(1)),
            &tx,
            &mut HashMap::new(),
        )
        .await;

        assert!(app.pending_initial_fetches.is_empty());
        assert_eq!(app.pending_refresh_fetches.len(), 1);
    }

    /// Tracks MRs "1".."n" with the given `(title, milestone)` pairs.
    async fn app_with_mrs(mrs: &[(&str, &str)]) -> App {
        let mut app = test_app();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        for (i, _) in mrs.iter().enumerate() {
            app.apply_event(
                AppEvent::MrAdded((i + 1).to_string()),
                Arc::new(Semaphore::new(1)),
                &tx,
                &mut HashMap::new(),
            )
            .await;
        }
        for (mr, (title, milestone)) in app.mrs.iter_mut().zip(mrs) {
            mr.title = (*title).into();
            mr.milestone = (*milestone).into();
        }
        app
    }

    fn ids(app: &App) -> Vec<&str> {
        app.mrs.iter().map(|mr| mr.id.as_str()).collect()
    }

    #[tokio::test]
    async fn sort_keys_and_stable_descending() {
        let mut app = app_with_mrs(&[("beta", "None"), ("Alpha", "v2"), ("beta", "V1")]).await;

        app.sort_column = SortColumn::Title;
        app.sort_order = SortOrder::Ascending;
        app.sort_mrs();
        assert_eq!(ids(&app), ["2", "1", "3"]); // case-insensitive, ties keep order

        app.sort_order = SortOrder::Descending;
        app.sort_mrs();
        assert_eq!(ids(&app), ["1", "3", "2"]); // ties still keep order
        app.sort_mrs();
        assert_eq!(ids(&app), ["1", "3", "2"]); // re-sorting never swaps ties

        app.sort_column = SortColumn::Milestone;
        app.sort_order = SortOrder::Ascending;
        app.sort_mrs();
        assert_eq!(ids(&app), ["3", "2", "1"]); // "None" last

        app.sort_column = SortColumn::Id;
        app.sort_order = SortOrder::Descending;
        app.sort_mrs();
        assert_eq!(ids(&app), ["3", "2", "1"]);
    }

    #[tokio::test]
    async fn render_cache_matches_live_visible_list() {
        let mut app = app_with_mrs(&[("fix login", "None"), ("add stats", "None")]).await;
        app.input = "stats".into();
        let live: Vec<String> = app.visible_mrs().map(|mr| mr.id.clone()).collect();

        app.begin_render_cache();
        let cached: Vec<String> = app.visible_mrs().map(|mr| mr.id.clone()).collect();
        app.end_render_cache();

        assert_eq!(live, ["2"]);
        assert_eq!(cached, live);
    }

    #[tokio::test]
    async fn delete_key_removes_selected_visible_mr() {
        let mut app = app_with_mrs(&[
            ("fix login", "None"),
            ("add stats", "None"),
            ("stats view", "None"),
        ])
        .await;
        app.input = "stats".into();
        // MR "1" (= `mrs[0]`) is hidden by the search, so visible row 0 is another MR:
        // the old index-based removal deleted `mrs[0]` instead.
        app.table_state.select(Some(0));
        let victim = app.visible_mrs().next().unwrap().id.clone();
        let survivor = app.visible_mrs().nth(1).unwrap().id.clone();
        assert_ne!(victim, "1");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Delete);
        crate::events::handle_key_event(
            key,
            &mut app,
            &Arc::new(Semaphore::new(1)),
            &tx,
            &mut HashMap::new(),
        )
        .await;
        while let Ok(event) = rx.try_recv() {
            app.apply_event(event, Arc::new(Semaphore::new(1)), &tx, &mut HashMap::new())
                .await;
        }

        assert_eq!(app.mrs.len(), 2);
        assert!(app.mrs.iter().any(|m| m.id == "1") && app.mrs.iter().any(|m| m.id == survivor));
        assert!(app.dismissed_mr_ids.contains(&victim));
        // The selection is clamped to the (now single-row) visible list.
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[cfg(feature = "stats")]
    #[tokio::test]
    async fn stats_snapshot_stores_sentinels_as_null() {
        use gitlab_tracker_stats::snapshot::SnapshotTrigger;
        let mut app = app_with_mrs(&[("t", "None")]).await;
        let profile = app.config.complexity_profile.clone();
        let mr = &mut app.mrs[0];
        // Placeholder: assignee "Loading", milestone "None".
        let snap = mr.stats_snapshot(SnapshotTrigger::OnRefresh, "42", &profile);
        assert_eq!((snap.assignee, snap.milestone), (None, None));

        mr.assignee = "Jane (@jane)".into();
        mr.milestone = "v1.0".into();
        let snap = mr.stats_snapshot(SnapshotTrigger::OnRefresh, "42", &profile);
        assert_eq!(snap.assignee.as_deref(), Some("Jane (@jane)"));
        assert_eq!(snap.milestone.as_deref(), Some("v1.0"));
        assert_eq!(snap.project_id, "42");
    }

    #[test]
    fn new_branch_notifications_skip_first_sighting_at_startup() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
        let main = set(&["main"]);
        // First launch, startup sync: silent.
        assert!(branches_to_notify(None, &main, false).is_empty());
        // Known open MR (empty set) that merged while the app was closed: notifies.
        assert_eq!(branches_to_notify(Some(&set(&[])), &main, false), ["main"]);
        // Already known on main: nothing new.
        assert!(branches_to_notify(Some(&main), &main, true).is_empty());
        // MR added after startup: notifies as before.
        assert_eq!(branches_to_notify(None, &main, true), ["main"]);
    }

    #[test]
    fn complete_fetch_flags() {
        let mut app = test_app();
        app.pending_initial_fetches
            .extend(["1".to_string(), "2".to_string()]);

        app.pending_refresh_fetches.insert("3".into());
        let done =
            |notify_allowed, initial_sync_just_completed, from_refresh_cycle| FetchCompletion {
                notify_allowed,
                initial_sync_just_completed,
                from_refresh_cycle,
            };

        // An untracked fetch (e.g. MrAdded) must not release the startup fence.
        assert_eq!(app.complete_fetch("99"), done(false, false, false));
        assert_eq!(app.complete_fetch("1"), done(false, false, false));
        // Last startup fetch: completes the initial sync, notifications still off for it.
        assert_eq!(app.complete_fetch("2"), done(false, true, false));
        // Refresh-cycle fetch: its ticket was already re-fetched by the cycle.
        assert_eq!(app.complete_fetch("3"), done(true, false, true));
    }
}
