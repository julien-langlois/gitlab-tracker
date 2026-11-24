//! Loading and refreshing: the startup restore, fetch completion, refresh cycles,
//! discovery and the API call estimates.

use super::*;

/// A refresh cycle being timed (see `App::start_refresh_cycle`).
#[derive(Debug)]
pub(super) struct RefreshCycle {
    pub(super) started: std::time::Instant,
    pub(super) kind: &'static str,
    pub(super) mrs: usize,
    pub(super) estimated_gitlab_calls: usize,
}

impl App {
    /// Marks the fetch of MR `id` as finished (loaded or failed).
    pub fn complete_fetch(&mut self, id: &str) -> FetchCompletion {
        let notify_allowed = self.pending_initial_fetches.is_empty();
        let initial_sync_just_completed =
            self.pending_initial_fetches.remove(id) && self.pending_initial_fetches.is_empty();
        let from_refresh_cycle = self.pending_refresh_fetches.remove(id);
        if from_refresh_cycle && self.pending_refresh_fetches.is_empty() {
            if let Some(cycle) = self.refresh_cycle.take() {
                tracing::info!(
                    kind = cycle.kind,
                    mrs = cycle.mrs,
                    estimated_gitlab_calls = cycle.estimated_gitlab_calls,
                    elapsed_ms = cycle.started.elapsed().as_millis() as u64,
                    "Refresh cycle complete"
                );
            }
        }
        FetchCompletion {
            notify_allowed,
            initial_sync_just_completed,
            from_refresh_cycle,
        }
    }

    /// `completion` ended the startup load or a refresh cycle and nothing else is
    /// in flight: branch data is fresh, so dependent scans can run.
    pub(super) fn fetches_just_settled(&self, completion: FetchCompletion) -> bool {
        (completion.initial_sync_just_completed || completion.from_refresh_cycle)
            && self.pending_initial_fetches.is_empty()
            && self.pending_refresh_fetches.is_empty()
    }

    /// GitLab fetches or branch scans still in flight: a new refresh would only
    /// queue the same requests again on a server that is already slow to answer.
    pub fn refresh_in_flight(&self) -> bool {
        !self.pending_initial_fetches.is_empty()
            || !self.pending_refresh_fetches.is_empty()
            || self.pending_branch_scans > 0
    }

    /// Starts timing the refresh cycle just spawned (`kind`: "auto" or "manual").
    /// `complete_fetch` logs its duration once the last of its MRs is back, so a
    /// real cycle can be measured from the log (`RUST_LOG=info`).
    pub fn start_refresh_cycle(&mut self, kind: &'static str) {
        if self.pending_refresh_fetches.is_empty() {
            return;
        }
        self.refresh_cycle = Some(RefreshCycle {
            started: std::time::Instant::now(),
            kind,
            mrs: self.pending_refresh_fetches.len(),
            estimated_gitlab_calls: self.estimated_gitlab_calls,
        });
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

    /// Recomputes `estimated_gitlab_calls` and `estimated_tracker_calls` from the
    /// current MR list and tracker state.
    ///
    /// Must be called whenever the MR list changes (startup, add, remove) or just
    /// before a refresh cycle is triggered — this ensures the displayed counters are
    /// always up to date.
    ///
    /// # Breakdown
    /// For each MR that is *not* fully frozen (merged into all branches with a known SHA):
    ///   - GitLab calls: `TrackedMr::estimate()` (mirrors the fetch logic), plus once per
    ///     cycle the list reads (`MRS_PER_LIST` MRs each), the branch scans and the
    ///     discovery poll. The pipelines due count one GraphQL query per batch
    ///     (`gitlab_graphql::queries_for`) instead of their REST reads.
    ///   - Tracker calls: `estimate_calls_for_tickets()` over the distinct linked tickets
    ///     (read in bulk). New MRs (no linked ticket yet) cost 0 tracker calls — the
    ///     ticket is only discovered after `MrLoaded` fires and `detect_ticket_id` succeeds.
    ///   - `saved_*`: what reading each MR and ticket on its own would have added.
    ///
    /// The two counters are kept separate so the UI can display them independently.
    pub fn recompute_api_call_estimate(&mut self) {
        let mut gitlab_total: usize = 0;
        let mut gitlab_saved: usize = 0;
        let mut refreshed: usize = 0;
        let mut tickets = HashSet::new();
        let mut linked_mrs: usize = 0;
        // MRs whose pipelines are due, and what reading them over REST costs.
        let mut pipeline_mrs: usize = 0;
        let mut rest_pipelines: usize = 0;

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

            refreshed += 1;
            let has_merge_sha = mr.sha.is_some() || mr.state == GitlabMrState::Merged;
            let estimate = mr.estimate(&mr.state, has_merge_sha);
            gitlab_total += estimate.total();
            gitlab_saved += estimate.saved;
            if estimate.pipelines > 0 {
                pipeline_mrs += 1;
                rest_pipelines += estimate.pipelines;
            }

            // Tracker calls only for MRs that already have a linked ticket resolved.
            if let Some(ticket) = &mr.linked_ticket {
                tickets.insert(ticket.id.as_str());
                linked_mrs += 1;
            }
        }

        // Pipelines due are read together over GraphQL when the instance allows it.
        if let Some(queries) = crate::gitlab_graphql::queries_for(&self.base_url, pipeline_mrs) {
            gitlab_total = gitlab_total - rest_pipelines + queries;
            gitlab_saved += rest_pipelines.saturating_sub(queries);
        }
        // The pages read one by one before are now list reads.
        let lists = refreshed.div_ceil(crate::gitlab::MRS_PER_LIST);
        gitlab_total += lists;
        gitlab_saved = gitlab_saved.saturating_sub(lists);
        // After the cycle: one scan per branch some MR is missing from (a single
        // page once the scan is incremental), and the discovery poll.
        gitlab_total += self.branches_to_scan() + usize::from(self.discovery_enabled);

        let (tracker_total, tracker_saved) = match &self.tracker {
            Some(provider) => {
                let total = provider.estimate_calls_for_tickets(tickets.len());
                let one_by_one = linked_mrs * provider.estimate_calls_per_ticket();
                (total, one_by_one.saturating_sub(total))
            }
            None => (0, 0),
        };

        self.estimated_gitlab_calls = gitlab_total;
        self.estimated_tracker_calls = tracker_total;
        self.saved_gitlab_calls = gitlab_saved;
        self.saved_tracker_calls = tracker_saved;
    }

    /// Branches the next branch scan reads (see `spawn_branch_scans`): those some
    /// tracked merged MR is not on by SHA.
    pub(super) fn branches_to_scan(&self) -> usize {
        self.branches
            .iter()
            .filter(|branch| {
                self.mrs.iter().any(
                    |mr| matches!(&mr.status, MrStatus::MergedIn(found) if !found.contains(*branch)),
                )
            })
            .count()
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
        let mut batch = Vec::new();
        let mut settled = Vec::new();

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

            let mut data = saved.data;
            // State files written before `Option` was used stored display sentinels.
            data.assignee = crate::models::without_sentinel(data.assignee);
            data.milestone = crate::models::without_sentinel(data.milestone);
            let mr = TrackedMr {
                id: saved.id,
                status: initial_status,
                // Not persisted — re-fetched live on startup.
                mergeability: MergeabilityStatus::Unknown,
                recently_updated: false,
                flagged: saved.flagged,
                snooze: saved.snooze,
                // Restored so the tracker is not queried again on every restart.
                linked_ticket: saved.linked_ticket,
                data,
            };

            if mr.status == MrStatus::Loading {
                // Same cache hints as a refresh cycle: unchanged pipelines, diff stats
                // and notes are reused; the persisted state keeps the notes cache of
                // already-merged MRs valid.
                let cached = CachedMrData::from(&mr);
                // Count each pending fetch so we can suppress change notifications
                // until the initial sync is complete (avoids spurious toasts on launch).
                self.pending_initial_fetches.insert(mr.id.clone());
                self.startup_baseline.insert(
                    mr.id.clone(),
                    MrBaseline {
                        state: mr.state,
                        branches: saved.found_branches,
                        pipeline: mr.pipelines.first().map(|p| p.status),
                    },
                );
                batch.push((mr.id.clone(), cached));
            } else {
                // Not read again, but changes made since (milestone…) are looked for.
                settled.push(crate::gitlab::SettledMr::from(&mr));
            }
            self.mrs.push(mr);
        }
        crate::gitlab::spawn_settled_check(ctx.clone(), settled, semaphore.clone(), tx.clone());
        crate::gitlab::spawn_mr_fetches(ctx, batch, semaphore, tx);

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
}
