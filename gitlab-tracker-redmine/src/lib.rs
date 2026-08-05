mod client;
mod detector;

pub mod config;
pub mod keyring;

use async_trait::async_trait;
use gitlab_tracker_core::{Activity, LinkedTicket, TimeEntry, TimeEntryRequest, TrackerProvider};

pub use config::{load_or_create_config, RedmineConfig};
pub use keyring::get_or_prompt_token;

/// Redmine implementation of the [`TrackerProvider`] contract.
///
/// Constructed once at startup and shared via `Arc<dyn TrackerProvider>`.
/// Requires a valid API token and a populated [`RedmineConfig`].
pub struct RedmineProvider {
    config: RedmineConfig,
    /// Redmine API token — stored as a plain `String` here because `Arc<dyn TrackerProvider>`
    /// requires `Sync`, and `Zeroizing<String>` is `Sync`. We clone it from the
    /// `Zeroizing` wrapper immediately after the keyring lookup in the caller.
    token: String,
    /// Pre-built HTTP client — reused across all requests (connection pooling).
    http: reqwest::Client,
}

impl RedmineProvider {
    /// Creates a new [`RedmineProvider`] from a loaded config and a token.
    pub fn new(config: RedmineConfig, token: String) -> Self {
        Self {
            config,
            token,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl TrackerProvider for RedmineProvider {
    fn name(&self) -> &'static str {
        "Redmine"
    }

    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String> {
        detector::detect_ticket_id(title, description, &self.config.ticket_patterns)
    }

    async fn fetch_ticket(&self, ticket_id: &str) -> Option<LinkedTicket> {
        let issue =
            client::fetch_issue(&self.http, &self.config.redmine_url, &self.token, ticket_id)
                .await?;

        Some(LinkedTicket {
            id: issue.id.to_string(),
            subject: issue.subject,
            status: issue.status.name,
            url: self.ticket_url(ticket_id),
            author: issue.author.map(|u| u.name),
            assignee: issue.assigned_to.map(|u| u.name),
            // Redmine returns hours — convert to seconds for the generic LinkedTicket contract.
            time_estimate: issue.estimated_hours.map(|h| (h * 3600.0).round() as u32),
            time_spent: issue.spent_hours.map(|h| (h * 3600.0).round() as u32),
        })
    }

    fn ticket_url(&self, ticket_id: &str) -> String {
        format!(
            "{}/issues/{}",
            self.config.redmine_url.trim_end_matches('/'),
            ticket_id
        )
    }

    /// Fetches all available time-tracking activity categories from Redmine.
    ///
    /// Delegates to `GET /enumerations/time_entry_activities.json`.
    async fn fetch_activities(&self) -> Vec<Activity> {
        client::fetch_activities(&self.http, &self.config.redmine_url, &self.token).await
    }

    /// Fetches all time entries recorded on a Redmine issue.
    ///
    /// Delegates to `GET /time_entries.json?issue_id={id}`.
    async fn fetch_time_entries(&self, ticket_id: &str) -> Vec<TimeEntry> {
        client::fetch_time_entries(&self.http, &self.config.redmine_url, &self.token, ticket_id)
            .await
    }

    /// Submits a new time entry on the given Redmine issue.
    ///
    /// Fetches the issue beforehand to attach `budget_hours` and `remaining_hours`
    /// (ETC) directly to the time entry payload — as required by the Redmine Budget
    /// plugin. Both fields are omitted when the issue does not expose them (vanilla
    /// Redmine). The fetch failure is non-fatal: submission proceeds without them.
    ///
    /// Delegates to `POST /time_entries.json`.
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), String> {
        // Fetch the issue to get remaining_hours / estimated_hours for ETC computation.
        // A None here simply means the ETC update step will be skipped — not an error.
        let issue =
            client::fetch_issue(&self.http, &self.config.redmine_url, &self.token, ticket_id).await;

        client::log_time(
            &self.http,
            &self.config.redmine_url,
            &self.token,
            ticket_id,
            entry,
            issue.as_ref(),
        )
        .await
    }
}
