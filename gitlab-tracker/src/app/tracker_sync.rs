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

/// Tracker requests in flight at once: a refresh cycle fetches every linked ticket,
/// which must not hit the tracker as one burst.
static TRACKER_PERMITS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);

/// Fetches a tracker ticket in the background and emits `TrackerTicketLoaded`.
///
/// On failure nothing is sent: the MR keeps its cached ticket (a network blip or
/// an expired token must not blank the Tracker pane) and the error is logged.
pub(super) fn spawn_ticket_fetch(
    provider: Arc<dyn gitlab_tracker_core::TrackerProvider>,
    ticket_id: String,
    mr_id: String,
    tx: &UnboundedSender<AppEvent>,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let _permit = TRACKER_PERMITS.acquire().await;
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

/// Re-reads every linked ticket of a refresh cycle at once (`links`: ticket id →
/// MR id): one [`TrackerProvider::fetch_tickets`] call for the distinct tickets,
/// then one `TrackerTicketLoaded` per MR. A ticket shared by several MRs is read
/// once. Tickets the tracker does not return keep their cached copy.
///
/// [`TrackerProvider::fetch_tickets`]: gitlab_tracker_core::TrackerProvider::fetch_tickets
pub fn spawn_tickets_fetch(
    provider: Arc<dyn gitlab_tracker_core::TrackerProvider>,
    links: Vec<(String, String)>,
    tx: &UnboundedSender<AppEvent>,
) {
    if links.is_empty() {
        return;
    }
    let tx = tx.clone();
    tokio::spawn(async move {
        let _permit = TRACKER_PERMITS.acquire().await;
        let ids = distinct_tickets(&links);
        match provider.fetch_tickets(&ids).await {
            Ok(tickets) => {
                for (ticket, mr_id) in pair_tickets(tickets, &links) {
                    let _ = tx.send(AppEvent::TrackerTicketLoaded {
                        mr_id,
                        ticket: Box::new(ticket),
                    });
                }
            }
            Err(error) => {
                tracing::warn!(tickets = ids.len(), error = %error, "Tracker tickets fetch failed; keeping cached copies");
            }
        }
    });
}

/// The distinct ticket ids of `links`, in first-seen order.
pub(crate) fn distinct_tickets(links: &[(String, String)]) -> Vec<String> {
    let mut seen = HashSet::new();
    links
        .iter()
        .filter(|(ticket, _)| seen.insert(ticket.as_str()))
        .map(|(ticket, _)| ticket.clone())
        .collect()
}

/// Each fetched ticket with every MR linked to it (ids compared without leading
/// zeros: `#042` and `42` are the same ticket).
fn pair_tickets(
    tickets: Vec<gitlab_tracker_core::LinkedTicket>,
    links: &[(String, String)],
) -> Vec<(gitlab_tracker_core::LinkedTicket, String)> {
    let norm = |id: &str| id.trim_start_matches('0').to_string();
    let by_id: HashMap<String, gitlab_tracker_core::LinkedTicket> =
        tickets.into_iter().map(|t| (norm(&t.id), t)).collect();
    links
        .iter()
        .filter_map(|(ticket, mr)| Some((by_id.get(&norm(ticket))?.clone(), mr.clone())))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use gitlab_tracker_core::{LinkedTicket, TrackerError, TrackerProvider};

    fn ticket(id: &str) -> LinkedTicket {
        serde_json::from_value(serde_json::json!({
            "schema_version": gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION,
            "id": id, "subject": "s", "status": "New", "url": "u"
        }))
        .unwrap()
    }

    fn links(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(t, m)| (t.to_string(), m.to_string()))
            .collect()
    }

    /// Bulk tracker double: records each `fetch_tickets` call, returns `known`.
    struct BulkTracker {
        calls: std::sync::Mutex<Vec<Vec<String>>>,
        known: Vec<&'static str>,
    }

    #[async_trait::async_trait]
    impl TrackerProvider for BulkTracker {
        fn name(&self) -> &'static str {
            "Bulk"
        }
        fn detect_ticket_id(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        async fn fetch_ticket(&self, id: &str) -> Result<LinkedTicket, TrackerError> {
            panic!("ticket {id} read on its own during a refresh cycle")
        }
        async fn fetch_tickets(&self, ids: &[String]) -> Result<Vec<LinkedTicket>, TrackerError> {
            self.calls.lock().unwrap().push(ids.to_vec());
            Ok(self.known.iter().map(|id| ticket(id)).collect())
        }
        fn ticket_url(&self, id: &str) -> String {
            id.into()
        }
    }

    #[test]
    fn a_ticket_shared_by_several_mrs_is_asked_once_and_given_to_each() {
        let l = links(&[("42", "1"), ("7", "2"), ("042", "3"), ("42", "4")]);
        assert_eq!(distinct_tickets(&l), ["42", "7", "042"]);
        let paired: Vec<_> = pair_tickets(vec![ticket("42")], &l)
            .into_iter()
            .map(|(t, mr)| (t.id, mr))
            .collect();
        let expected =
            [("42", "1"), ("42", "3"), ("42", "4")].map(|(t, m)| (t.to_string(), m.to_string()));
        assert_eq!(paired, expected);
    }

    #[tokio::test]
    async fn a_refresh_reads_every_ticket_in_one_call() {
        let tracker = Arc::new(BulkTracker {
            calls: Default::default(),
            known: vec!["42"], // "7" is gone: its MR keeps its cached copy
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_tickets_fetch(
            tracker.clone(),
            links(&[("42", "1"), ("7", "2"), ("42", "3")]),
            &tx,
        );
        drop(tx);
        let mut loaded = Vec::new();
        while let Some(event) = rx.recv().await {
            if let AppEvent::TrackerTicketLoaded { mr_id, ticket } = event {
                loaded.push((mr_id, ticket.id));
            }
        }
        assert_eq!(
            *tracker.calls.lock().unwrap(),
            [vec!["42".to_string(), "7".to_string()]]
        );
        let expected = [("1", "42"), ("3", "42")].map(|(m, t)| (m.to_string(), t.to_string()));
        assert_eq!(loaded, expected);
    }

    #[tokio::test]
    async fn the_default_bulk_read_goes_ticket_by_ticket() {
        struct OneByOne;
        #[async_trait::async_trait]
        impl TrackerProvider for OneByOne {
            fn name(&self) -> &'static str {
                "One"
            }
            fn detect_ticket_id(&self, _: &str, _: &str) -> Option<String> {
                None
            }
            async fn fetch_ticket(&self, id: &str) -> Result<LinkedTicket, TrackerError> {
                match id {
                    "1" => Ok(ticket("1")),
                    _ => Err(TrackerError::NotFound(id.into())),
                }
            }
            fn ticket_url(&self, id: &str) -> String {
                id.into()
            }
        }
        let read = OneByOne
            .fetch_tickets(&["1".into(), "2".into()])
            .await
            .unwrap();
        assert_eq!(read.len(), 1);
        assert!(matches!(
            OneByOne.fetch_tickets(&["2".into()]).await,
            Err(TrackerError::NotFound(_))
        ));
        assert_eq!(OneByOne.estimate_calls_for_tickets(5), 5);
    }
}
