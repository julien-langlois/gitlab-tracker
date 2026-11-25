//! The visible list: filters, the filter picker, search ranking, status picks and
//! milestone suggestions.

use super::*;

impl App {
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
            suggestion: None,
        };
        self.input_mode = InputMode::FilterPicker;
    }

    /// Label autocompletion for the filter picker, empty unless its cursor is on
    /// the "Label" filter: every label of the tracked MRs, plus each scope they
    /// use (`review::` for `review::ongoing`), sorted, whose normalized form
    /// contains the typed text.
    pub fn label_suggestions(&self) -> Vec<String> {
        use crate::filters_core::normalize_label;

        let on_label_filter = self
            .visible_filter_defs()
            .get(self.filter_picker.cursor)
            .is_some_and(|(_, def)| def.id == "label");
        if !on_label_filter {
            return Vec::new();
        }
        // Keyed by normalized form: one entry per label whatever its casing,
        // shown as first seen.
        let mut entries: std::collections::BTreeMap<String, String> = Default::default();
        for label in self.mrs.iter().flat_map(|mr| &mr.labels) {
            let label = label.trim();
            if label.is_empty() {
                continue;
            }
            let scopes = label
                .match_indices("::")
                .map(|(at, sep)| &label[..at + sep.len()]);
            for entry in scopes.chain([label]) {
                entries
                    .entry(normalize_label(entry))
                    .or_insert_with(|| entry.to_string());
            }
        }
        let query = normalize_label(&self.filter_picker.input);
        entries
            .into_iter()
            .filter(|(key, _)| key.contains(&query))
            .map(|(_, label)| label)
            .collect()
    }

    /// Moves the highlighted label suggestion forward (or back), wrapping around;
    /// the first press highlights the first (or last) one.
    pub fn cycle_label_suggestion(&mut self, forward: bool) {
        let len = self.label_suggestions().len();
        if len == 0 {
            return;
        }
        self.filter_picker.suggestion = Some(match (self.filter_picker.suggestion, forward) {
            (None, true) => 0,
            (None, false) => len - 1,
            (Some(i), true) => (i + 1) % len,
            (Some(i), false) => (i + len - 1) % len,
        });
    }

    /// Applies the filter picker selection and closes the popup.
    ///
    /// The picker cursor is a position in the **visible** filter list.
    /// We resolve it back to the full `filter_defs` index before storing it in
    /// `active_filter`, so `apply_filter` always finds the correct predicate.
    pub fn apply_filter_picker(&mut self) {
        // A suggestion picked with Tab replaces the typed text.
        let picked = self
            .filter_picker
            .suggestion
            .and_then(|i| self.label_suggestions().get(i).cloned());
        if let Some(label) = picked {
            self.filter_picker.input = label;
        }
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

        self.input_mode = InputMode::Normal;
        self.set_filter(final_idx, final_query);
    }

    /// Activates the filter at `index` in `filter_defs` (with `query` for the
    /// filters that take one) and selects the first MR it shows.
    pub fn set_filter(&mut self, index: usize, query: String) {
        self.active_filter = ActiveFilter { index, query };
        // Reset selection so we never point past the end of the filtered list.
        if self.visible_mrs().next().is_some() {
            self.table_state.select(Some(0));
        } else {
            self.table_state.select(None);
        }
        self.layout.inspector.reset();
    }

    /// Sets the filters on top of the active one (the window's sidebar), and
    /// selects the first MR left.
    pub fn set_filters(&mut self, filters: Vec<ActiveFilter>) {
        self.filters = filters;
        self.visible_cache = None;
        if self.visible_mrs().next().is_some() {
            self.table_state.select(Some(0));
        } else {
            self.table_state.select(None);
        }
        self.layout.inspector.reset();
    }

    /// Shows only the MRs with the status `pick` (or all of them again with
    /// `None`), on top of the filter, the focus and the search; selects the first.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn set_status_pick(&mut self, pick: Option<crate::status::StatusKind>) {
        self.status_pick = pick;
        self.visible_cache = None;
        let any = self.visible_mrs().next().is_some();
        self.table_state.select(any.then_some(0));
        self.layout.inspector.reset();
    }

    /// How many MRs of the list have each status, in the status bar's order,
    /// the statuses no MR has left out. Counted on the list as the filter, the
    /// focus and the search leave it, whatever status is picked.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn status_counts(&self) -> Vec<(crate::status::StatusKind, usize)> {
        let mut counts = std::collections::BTreeMap::new();
        for mr in self.mrs.iter().filter(|mr| self.list_score(mr).is_some()) {
            *counts
                .entry(crate::status::StatusKind::of(mr.state, &mr.mergeability))
                .or_insert(0) += 1;
        }
        counts.into_iter().collect()
    }

    /// The MR's search relevance when it passes the filter, the focus and the
    /// search (not the status picked); `None` when it does not.
    pub(super) fn list_score(&self, mr: &TrackedMr) -> Option<f64> {
        let in_focus = match &self.focus {
            Some(focus) if !self.focus_suspended => focus.ids.contains(&mr.id),
            _ => true,
        };
        if !in_focus || !self.filter_passes(mr) {
            return None;
        }
        self.search_score(mr)
    }

    /// The MR's fuzzy relevance to the search typed (`Some(0.0)` with no search,
    /// `None` when it does not match).
    pub(super) fn search_score(&self, mr: &TrackedMr) -> Option<f64> {
        use crate::utils::fuzzy_score;
        if self.input.is_empty() || self.input.starts_with('@') {
            return Some(0.0);
        }
        let q = &self.input;
        // Score against the most user-visible fields — highest wins.
        [
            fuzzy_score(q, &mr.title),
            fuzzy_score(q, &mr.author),
            mr.assignee.as_deref().and_then(|a| fuzzy_score(q, a)),
            fuzzy_score(q, &mr.id),
        ]
        .into_iter()
        .flatten()
        .reduce(f64::max)
    }

    /// Shows only the MRs of `focus` (or every MR again with `None`), on top of
    /// the active filter, and selects the first one.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn set_focus(&mut self, focus: Option<Focus>) {
        self.focus = focus;
        self.visible_cache = None;
        let any = self.visible_mrs().next().is_some();
        self.table_state.select(any.then_some(0));
        self.layout.inspector.reset();
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
    pub(crate) fn compute_visible_indices(&self) -> Vec<usize> {
        // Only rank by relevance when the user has typed a plain search query
        // (not a `@milestone` autocomplete prefix).
        let query = (!self.input.is_empty() && !self.input.starts_with('@')).then_some(());
        let kept = |mr: &TrackedMr| self.kept_visible.as_deref() == Some(mr.id.as_str());
        let mut scored: Vec<(f64, usize)> = self
            .mrs
            .iter()
            .enumerate()
            .filter_map(|(i, mr)| {
                let picked = self.status_pick.is_none_or(|pick| {
                    crate::status::StatusKind::of(mr.state, &mr.mergeability) == pick
                });
                let score = picked.then(|| self.list_score(mr)).flatten();
                // The kept MR stays where it was, last when ranked by a search.
                score
                    .or_else(|| kept(mr).then_some(f64::MIN))
                    .map(|score| (score, i))
            })
            .collect();

        // Sort by descending relevance only when a query is active.
        if query.is_some() {
            scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        } else if let Some(username) = self.review_queue_user() {
            // "To review: me": oldest review request first (stable for ties).
            scored.sort_by_key(|(_, i)| {
                let mr = &self.mrs[*i];
                crate::utils::review_waiting_since(
                    mr.review.as_ref(),
                    mr.created_at.as_deref(),
                    username,
                )
                .map_or(i64::MAX, |at| at.timestamp())
            });
        }

        scored.into_iter().map(|(_, i)| i).collect()
    }

    /// The configured username while the "To review: me" filter is active.
    pub(super) fn review_queue_user(&self) -> Option<&str> {
        let reviewer_me = |f: &ActiveFilter| {
            self.filter_defs
                .get(f.index)
                .is_some_and(|d| d.id == "reviewer_me")
        };
        (reviewer_me(&self.active_filter) || self.filters.iter().any(reviewer_me))
            .then_some(self.config.gitlab_username.as_deref())
            .flatten()
    }

    /// Returns `true` when `mr` passes the currently active filter and is not
    /// hidden by the "hide merged" toggle.
    ///
    /// Builds a [`MrSnapshot`] and delegates to the registered `FilterDef::apply`
    /// function — no match arm needed when a new filter is added.
    pub(super) fn filter_passes(&self, mr: &TrackedMr) -> bool {
        // A filter of merged MRs, as the view or on top of it, shows them even
        // with "hide merged" on: it was picked to see them.
        let asks_merged = std::iter::once(&self.active_filter)
            .chain(&self.filters)
            .any(|f| self.lists_merged(f.index));
        if self.config.hide_merged && mr.state == GitlabMrState::Merged && !asks_merged {
            return false;
        }
        self.applies(mr, self.active_filter.index, &self.active_filter.query)
            && self
                .filters
                .iter()
                .all(|f| self.applies(mr, f.index, &f.query))
    }

    /// Whether the filter at `index` lists merged MRs only ("State: Merged",
    /// "Merged by me", "Merged between"): "hide merged" does not apply to it.
    pub(super) fn lists_merged(&self, index: usize) -> bool {
        self.filter_defs
            .get(index)
            .is_some_and(|d| matches!(d.id, "state_merged" | "merged_by_me" | "merged_between"))
    }

    /// How many tracked MRs the filter at `index` (in `filter_defs`) shows without a
    /// query, "hide merged" included: the counts next to the views in the window.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn filter_count(&self, index: usize) -> usize {
        self.mrs
            .iter()
            .filter(|mr| self.passes(mr, index, ""))
            .count()
    }

    /// Whether `mr` passes the filter at `index` with `query`, and is not hidden
    /// by the "hide merged" toggle.
    pub(super) fn passes(&self, mr: &TrackedMr, index: usize, query: &str) -> bool {
        if self.config.hide_merged && mr.state == GitlabMrState::Merged && !self.lists_merged(index)
        {
            return false;
        }
        self.applies(mr, index, query)
    }

    /// Whether `mr` passes the filter at `index` with `query`, "hide merged" aside.
    pub(super) fn applies(&self, mr: &TrackedMr, index: usize, query: &str) -> bool {
        let Some(def) = self.filter_defs.get(index) else {
            return true; // Unknown index → show all.
        };
        // The effort filters only need relative bands (< 0.33 / >= 0.66) of the
        // score computed with the project's complexity profile.
        let diff_difficulty = mr
            .diff_stats
            .as_ref()
            .map(|s| s.difficulty(&self.config.complexity_profile));
        let absent_branches: Vec<&str> = self
            .branches
            .iter()
            .filter(|b| {
                self.branch_presence(mr, b)
                    .is_some_and(BranchPresence::is_missing)
            })
            .map(String::as_str)
            .collect();
        let mut snapshot = MrSnapshot::new(mr.state, mr.mergeability, &mr.target_branch);
        snapshot.flagged = mr.flagged;
        snapshot.user_notes_count = mr.user_notes_count;
        snapshot.milestone = mr.milestone.as_deref();
        snapshot.assignee = mr.assignee.as_deref();
        snapshot.linked_ticket = mr.linked_ticket.as_ref();
        snapshot.pipeline_status = mr.pipelines.first().map(|p| p.status);
        snapshot.reviewers = &mr.reviewers;
        snapshot.gitlab_username = self.config.gitlab_username.as_deref();
        snapshot.diff_difficulty = diff_difficulty;
        snapshot.absent_branches = &absent_branches;
        snapshot.approved_by = &mr.approved_by;
        snapshot.merged_by = mr.merged_by.as_deref();
        snapshot.merged_at = mr.merged_at.as_deref();
        snapshot.labels = &mr.labels;
        (def.apply)(snapshot, query)
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
        self.filter_defs
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, def)| def.requires.is_none_or(|r| self.requirement_met(r)))
            .collect()
    }

    /// Columns offered in the column picker: those whose requirement is met.
    pub fn visible_column_defs(&self) -> Vec<&'static ColumnDef> {
        self.column_defs
            .iter()
            .copied()
            .filter(|c| c.requires.is_none_or(|r| self.requirement_met(r)))
            .collect()
    }

    /// Whether an optional column / filter can be offered in this session.
    pub fn requirement_met(&self, requirement: gitlab_tracker_core::Requirement) -> bool {
        match requirement {
            gitlab_tracker_core::Requirement::Tracker => self.tracker.is_some(),
            gitlab_tracker_core::Requirement::GitlabUsername => {
                self.config.gitlab_username.is_some()
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
}
