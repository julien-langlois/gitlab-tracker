use gitlab_tracker_core::{
    Activity, TicketTransitionTarget, TimeEntry, TimeEntryRequest, TrackerError,
};
use serde::{Deserialize, Serialize};

/// A Redmine user reference as returned in nested fields (`author`, `assigned_to`).
#[derive(Debug, Clone, Deserialize)]
pub struct RedmineUser {
    pub name: String,
}

/// A named reference as returned by Redmine for nested objects such as tracker type,
/// priority, or fixed version (e.g. `{ "id": 2, "name": "Evolution" }`).
#[derive(Debug, Clone, Deserialize)]
pub struct RedmineNamedRef {
    pub name: String,
}

/// Minimal Redmine issue fields needed to populate a [`LinkedTicket`].
///
/// The Redmine REST API wraps the issue under an `"issue"` key:
/// `GET /issues/{id}.json` → `{ "issue": { ... } }`
#[derive(Debug, Clone, Deserialize)]
pub struct RedmineIssue {
    pub id: u64,
    pub subject: String,
    pub status: RedmineStatus,
    /// Tracker type (e.g. "Bug", "Evolution") — the `tracker` field in the Redmine API.
    pub tracker: Option<RedmineNamedRef>,
    /// Priority label (e.g. "Normal", "High") — the `priority` field in the Redmine API.
    pub priority: Option<RedmineNamedRef>,
    /// Target version / sprint — the `fixed_version` field in the Redmine API.
    pub fixed_version: Option<RedmineNamedRef>,
    /// Original creator of the issue.
    pub author: Option<RedmineUser>,
    /// User currently assigned to the issue (`assigned_to` in the Redmine API).
    pub assigned_to: Option<RedmineUser>,
    /// Start date in `YYYY-MM-DD` format — the `start_date` field in the Redmine API.
    pub start_date: Option<String>,
    /// Completion percentage (0–100) — the `done_ratio` field in the Redmine API.
    pub done_ratio: Option<u32>,
    /// Estimated time in hours (`estimated_hours` — standard Redmine field).
    /// Converted to seconds when building [`LinkedTicket`].
    pub estimated_hours: Option<f32>,
    /// Total time spent in hours (`spent_hours` — standard Redmine field).
    /// Converted to seconds when building [`LinkedTicket`].
    pub spent_hours: Option<f32>,
    /// Estimate to Complete (ETC) in hours — provided by some Redmine plugins (e.g. Redmine Budget).
    /// When present, used directly to compute the updated ETC on time entry submission.
    /// Absent on vanilla Redmine instances; always treated as optional.
    pub remaining_hours: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedmineStatus {
    pub id: u64,
    pub name: String,
}

/// Envelope returned by `GET /issues/{id}.json`.
#[derive(Debug, Deserialize)]
struct IssueEnvelope {
    issue: RedmineIssue,
}

/// Envelope returned by `GET /issue_statuses.json`.
#[derive(Debug, Deserialize)]
struct IssueStatusesEnvelope {
    issue_statuses: Vec<RedmineStatus>,
}

// ── Time entry activity ──────────────────────────────────────────────────────

/// Activity enumeration entry as returned by Redmine.
/// `GET /enumerations/time_entry_activities.json`
#[derive(Debug, Clone, Deserialize)]
struct RedmineActivity {
    pub id: u32,
    pub name: String,
}

/// Envelope returned by `GET /enumerations/time_entry_activities.json`.
#[derive(Debug, Deserialize)]
struct ActivitiesEnvelope {
    time_entry_activities: Vec<RedmineActivity>,
}

// ── Time entries ─────────────────────────────────────────────────────────────

/// A single time entry as returned by Redmine.
/// `GET /time_entries.json?issue_id={id}`
#[derive(Debug, Clone, Deserialize)]
struct RedmineTimeEntry {
    pub id: u64,
    pub hours: f32,
    pub activity: RedmineActivity,
    #[serde(default)]
    pub comments: String,
    pub user: RedmineUser,
    pub spent_on: String,
}

/// Envelope returned by `GET /issues/{id}/time_entries.json`.
#[derive(Debug, Deserialize)]
struct TimeEntriesEnvelope {
    time_entries: Vec<RedmineTimeEntry>,
    /// Total number of entries across all pages (absent on very old Redmine versions:
    /// only the first page is then read).
    total_count: Option<usize>,
}

// ── POST payload ─────────────────────────────────────────────────────────────

/// Body sent to `POST /time_entries.json`.
#[derive(Debug, Serialize)]
struct PostTimeEntryBody {
    time_entry: PostTimeEntry,
}

#[derive(Debug, Serialize)]
struct PostTimeEntry {
    issue_id: String,
    hours: f32,
    activity_id: u32,
    comments: String,
    spent_on: String,
    /// Time budget in hours — plugin field (e.g. Redmine Budget).
    /// Omitted when the instance does not support it.
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_hours: Option<f32>,
    /// Estimate to Complete (ETC) in hours — plugin field (e.g. Redmine Budget).
    /// Omitted when the instance does not support it.
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_hours: Option<f32>,
}

// ── ETC computation ───────────────────────────────────────────────────────────

/// Computes the updated Estimate to Complete (ETC) after a new time entry is submitted.
///
/// **Strategy 1 — plugin field** (preferred):
/// Use `remaining_hours` from the issue directly — this field is maintained by
/// Redmine plugins such as Redmine Budget and always reflects the current ETC.
/// `ETC = issue.remaining_hours − new_hours`
///
/// **Strategy 2 — estimate fallback**:
/// When the plugin field is absent (vanilla Redmine), derive from the estimate:
/// `ETC = estimated_hours − spent_hours − new_hours`
///
/// Both strategies clamp the result to `0.0` — ETC cannot be negative.
/// Returns `None` when neither field is available (no-op for the caller).
pub fn compute_etc(issue: &RedmineIssue, new_hours: f32) -> Option<f32> {
    // Strategy 1: remaining_hours is directly tracked by the Redmine plugin.
    if let Some(remaining) = issue.remaining_hours {
        return Some((remaining - new_hours).max(0.0));
    }

    // Strategy 2: derive from estimate and already-spent time.
    if let (Some(estimated), Some(spent)) = (issue.estimated_hours, issue.spent_hours) {
        return Some((estimated - spent - new_hours).max(0.0));
    }

    None
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Maps an HTTP status to the typed error the orchestrator can act on.
fn status_error(status: reqwest::StatusCode) -> TrackerError {
    let msg = format!("Redmine API returned HTTP {status}");
    match status.as_u16() {
        401 | 403 => TrackerError::Auth(msg),
        404 => TrackerError::NotFound(msg),
        _ => TrackerError::Other(msg),
    }
}

/// `GET url` with the API key, then deserializes the JSON body.
async fn get_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<T, TrackerError> {
    tracing::debug!(url = %url, "Redmine GET");
    let resp = http
        .get(url)
        .header("X-Redmine-API-Key", token)
        .send()
        .await
        .map_err(|e| TrackerError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    resp.json::<T>()
        .await
        .map_err(|e| TrackerError::Other(format!("Invalid Redmine response: {e}")))
}

/// Fetches all issue statuses configured on the Redmine instance.
///
/// Calls `GET /issue_statuses.json`. The returned `(id, label)` pairs are meant to be
/// surfaced by callers so users can configure GitLab-to-Redmine transition mappings.
pub async fn fetch_issue_statuses(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<Vec<TicketTransitionTarget>, TrackerError> {
    let url = format!("{}/issue_statuses.json", base_url.trim_end_matches('/'));

    get_json::<IssueStatusesEnvelope>(http, &url, token)
        .await
        .map(|env| {
            env.issue_statuses
                .into_iter()
                .map(|status| TicketTransitionTarget {
                    id: status.id.to_string(),
                    label: status.name,
                })
                .collect()
        })
}

/// Redmine issue ids are numeric. The id comes from a user-configurable regex
/// capture and is interpolated into request URLs sent with the API key, so anything
/// else (`/`, `?`, `..`) could target another endpoint: reject it.
fn is_valid_ticket_id(ticket_id: &str) -> bool {
    !ticket_id.is_empty() && ticket_id.bytes().all(|b| b.is_ascii_digit())
}

/// Rejects a non-numeric ticket id before it reaches a request URL.
fn check_ticket_id(ticket_id: &str) -> Result<(), TrackerError> {
    if is_valid_ticket_id(ticket_id) {
        Ok(())
    } else {
        tracing::warn!(ticket_id = %ticket_id, "Rejected non-numeric Redmine ticket id");
        Err(TrackerError::Other(format!(
            "Invalid ticket id \"{ticket_id}\""
        )))
    }
}

/// Fetches a single Redmine issue by its numeric ID.
///
/// Uses the `X-Redmine-API-Key` header for authentication (standard Redmine REST API).
pub async fn fetch_issue(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
) -> Result<RedmineIssue, TrackerError> {
    check_ticket_id(ticket_id)?;
    let url = format!(
        "{}/issues/{}.json",
        base_url.trim_end_matches('/'),
        ticket_id
    );
    get_json::<IssueEnvelope>(http, &url, token)
        .await
        .map(|env| env.issue)
}

/// Fetches the list of time-tracking activity categories from Redmine.
///
/// Calls `GET /enumerations/time_entry_activities.json`.
pub async fn fetch_activities(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<Vec<Activity>, TrackerError> {
    let url = format!(
        "{}/enumerations/time_entry_activities.json",
        base_url.trim_end_matches('/')
    );
    let env = get_json::<ActivitiesEnvelope>(http, &url, token).await?;
    Ok(env
        .time_entry_activities
        .into_iter()
        .map(|a| Activity {
            id: a.id,
            name: a.name,
        })
        .collect())
}

/// Page size of `GET /time_entries.json` (Redmine's maximum `limit`).
const TIME_ENTRIES_PAGE: usize = 100;
/// Safety cap on the number of pages read for one ticket (10 000 entries).
const TIME_ENTRIES_MAX_PAGES: usize = 100;

/// Offset of the next page to read, or `None` when every entry has been read.
/// `total_count` is Redmine's own total; an empty page also ends the loop.
fn next_offset(read: usize, page_len: usize, total_count: usize) -> Option<usize> {
    (page_len > 0 && read < total_count).then_some(read)
}

/// Fetches all time entries recorded on a Redmine issue, following pagination.
///
/// Calls `GET /time_entries.json?issue_id={id}&limit=100&offset=…`.
pub async fn fetch_time_entries(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
) -> Result<Vec<TimeEntry>, TrackerError> {
    check_ticket_id(ticket_id)?;
    let mut entries = Vec::new();
    let mut offset = Some(0);
    for _ in 0..TIME_ENTRIES_MAX_PAGES {
        let Some(current) = offset else {
            return Ok(entries);
        };
        // Redmine exposes time entries via a global endpoint filtered by issue_id.
        // The route `/issues/{id}/time_entries.json` does not exist and returns 404.
        let url = format!(
            "{}/time_entries.json?issue_id={}&limit={TIME_ENTRIES_PAGE}&offset={current}",
            base_url.trim_end_matches('/'),
            ticket_id
        );
        let env = get_json::<TimeEntriesEnvelope>(http, &url, token).await?;
        let page_len = env.time_entries.len();
        entries.extend(env.time_entries.into_iter().map(|e| TimeEntry {
            id: e.id,
            hours: e.hours,
            activity: Activity {
                id: e.activity.id,
                name: e.activity.name,
            },
            comment: e.comments,
            user: e.user.name,
            spent_on: e.spent_on,
        }));
        offset = next_offset(entries.len(), page_len, env.total_count.unwrap_or(0));
    }
    tracing::warn!(
        ticket_id,
        "Time entries truncated at the pagination safety cap"
    );
    Ok(entries)
}

/// Updates the Redmine issue status.
///
/// Calls `PUT /issues/{id}.json` with a `status_id`. Redmine may still reject the
/// transition depending on the issue tracker, current status, workflow rules, or API user role.
pub async fn update_issue_status(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
    status_id: u64,
) -> Result<(), String> {
    if !is_valid_ticket_id(ticket_id) {
        tracing::warn!(ticket_id = %ticket_id, "Rejected non-numeric Redmine ticket id");
        return Err(format!("Invalid ticket id \"{ticket_id}\""));
    }
    #[derive(Debug, Serialize)]
    struct PutIssueBody {
        issue: PutIssue,
    }

    #[derive(Debug, Serialize)]
    struct PutIssue {
        status_id: u64,
    }

    let url = format!(
        "{}/issues/{}.json",
        base_url.trim_end_matches('/'),
        ticket_id
    );
    let body = PutIssueBody {
        issue: PutIssue { status_id },
    };

    tracing::debug!(
        url = %url,
        ticket_id = %ticket_id,
        status_id = status_id,
        "Updating Redmine issue status"
    );

    let resp = http
        .put(&url)
        .header("X-Redmine-API-Key", token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Redmine API error: HTTP {}", resp.status()))
    }
}

/// Submits a new time entry on the given Redmine issue.
///
/// Accepts a pre-fetched `issue` so the caller can reuse it without an extra
/// network round-trip. When the issue exposes `remaining_hours` or `estimated_hours`,
/// the ETC is computed and attached directly to the time entry payload — this is
/// how Redmine Budget plugin tracks remaining time (fields on the entry, not the issue).
///
/// Both `budget_hours` and `remaining_hours` are omitted from the payload when the
/// issue does not expose the necessary fields, ensuring compatibility with vanilla
/// Redmine instances.
///
/// Calls `POST /time_entries.json`.
/// Returns `Ok(())` on success or an error string suitable for inline TUI display.
pub async fn log_time(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
    entry: TimeEntryRequest,
    issue: Option<&RedmineIssue>,
) -> Result<(), String> {
    if !is_valid_ticket_id(ticket_id) {
        tracing::warn!(ticket_id = %ticket_id, "Rejected non-numeric Redmine ticket id");
        return Err(format!("Invalid ticket id \"{ticket_id}\""));
    }
    let url = format!("{}/time_entries.json", base_url.trim_end_matches('/'));

    // Compute budget and ETC from the issue when available.
    // Both are None on vanilla Redmine — skipped_serializing_if handles the rest.
    let budget_hours = issue.and_then(|i| i.estimated_hours);
    let remaining_hours = issue.and_then(|i| compute_etc(i, entry.hours));

    let body = PostTimeEntryBody {
        time_entry: PostTimeEntry {
            issue_id: ticket_id.to_string(),
            hours: entry.hours,
            activity_id: entry.activity_id,
            comments: entry.comment,
            spent_on: entry.spent_on,
            budget_hours,
            remaining_hours,
        },
    };

    tracing::debug!(
        url = %url,
        ticket_id = %ticket_id,
        budget_hours = ?budget_hours,
        remaining_hours = ?remaining_hours,
        "Posting Redmine time entry"
    );

    let resp = http
        .post(&url)
        .header("X-Redmine-API-Key", token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Redmine API error: HTTP {}", resp.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_maps_to_typed_errors() {
        use reqwest::StatusCode;
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            TrackerError::Auth(_)
        ));
        assert!(matches!(
            status_error(StatusCode::FORBIDDEN),
            TrackerError::Auth(_)
        ));
        assert!(matches!(
            status_error(StatusCode::NOT_FOUND),
            TrackerError::NotFound(_)
        ));
        assert!(matches!(
            status_error(StatusCode::INTERNAL_SERVER_ERROR),
            TrackerError::Other(_)
        ));
    }

    #[test]
    fn time_entries_pagination_stops() {
        assert_eq!(next_offset(100, 100, 250), Some(100));
        assert_eq!(next_offset(250, 50, 250), None); // all read
        assert_eq!(next_offset(100, 100, 0), None); // no total_count: first page only
        assert_eq!(next_offset(100, 0, 250), None); // empty page: stop anyway
    }

    #[test]
    fn ticket_id_must_be_numeric() {
        assert!(is_valid_ticket_id("1234"));
        for bad in ["", "12/../users", "1?key=x", "١٢", "12 ", "-1"] {
            assert!(!is_valid_ticket_id(bad), "{bad:?} accepted");
        }
    }
}
