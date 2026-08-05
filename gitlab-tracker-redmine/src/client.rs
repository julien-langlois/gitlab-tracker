use gitlab_tracker_core::{Activity, TimeEntry, TimeEntryRequest};
use serde::{Deserialize, Serialize};

/// Minimal Redmine issue fields needed to populate a [`LinkedTicket`].
///
/// The Redmine REST API wraps the issue under an `"issue"` key:
/// `GET /issues/{id}.json` → `{ "issue": { ... } }`
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
    pub name: String,
}

/// Envelope returned by `GET /issues/{id}.json`.
#[derive(Debug, Deserialize)]
struct IssueEnvelope {
    issue: RedmineIssue,
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

/// Fetches a single Redmine issue by its numeric ID.
///
/// Uses the `X-Redmine-API-Key` header for authentication (standard Redmine REST API).
/// Returns `None` on any network error, non-2xx response, or JSON parse failure.
pub async fn fetch_issue(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
) -> Option<RedmineIssue> {
    let url = format!(
        "{}/issues/{}.json",
        base_url.trim_end_matches('/'),
        ticket_id
    );

    tracing::debug!(url = %url, "Fetching Redmine issue");

    let resp = http
        .get(&url)
        .header("X-Redmine-API-Key", token)
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, url = %url, "Redmine HTTP request failed");
        })
        .ok()?;

    if !resp.status().is_success() {
        tracing::warn!(
            status = %resp.status(),
            url = %url,
            "Redmine API returned a non-2xx status"
        );
        return None;
    }

    resp.json::<IssueEnvelope>()
        .await
        .map(|env| env.issue)
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to deserialize Redmine issue response");
        })
        .ok()
}

/// Fetches the list of time-tracking activity categories from Redmine.
///
/// Calls `GET /enumerations/time_entry_activities.json`.
/// Returns an empty `Vec` on any failure so the caller can degrade gracefully.
pub async fn fetch_activities(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Vec<Activity> {
    let url = format!(
        "{}/enumerations/time_entry_activities.json",
        base_url.trim_end_matches('/')
    );

    tracing::debug!(url = %url, "Fetching Redmine time entry activities");

    let resp = match http
        .get(&url)
        .header("X-Redmine-API-Key", token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "Redmine activities request failed");
            return vec![];
        }
    };

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "Redmine activities returned non-2xx");
        return vec![];
    }

    match resp.json::<ActivitiesEnvelope>().await {
        Ok(env) => env
            .time_entry_activities
            .into_iter()
            .map(|a| Activity {
                id: a.id,
                name: a.name,
            })
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to deserialize Redmine activities");
            vec![]
        }
    }
}

/// Fetches all time entries recorded on a Redmine issue.
///
/// Calls `GET /time_entries.json?issue_id={id}`.
/// Returns an empty `Vec` on any failure.
pub async fn fetch_time_entries(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
) -> Vec<TimeEntry> {
    // Redmine exposes time entries via a global endpoint filtered by issue_id.
    // The route `/issues/{id}/time_entries.json` does not exist and returns 404.
    let url = format!(
        "{}/time_entries.json?issue_id={}&limit=100",
        base_url.trim_end_matches('/'),
        ticket_id
    );

    tracing::debug!(url = %url, "Fetching Redmine time entries");

    let resp = match http
        .get(&url)
        .header("X-Redmine-API-Key", token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "Redmine time entries request failed");
            return vec![];
        }
    };

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "Redmine time entries returned non-2xx");
        return vec![];
    }

    match resp.json::<TimeEntriesEnvelope>().await {
        Ok(env) => env
            .time_entries
            .into_iter()
            .map(|e| TimeEntry {
                id: e.id,
                hours: e.hours,
                activity: Activity {
                    id: e.activity.id,
                    name: e.activity.name,
                },
                comment: e.comments,
                user: e.user.name,
                spent_on: e.spent_on,
            })
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to deserialize Redmine time entries");
            vec![]
        }
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
