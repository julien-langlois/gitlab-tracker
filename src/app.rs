use crate::config::AppConfig;
use crate::models::TrackedMr;
use ratatui::widgets::TableState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    None,
    Id,
    Milestone,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

pub struct App {
    pub mrs: Vec<TrackedMr>,
    pub branches: Vec<String>,
    pub input: String,
    pub token: String,
    pub project_id: String,
    pub base_url: String,
    pub time_left: u64,
    pub refresh_interval_secs: u64,
    pub table_state: TableState,
    pub config: AppConfig,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
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
            token,
            project_id,
            base_url,
            refresh_interval_secs,
            time_left: refresh_interval_secs,
            table_state,
            config,
            sort_column: SortColumn::None,
            sort_order: SortOrder::Ascending,
        }
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
    }

    pub fn cycle_sort_column(&mut self) {
        self.sort_column = match self.sort_column {
            SortColumn::None => SortColumn::Id,
            SortColumn::Id => SortColumn::Milestone,
            SortColumn::Milestone => SortColumn::Title,
            SortColumn::Title => SortColumn::None,
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
        if self.sort_column == SortColumn::None {
            return;
        }

        let order = self.sort_order;
        let col = self.sort_column;

        self.mrs.sort_by(|a, b| {
            let cmp = match col {
                SortColumn::None => std::cmp::Ordering::Equal,
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
