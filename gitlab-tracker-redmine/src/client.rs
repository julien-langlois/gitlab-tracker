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

#[derive(Debug, Clone, Deserialize)]
pub struct RedmineIssue {
    pub id: u64,
    pub subject: String,
    pub status: RedmineStatus,
    /// Original creator of the issue.
    pub author: Option<RedmineUser>,
    /// User currently assigned to the issue (`assigned_to` in the Redmine API).
    pub assigned_to: Option<RedmineUser>,
    /// Estimated time in hours as returned by Redmine (`estimated_hours`).
    /// Converted to seconds when building [`LinkedTicket`].
    pub estimated_hours: Option<f32>,
    /// Total time spent in hours as returned by Redmine (`spent_hours`).
    /// Converted to seconds when building [`LinkedTicket`].
    pub spent_hours: Option<f32>,
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
/// Calls `POST /time_entries.json` with the JSON payload.
/// Returns `Ok(())` on success or an error message on failure.
pub async fn log_time(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    ticket_id: &str,
    entry: TimeEntryRequest,
) -> Result<(), String> {
    let url = format!("{}/time_entries.json", base_url.trim_end_matches('/'));

    let body = PostTimeEntryBody {
        time_entry: PostTimeEntry {
            issue_id: ticket_id.to_string(),
            hours: entry.hours,
            activity_id: entry.activity_id,
            comments: entry.comment,
            spent_on: entry.spent_on,
        },
    };

    tracing::debug!(url = %url, ticket_id = %ticket_id, "Posting Redmine time entry");

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
