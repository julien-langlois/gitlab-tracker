//! The 1 s tick: countdowns, the sentinel, flash messages.

use super::*;

/// Ticks (~seconds) a [`Flash`] message stays in the status bar.
pub(super) const FLASH_TICKS: u8 = 4;
/// The "since last session" summary is longer to read: it stays longer.
pub(super) const DIGEST_TICKS: u8 = 20;

impl App {
    /// Shows `text` in the status bar for a few seconds (red when `is_error`).
    pub fn flash(&mut self, text: impl Into<String>, is_error: bool) {
        self.flash_for(text, is_error, FLASH_TICKS);
    }

    pub(super) fn flash_for(&mut self, text: impl Into<String>, is_error: bool, ticks: u8) {
        self.flash = Some(Flash {
            text: text.into(),
            is_error,
            ticks_left: ticks,
        });
    }

    /// The startup load or a refresh cycle just finished: scan for cherry-picks and,
    /// after the startup load, summarise what changed while the app was closed.
    pub(super) fn on_fetches_settled(
        &mut self,
        initial_sync: bool,
        semaphore: &Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
    ) {
        self.spawn_branch_scans(semaphore, tx);
        self.spawn_signature_checks(semaphore, tx);
        if initial_sync {
            let baseline = std::mem::take(&mut self.startup_baseline);
            self.last_changes = session_changes(&baseline, &self.mrs);
            if let Some(digest) = session_digest(&baseline, &self.mrs) {
                self.last_digest = Some(digest.clone());
                self.flash_for(digest, false, DIGEST_TICKS);
            }
        }
    }

    /// Ages the flash message by one tick, clearing it once expired.
    pub fn tick_flash(&mut self) {
        if let Some(flash) = &mut self.flash {
            flash.ticks_left = flash.ticks_left.saturating_sub(1);
            if flash.ticks_left == 0 {
                self.flash = None;
            }
        }
    }

    /// One-second tick: countdowns, debounced stats refresh, and — when the refresh
    /// timer elapses — a full refresh cycle (discovery, MR and ticket re-fetches).
    /// Returns whether state must be persisted.
    pub(super) fn on_tick(
        &mut self,
        semaphore: &Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
    ) -> bool {
        #[cfg(feature = "stats")]
        {
            // Midnight (local): "merged today" and the other day-relative figures
            // are stale even though nothing was recorded.
            let today = chrono::Local::now().date_naive();
            if self.stats_view.day_changed(today) {
                self.stats_report_dirty = true;
            }
            if std::mem::take(&mut self.stats_report_dirty) {
                crate::ui::stats::trigger_background_stats_refresh(self, tx);
            }
        }

        self.tick_flash();

        // Decrement the highlight fade countdown and clear flags when expired.
        if self.update_highlight_ticks > 0 {
            self.update_highlight_ticks -= 1;
            if self.update_highlight_ticks == 0 {
                for mr in &mut self.mrs {
                    mr.recently_updated = false;
                }
            }
        }

        self.tick_sentinel(semaphore, tx);

        if self.time_left > 0 {
            self.time_left -= 1;
            return false;
        }

        // Timer elapsed — trigger a full refresh of all MRs.
        self.time_left = self.refresh_interval_secs;
        // The cycle reads everything: no check needed right after it.
        self.sentinel_left = self.sentinel_interval_secs;
        if self.refresh_in_flight() {
            // A slow server must not get a second cycle stacked on the first one.
            tracing::warn!("Previous refresh still running — skipping this cycle");
            return false;
        }
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

        let mut ticket_links = Vec::new();
        let mut batch = Vec::new();
        let mut settled = Vec::new();
        for mr in &mut self.mrs {
            if let MrStatus::MergedIn(ref found) = mr.status {
                // MRs fully merged into all branches are only checked for changes
                // (milestone, title…) in one list read — state != Opened ensures
                // we don't skip still-open MRs.
                if self.branches.iter().all(|b| found.contains(b))
                    && mr.sha.is_some()
                    && mr.state != GitlabMrState::Opened
                {
                    settled.push(crate::gitlab::SettledMr::from(&*mr));
                    continue;
                }
            }

            mr.status = MrStatus::Loading;
            batch.push((mr.id.clone(), CachedMrData::from(&*mr)));
            // Track pending auto-refresh fetches to drive the spinner.
            self.pending_refresh_fetches.insert(mr.id.clone());

            // Re-read the linked ticket on each auto-refresh cycle, mirroring the manual
            // [R] refresh: the MR may not have changed while the ticket's status, spent
            // time or priority did. All of them are read together after the loop.
            if let Some(ticket) = &mr.linked_ticket {
                ticket_links.push((ticket.id.clone(), mr.id.clone()));
            }
        }
        crate::gitlab::spawn_settled_check(ctx.clone(), settled, semaphore.clone(), tx.clone());
        crate::gitlab::spawn_mr_fetches(ctx, batch, semaphore.clone(), tx.clone());
        if let Some(provider) = self.tracker.as_ref().map(Arc::clone) {
            spawn_tickets_fetch(provider, ticket_links, tx);
        }
        self.start_refresh_cycle("auto");

        false
    }

    /// Counts down to the next sentinel check and starts it. Skipped while a
    /// refresh runs (it reads everything anyway) or is about to; after a long
    /// gap (sleep, network down), a full cycle runs instead.
    pub(super) fn tick_sentinel(
        &mut self,
        semaphore: &Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
    ) {
        if self.sentinel_interval_secs == 0 || self.read_only {
            return;
        }
        if self.sentinel_left > 0 {
            self.sentinel_left -= 1;
            return;
        }
        self.sentinel_left = self.sentinel_interval_secs;
        if self.sentinel.in_flight
            || self.time_left <= 1
            || self.refresh_in_flight()
            || !self.pending_initial_fetches.is_empty()
        {
            return;
        }
        let server_now = self.sentinel.server_now(chrono::Utc::now());
        let max_gap = crate::sentinel::MAX_GAP.max(chrono::TimeDelta::seconds(
            3 * self.sentinel_interval_secs as i64,
        ));
        if server_now - self.sentinel.since > max_gap {
            tracing::info!("Sentinel watermark too old — running a full refresh instead");
            self.sentinel.since = server_now - crate::sentinel::OVERLAP;
            self.time_left = 0;
            return;
        }
        self.sentinel.in_flight = true;
        crate::sentinel::spawn_check(
            self.fetch_context(),
            self.sentinel.since,
            semaphore.clone(),
            tx.clone(),
        );
    }

    /// Reads again what a sentinel check found changed: the MRs (with their
    /// linked tickets), the branch scans, the discovery poll.
    pub(super) fn on_sentinel_checked(
        &mut self,
        report: Option<Box<crate::sentinel::SentinelReport>>,
        semaphore: &Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
    ) {
        self.sentinel.in_flight = false;
        let Some(report) = report else {
            return;
        };
        let plan = crate::sentinel::plan(
            &report,
            &self.mrs,
            &self.dismissed_mr_ids,
            &self.branches,
            &self.sentinel.seen_events,
        );
        self.sentinel.advance(&report);
        if report.truncated {
            // Too much changed for a light check: read everything now.
            self.time_left = 0;
        }

        let ctx = self.fetch_context();
        let mut batch = Vec::new();
        let mut ticket_links = Vec::new();
        for mr in &mut self.mrs {
            let Some(force) = plan.refresh.get(&mr.id) else {
                continue;
            };
            // Already being read (a refresh started since the check): skip.
            if self.pending_refresh_fetches.contains(&mr.id)
                || self.pending_initial_fetches.contains(&mr.id)
            {
                continue;
            }
            mr.status = MrStatus::Loading;
            let mut cached = CachedMrData::from(&*mr);
            cached.cache_policy = force.policy();
            batch.push((mr.id.clone(), cached));
            self.pending_refresh_fetches.insert(mr.id.clone());
            if let Some(ticket) = &mr.linked_ticket {
                ticket_links.push((ticket.id.clone(), mr.id.clone()));
            }
        }
        tracing::info!(
            mrs = batch.len(),
            scan_branches = plan.scan_branches,
            untracked = plan.untracked,
            events = report.events.len(),
            "Sentinel check"
        );
        let scan_now = plan.scan_branches && batch.is_empty() && self.pending_branch_scans == 0;
        if !batch.is_empty() {
            crate::gitlab::spawn_mr_fetches(ctx.clone(), batch, semaphore.clone(), tx.clone());
            if self.refresh_cycle.is_none() {
                self.start_refresh_cycle("sentinel");
            }
        }
        if let Some(provider) = self.tracker.as_ref().map(Arc::clone) {
            if !ticket_links.is_empty() {
                spawn_tickets_fetch(provider, ticket_links, tx);
            }
        }
        // With MRs in flight, the scans run once they are back (`on_fetches_settled`).
        if scan_now {
            self.spawn_branch_scans(semaphore, tx);
        }
        if plan.untracked && self.discovery_enabled {
            self.spawn_discovery(ctx, tx);
        }
    }
}
