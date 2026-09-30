mod client;
mod detector;

pub mod columns;
pub mod config;
pub mod filters;
pub mod keyring;
pub mod shortcuts;

use async_trait::async_trait;
use gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION;
use gitlab_tracker_core::{
    Activity, LabelColorMaps, LinkedTicket, TicketTransitionProvider, TicketTransitionTarget,
    TimeEntry, TimeEntryRequest, TrackerError, TrackerProvider,
};
use zeroize::Zeroizing;

pub use config::RedmineConfig;
pub use keyring::get_or_prompt_token;

/// Redmine implementation of the [`TrackerProvider`] contract.
///
/// Constructed once at startup and shared via `Arc<dyn TrackerProvider>`.
/// Requires a valid API token and a populated [`RedmineConfig`].
pub struct RedmineProvider {
    config: RedmineConfig,
    /// Redmine API token, wiped from memory when the provider is dropped.
    token: Zeroizing<String>,
    /// `config.ticket_patterns` compiled once at construction.
    ticket_patterns: Vec<regex::Regex>,
    /// Pre-built HTTP client — reused across all requests (connection pooling).
    http: reqwest::Client,
}

impl RedmineProvider {
    /// Creates a new [`RedmineProvider`] from a loaded config and a token.
    pub fn new(config: RedmineConfig, token: Zeroizing<String>) -> Self {
        Self {
            ticket_patterns: detector::compile_patterns(&config.ticket_patterns),
            config,
            token,
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl TicketTransitionProvider for RedmineProvider {
    async fn fetch_transition_targets(
        &self,
    ) -> Result<Vec<TicketTransitionTarget>, gitlab_tracker_core::TrackerError> {
        client::fetch_issue_statuses(&self.http, &self.config.url, &self.token).await
    }

    async fn transition_ticket_status(
        &self,
        ticket_id: &str,
        target_id: &str,
    ) -> Result<(), gitlab_tracker_core::TrackerError> {
        let status_id = target_id.parse::<u64>().map_err(|_| {
            gitlab_tracker_core::TrackerError::Other(format!(
                "Invalid Redmine status id: {}",
                target_id
            ))
        })?;

        client::update_issue_status(
            &self.http,
            &self.config.url,
            &self.token,
            ticket_id,
            status_id,
        )
        .await
        .map_err(gitlab_tracker_core::TrackerError::Other)
    }
}

#[async_trait]
impl TrackerProvider for RedmineProvider {
    fn name(&self) -> &'static str {
        "Redmine"
    }

    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String> {
        detector::detect_ticket_id(title, description, &self.ticket_patterns)
    }

    /// Exposes the colour maps configured in `redmine.yaml` to the orchestrator.
    ///
    /// The orchestrator converts the raw strings to `ratatui::Color` values — this
    /// crate stays free of any UI/rendering dependency.
    fn label_colors(&self) -> LabelColorMaps {
        let to_map = |source: &std::collections::HashMap<String, config::LabelColorConfig>| {
            source
                .iter()
                .map(|(k, cfg)| {
                    // Store wildcard as-is; all other keys are lowercased so the
                    // renderer's case-insensitive lookup can use a plain HashMap::get.
                    let key = if k == "*" {
                        k.clone()
                    } else {
                        k.to_lowercase()
                    };
                    (key, (cfg.bg.clone(), cfg.fg.clone()))
                })
                .collect()
        };

        LabelColorMaps {
            tracker_type: to_map(&self.config.tracker_type_colors),
            priority: to_map(&self.config.priority_colors),
        }
    }

    async fn fetch_ticket(&self, ticket_id: &str) -> Result<LinkedTicket, TrackerError> {
        let issue =
            client::fetch_issue(&self.http, &self.config.url, &self.token, ticket_id).await?;

        Ok(LinkedTicket {
            schema_version: LINKED_TICKET_SCHEMA_VERSION,
            id: issue.id.to_string(),
            subject: issue.subject,
            status: issue.status.name,
            url: self.ticket_url(ticket_id),
            author: issue.author.map(|u| u.name),
            assignee: issue.assigned_to.map(|u| u.name),
            // Redmine returns hours — convert to seconds for the generic LinkedTicket contract.
            time_estimate: issue.estimated_hours.map(|h| (h * 3600.0).round() as u32),
            time_spent: issue.spent_hours.map(|h| (h * 3600.0).round() as u32),
            time_remaining: issue.remaining_hours.map(|h| (h * 3600.0).round() as u32),
            tracker_type: issue.tracker.map(|t| t.name),
            priority: issue.priority.map(|p| p.name),
            version: issue.fixed_version.map(|v| v.name),
            start_date: issue.start_date,
            done_ratio: issue.done_ratio,
        })
    }

    fn ticket_url(&self, ticket_id: &str) -> String {
        format!(
            "{}/issues/{}",
            self.config.url.trim_end_matches('/'),
            ticket_id
        )
    }

    /// Fetches all available time-tracking activity categories from Redmine.
    ///
    /// Delegates to `GET /enumerations/time_entry_activities.json`.
    async fn fetch_activities(&self) -> Result<Vec<Activity>, TrackerError> {
        client::fetch_activities(&self.http, &self.config.url, &self.token).await
    }

    /// Fetches all time entries recorded on a Redmine issue.
    ///
    /// Delegates to `GET /time_entries.json?issue_id={id}`.
    async fn fetch_time_entries(&self, ticket_id: &str) -> Result<Vec<TimeEntry>, TrackerError> {
        client::fetch_time_entries(&self.http, &self.config.url, &self.token, ticket_id).await
    }

    /// Submits a new time entry on the given Redmine issue.
    ///
    /// Fetches the issue beforehand to attach `budget_hours` and `remaining_hours`
    /// (ETC) directly to the time entry payload — as required by the Redmine Budget
    /// plugin. Both fields are omitted when the issue does not expose them (vanilla
    /// Redmine). The fetch failure is non-fatal: submission proceeds without them.
    ///
    /// Delegates to `POST /time_entries.json`.
    async fn log_time(
        &self,
        ticket_id: &str,
        entry: TimeEntryRequest,
    ) -> Result<(), gitlab_tracker_core::TrackerError> {
        // Fetch the issue to get remaining_hours / estimated_hours for ETC computation.
        // A failure here simply means the ETC update step is skipped — not an error.
        let issue = client::fetch_issue(&self.http, &self.config.url, &self.token, ticket_id)
            .await
            .map_err(|e| tracing::warn!(error = %e, "Issue fetch before time logging failed"))
            .ok();

        client::log_time(
            &self.http,
            &self.config.url,
            &self.token,
            ticket_id,
            entry,
            issue.as_ref(),
        )
        .await
        .map_err(gitlab_tracker_core::TrackerError::Other)
    }
}
