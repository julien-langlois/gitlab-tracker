//! The table selection, row navigation and sorting.

use super::*;

impl App {
    /// The MR at the table selection. The selection indexes the visible list
    /// (filter + search ranking), not `self.mrs`.
    pub fn selected_mr(&self) -> Option<&TrackedMr> {
        self.table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
    }

    /// Toggles the flagged state of the currently selected MR.
    ///
    /// Returns the MR id if a MR was toggled, `None` if no MR is selected.
    pub fn toggle_flag_selected(&mut self) -> Option<String> {
        let selected = self.table_state.selected()?;
        // The selection indexes the visible list (filter + search ranking), not `self.mrs`.
        let index = *self.compute_visible_indices().get(selected)?;
        let mr = &mut self.mrs[index];
        mr.flagged = !mr.flagged;
        Some(mr.id.clone())
    }

    /// Returns true when the selected MR has a linked tracker ticket.
    pub fn has_tracker_ticket(&self) -> bool {
        self.table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
            .and_then(|mr| mr.linked_ticket.as_ref())
            .is_some()
    }

    /// The id of the selected MR, to find it again with [`App::keep_selection`]
    /// once the list changed.
    pub fn selected_id(&self) -> Option<String> {
        self.selected_mr().map(|mr| mr.id.clone())
    }

    /// Puts the selection back on `id`, the MR selected before a batch of events
    /// changed the list: an MR merged, closed or reordered must not hand the
    /// selection — and the MR page — to its neighbour. When `id` no longer passes
    /// the filters it stays listed until the selection moves (see
    /// [`App::release_kept`]); when it is no longer tracked, the selection is
    /// only clamped.
    pub fn keep_selection(&mut self, id: Option<String>) {
        let moved = std::mem::take(&mut self.selection_moved);
        let Some(id) = id.filter(|_| !moved) else {
            return;
        };
        if !self.mrs.iter().any(|mr| mr.id == id) {
            self.clamp_selection();
            return;
        }
        if !self.visible_mrs().any(|mr| mr.id == id) {
            self.kept_visible = Some(id.clone());
            self.visible_cache = None;
        }
        let pos = self.visible_mrs().position(|mr| mr.id == id);
        self.table_state.select(pos);
    }

    /// Drops the MR kept by [`App::keep_selection`] once another MR is selected,
    /// keeping the selection on that other MR as the list shrinks.
    pub fn release_kept(&mut self) {
        let Some(kept) = &self.kept_visible else {
            return;
        };
        let selected = self.selected_id();
        if selected.as_ref() == Some(kept) {
            return;
        }
        self.kept_visible = None;
        self.visible_cache = None;
        match selected {
            Some(id) => self.keep_selection(Some(id)),
            None => self.clamp_selection(),
        }
    }

    /// Keeps the selection inside the visible list after it shrinks.
    pub(crate) fn clamp_selection(&mut self) {
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
        self.layout.inspector.reset();
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
        self.layout.inspector.reset();
    }

    /// Sorts by `column`; on the current column, reverses the order instead.
    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.toggle_sort_order();
            return;
        }
        self.sort_column = column;
        self.sort_order = match column {
            SortColumn::UpdatedAt | SortColumn::MergedAt => SortOrder::Descending,
            _ => SortOrder::Ascending,
        };
        self.sort_mrs();
    }

    pub fn cycle_sort_column(&mut self) {
        self.sort_column = match self.sort_column {
            SortColumn::UpdatedAt => SortColumn::Id,
            SortColumn::Id => SortColumn::Milestone,
            SortColumn::Milestone => SortColumn::Title,
            SortColumn::Title => SortColumn::MergedAt,
            SortColumn::MergedAt => SortColumn::UpdatedAt,
        };
        // Reset to a sensible default order when switching columns.
        self.sort_order = match self.sort_column {
            SortColumn::UpdatedAt | SortColumn::MergedAt => SortOrder::Descending,
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
            // MRs without a milestone go last in ascending order.
            SortColumn::Milestone => sort_by_key_dir(&mut self.mrs, descending, |mr| {
                (
                    mr.milestone.is_none(),
                    mr.milestone.as_deref().map(str::to_lowercase),
                )
            }),
            SortColumn::Title => {
                sort_by_key_dir(&mut self.mrs, descending, |mr| mr.title.to_lowercase())
            }
            // By the instant, not the text: GitLab writes the offset of its server.
            // The MRs not merged go last, whatever the order.
            SortColumn::MergedAt => {
                let at = |mr: &TrackedMr| {
                    mr.merged_at
                        .as_deref()
                        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                        .map(|at| at.timestamp_millis())
                };
                self.mrs.sort_by_cached_key(|mr| match at(mr) {
                    Some(t) if descending => (false, -t),
                    Some(t) => (false, t),
                    None => (true, 0),
                });
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
