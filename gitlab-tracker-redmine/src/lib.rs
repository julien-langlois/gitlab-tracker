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

/// Why [`RedmineProvider::from_tracker_section`] could not build a provider.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("[project.tracker] provider is redmine but url is empty")]
    EmptyUrl,
    #[error("no Redmine token given (REDMINE_TOKEN, OS keyring or prompt)")]
    NoToken,
}

impl RedmineProvider {
    /// Builds the provider from a `[project.tracker]` section: its `url` and the
    /// provider-specific `extra` fields ([`RedmineConfig::from_tracker_section`]).
    /// Applies `REDMINE_URL`, then resolves the token (env → keyring → prompt).
    /// Shared by the TUI and the CLI commands.
    pub fn from_tracker_section(url: &str, extra: toml::Table) -> Result<Self, SetupError> {
        let mut config = RedmineConfig::from_tracker_section(url, extra);
        config.apply_env_override();
        if !config.is_active() {
            return Err(SetupError::EmptyUrl);
        }
        let token = get_or_prompt_token(&config.url).ok_or(SetupError::NoToken)?;
        tracing::info!(url = %config.url, "Redmine integration active (from projects.toml)");
        Ok(Self::new(config, token))
    }

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

    /// Exposes the colour maps configured under `[project.tracker]` in `projects.toml` to the orchestrator.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// One request received by the stub.
    #[derive(Debug)]
    struct Received {
        /// Request line without the protocol, e.g. `GET /issues/42.json`.
        line: String,
        api_key: Option<String>,
        body: String,
    }

    impl Received {
        fn json(&self) -> Value {
            serde_json::from_str(&self.body).expect("JSON request body")
        }
    }

    /// A stand-in for Redmine: answers one connection per `(status, body)` pair, in
    /// order, and hands back what it received.
    struct Stub {
        url: String,
        task: tokio::task::JoinHandle<Vec<Received>>,
    }

    impl Stub {
        async fn start(responses: &[(u16, &str)]) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            // Trailing slash: every request URL must be built without doubling it.
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let responses: Vec<(u16, String)> = responses
                .iter()
                .map(|(status, body)| (*status, body.to_string()))
                .collect();
            let task = tokio::spawn(async move {
                let mut received = Vec::new();
                for (status, body) in responses {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    received.push(read_request(&mut socket).await);
                    let response = format!(
                        "HTTP/1.1 {status} Stub\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                    let _ = socket.shutdown().await;
                }
                received
            });
            Self { url, task }
        }

        fn provider(&self) -> RedmineProvider {
            provider_at(&self.url)
        }

        /// The requests received, once every planned response has been served.
        async fn requests(self) -> Vec<Received> {
            tokio::time::timeout(std::time::Duration::from_secs(5), self.task)
                .await
                .expect("the provider sent fewer requests than planned")
                .unwrap()
        }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> Received {
        let mut raw = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head_len, content_length) = loop {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(read > 0, "connection closed before the request head");
            raw.extend_from_slice(&chunk[..read]);
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse::<usize>().unwrap());
                break (end + 4, length);
            }
        };
        while raw.len() < head_len + content_length {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(read > 0, "connection closed before the request body");
            raw.extend_from_slice(&chunk[..read]);
        }
        let head = String::from_utf8_lossy(&raw[..head_len]).to_string();
        let mut lines = head.lines();
        let line = lines
            .next()
            .unwrap()
            .trim_end_matches(" HTTP/1.1")
            .to_string();
        let api_key = lines.find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("x-redmine-api-key")
                .then(|| value.trim().to_string())
        });
        Received {
            line,
            api_key,
            body: String::from_utf8_lossy(&raw[head_len..]).to_string(),
        }
    }

    fn provider_at(url: &str) -> RedmineProvider {
        let config = RedmineConfig {
            url: url.to_string(),
            ..RedmineConfig::default()
        };
        RedmineProvider::new(config, Zeroizing::new("secret-key".to_string()))
    }

    /// A provider that must not send anything: its URL accepts no connection.
    async fn offline_provider() -> RedmineProvider {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        provider_at(&url)
    }

    const FULL_ISSUE: &str = r#"{"issue": {
        "id": 42, "subject": "Expose MR metadata",
        "status": {"id": 2, "name": "In Progress"},
        "tracker": {"id": 2, "name": "Evolution"},
        "priority": {"id": 3, "name": "High"},
        "fixed_version": {"id": 7, "name": "v2.4.0"},
        "author": {"id": 1, "name": "Marina"},
        "assigned_to": {"id": 5, "name": "Thomas"},
        "start_date": "2024-04-15", "done_ratio": 65,
        "estimated_hours": 8.0, "spent_hours": 1.5, "remaining_hours": 5.0,
        "unknown_field": {"ignored": true}
    }}"#;
    const BARE_ISSUE: &str =
        r#"{"issue": {"id": 7, "subject": "Bare", "status": {"id": 1, "name": "New"}}}"#;

    fn time_request(hours: f32) -> TimeEntryRequest {
        TimeEntryRequest {
            hours,
            activity_id: 9,
            comment: "Review".into(),
            spent_on: "2024-04-16".into(),
        }
    }

    #[tokio::test]
    async fn ticket_is_mapped_to_the_generic_contract() {
        let stub = Stub::start(&[(200, FULL_ISSUE), (200, BARE_ISSUE)]).await;
        let provider = stub.provider();

        let ticket = provider.fetch_ticket("42").await.unwrap();
        assert_eq!(ticket.schema_version, LINKED_TICKET_SCHEMA_VERSION);
        assert_eq!(
            (ticket.id.as_str(), ticket.subject.as_str()),
            ("42", "Expose MR metadata")
        );
        assert_eq!(ticket.status, "In Progress");
        assert_eq!(ticket.url, format!("{}issues/42", stub.url));
        assert_eq!(ticket.author.as_deref(), Some("Marina"));
        assert_eq!(ticket.assignee.as_deref(), Some("Thomas"));
        assert_eq!(ticket.tracker_type.as_deref(), Some("Evolution"));
        assert_eq!(ticket.priority.as_deref(), Some("High"));
        assert_eq!(ticket.version.as_deref(), Some("v2.4.0"));
        assert_eq!(ticket.start_date.as_deref(), Some("2024-04-15"));
        assert_eq!(ticket.done_ratio, Some(65));
        // Redmine speaks hours, the contract seconds.
        assert_eq!(ticket.time_estimate, Some(28_800));
        assert_eq!(ticket.time_spent, Some(5_400));
        assert_eq!(ticket.time_remaining, Some(18_000));

        // Everything optional may be missing.
        let bare = provider.fetch_ticket("7").await.unwrap();
        assert_eq!(bare.status, "New");
        assert!(bare.author.is_none() && bare.assignee.is_none() && bare.priority.is_none());
        assert!(bare.time_estimate.is_none() && bare.done_ratio.is_none());

        let requests = stub.requests().await;
        assert_eq!(requests[0].line, "GET /issues/42.json");
        assert_eq!(requests[1].line, "GET /issues/7.json");
        assert!(requests
            .iter()
            .all(|r| r.api_key.as_deref() == Some("secret-key")));
    }

    #[tokio::test]
    async fn failures_are_typed_for_the_orchestrator() {
        let stub = Stub::start(&[
            (401, "{}"),
            (403, "{}"),
            (404, "{}"),
            (500, "boom"),
            (200, r#"{"issue": {"id": "not a number"}}"#),
        ])
        .await;
        let provider = stub.provider();
        let mut errors = Vec::new();
        for _ in 0..5 {
            errors.push(provider.fetch_ticket("42").await.unwrap_err());
        }
        assert!(
            matches!(errors[0], TrackerError::Auth(_)),
            "{:?}",
            errors[0]
        );
        assert!(
            matches!(errors[1], TrackerError::Auth(_)),
            "{:?}",
            errors[1]
        );
        assert!(
            matches!(errors[2], TrackerError::NotFound(_)),
            "{:?}",
            errors[2]
        );
        assert!(
            matches!(&errors[3], TrackerError::Other(m) if m.contains("500")),
            "{:?}",
            errors[3]
        );
        assert!(
            matches!(&errors[4], TrackerError::Other(m) if m.contains("Invalid Redmine response")),
            "{:?}",
            errors[4]
        );
        stub.requests().await;

        let offline = offline_provider().await;
        assert!(matches!(
            offline.fetch_ticket("42").await.unwrap_err(),
            TrackerError::Network(_)
        ));
    }

    #[tokio::test]
    async fn non_numeric_ids_never_reach_the_network() {
        // Nothing listens: a request would fail with `Network`, not `Other`.
        let provider = offline_provider().await;
        let id = "12/../users";
        let invalid = |result: Result<(), TrackerError>| matches!(result, Err(TrackerError::Other(m)) if m.contains("Invalid ticket id"));
        assert!(invalid(provider.fetch_ticket(id).await.map(drop)));
        assert!(invalid(provider.fetch_time_entries(id).await.map(drop)));
        assert!(invalid(provider.transition_ticket_status(id, "5").await));
        assert!(invalid(provider.log_time(id, time_request(1.0)).await));
        // The status id is checked before anything is sent, too.
        assert!(matches!(
            provider.transition_ticket_status("42", "done").await,
            Err(TrackerError::Other(m)) if m.contains("Invalid Redmine status id")
        ));
    }

    #[tokio::test]
    async fn time_entries_follow_pagination() {
        let page = |ids: &[u64], total: usize| {
            let entries: Vec<Value> = ids
                .iter()
                .map(|id| {
                    json!({
                        "id": id, "hours": 1.5, "spent_on": "2024-04-16",
                        "activity": {"id": 9, "name": "Development"},
                        "user": {"id": 5, "name": "Thomas"},
                        "comments": format!("entry {id}")
                    })
                })
                .collect();
            json!({"time_entries": entries, "total_count": total}).to_string()
        };
        let (first, second) = (page(&[1, 2], 3), page(&[3], 3));
        // `comments` and `total_count` are optional: a single page is then read.
        let legacy = r#"{"time_entries": [{"id": 4, "hours": 0.5, "spent_on": "2024-04-17",
            "activity": {"id": 9, "name": "Development"}, "user": {"id": 5, "name": "Thomas"}}]}"#;
        let stub = Stub::start(&[(200, &first), (200, &second), (200, legacy)]).await;
        let provider = stub.provider();

        let entries = provider.fetch_time_entries("42").await.unwrap();
        assert_eq!(entries.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(entries[0].hours, 1.5);
        assert_eq!(
            (entries[0].activity.id, entries[0].activity.name.as_str()),
            (9, "Development")
        );
        assert_eq!(
            (entries[2].user.as_str(), entries[2].comment.as_str()),
            ("Thomas", "entry 3")
        );

        let legacy_entries = provider.fetch_time_entries("43").await.unwrap();
        assert_eq!(legacy_entries.len(), 1);
        assert_eq!(legacy_entries[0].comment, "");

        let lines: Vec<String> = stub.requests().await.into_iter().map(|r| r.line).collect();
        assert_eq!(
            lines,
            [
                "GET /time_entries.json?issue_id=42&limit=100&offset=0",
                "GET /time_entries.json?issue_id=42&limit=100&offset=2",
                "GET /time_entries.json?issue_id=43&limit=100&offset=0",
            ]
        );
    }

    #[tokio::test]
    async fn logging_time_attaches_budget_fields_when_the_issue_has_them() {
        let stub = Stub::start(&[
            // Budget plugin: remaining_hours 5 → 3.5 after a 1.5 h entry.
            (200, FULL_ISSUE),
            (201, "{}"),
            // Vanilla issue: no budget fields in the payload.
            (200, BARE_ISSUE),
            (201, "{}"),
            // Issue fetch fails: the entry is still posted, without them.
            (500, "{}"),
            (201, "{}"),
            // Redmine refuses the entry: typed error.
            (200, BARE_ISSUE),
            (422, r#"{"errors": ["Activity is invalid"]}"#),
        ])
        .await;
        let provider = stub.provider();

        provider.log_time("42", time_request(1.5)).await.unwrap();
        provider.log_time("7", time_request(2.0)).await.unwrap();
        provider.log_time("8", time_request(0.5)).await.unwrap();
        let refused = provider.log_time("7", time_request(1.0)).await.unwrap_err();
        assert!(
            matches!(&refused, TrackerError::Other(m) if m.contains("422")),
            "{refused:?}"
        );

        let requests = stub.requests().await;
        assert_eq!(requests[0].line, "GET /issues/42.json");
        assert_eq!(requests[1].line, "POST /time_entries.json");
        assert_eq!(requests[1].api_key.as_deref(), Some("secret-key"));
        assert_eq!(
            requests[1].json(),
            json!({"time_entry": {
                "issue_id": "42", "hours": 1.5, "activity_id": 9,
                "comments": "Review", "spent_on": "2024-04-16",
                "budget_hours": 8.0, "remaining_hours": 3.5
            }})
        );
        let vanilla = json!({"time_entry": {
            "issue_id": "7", "hours": 2.0, "activity_id": 9,
            "comments": "Review", "spent_on": "2024-04-16"
        }});
        assert_eq!(requests[3].json(), vanilla);
        let after_failed_fetch = requests[5].json();
        assert_eq!(after_failed_fetch["time_entry"]["issue_id"], "8");
        assert!(after_failed_fetch["time_entry"]
            .get("remaining_hours")
            .is_none());
        assert!(after_failed_fetch["time_entry"]
            .get("budget_hours")
            .is_none());
    }

    #[tokio::test]
    async fn statuses_are_listed_and_applied() {
        let statuses = r#"{"issue_statuses": [
            {"id": 1, "name": "New", "is_closed": false},
            {"id": 5, "name": "Closed", "is_closed": true}
        ]}"#;
        let activities = r#"{"time_entry_activities": [
            {"id": 8, "name": "Design"}, {"id": 9, "name": "Development", "is_default": true}
        ]}"#;
        let stub = Stub::start(&[(200, statuses), (204, ""), (403, "{}"), (200, activities)]).await;
        let provider = stub.provider();

        let targets = provider.fetch_transition_targets().await.unwrap();
        let pairs: Vec<(&str, &str)> = targets
            .iter()
            .map(|t| (t.id.as_str(), t.label.as_str()))
            .collect();
        assert_eq!(pairs, [("1", "New"), ("5", "Closed")]);

        provider.transition_ticket_status("42", "5").await.unwrap();
        // Workflow rules or role forbid it: reported, not swallowed.
        assert!(matches!(
            provider.transition_ticket_status("42", "5").await,
            Err(TrackerError::Auth(_))
        ));

        let found = provider.fetch_activities().await.unwrap();
        assert_eq!(
            found,
            [
                Activity {
                    id: 8,
                    name: "Design".into()
                },
                Activity {
                    id: 9,
                    name: "Development".into()
                }
            ]
        );

        let requests = stub.requests().await;
        assert_eq!(requests[0].line, "GET /issue_statuses.json");
        assert_eq!(requests[1].line, "PUT /issues/42.json");
        assert_eq!(requests[1].json(), json!({"issue": {"status_id": 5}}));
        assert_eq!(requests[1].api_key.as_deref(), Some("secret-key"));
        assert_eq!(
            requests[3].line,
            "GET /enumerations/time_entry_activities.json"
        );
    }

    #[test]
    fn provider_exposes_its_configuration() {
        let config = RedmineConfig::from_tracker_section(
            "https://redmine.example/",
            toml::from_str(
                r##"
                ticket_patterns = ['RM-(\d+)']
                [tracker_type_colors]
                "Bug" = { bg = "red", fg = "white" }
                "*" = { bg = "dark_gray", fg = "white" }
                [priority_colors]
                "Haute" = { bg = "yellow", fg = "black" }
                "##,
            )
            .unwrap(),
        );
        let provider = RedmineProvider::new(config, Zeroizing::new("k".to_string()));
        assert_eq!(provider.name(), "Redmine");
        assert_eq!(
            provider.ticket_url("42"),
            "https://redmine.example/issues/42"
        );

        // Only the configured patterns apply: a plain `#12` is not a ticket here.
        assert_eq!(
            provider.detect_ticket_id("RM-77 fix", "").as_deref(),
            Some("77")
        );
        assert_eq!(
            provider.detect_ticket_id("fix #12", "see RM-5").as_deref(),
            Some("5")
        );
        assert_eq!(provider.detect_ticket_id("fix #12", ""), None);

        // Keys are lowercased for the renderer's lookup; the wildcard is kept.
        let colors = provider.label_colors();
        assert_eq!(
            colors.tracker_type["bug"],
            ("red".to_string(), "white".to_string())
        );
        assert!(colors.tracker_type.contains_key("*") && !colors.tracker_type.contains_key("Bug"));
        assert_eq!(colors.priority["haute"].0, "yellow");
    }
}
