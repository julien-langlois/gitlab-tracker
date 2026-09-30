//! Tracker side of the app: linked-ticket fetches and status transitions, and the
//! TimeLog cache. Split out of `app.rs` (it only needs `App`'s public fields).

use super::*;

/// Time entries of one ticket in the TimeLog view.
#[derive(Debug, Clone)]
pub enum TimeLogState {
    /// Request in flight.
    Loading,
    Loaded(Vec<gitlab_tracker_core::TimeEntry>),
    /// Fetch failed: the message is shown instead of an (empty) entry list.
    Failed(String),
}

/// Fetches a tracker ticket in the background and emits `TrackerTicketLoaded`.
///
/// On failure nothing is sent: the MR keeps its cached ticket (a network blip or
/// an expired token must not blank the Tracker pane) and the error is logged.
pub fn spawn_ticket_fetch(
    provider: Arc<dyn gitlab_tracker_core::TrackerProvider>,
    ticket_id: String,
    mr_id: String,
    tx: &UnboundedSender<AppEvent>,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        match provider.fetch_ticket(&ticket_id).await {
            Ok(ticket) => {
                let _ = tx.send(AppEvent::TrackerTicketLoaded {
                    mr_id,
                    ticket: Box::new(ticket),
                });
            }
            Err(error) => {
                tracing::warn!(ticket_id = %ticket_id, error = %error, "Tracker ticket fetch failed; keeping cached copy");
            }
        }
    });
}

pub(super) fn transition_target_for_state(
    project: &ProjectEntry,
    state: &GitlabMrState,
) -> Option<String> {
    let key = match state {
        GitlabMrState::Merged => "merged",
        GitlabMrState::Closed => "closed",
        GitlabMrState::Opened => return None,
    };

    project
        .tracker
        .as_ref()?
        .extra
        .get("status_transitions")?
        .as_table()?
        .get("gitlab_state")?
        .as_table()?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|target_id| !target_id.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) struct TicketTransitionRequest<'a> {
    pub(super) tracker: Option<&'a TrackerHandle>,
    pub(super) transitioner: Option<&'a TicketTransitionHandle>,
    pub(super) tx: &'a UnboundedSender<AppEvent>,
    pub(super) mr_id: &'a str,
    pub(super) mr_title: &'a str,
    pub(super) previous_state: &'a GitlabMrState,
    pub(super) current_state: &'a GitlabMrState,
    pub(super) previous_ticket: Option<LinkedTicket>,
    pub(super) target_id: Option<String>,
}

pub(super) fn spawn_ticket_transition_if_needed(request: TicketTransitionRequest<'_>) {
    if !matches!(
        (request.previous_state, request.current_state),
        (GitlabMrState::Opened, GitlabMrState::Merged)
            | (GitlabMrState::Opened, GitlabMrState::Closed)
    ) {
        return;
    }

    let (Some(provider), Some(transitioner), Some(previous_ticket), Some(target_id)) = (
        request.tracker,
        request.transitioner,
        request.previous_ticket,
        request.target_id,
    ) else {
        return;
    };

    let provider = Arc::clone(provider);
    let transitioner = Arc::clone(transitioner);
    let tx = request.tx.clone();
    let mr_id = request.mr_id.to_string();
    let mr_title = request.mr_title.to_string();
    let ticket_id = previous_ticket.id;
    let expected_status = previous_ticket.status;

    tokio::spawn(async move {
        let upstream_ticket = match provider.fetch_ticket(&ticket_id).await {
            Ok(ticket) => ticket,
            Err(error) => {
                tracing::warn!(ticket_id = %ticket_id, error = %error, "Skipping tracker transition: ticket refetch failed");
                return;
            }
        };

        if upstream_ticket.status != expected_status {
            tracing::info!(
                ticket_id = %ticket_id,
                expected = %expected_status,
                upstream = %upstream_ticket.status,
                "Skipping tracker transition: upstream status changed since last local snapshot",
            );
            let _ = tx.send(AppEvent::TrackerTicketLoaded {
                mr_id,
                ticket: Box::new(upstream_ticket),
            });
            return;
        }

        if let Err(error) = transitioner
            .transition_ticket_status(&ticket_id, &target_id)
            .await
        {
            tracing::warn!(
                ticket_id = %ticket_id,
                target_id = %target_id,
                error = %error,
                "Tracker transition failed",
            );
            let _ = tx.send(AppEvent::TrackerTicketLoaded {
                mr_id,
                ticket: Box::new(upstream_ticket),
            });
            return;
        }

        tracing::info!(
            ticket_id = %ticket_id,
            target_id = %target_id,
            "Tracker transition succeeded",
        );

        if let Ok(refreshed_ticket) = provider.fetch_ticket(&ticket_id).await {
            notify::ticket_status_transitioned(
                &ticket_id,
                &mr_title,
                &expected_status,
                &refreshed_ticket.status,
                &refreshed_ticket.url,
            );
            let _ = tx.send(AppEvent::TrackerTicketLoaded {
                mr_id,
                ticket: Box::new(refreshed_ticket),
            });
        }
    });
}

impl App {
    /// Fetches the time entries of the selected MR's ticket when the TimeLog view is
    /// shown and they are neither cached nor in flight. Called once per main-loop
    /// iteration: navigation (keys, mouse, filters) never spawns a request itself,
    /// and a ticket is fetched at most once per refresh cycle.
    pub fn ensure_time_entries(&mut self, tx: &UnboundedSender<AppEvent>) {
        if self.tracker_view != TrackerView::TimeLog {
            return;
        }
        let Some(provider) = self.tracker.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(ticket_id) = self
            .table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
            .and_then(|mr| mr.linked_ticket.as_ref())
            .map(|t| t.id.clone())
        else {
            return;
        };
        if self.time_entries.contains_key(&ticket_id) {
            return;
        }
        self.time_entries
            .insert(ticket_id.clone(), TimeLogState::Loading);
        let tx = tx.clone();
        tokio::spawn(async move {
            let entries = provider.fetch_time_entries(&ticket_id).await;
            let _ = tx.send(AppEvent::TimeEntriesLoaded { ticket_id, entries });
        });
    }

    /// Re-fetches the linked ticket of `mr_id` after a GitLab load when needed:
    /// the detected ticket id is new or changed, the MR was updated (spent time or
    /// status may have moved; skipped for refresh-cycle loads, which already
    /// re-fetched it), or the cached ticket predates the current schema. Clears the
    /// ticket when the MR no longer references one.
    pub(super) fn sync_linked_ticket(
        &mut self,
        mr_id: &str,
        was_updated: bool,
        from_refresh_cycle: bool,
        tx: &UnboundedSender<AppEvent>,
    ) {
        let Some(provider) = self.tracker.clone() else {
            return;
        };
        let Some(mr) = self.mrs.find_mut(mr_id) else {
            return;
        };
        let detected_id = provider.detect_ticket_id(&mr.title, &mr.description);
        let cached_id = mr.linked_ticket.as_ref().map(|t| t.id.clone());
        let cache_is_stale = mr
            .linked_ticket
            .as_ref()
            .is_some_and(|t| t.schema_version < gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION);

        let fetch_id = if detected_id != cached_id
            || (detected_id.is_some() && was_updated && !from_refresh_cycle)
            || (detected_id.is_some() && cache_is_stale)
        {
            detected_id.clone()
        } else {
            None
        };

        if let Some(raw_id) = fetch_id {
            spawn_ticket_fetch(provider, raw_id, mr.id.clone(), tx);
        } else if detected_id.is_none() && cached_id.is_some() {
            // Ticket reference was removed from the MR — clear the cache.
            mr.linked_ticket = None;
        }
    }
}
