//! The events the app's tasks send to its event loop (fetches done, timers,
//! user actions finished in the background): an MR loaded, a sentinel check, a
//! stats report… Applied one by one by `App::apply_event`.

use gitlab_tracker_gitlab::models::*;

pub enum AppEvent {
    MrLoaded(Box<MrLoadedData>),
    MrFailed {
        id: String,
        error: crate::gitlab::GitlabError,
    },
    /// Fired while GitLab is still computing mergeability and the fetcher is retrying.
    MrMergeabilityRetrying {
        id: String,
    },
    /// Fired when the user requests adding a new MR to the tracking list (by ID).
    /// `apply_event` is the single place that pushes to `app.mrs` and recomputes
    /// the API call estimate — `events.rs` must never mutate `app.mrs` directly.
    MrAdded(String),
    /// Fired when the user removes a MR: Delete key on the selected row, or `-<id>`
    /// typed in the input field.
    MrRemovedById(String),
    /// Fired when the project label list (with colours) has been fetched from GitLab.
    GitlabLabelsLoaded(Vec<GitLabLabelDetail>),
    /// Fired when the milestone list has been fetched from GitLab.
    MilestonesLoaded(Vec<GitLabMilestone>),
    /// Fired when the MR IDs linked to a milestone have been resolved.
    MilestoneMrsLoaded {
        milestone_title: String,
        mr_ids: Vec<String>,
    },
    /// Fired when a tracker plugin has resolved the ticket linked to a MR.
    /// Emitted by any active tracker provider (Redmine, Jira, Trello, …).
    /// `ticket` is boxed to keep the enum variant size in check (Clippy `large_enum_variant`).
    TrackerTicketLoaded {
        mr_id: String,
        ticket: Box<gitlab_tracker_core::LinkedTicket>,
    },
    /// Fired when the list of time-tracking activity categories has been fetched.
    /// Stored in `App` for use in the Log Time popup selector.
    /// `Err` is shown in the Log Time popup.
    ActivitiesLoaded(Result<Vec<gitlab_tracker_core::Activity>, gitlab_tracker_core::TrackerError>),
    /// Fired when time entries for a ticket have been fetched from the tracker.
    /// Keyed by ticket id so a late response can never land on another ticket.
    TimeEntriesLoaded {
        ticket_id: String,
        /// `Err` is shown in the TimeLog view.
        entries: Result<Vec<gitlab_tracker_core::TimeEntry>, gitlab_tracker_core::TrackerError>,
    },
    /// Fired when a time entry has been successfully submitted to the tracker.
    /// Carries both the MR id (to update the right `linked_ticket` in memory) and the
    /// raw ticket id (to re-fetch time entries and the updated spent total).
    TimeLogSubmitted {
        /// The GitLab MR id that owns the ticket (used to route `TrackerTicketLoaded`).
        mr_id: String,
        /// The tracker ticket id that received the new time entry.
        ticket_id: String,
    },
    /// Fired when a time entry submission failed.
    /// The error is shown inline in the popup.
    TimeLogFailed {
        error: gitlab_tracker_core::TrackerError,
    },
    /// Fired when an async stats aggregation finishes. `generation` identifies the
    /// request (see `StatsViewState::apply_result`); the report is boxed to keep the
    /// enum variant size in check (Clippy `large_enum_variant`).
    #[cfg(feature = "stats")]
    StatsReportLoaded {
        generation: u64,
        result: Result<Box<gitlab_tracker_stats::StatReport>, gitlab_tracker_stats::StatsError>,
    },
    /// Fired when the aggregation behind the cockpit (whole retained history)
    /// finishes; see `StatsViewState::apply_cockpit_result`.
    #[cfg(feature = "stats")]
    StatsCockpitLoaded {
        generation: u64,
        result:
            Result<Box<gitlab_tracker_stats::AggregatedStats>, gitlab_tracker_stats::StatsError>,
    },
    /// Fired when the daily snapshot of an open MR could not be written, so the
    /// next load of that MR tries again instead of waiting for tomorrow.
    #[cfg(feature = "stats")]
    StatsSnapshotFailed {
        mr_id: String,
    },
    /// The first arrival of each MR on each tracked branch, read from the stats DB.
    #[cfg(feature = "stats")]
    BranchArrivalsLoaded(
        std::collections::HashMap<(String, String), chrono::DateTime<chrono::Utc>>,
    ),
    /// Fired after a snapshot was written to the stats DB; marks the report stale.
    #[cfg(feature = "stats")]
    StatsSnapshotRecorded,
    /// Fired when an MR discovery poll completes: `mr_ids` are the IIDs not yet in the
    /// tracking list (possibly none), `next_anchor` the advanced `created_after` anchor.
    /// Only emitted when `discover_new_mrs = true` in `[project.stats]`.
    NewMrsDiscovered {
        mr_ids: Vec<String>,
        next_anchor: String,
    },
    /// Settled MRs (merged on every tracked branch, left out of refresh cycles)
    /// that changed on GitLab since their last read: to refresh.
    SettledMrsChanged(Vec<String>),
    /// A sentinel check came back (`None`: a read failed), see `crate::sentinel`.
    SentinelChecked(Option<Box<crate::sentinel::SentinelReport>>),
    /// Recent commits of `branch`, scanned to recognise MRs copied there under
    /// other SHAs (`None`: the scan failed, the previous result is kept).
    /// What was read to check an MR's commit signatures
    /// (`gitlab_signatures::spawn_signature_check`).
    SignaturesChecked {
        mr_id: String,
        data: Box<crate::signatures::SignatureData>,
    },
    BranchCommitsLoaded {
        branch: String,
        commits: Option<BranchCommits>,
        /// What was read: the index to keep (`Full`) or commits to add (`Range`).
        scan: crate::gitlab::BranchScan,
        /// The branch head after this scan (the next scan reads from it).
        head: Option<String>,
    },
    Tick,
}

impl AppEvent {
    /// Whether applying this event changes the tracked-MR list or its data, so the
    /// state file must be rewritten (once per drained batch, see the main loop).
    /// A failed fetch does not persist: errors are not saved to disk.
    pub fn persists_state(&self) -> bool {
        matches!(
            self,
            AppEvent::MrAdded(_)
                | AppEvent::MrRemovedById(_)
                | AppEvent::MrLoaded(_)
                | AppEvent::SignaturesChecked { .. }
        )
    }
}

/// Each outcome of a GitLab task is the app's event of the same name.
impl From<gitlab_tracker_gitlab::GitlabEvent> for AppEvent {
    fn from(event: gitlab_tracker_gitlab::GitlabEvent) -> Self {
        use gitlab_tracker_gitlab::GitlabEvent as G;
        match event {
            G::BranchCommitsLoaded {
                branch,
                commits,
                scan,
                head,
            } => AppEvent::BranchCommitsLoaded {
                branch,
                commits,
                scan,
                head,
            },
            G::GitlabLabelsLoaded(labels) => AppEvent::GitlabLabelsLoaded(labels),
            G::MilestoneMrsLoaded {
                milestone_title,
                mr_ids,
            } => AppEvent::MilestoneMrsLoaded {
                milestone_title,
                mr_ids,
            },
            G::MilestonesLoaded(milestones) => AppEvent::MilestonesLoaded(milestones),
            G::MrFailed { id, error } => AppEvent::MrFailed { id, error },
            G::MrLoaded(data) => AppEvent::MrLoaded(data),
            G::MrMergeabilityRetrying { id } => AppEvent::MrMergeabilityRetrying { id },
            G::NewMrsDiscovered {
                mr_ids,
                next_anchor,
            } => AppEvent::NewMrsDiscovered {
                mr_ids,
                next_anchor,
            },
            G::SentinelChecked(report) => AppEvent::SentinelChecked(report),
            G::SettledMrsChanged(ids) => AppEvent::SettledMrsChanged(ids),
            G::SignaturesChecked { mr_id, data } => AppEvent::SignaturesChecked { mr_id, data },
        }
    }
}
