//! The event dispatcher: every state change goes through `App::apply_event`.

use super::*;

impl App {
    /// Applies a single `AppEvent` to the application state.
    ///
    /// This is the central event dispatch extracted from `main.rs` to keep the
    /// event loop thin. Returns `true` if the state was mutated in a way that
    /// requires persisting (caller must then call `save_state_async`); see
    /// [`AppEvent::persists_state`].
    pub fn apply_event(
        &mut self,
        event: AppEvent,
        semaphore: Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
        last_known_branches: &mut HashMap<String, HashSet<String>>,
    ) -> bool {
        let needs_persist = event.persists_state();

        match event {
            // ── Tracker ticket resolved ───────────────────────────────────────
            AppEvent::TrackerTicketLoaded { mr_id, ticket } => {
                if let Some(mr) = self.mrs.find_mut(&mr_id) {
                    // Compute the diff between the cached ticket and the freshly fetched one.
                    // The diff logic lives entirely in `core` (LinkedTicket::diff) — this site
                    // only dispatches the resulting changes to the notification layer.
                    // Adding a new tracked field only touches `core::LinkedTicket::diff`.
                    if let Some(old) = &mr.linked_ticket {
                        let mr_title = mr.title.clone();
                        let ticket_url = ticket.url.clone();
                        let ticket_id = ticket.id.clone();

                        for change in old.diff(&ticket) {
                            let (old_val, new_val) = (change.old.as_str(), change.new.as_str());
                            tracing::info!(
                                ticket_id = %ticket_id,
                                field = %change.field,
                                old = %old_val,
                                new = %new_val,
                                "Tracker ticket field changed",
                            );
                            notify::ticket_field_changed(
                                &ticket_id,
                                &mr_title,
                                change.field,
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
                match activities {
                    Ok(activities) => {
                        self.activities = activities;
                        self.activities_error = None;
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Tracker activities fetch failed");
                        self.activities_error = Some(error.to_string());
                    }
                }
                false
            }

            // ── Time entries loaded for a ticket ─────────────────────────────
            AppEvent::TimeEntriesLoaded { ticket_id, entries } => {
                let state = match entries {
                    Ok(entries) => TimeLogState::Loaded(entries),
                    Err(error) => {
                        tracing::warn!(ticket_id = %ticket_id, error = %error, "Time entries fetch failed");
                        TimeLogState::Failed(error.to_string())
                    }
                };
                self.time_entries.insert(ticket_id, state);
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
                    self.time_entries.insert(tid.clone(), TimeLogState::Loading);
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
                        if let Ok(ticket) = ticket {
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
                self.log_time_form.error = Some(error.to_string());
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
                    None,
                ));
                // The selection indexes the visible (filtered) list, not `mrs`.
                let pos = self.visible_mrs().position(|m| m.id == id);
                if pos.is_some() {
                    self.table_state.select(pos);
                    self.selection_moved = true;
                }
                spawn_mr_fetch(
                    self.fetch_context(),
                    id,
                    CachedMrData::default(),
                    semaphore.clone(),
                    tx.clone(),
                );
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
                let fetches_settled = self.fetches_just_settled(completion);
                // Loads after the startup sync refresh the stats report (cockpit Flow
                // counters) whenever they record something new — a merge seen by a
                // refresh cycle or an MR discovered already merged.
                #[cfg(feature = "stats")]
                let refresh_report = if completion.initial_sync_just_completed {
                    stats_recorder::ReportRefresh::Always
                } else if completion.notify_allowed {
                    stats_recorder::ReportRefresh::IfWritten
                } else {
                    stats_recorder::ReportRefresh::Never
                };

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
                    if !self.just_added_branches.contains(b) {
                        notify::mr_on_new_branch(&data.id, &data.data.title, b, &data.data.web_url);
                    }
                }
                // Branches reached since the last load (never on first sighting: when it
                // got there is unknown), for the promotion lead time.
                #[cfg(feature = "stats")]
                let reached = reached_branches(
                    last_known_branches.get(&data.id),
                    &data.branches,
                    &data.data.target_branch,
                    &self.just_added_branches,
                );

                // Update the persisted reference so subsequent refreshes won't re-notify.
                last_known_branches.insert(data.id.clone(), data.branches.clone());

                // Detect whether this MR was actually updated since the last refresh.
                // We compare the old `updated_at` before overwriting it.
                let was_updated = mr.updated_at.is_some() && mr.updated_at != data.data.updated_at;
                let previous_state = mr.state;
                let previous_ticket = mr.linked_ticket.clone();

                mr_changes::log_and_notify_changes(
                    mr,
                    &data,
                    was_updated,
                    notify_allowed,
                    &self.config.complexity_profile,
                );

                mr.status = MrStatus::MergedIn(data.branches);
                mr.mergeability = data.mergeability;
                mr.recently_updated = was_updated;

                // All GitLab fields at once (title, branches, pipelines, diff stats…).
                // The signatures are read apart from the MR: keep what was known.
                let signatures = mr.data.signatures.take();
                mr.data = data.data;
                mr.data.signatures = signatures;

                if let Some(username) = self.config.gitlab_username.as_deref() {
                    mr_changes::remind_review(
                        mr,
                        username,
                        self.config.review_reminder_hours,
                        notify_allowed,
                        &mut self.review_reminders,
                    );
                }

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

                let mr_id = mr.id.clone();

                // Arm (or re-arm) the global fade countdown.
                if was_updated {
                    self.update_highlight_ticks = RECENT_UPDATE_FADE_TICKS;
                }
                self.sync_linked_ticket(&mr_id, was_updated, completion.from_refresh_cycle, tx);
                if fetches_settled {
                    self.on_fetches_settled(completion.initial_sync_just_completed, &semaphore, tx);
                }
                // Sorting is deferred to the end of the event drain (see
                // `flush_pending_sort`) so a burst of N loads costs one sort, not N.
                self.needs_sort = true;
                #[cfg(feature = "stats")]
                self.record_stats_snapshot(&mr_id, refresh_report, tx);
                #[cfg(feature = "stats")]
                self.record_sha_arrivals(&mr_id, reached, tx);
                self.release_added_branches();

                // MrLoaded persists state (see `AppEvent::persists_state`).
                needs_persist
            }

            AppEvent::MrFailed { id, error } => {
                // A failed fetch is finished too — otherwise the spinner never stops.
                let completion = self.complete_fetch(&id);
                if let Some(mr) = self.mrs.find_mut(&id) {
                    mr.status = MrStatus::Error(error.to_string());
                }
                self.release_added_branches();
                if self.fetches_just_settled(completion) {
                    self.on_fetches_settled(completion.initial_sync_just_completed, &semaphore, tx);
                }
                // Not persisted: errors are not saved to disk (see `persists_state`).
                needs_persist
            }

            // ── Stats async results ───────────────────────────────────────────
            #[cfg(feature = "stats")]
            AppEvent::StatsReportLoaded { generation, result } => {
                self.stats_view.apply_result(generation, result);
                false
            }

            #[cfg(feature = "stats")]
            AppEvent::StatsCockpitLoaded { generation, result } => {
                self.stats_view.apply_cockpit_result(generation, result);
                false
            }

            #[cfg(feature = "stats")]
            AppEvent::StatsSnapshotFailed { mr_id } => {
                self.stats_last_refresh_date.remove(&mr_id);
                false
            }

            #[cfg(feature = "stats")]
            AppEvent::BranchArrivalsLoaded(arrivals) => {
                self.branch_arrivals = arrivals;
                false
            }

            #[cfg(feature = "stats")]
            AppEvent::StatsSnapshotRecorded => {
                self.stats_report_dirty = true;
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
                        Some(milestone_title.clone()),
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
                    self.selection_moved = true;
                    true
                } else {
                    false
                }
            }

            AppEvent::SignaturesChecked { mr_id, data } => {
                self.signature_checks.remove(&mr_id);
                match self.mrs.find_mut(&mr_id) {
                    Some(mr) => {
                        mr.data.signatures = Some(*data);
                        true
                    }
                    None => false,
                }
            }

            AppEvent::BranchCommitsLoaded {
                branch,
                commits,
                scan,
                head,
            } => {
                self.pending_branch_scans = self.pending_branch_scans.saturating_sub(1);
                let Some(index) = self.scanned_index(&branch, commits, scan, head) else {
                    return false; // failed scan: keep the previous index
                };
                // The first scan of a branch in a session only records: restarting must
                // not toast every copy already there.
                if let Some(before) = self.branch_commits.get(&branch) {
                    for (mr, how) in newly_copied(&self.mrs, &branch, before, &index) {
                        notify::mr_copied_to_branch(&mr.id, &mr.title, &branch, how, &mr.web_url);
                    }
                }
                #[cfg(feature = "stats")]
                self.record_copy_arrivals(&branch, &index, tx);
                self.branch_commits.insert(branch, index);
                false
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
                        None,
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

            AppEvent::SettledMrsChanged(ids) => {
                let ctx = self.fetch_context();
                let mut batch = Vec::new();
                for mr in &mut self.mrs {
                    // Already being read (a manual refresh since the check): skip.
                    if !ids.contains(&mr.id)
                        || self.pending_refresh_fetches.contains(&mr.id)
                        || self.pending_initial_fetches.contains(&mr.id)
                    {
                        continue;
                    }
                    mr.status = MrStatus::Loading;
                    batch.push((mr.id.clone(), CachedMrData::from(&*mr)));
                    self.pending_refresh_fetches.insert(mr.id.clone());
                }
                tracing::info!(
                    mrs = batch.len(),
                    "Settled MRs changed on GitLab — refreshing"
                );
                crate::gitlab::spawn_mr_fetches(ctx, batch, semaphore.clone(), tx.clone());
                false
            }

            AppEvent::SentinelChecked(report) => {
                self.on_sentinel_checked(report, &semaphore, tx);
                false
            }

            AppEvent::Tick => self.on_tick(&semaphore, tx),
        }
    }
}
