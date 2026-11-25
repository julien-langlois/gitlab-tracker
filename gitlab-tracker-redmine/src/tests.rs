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

fn listed(id: u64, spent: Option<f32>) -> String {
    let spent = spent.map_or(String::new(), |h| format!(r#", "spent_hours": {h}"#));
    format!(r#"{{"id": {id}, "subject": "T{id}", "status": {{"id": 1, "name": "New"}}{spent}}}"#)
}

#[tokio::test]
async fn tickets_are_read_in_one_request() {
    let body = format!(
        r#"{{"issues": [{}, {}], "total_count": 2}}"#,
        listed(12, Some(1.0)),
        listed(34, Some(0.0))
    );
    let stub = Stub::start(&[(200, &body)]).await;
    let provider = stub.provider();
    let ids = ["34", "12", "12", "not-a-ticket"].map(String::from);

    let tickets = provider.fetch_tickets(&ids).await.unwrap();
    let read: Vec<_> = tickets
        .iter()
        .map(|t| (t.id.as_str(), t.time_spent))
        .collect();
    assert_eq!(read, [("12", Some(3_600)), ("34", Some(0))]);
    assert_eq!(tickets[0].url, format!("{}issues/12", stub.url));
    // Duplicates and invalid ids dropped, closed tickets included.
    assert_eq!(provider.estimate_calls_for_tickets(2), 1);
    assert_eq!(provider.estimate_calls_for_tickets(101), 2);
    let requests = stub.requests().await;
    assert_eq!(
        requests[0].line,
        "GET /issues.json?issue_id=12,34&status_id=*&limit=100"
    );
}

#[tokio::test]
async fn a_list_without_spent_time_falls_back_to_each_page() {
    let first = format!(r#"{{"issues": [{}]}}"#, listed(12, None));
    let page = format!(r#"{{"issue": {}}}"#, listed(12, Some(2.0)));
    let second = format!(r#"{{"issues": [{}]}}"#, listed(34, None));
    let page2 = format!(r#"{{"issue": {}}}"#, listed(34, Some(0.5)));
    let stub = Stub::start(&[(200, &first), (200, &page), (200, &second), (200, &page2)]).await;
    let provider = stub.provider();

    let tickets = provider.fetch_tickets(&["12".into()]).await.unwrap();
    assert_eq!(tickets[0].time_spent, Some(7_200));
    // Learnt: this Redmine's list lacks spent time — every such ticket gets its page.
    let tickets = provider.fetch_tickets(&["34".into()]).await.unwrap();
    assert_eq!(tickets[0].time_spent, Some(1_800));
    assert_eq!(provider.estimate_calls_for_tickets(3), 4);

    let lines: Vec<_> = stub.requests().await.into_iter().map(|r| r.line).collect();
    assert_eq!(lines[1], "GET /issues/12.json");
    assert_eq!(lines[3], "GET /issues/34.json");
}

#[tokio::test]
async fn hidden_spent_time_is_not_read_page_by_page() {
    // No permission to see time: neither the list nor the page shows it.
    let list = format!(r#"{{"issues": [{}]}}"#, listed(12, None));
    let page = format!(r#"{{"issue": {}}}"#, listed(12, None));
    let stub = Stub::start(&[(200, &list), (200, &page), (200, &list)]).await;
    let provider = stub.provider();

    provider.fetch_tickets(&["12".into()]).await.unwrap();
    let again = provider.fetch_tickets(&["12".into()]).await.unwrap();
    assert_eq!(again[0].time_spent, None);
    assert_eq!(provider.estimate_calls_for_tickets(3), 1);
    let lines: Vec<_> = stub.requests().await.into_iter().map(|r| r.line).collect();
    assert!(lines[2].starts_with("GET /issues.json"), "{lines:?}");
}
