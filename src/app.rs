use crate::config::AppConfig;
use crate::models::TrackedMr;
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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InputMode {
    /// Shortcut keys are active; the input field is passive.
    #[default]
    Normal,
    /// The input field has exclusive focus; shortcuts are suspended.
    Editing,
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
}

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
        }
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
        if self.mrs.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.mrs.len() - 1 {
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
        if self.mrs.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.mrs.len() - 1
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
