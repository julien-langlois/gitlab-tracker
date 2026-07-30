use crate::config::AppConfig;
use crate::models::{GitLabMilestone, TrackedMr};
use ratatui::widgets::TableState;

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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InputMode {
    /// Shortcut keys are active; the input field is passive.
    #[default]
    Normal,
    /// The input field has exclusive focus; shortcuts are suspended.
    Editing,
    /// The column-picker popup is open — arrow keys and Space toggle columns.
    ColumnPicker,
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
    /// The MR detail side viewer (right pane).
    Inspector,
}

impl ActivePane {
    /// Cycles to the next pane in a round-robin fashion.
    pub fn next(self) -> Self {
        match self {
            ActivePane::Dashboard => ActivePane::Inspector,
            ActivePane::Inspector => ActivePane::Dashboard,
        }
    }
}

/// Controls which view is rendered inside the Inspector side panel.
///
/// Toggled with [P] — switches between the MR metadata view and the
/// pipeline list view without changing pane focus.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InspectorView {
    /// Default: MR metadata, description, labels (existing behaviour).
    #[default]
    MrInfo,
    /// Pipeline list for the selected MR.
    Pipelines,
}

/// Controls which MRs are displayed in the table.
///
/// Cycles with the [F] key in Normal mode:
///   - `All`     → all tracked MRs are shown (default)
///   - `Flagged` → only manually flagged MRs are shown
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FilterMode {
    /// Show all tracked MRs (no filtering).
    #[default]
    All,
    /// Show only MRs that have been manually flagged with Space.
    Flagged,
}

impl FilterMode {
    /// Cycles to the next filter mode in a round-robin fashion.
    pub fn next(self) -> Self {
        match self {
            FilterMode::All => FilterMode::Flagged,
            FilterMode::Flagged => FilterMode::All,
        }
    }

    /// Returns the display label shown in the header.
    pub fn label(self) -> &'static str {
        match self {
            FilterMode::All => "All",
            FilterMode::Flagged => "Flagged ★",
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
    /// Active filter applied to the MR table — toggled with [F] in Normal mode.
    pub filter_mode: FilterMode,
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
            column_picker_cursor: 0,
            update_highlight_ticks: 0,
            milestones: Vec::new(),
            milestone_suggestions: Vec::new(),
            milestone_suggestion_cursor: 0,
            filter_mode: FilterMode::default(),
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

    /// Cycles the active filter to the next mode.
    ///
    /// Resets the table selection to the first row to avoid out-of-bounds access
    /// when the filtered list is shorter than the current selection index.
    pub fn cycle_filter(&mut self) {
        self.filter_mode = self.filter_mode.next();
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
        self.mrs.iter().filter(move |mr| match self.filter_mode {
            FilterMode::All => true,
            FilterMode::Flagged => mr.flagged,
        })
    }

    /// Returns a mutable iterator over the MRs that pass the current filter.
    fn visible_mrs_mut(&mut self) -> impl Iterator<Item = &mut TrackedMr> {
        let filter = self.filter_mode;
        self.mrs.iter_mut().filter(move |mr| match filter {
            FilterMode::All => true,
            FilterMode::Flagged => mr.flagged,
        })
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
