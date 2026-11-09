//! The demo's Redmine: its statuses, its tickets and the time logged on them.

use super::clock::{clock, date};
use super::model::TicketSpec;
use crate::app::TimeLogState;
use gitlab_tracker_core::{
    Activity, LinkedTicket, TimeEntry, TrackerError, TrackerProvider, LINKED_TICKET_SCHEMA_VERSION,
};
use std::collections::HashMap;

pub const TRACKER_URL: &str = "https://redmine.orbit-logistics.dev";

/// A Redmine stand-in: it names the tracker and builds ticket URLs, so the
/// tracker page, the ticket filter and the log-time dialog show up. It never
/// fetches: the tickets are in the app already, and the demo drops fetch
/// results.
pub(crate) struct DemoTracker;

#[async_trait::async_trait]
impl TrackerProvider for DemoTracker {
    fn name(&self) -> &'static str {
        "Redmine"
    }

    fn detect_ticket_id(&self, _title: &str, _description: &str) -> Option<String> {
        None
    }

    async fn fetch_ticket(&self, ticket_id: &str) -> Result<LinkedTicket, TrackerError> {
        Err(TrackerError::NotFound(ticket_id.to_string()))
    }

    fn ticket_url(&self, ticket_id: &str) -> String {
        format!("{TRACKER_URL}/issues/{ticket_id}")
    }
}

/// The statuses of the board's columns. Moving a ticket is refused: the demo
/// is read-only (the window checks first).
#[cfg(feature = "gui")]
#[async_trait::async_trait]
impl gitlab_tracker_core::TicketTransitionProvider for DemoTracker {
    async fn fetch_transition_targets(
        &self,
    ) -> Result<Vec<gitlab_tracker_core::TicketTransitionTarget>, TrackerError> {
        Ok(STATUSES
            .iter()
            .enumerate()
            .map(|(i, label)| gitlab_tracker_core::TicketTransitionTarget {
                id: (i + 1).to_string(),
                label: (*label).into(),
            })
            .collect())
    }

    async fn transition_ticket_status(
        &self,
        ticket_id: &str,
        _target_id: &str,
    ) -> Result<(), TrackerError> {
        Err(TrackerError::NotFound(ticket_id.to_string()))
    }
}

/// Redmine's default workflow.
pub const STATUSES: [&str; 5] = ["New", "In Progress", "Assigned", "Feedback", "Resolved"];

/// The time-tracking activities.
pub fn activities() -> Vec<Activity> {
    [(9, "Development"), (10, "Review"), (11, "Testing")]
        .into_iter()
        .map(|(id, name)| Activity {
            id,
            name: name.into(),
        })
        .collect()
}

/// The tickets of a project, as the tracker gives them.
pub struct Tickets {
    specs: Vec<TicketSpec>,
}

impl Tickets {
    pub fn new(specs: Vec<TicketSpec>) -> Self {
        Self { specs }
    }

    fn spent_hours(spec: &TicketSpec) -> f32 {
        spec.entries.iter().map(|e| e.1).sum()
    }

    pub fn linked(&self, id: u32) -> Option<LinkedTicket> {
        let spec = self.specs.iter().find(|t| t.id == id)?;
        let spent = Self::spent_hours(spec);
        let secs = |hours: f32| (hours * 3600.0).round() as u32;
        Some(LinkedTicket {
            schema_version: LINKED_TICKET_SCHEMA_VERSION,
            id: id.to_string(),
            subject: spec.subject.into(),
            status: spec.status.into(),
            url: format!("{TRACKER_URL}/issues/{id}"),
            author: Some(spec.author.name.into()),
            assignee: Some(spec.assignee.name.into()),
            tracker_type: Some(spec.tracker.into()),
            priority: Some(spec.priority.into()),
            version: spec.version.map(str::to_string),
            start_date: Some(date(clock().at(spec.start))),
            done_ratio: Some(u32::from(spec.done)),
            time_estimate: spec.estimate_hours.map(secs),
            time_spent: Some(secs(spent)),
            time_remaining: spec
                .estimate_hours
                .map(|estimate| secs((estimate - spent).max(0.0))),
        })
    }

    /// Each ticket's time log, adding up to its time spent.
    pub fn time_entries(&self, first_id: u64) -> HashMap<String, TimeLogState> {
        let mut id = first_id;
        self.specs
            .iter()
            .map(|spec| {
                let mut entries: Vec<TimeEntry> = spec
                    .entries
                    .iter()
                    .map(|(when, hours, activity, comment, who)| {
                        id += 1;
                        TimeEntry {
                            id,
                            hours: *hours,
                            activity: activities().swap_remove(*activity),
                            comment: (*comment).into(),
                            user: who.name.into(),
                            spent_on: date(clock().at(*when)),
                        }
                    })
                    .collect();
                // Redmine lists the latest first.
                entries.reverse();
                (spec.id.to_string(), TimeLogState::Loaded(entries))
            })
            .collect()
    }
}
