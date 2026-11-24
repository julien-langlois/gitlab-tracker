use crate::config::AppConfig;
use crate::gitlab::{spawn_mr_fetch, CachedMrData, CountApiCalls, FetchContext};
use crate::i18n::{tf, tl, tn};
use crate::models::{
    AppEvent, BranchCommits, BranchPresence, BranchRow, GitLabMilestone, GitlabMrState,
    MergeabilityStatus, MrStatus, PipelineState, SavedMr, TrackedMr,
};
use crate::notify;
use crate::settings::SettingsEditorState;
use crate::storage::ProjectEntry;
use gitlab_tracker_core::{
    collect_all_columns, collect_all_filters, ColumnDef, FilterDef, LinkedTicket, MrSnapshot,
};
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Semaphore;

mod branches;
mod clipboard;
mod events;
mod filters;
mod mr_changes;
mod refresh;
mod selection;
mod session;
#[cfg(feature = "stats")]
mod stats_recorder;
#[cfg(feature = "stats")]
mod stats_view;
mod ticks;
mod tracker_sync;

use branches::newly_copied;
#[cfg(any(test, feature = "stats"))]
use branches::reached_branches;
use mr_changes::branches_to_notify;
use refresh::RefreshCycle;
use session::{session_changes, session_digest, MrBaseline};
#[cfg_attr(not(feature = "gui"), allow(unused_imports))]
pub use session::{ChangeKind, SessionChange};
#[cfg(all(feature = "stats", feature = "gui"))]
pub use stats_view::StatsWindow;
#[cfg(feature = "stats")]
pub use stats_view::{StatsTab, StatsViewState};
use ticks::DIGEST_TICKS;
use tracker_sync::{
    spawn_ticket_transition_if_needed, transition_target_for_state, TicketTransitionRequest,
};
pub use tracker_sync::{spawn_tickets_fetch, TimeLogState};

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
    /// When it was merged; the MRs not merged go after the merged ones.
    MergedAt,
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
    /// `[Esc]` or `[G]` closes it and returns to Normal mode.
    #[cfg(feature = "stats")]
    Stats,
    /// The project picker popup is open — arrow keys navigate, Enter switches.
    ProjectPicker,
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
/// Cycled with `[P]` — rotates between MrInfo and Pipelines only.
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
/// Cycled with `[P]` when the Tracker pane is focused.
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
            return tl("All").to_string();
        };
        let label = tl(def.active_label());
        if def.id == "merged_between" {
            if let Some(range) = crate::merged_range::MergedRange::parse(&self.query) {
                let today = chrono::Local::now().date_naive();
                return format!("{label} {}", range.describe(today));
            }
        }
        if def.needs_text_input && !self.query.is_empty() {
            format!("{label} {}", self.query)
        } else {
            label.to_string()
        }
    }
}

/// A set of MRs picked from elsewhere (a cockpit counter in the window), shown
/// on top of the active filter until it is cleared.
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub struct Focus {
    /// The counter it came from, to refresh `ids` as the MRs change.
    pub key: String,
    /// What the window shows for it ("Blocked").
    pub label: String,
    pub ids: HashSet<String>,
}

/// State held by the filter picker popup while it is open.
#[derive(Debug, Clone, Default)]
pub struct FilterPickerState {
    /// Index of the currently highlighted row (0-based).
    pub cursor: usize,
    /// Free-text input used for parametric filters (Milestone, Assignee, …).
    pub input: String,
    /// Highlighted entry of [`App::label_suggestions`], picked with Tab; `None`
    /// until then, so Enter applies the typed text (e.g. a `review::` scope).
    pub suggestion: Option<usize>,
}

/// Shown in place of a missing assignee or milestone.
pub const NO_VALUE: &str = "None";

/// Scroll state of a pane (Inspector or Tracker): the offset, plus what the last
/// render measured, so scrolling stops at the last line actually displayed.
#[derive(Debug, Default, Clone, Copy)]
pub struct PaneScroll {
    /// Vertical scroll offset (in lines).
    pub offset: u16,
    /// Rendered (wrapped) line count, borders included — set at each render.
    pub content_lines: u16,
    /// Pane height, borders included — set at each render.
    pub height: u16,
}

impl PaneScroll {
    /// Scrolls down, never past the last line of content.
    pub fn down(&mut self, amount: u16) {
        let max = self.content_lines.saturating_sub(self.height);
        self.offset = self.offset.saturating_add(amount).min(max);
    }

    pub fn up(&mut self, amount: u16) {
        self.offset = self.offset.saturating_sub(amount);
    }

    /// Back to the top (e.g. when another MR is selected).
    pub fn reset(&mut self) {
        self.offset = 0;
    }
}

/// UI state written by the renderer and read back by input handling: pane
/// geometry (mouse hit-test), scroll limits and the spinner frame.
#[derive(Debug, Default)]
pub struct UiLayout {
    pub inspector: PaneScroll,
    pub tracker: PaneScroll,
    /// Screen area of the Inspector pane at the last render.
    pub inspector_area: ratatui::layout::Rect,
    /// Screen area of the Tracker pane at the last render; `None` when hidden.
    pub tracker_area: Option<ratatui::layout::Rect>,
    /// Bumped on every render frame, to animate the spinner independently of the
    /// one-second tick.
    pub spinner_frame: usize,
}

/// An MR's place on a GitLab merge train.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "only the window reads the merge trains")
)]
pub struct TrainPlace {
    /// Its car, from 1.
    pub car: usize,
    /// The train's length.
    pub of: usize,
    /// When it joined the train.
    pub joined: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct App {
    pub mrs: Vec<TrackedMr>,
    pub branches: Vec<String>,
    pub input: String,
    /// Whether the input field has exclusive keyboard focus.
    /// In `Editing` mode all printable keys feed the field; shortcuts are suspended.
    pub input_mode: InputMode,
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
    pub base_url: String,
    /// Optional human-readable alias for this project, as set in `projects.toml` (`name` field).
    /// Displayed in the table title alongside the URL when present.
    pub project_name: Option<String>,
    pub time_left: u64,
    pub refresh_interval_secs: u64,
    /// Seconds until the next sentinel check, and between two (`0`: off).
    pub sentinel_left: u64,
    pub sentinel_interval_secs: u64,
    /// Watermark and in-flight flag of the sentinel (see `crate::sentinel`).
    pub sentinel: crate::sentinel::SentinelState,
    pub table_state: TableState,
    pub config: AppConfig,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    /// Which pane currently holds focus (drives keyboard & scroll routing).
    pub active_pane: ActivePane,
    /// Which view is rendered inside the Inspector panel (`[P]` toggles).
    pub inspector_view: InspectorView,
    /// Pane geometry and scroll written by the renderer (see [`UiLayout`]).
    pub layout: UiLayout,
    /// Which view is rendered inside the Tracker pane (`[P]` toggles when focused).
    pub tracker_view: TrackerView,
    /// Index of the currently highlighted row in the column-picker popup (0-based).
    pub column_picker_cursor: usize,
    /// `projects.toml` entries listed by the project picker (loaded when it opens).
    pub project_picker: Vec<ProjectEntry>,
    pub project_picker_cursor: usize,
    /// Project chosen in the picker; the main loop saves the current one and
    /// rebuilds the session for it.
    pub switch_to: Option<ProjectEntry>,
    /// Draft state for the project settings popup.
    pub settings_editor: SettingsEditorState,
    /// The active project entry as loaded from `projects.toml`.
    /// Kept as a TOML-backed settings source for plugin-provided settings.
    pub project_settings: ProjectEntry,
    /// Countdown (in ticks ~= seconds) during which recently-updated rows stay highlighted.
    /// Reset to `RECENT_UPDATE_FADE_TICKS` each time a MR update is detected.
    /// Decremented on every Tick; rows are highlighted while this is > 0.
    pub update_highlight_ticks: u64,
    /// Short-lived feedback shown in the status bar (copy done, switch failed…).
    pub flash: Option<Flash>,
    /// State of the MRs fetched at startup as it was saved, compared once the
    /// initial sync is over for the "since last session" summary.
    startup_baseline: HashMap<String, MrBaseline>,
    /// What changed while the app was closed, once the startup load is over
    /// (the window shows it on the cockpit).
    pub last_digest: Option<String>,
    /// The same changes one by one, for the window's Today page.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub last_changes: Vec<SessionChange>,
    /// List of active/upcoming milestones fetched from GitLab on startup.
    /// Used to power the milestone autocomplete in the input field.
    pub milestones: Vec<GitLabMilestone>,
    /// When the user types `@` followed by text in Editing mode, this holds the
    /// filtered list of milestone titles matching the current query.
    /// Empty when autocomplete is not active.
    pub milestone_suggestions: Vec<String>,
    /// Index of the currently highlighted suggestion in the autocomplete popup.
    pub milestone_suggestion_cursor: usize,
    /// Active filter applied to the MR table — selected via the `[F]` picker popup.
    pub active_filter: ActiveFilter,
    /// Filters on top of it, every one to pass (the window's sidebar filters:
    /// absent from staging and from production, with a label…). Empty in the
    /// terminal UI, which shows one filter at a time.
    pub filters: Vec<ActiveFilter>,
    /// MRs picked from a cockpit counter, on top of the filter (window only).
    pub focus: Option<Focus>,
    /// Set while the cockpit counts the MRs: they ignore `focus`.
    pub focus_suspended: bool,
    /// The one status the list shows, picked on the status bar (window only).
    pub status_pick: Option<crate::status::StatusKind>,
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
    /// Timing of the refresh cycle in flight, logged when its last MR is back.
    refresh_cycle: Option<RefreshCycle>,
    /// Estimated GitLab API HTTP calls for the *next* refresh cycle (GitLab only).
    /// Computed just before fetches are spawned so it reflects the real cache state.
    pub estimated_gitlab_calls: usize,
    /// Estimated tracker API calls for the *next* refresh cycle (Redmine, Jira, …).
    /// Kept separate from GitLab calls so the UI can display them independently.
    /// `0` when no tracker provider is configured.
    pub estimated_tracker_calls: usize,
    /// Requests the next refresh avoids compared with reading each MR and each
    /// ticket on its own (list reads, review caching, tracker bulk reads).
    pub saved_gitlab_calls: usize,
    pub saved_tracker_calls: usize,
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
    /// shown (see `ensure_time_entries`). Cleared at each refresh cycle (and `[R]`) so
    /// entries logged elsewhere show up — and a failed fetch is retried — within one cycle.
    pub time_entries: HashMap<String, TimeLogState>,
    /// Error of the last activity-list fetch, shown in the Log Time popup.
    pub activities_error: Option<String>,
    /// System clipboard, created on first yank and kept alive: on Linux without a
    /// clipboard manager the copied text is lost as soon as its owner is dropped.
    clipboard: Option<arboard::Clipboard>,
    /// State of the Log Time popup form. Reset each time the popup is opened.
    pub log_time_form: LogTimeForm,
    /// When `true`, the user has pressed Esc once and is being asked to confirm quitting.
    /// A second Esc (or `y`) confirms; any other key cancels.
    pub quit_confirm: bool,
    /// Demo mode: the key handler skips every disk write and network request
    /// (settings, branches, flags, refresh, MR imports). Events it sends to the bus
    /// (add / remove MR…) are not applied by the demo loop either.
    pub read_only: bool,
    /// Recent commits of each tracked branch (`BranchCommitsLoaded`), to recognise
    /// MRs copied there under other SHAs; rescanned after every refresh cycle.
    pub branch_commits: HashMap<String, BranchCommits>,
    /// What each `branch_commits` index covers, so the next scan only reads the
    /// commits that arrived since (`gitlab::plan_branch_scan`).
    pub branch_scan_scopes: HashMap<String, crate::gitlab::BranchScanScope>,
    /// Branches added in this session whose MRs have not all been re-fetched yet:
    /// an MR already on such a branch did not just "arrive" there, so it is neither
    /// notified nor recorded as a branch arrival.
    pub just_added_branches: HashSet<String>,
    /// Review reminders already sent this session, by MR id.
    review_reminders: HashMap<String, mr_changes::ReviewReminded>,
    /// Branch scans spawned and not answered yet.
    pub pending_branch_scans: usize,
    /// MRs whose commit signatures are being checked (`SignaturesChecked`).
    pub signature_checks: HashSet<String>,
    /// Set by `MrLoaded`; consumed once per event drain by [`App::flush_pending_sort`].
    pub needs_sort: bool,
    /// Per-render snapshot of `visible_mrs()` indices — see [`App::begin_render_cache`].
    visible_cache: Option<Vec<usize>>,
    /// The selected MR kept in the list after a refresh made it fail the filters
    /// (merged under "Hide merged MRs"…), until the selection moves to another
    /// MR — see [`App::keep_selection`]. Without it the selection, an index into
    /// the visible list, would land on the next MR.
    kept_visible: Option<String>,
    /// An event moved the selection on purpose (an MR added, a milestone's MRs
    /// imported): [`App::keep_selection`] leaves it there.
    selection_moved: bool,
    /// Shortcut blocks collected at startup via `inventory` from every linked crate.
    ///
    /// Populated once by `gitlab_tracker_core::collect_all_blocks()` — no explicit
    /// provider registration needed in `main.rs`. The help popup iterates this list
    /// in collection order (link order: Core first, optional plugins after).
    pub shortcut_providers: Vec<gitlab_tracker_core::ShortcutBlock>,
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
    /// When each MR first reached each tracked branch other than its target, as
    /// recorded in the stats DB (empty without it), by `(mr_id, branch)`.
    #[cfg_attr(
        not(feature = "gui"),
        allow(dead_code, reason = "only the window draws the branch flow")
    )]
    pub branch_arrivals: HashMap<(String, String), chrono::DateTime<chrono::Utc>>,
    /// The MRs on a GitLab merge train, by id: their car and the train's
    /// length. Set by the window each frame (it reads the trains).
    #[cfg_attr(
        not(feature = "gui"),
        allow(dead_code, reason = "only the window reads the merge trains")
    )]
    pub merge_train: HashMap<String, TrainPlace>,
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
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
    pub base_url: String,
    pub project_name: Option<String>,
    pub refresh_interval_secs: u64,
    pub sentinel_interval_secs: u64,
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
            sentinel_interval_secs,
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
            sentinel_left: sentinel_interval_secs,
            sentinel_interval_secs,
            sentinel: crate::sentinel::SentinelState::new(chrono::Utc::now()),
            table_state,
            config,
            sort_column: SortColumn::UpdatedAt,
            sort_order: SortOrder::Descending,
            active_pane: ActivePane::default(),
            inspector_view: InspectorView::default(),
            layout: UiLayout::default(),
            tracker_view: TrackerView::default(),
            column_picker_cursor: 0,
            project_picker: Vec::new(),
            project_picker_cursor: 0,
            switch_to: None,
            settings_editor,
            project_settings,
            update_highlight_ticks: 0,
            flash: None,
            startup_baseline: HashMap::new(),
            last_digest: None,
            last_changes: Vec::new(),
            milestones: Vec::new(),
            milestone_suggestions: Vec::new(),
            milestone_suggestion_cursor: 0,
            active_filter: ActiveFilter::default(),
            filters: Vec::new(),
            focus: None,
            focus_suspended: false,
            status_pick: None,
            filter_picker: FilterPickerState::default(),
            filter_defs: collect_all_filters(),
            column_defs,
            // Initialised to 0 — main.rs sets this to the number of MRs loaded from state
            // before the first fetch cycle begins, then decrements it on each MrLoaded event.
            pending_initial_fetches: HashSet::new(),
            pending_refresh_fetches: HashSet::new(),
            refresh_cycle: None,
            estimated_gitlab_calls: 0,
            estimated_tracker_calls: 0,
            saved_gitlab_calls: 0,
            saved_tracker_calls: 0,
            startup_gitlab_estimate: None,
            startup_tracker_estimate: None,
            // Initialised to None — main.rs injects the provider after keyring lookup.
            tracker: None,
            ticket_transitioner: None,
            tracker_colors: crate::ui::tracker::TrackerLabelColors::default(),
            activities: Vec::new(),
            time_entries: HashMap::new(),
            activities_error: None,
            clipboard: None,
            log_time_form: LogTimeForm::default(),
            quit_confirm: false,
            read_only: false,
            branch_commits: HashMap::new(),
            branch_scan_scopes: HashMap::new(),
            pending_branch_scans: 0,
            signature_checks: HashSet::new(),
            just_added_branches: HashSet::new(),
            review_reminders: HashMap::new(),
            theme,
            needs_sort: false,
            visible_cache: None,
            kept_visible: None,
            selection_moved: false,
            // Populated at startup by main.rs — at least CoreShortcutProvider is always pushed.
            shortcut_providers: gitlab_tracker_core::collect_all_blocks(),
            #[cfg(feature = "stats")]
            stats_view: StatsViewState::default(),
            #[cfg(feature = "stats")]
            stats_db: None,
            #[cfg(feature = "stats")]
            stats_last_refresh_date: std::collections::HashMap::new(),
            #[cfg(feature = "stats")]
            stats_report_dirty: false,
            branch_arrivals: HashMap::new(),
            merge_train: HashMap::new(),
            // Disabled by default — opt-in via `discover_new_mrs = true` in projects.toml.
            discovery_enabled: false,
            discovery_started_at: None,
            dismissed_mr_ids: HashSet::new(),
        }
    }
}

/// Share of the requests `saved` represents out of the `next + saved` that reading
/// each MR and ticket on its own would send, in percent (status bar counters).
pub fn saved_percent(next: usize, saved: usize) -> usize {
    (saved * 100).checked_div(next + saved).unwrap_or(0)
}

/// A transient status-bar message: the result of an action that has no other
/// visible effect (clipboard copy, failed project switch…).
#[derive(Debug, Clone, PartialEq)]
pub struct Flash {
    pub text: String,
    pub is_error: bool,
    ticks_left: u8,
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

impl TrackedMrExt for Vec<TrackedMr> {
    fn find_mut(&mut self, id: &str) -> Option<&mut TrackedMr> {
        self.iter_mut().find(|m| m.id == id)
    }
}

#[cfg(test)]
pub(crate) mod tests;
