use crate::config::AppConfig;
use crate::gitlab::{spawn_mr_fetch, CachedMrData, FetchContext};
use crate::models::{
    AppEvent, GitLabMilestone, GitlabMrState, MergeabilityStatus, MrStatus, SavedMr, TrackedMr,
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
/// - `FilterPicker`: the filter picker popup is open — arrow keys navigate, Enter confirms,
///   typing feeds the text input for Milestone/Assignee entries.
/// - `LogTime`: the Log Time popup is open — Tab navigates fields, Enter submits.
///   Only reachable when a tracker provider is configured (`app.tracker.is_some()`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InputMode {
    /// Shortcut keys are active; the input field is passive.
    #[default]
    Normal,
    /// The input field has exclusive focus; shortcuts are suspended.
    Editing,
    /// The column-picker popup is open — arrow keys and Space toggle columns.
    ColumnPicker,
    /// The filter picker popup is open — arrow keys navigate, Enter confirms.
    /// Typing feeds the text input for Milestone / Assignee entries.
    FilterPicker,
    /// The Log Time popup is open — Tab cycles fields, Enter submits.
    /// Only reachable when `app.tracker.is_some()`.
    LogTime,
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

/// Controls which MRs are displayed in the table.
///
/// Selected interactively via the [F] key which opens a `FilterPicker` popup.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FilterMode {
    /// Show all tracked MRs (no filtering).
    #[default]
    All,
    /// Show only MRs that have been manually flagged with Space.
    Flagged,
    /// Show only MRs with the given GitLab lifecycle state.
    State(crate::models::GitlabMrState),
    /// Show only MRs with the given mergeability status.
    Mergeability(crate::models::MergeabilityStatus),
    /// Show only MRs that have at least one comment or discussion thread.
    HasNotes,
    /// Show only MRs whose milestone title contains the given string (case-insensitive).
    Milestone(String),
    /// Show only MRs whose assignee contains the given string (case-insensitive).
    Assignee(String),
}

impl FilterMode {
    /// Returns the display label shown in the table header.
    pub fn label(&self) -> String {
        match self {
            FilterMode::All => "All".to_string(),
            FilterMode::Flagged => "Flagged ★".to_string(),
            FilterMode::State(s) => match s {
                crate::models::GitlabMrState::Opened => "State: Opened".to_string(),
                crate::models::GitlabMrState::Merged => "State: Merged".to_string(),
                crate::models::GitlabMrState::Closed => "State: Closed".to_string(),
            },
            FilterMode::Mergeability(m) => format!("Mergeability: {:?}", m),
            FilterMode::HasNotes => "Has comments 💬".to_string(),
            FilterMode::Milestone(q) => format!("Milestone: {}", q),
            FilterMode::Assignee(q) => format!("Assignee: {}", q),
        }
    }
}

/// Index of each row in the filter picker popup, in display order.
pub const FILTER_PICKER_ENTRIES: &[&str] = &[
    "All (no filter)",
    "Flagged ★",
    "State: Opened",
    "State: Merged",
    "State: Closed",
    "Mergeability: Mergeable",
    "Mergeability: Conflict",
    "Mergeability: Needs Rebase",
    "Mergeability: Not Approved",
    "Mergeability: Requested Changes",
    "Mergeability: Draft",
    "Mergeability: Discussions",
    "Has comments 💬",
    "Milestone… (type below)",
    "Assignee… (type below)",
];

/// State held by the filter picker popup while it is open.
#[derive(Debug, Clone, Default)]
pub struct FilterPickerState {
    /// Index of the currently highlighted row (0-based).
    pub cursor: usize,
    /// Free-text input used for Milestone and Assignee filters.
    pub input: String,
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
    pub filter_mode: FilterMode,
    /// State of the filter picker popup (cursor position + text input).
    /// Reset each time the popup is opened.
    pub filter_picker: FilterPickerState,
    /// Number of MR fetches still pending from the initial startup load.
    /// Change notifications (updated_at, mergeability, milestone) are suppressed
    /// until this reaches zero, preventing spurious toasts on first launch.
    pub pending_initial_fetches: usize,
    /// Active tracker provider (Redmine, Jira, Trello, …), shared across async tasks via Arc.
    /// `None` when no provider is configured or the user skipped the token prompt.
    pub tracker: Option<TrackerHandle>,
    /// Colour maps for tracker badge labels (type and priority).
    /// Populated from the active tracker's config at startup and forwarded to the Tracker pane renderer.
    /// Defaults to empty maps (dark_gray / white fallback) when no provider is configured.
    pub tracker_colors: crate::ui::tracker::TrackerLabelColors,
    /// Activity categories fetched from the tracker at startup.
    /// Populated by `AppEvent::ActivitiesLoaded` and used to fill the Log Time popup.
    pub activities: Vec<gitlab_tracker_core::Activity>,
    /// Time entries for the currently selected ticket, fetched when the TimeLog view opens.
    pub time_entries: Vec<gitlab_tracker_core::TimeEntry>,
    /// State of the Log Time popup form. Reset each time the popup is opened.
    pub log_time_form: LogTimeForm,
    /// When `true`, the user has pressed Esc once and is being asked to confirm quitting.
    /// A second Esc (or `y`) confirms; any other key cancels.
    pub quit_confirm: bool,
}

/// Duration (in seconds) of the green highlight fade after a MR is updated.
pub const RECENT_UPDATE_FADE_TICKS: u64 = 10;

impl App {
    pub fn new(
        token: String,
        project_id: String,
        base_url: String,
        refresh_interval_secs: u64,
        config: AppConfig,
    ) -> Self {
        let mut table_state = TableState::default();
        table_state.select(None);
        Self {
            mrs: Vec::new(),
            branches: Vec::new(),
            input: String::new(),
            input_mode: InputMode::default(),
            token,
            project_id,
            base_url,
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
            update_highlight_ticks: 0,
            milestones: Vec::new(),
            milestone_suggestions: Vec::new(),
            milestone_suggestion_cursor: 0,
            filter_mode: FilterMode::default(),
            filter_picker: FilterPickerState::default(),
            // Initialised to 0 — main.rs sets this to the number of MRs loaded from state
            // before the first fetch cycle begins, then decrements it on each MrLoaded event.
            pending_initial_fetches: 0,
            // Initialised to None — main.rs injects the provider after keyring lookup.
            tracker: None,
            tracker_colors: crate::ui::tracker::TrackerLabelColors::default(),
            activities: Vec::new(),
            time_entries: Vec::new(),
            log_time_form: LogTimeForm::default(),
            quit_confirm: false,
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

    /// Opens the filter picker popup, initialising its state from the current filter.
    ///
    /// Pre-selects the row that matches the active filter so the user sees their current
    /// selection highlighted when the popup opens.
    pub fn open_filter_picker(&mut self) {
        // Derive the initial cursor position from the active filter.
        let cursor = match &self.filter_mode {
            FilterMode::All => 0,
            FilterMode::Flagged => 1,
            FilterMode::State(crate::models::GitlabMrState::Opened) => 2,
            FilterMode::State(crate::models::GitlabMrState::Merged) => 3,
            FilterMode::State(crate::models::GitlabMrState::Closed) => 4,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::Mergeable) => 5,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::Conflict) => 6,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::NeedsRebase) => 7,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::NotApproved) => 8,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::RequestedChanges) => 9,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::Draft) => 10,
            FilterMode::Mergeability(crate::models::MergeabilityStatus::DiscussionsNotResolved) => {
                11
            }
            FilterMode::Mergeability(_) => 5,
            FilterMode::HasNotes => 12,
            FilterMode::Milestone(_) => 13,
            FilterMode::Assignee(_) => 14,
        };

        // Pre-fill the text input from the current filter value if applicable.
        let input = match &self.filter_mode {
            FilterMode::Milestone(q) => q.clone(),
            FilterMode::Assignee(q) => q.clone(),
            _ => String::new(),
        };

        self.filter_picker = FilterPickerState { cursor, input };
        self.input_mode = InputMode::FilterPicker;
    }

    /// Applies the filter picker selection and closes the popup.
    ///
    /// Derives a `FilterMode` from the current picker cursor and text input,
    /// then resets the table selection to avoid out-of-bounds access.
    pub fn apply_filter_picker(&mut self) {
        use crate::models::{GitlabMrState, MergeabilityStatus};
        let input = self.filter_picker.input.trim().to_string();

        self.filter_mode = match self.filter_picker.cursor {
            0 => FilterMode::All,
            1 => FilterMode::Flagged,
            2 => FilterMode::State(GitlabMrState::Opened),
            3 => FilterMode::State(GitlabMrState::Merged),
            4 => FilterMode::State(GitlabMrState::Closed),
            5 => FilterMode::Mergeability(MergeabilityStatus::Mergeable),
            6 => FilterMode::Mergeability(MergeabilityStatus::Conflict),
            7 => FilterMode::Mergeability(MergeabilityStatus::NeedsRebase),
            8 => FilterMode::Mergeability(MergeabilityStatus::NotApproved),
            9 => FilterMode::Mergeability(MergeabilityStatus::RequestedChanges),
            10 => FilterMode::Mergeability(MergeabilityStatus::Draft),
            11 => FilterMode::Mergeability(MergeabilityStatus::DiscussionsNotResolved),
            12 => FilterMode::HasNotes,
            13 => {
                if input.is_empty() {
                    FilterMode::All
                } else {
                    FilterMode::Milestone(input)
                }
            }
            14 => {
                if input.is_empty() {
                    FilterMode::All
                } else {
                    FilterMode::Assignee(input)
                }
            }
            _ => FilterMode::All,
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

    /// Returns an iterator over the MRs that pass the current filter.
    pub fn visible_mrs(&self) -> impl Iterator<Item = &TrackedMr> {
        self.mrs.iter().filter(move |mr| self.filter_passes(mr))
    }

    /// Returns a mutable iterator over the MRs that pass the current filter.
    fn visible_mrs_mut(&mut self) -> impl Iterator<Item = &mut TrackedMr> {
        let filter = self.filter_mode.clone();
        self.mrs
            .iter_mut()
            .filter(move |mr| Self::filter_passes_static(&filter, mr))
    }

    /// Returns `true` when `mr` passes the currently active filter.
    fn filter_passes(&self, mr: &TrackedMr) -> bool {
        Self::filter_passes_static(&self.filter_mode, mr)
    }

    /// Pure predicate — does not borrow `self`, usable inside `iter_mut` closures.
    fn filter_passes_static(filter: &FilterMode, mr: &TrackedMr) -> bool {
        match filter {
            FilterMode::All => true,
            FilterMode::Flagged => mr.flagged,
            FilterMode::State(s) => &mr.state == s,
            FilterMode::Mergeability(m) => &mr.mergeability == m,
            FilterMode::HasNotes => mr.user_notes_count > 0,
            FilterMode::Milestone(q) => mr.milestone.to_lowercase().contains(&q.to_lowercase()),
            FilterMode::Assignee(q) => {
                let q_lower = q.to_lowercase();
                // Match against the GitLab assignee field …
                let gitlab_match = mr.assignee.to_lowercase().contains(&q_lower);
                // … or the tracker ticket assignee when a ticket is linked.
                let tracker_match = mr
                    .linked_ticket
                    .as_ref()
                    .and_then(|t| t.assignee.as_deref())
                    .map(|a| a.to_lowercase().contains(&q_lower))
                    .unwrap_or(false);
                gitlab_match || tracker_match
            }
        }
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

    pub fn sort_mrs(&mut self) {
        let order = self.sort_order;
        let col = self.sort_column;

        self.mrs.sort_by(|a, b| {
            let cmp = match col {
                SortColumn::UpdatedAt => {
                    // MRs without a timestamp are pushed to the bottom.
                    match (&a.updated_at, &b.updated_at) {
                        (Some(ta), Some(tb)) => ta.cmp(tb),
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                }
                SortColumn::Id => {
                    let id_a = a.id.parse::<u64>().unwrap_or(0);
                    let id_b = b.id.parse::<u64>().unwrap_or(0);
                    id_a.cmp(&id_b)
                }
                SortColumn::Milestone => {
                    if a.milestone == "None" && b.milestone != "None" {
                        std::cmp::Ordering::Greater
                    } else if a.milestone != "None" && b.milestone == "None" {
                        std::cmp::Ordering::Less
                    } else {
                        a.milestone.to_lowercase().cmp(&b.milestone.to_lowercase())
                    }
                }
                SortColumn::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            };

            if order == SortOrder::Ascending {
                cmp
            } else {
                cmp.reverse()
            }
        });
    }
}

pub trait TrackedMrExt {
    fn find_mut(&mut self, id: &str) -> Option<&mut TrackedMr>;
}

impl TrackedMrExt for Vec<TrackedMr> {
    fn find_mut(&mut self, id: &str) -> Option<&mut TrackedMr> {
        self.iter_mut().find(|m| m.id == id)
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
            });

            if initial_status == MrStatus::Loading {
                let cached = CachedMrData {
                    title: Some(saved.title),
                    sha: saved.sha,
                    description: saved.description,
                    author: saved.author,
                    assignee: saved.assignee,
                    web_url: saved.web_url,
                    labels: saved.labels,
                    updated_at: saved.updated_at,
                    pipelines: saved.pipelines,
                };

                // Count each pending fetch so we can suppress change notifications
                // until the initial sync is complete (avoids spurious toasts on launch).
                self.pending_initial_fetches += 1;

                spawn_mr_fetch(ctx.clone(), saved.id, cached, semaphore.clone(), tx.clone());
            }
        }

        if !self.mrs.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    /// Applies a single `AppEvent` to the application state.
    ///
    /// This is the central event dispatch extracted from `main.rs` to keep the
    /// event loop thin. Returns `true` if the state was mutated in a way that
    /// requires persisting (caller must then call `save_state_async`).
    pub async fn apply_event(
        &mut self,
        event: AppEvent,
        semaphore: Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
        last_known_branches: &mut HashMap<String, HashSet<String>>,
    ) -> bool {
        match event {
            // ── Tracker ticket resolved ───────────────────────────────────────
            AppEvent::TrackerTicketLoaded { mr_id, ticket } => {
                if let Some(mr) = self.mrs.find_mut(&mr_id) {
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
            AppEvent::TimeEntriesLoaded { entries } => {
                self.time_entries = entries;
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
                    tokio::spawn(async move {
                        // Run both requests concurrently.
                        let (entries, ticket) = tokio::join!(
                            provider.fetch_time_entries(&tid),
                            provider.fetch_ticket(&tid),
                        );
                        let _ = tx2.send(AppEvent::TimeEntriesLoaded { entries });
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

            AppEvent::MrLoaded(data) => {
                let Some(mr) = self.mrs.find_mut(&data.id) else {
                    return false;
                };

                // Compare new branches against the last persisted state to avoid
                // re-notifying on restart or in-memory state that hasn't changed on disk.
                let previously_known = last_known_branches
                    .get(&data.id)
                    .cloned()
                    .unwrap_or_default();

                for b in &data.branches {
                    if !previously_known.contains(b) {
                        notify::mr_on_new_branch(&data.id, &data.title, b);
                    }
                }

                // Update the persisted reference so subsequent refreshes won't re-notify.
                last_known_branches.insert(data.id.clone(), data.branches.clone());

                // Decrement the startup fence: notifications are suppressed until
                // all MRs from the saved state have received their first API response.
                let notify_allowed = self.pending_initial_fetches == 0;
                if self.pending_initial_fetches > 0 {
                    self.pending_initial_fetches -= 1;
                }

                // Detect whether this MR was actually updated since the last refresh.
                // We compare the old `updated_at` before overwriting it.
                let was_updated = mr.updated_at.is_some() && mr.updated_at != data.updated_at;

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
                        notify::mr_updated(&data.id, &data.title, data.updated_at.as_deref());
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
                mr.web_url = data.web_url;
                mr.labels = data.labels;
                mr.updated_at = data.updated_at;
                mr.source_branch = data.source_branch;
                mr.target_branch = data.target_branch;
                mr.state = data.state;
                mr.merged_by = data.merged_by;
                mr.merged_at = data.merged_at;
                mr.mergeability = data.mergeability;
                mr.pipelines = data.pipelines;
                mr.recently_updated = was_updated;
                mr.user_notes_count = data.user_notes_count;

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
                    } else if detected_id.is_some() && was_updated {
                        // Same ID but the MR was updated: refresh to pick up new spent hours.
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

                self.sort_mrs();
                true
            }

            AppEvent::MrFailed { id, error } => {
                let Some(mr) = self.mrs.find_mut(&id) else {
                    return false;
                };
                mr.title = format!("⚠️ ERROR: {}", error);
                mr.status = MrStatus::Error;
                true
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
                    self.mrs.push(TrackedMr {
                        id: mr_id.clone(),
                        title: format!("Loading… ({})", milestone_title),
                        status: MrStatus::Loading,
                        state: GitlabMrState::Opened,
                        mergeability: MergeabilityStatus::Unknown,
                        sha: None,
                        description: String::new(),
                        author: "Loading".to_string(),
                        assignee: "Loading".to_string(),
                        reviewers: vec![],
                        milestone: milestone_title.clone(),
                        milestone_due_date: None,
                        web_url: String::new(),
                        labels: vec![],
                        updated_at: None,
                        source_branch: "unknown".to_string(),
                        target_branch: "unknown".to_string(),
                        merged_by: None,
                        merged_at: None,
                        pipelines: vec![],
                        recently_updated: false,
                        user_notes_count: 0,
                        // New MRs start unflagged.
                        flagged: false,
                        // Ticket resolved live after each MR fetch — never pre-populated.
                        linked_ticket: None,
                    });
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
                    self.table_state.select(Some(0));
                    true
                } else {
                    false
                }
            }

            AppEvent::Tick => {
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
                let ctx = self.fetch_context();

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
                    let cached = CachedMrData {
                        title: Some(mr.title.clone()),
                        sha: mr.sha.clone(),
                        description: Some(mr.description.clone()),
                        author: Some(mr.author.clone()),
                        assignee: Some(mr.assignee.clone()),
                        web_url: Some(mr.web_url.clone()),
                        labels: Some(mr.labels.clone()),
                        updated_at: mr.updated_at.clone(),
                        pipelines: mr.pipelines.clone(),
                    };
                    spawn_mr_fetch(
                        ctx.clone(),
                        mr.id.clone(),
                        cached,
                        semaphore.clone(),
                        tx.clone(),
                    );
                }

                false
            }
        }
    }
}
