use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A lightweight reference to an external tracker ticket linked to a MR.
///
/// This struct is the only data type exchanged between the orchestrator
/// (`gitlab-tracker`) and any tracker plugin (Redmine, Jira, Trello, …).
/// It is intentionally flat and display-oriented — the orchestrator does not
/// need to know anything about the internal data model of the tracker.
/// Current schema version for [`LinkedTicket`].
///
/// Increment this constant whenever fields are added to or removed from `LinkedTicket`.
/// Any cached ticket whose `schema_version` is lower than this value will be invalidated
/// and re-fetched from the tracker on the next startup, ensuring stale caches never
/// silently hide newly added fields.
pub const LINKED_TICKET_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedTicket {
    /// Schema version — used to detect stale cached tickets after a struct upgrade.
    /// Defaults to `0` when absent (pre-versioning cache entries), which guarantees
    /// they are always invalidated on first run after this field was introduced.
    #[serde(default)]
    pub schema_version: u32,
    /// Numeric or alphanumeric identifier as shown in the tracker UI (e.g. "1234", "PROJ-42").
    pub id: String,
    /// Short title / subject of the ticket.
    pub subject: String,
    /// Human-readable status label (e.g. "In Progress", "Resolved").
    pub status: String,
    /// Direct URL to open the ticket in a browser.
    pub url: String,
    /// Original creator of the ticket (display name).
    /// `None` when not provided by the tracker.
    #[serde(default)]
    pub author: Option<String>,
    /// User currently assigned to the ticket (display name).
    /// `None` when unassigned or not supported by the tracker.
    #[serde(default)]
    pub assignee: Option<String>,
    /// Estimated time to complete the ticket, in seconds (e.g. from Redmine `/estimate`).
    /// `None` when not set or not supported by the tracker.
    #[serde(default)]
    pub time_estimate: Option<u32>,
    /// Time already spent on the ticket, in seconds (e.g. from Redmine time entries).
    /// `None` when not set or not supported by the tracker.
    #[serde(default)]
    pub time_spent: Option<u32>,
    /// Remaining time (ETC) in seconds as reported by the tracker.
    /// `None` when not set or not supported by the tracker.
    #[serde(default)]
    pub time_remaining: Option<u32>,
    /// Type / category of the ticket as defined by the tracker (e.g. "Bug", "Evolution").
    /// The label is tracker-specific and may be in any language — do not hardcode colour logic
    /// on its value; use `label_colors` in `redmine.yaml` instead.
    /// `None` when not provided by the tracker.
    #[serde(default)]
    pub tracker_type: Option<String>,
    /// Priority label of the ticket as defined by the tracker (e.g. "High", "Low").
    /// Same caveat as `tracker_type` — colour mapping is user-configurable.
    /// `None` when not provided by the tracker.
    #[serde(default)]
    pub priority: Option<String>,
    /// Target version / sprint / release the ticket is assigned to (e.g. "v1.2.3", "Sprint 42").
    /// `None` when not set or not supported by the tracker.
    #[serde(default)]
    pub version: Option<String>,
    /// Start date of the ticket in `YYYY-MM-DD` format.
    /// `None` when not set or not supported by the tracker.
    #[serde(default)]
    pub start_date: Option<String>,
    /// Completion percentage as reported by the tracker (0–100).
    /// `None` when not set or not supported by the tracker.
    #[serde(default)]
    pub done_ratio: Option<u32>,
}

/// A time-tracking activity category as defined in the tracker (e.g. Redmine enumerations).
///
/// Activities are fetched once at startup and cached in `App` to populate
/// the Log Time popup selector without a network round-trip per keypress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Activity {
    /// Numeric identifier used when submitting a time entry via the API.
    pub id: u32,
    /// Human-readable label shown in the popup selector (e.g. "Development", "Design").
    pub name: String,
}

/// A single time entry recorded on a tracker ticket.
///
/// Returned by `fetch_time_entries` and displayed in the Inspector's TimeLog view.
#[derive(Debug, Clone)]
pub struct TimeEntry {
    /// Internal tracker identifier for this entry.
    pub id: u64,
    /// Duration in hours as stored by the tracker.
    pub hours: f32,
    /// Activity category associated with this entry.
    pub activity: Activity,
    /// Free-text comment left by the user when logging time.
    pub comment: String,
    /// Display name of the user who logged the time.
    pub user: String,
    /// Date on which the time was spent, in `YYYY-MM-DD` format.
    pub spent_on: String,
}

/// Payload sent to `log_time` when the user confirms the Log Time popup.
///
/// All fields are required — the popup enforces them before submission.
#[derive(Debug, Clone)]
pub struct TimeEntryRequest {
    /// Duration in hours (e.g. `1.5` for 1h30).
    pub hours: f32,
    /// Identifier of the selected activity category.
    pub activity_id: u32,
    /// Optional free-text comment. May be empty.
    pub comment: String,
    /// Date on which the time was spent, in `YYYY-MM-DD` format (defaults to today).
    pub spent_on: String,
}

/// Raw colour maps for badge labels, expressed as plain `(bg, fg)` string pairs.
///
/// Strings use the same vocabulary as `AppConfig::parse_color` in `gitlab-tracker`:
/// named colours (`"red"`, `"cyan"`, `"dark_gray"`, …) or 6-digit hex (`"#ff6600"`).
///
/// This type lives in `core` so every provider can return it without depending on
/// `ratatui`. The orchestrator (`gitlab-tracker`) is responsible for converting the
/// strings to `ratatui::Color` values via its own `parse_color` function.
#[derive(Debug, Clone, Default)]
pub struct LabelColorMaps {
    /// Colour map for tracker-type labels (e.g. "Bug", "Evolution").
    /// Keys are matched case-insensitively by the renderer; `"*"` is a catch-all fallback.
    pub tracker_type: HashMap<String, (String, String)>,
    /// Colour map for priority labels (e.g. "Normal", "High").
    /// Keys are matched case-insensitively by the renderer; `"*"` is a catch-all fallback.
    pub priority: HashMap<String, (String, String)>,
}

/// Contract that every external tracker integration must implement.
///
/// # Design
/// - The orchestrator holds a `Box<dyn TrackerProvider + Send + Sync>` and
///   calls `detect_ticket_id` + `fetch_ticket` without knowing the concrete type.
/// - Each implementation lives in its own crate (e.g. `gitlab-tracker-redmine`).
/// - Adding a new tracker (Jira, Trello, Linear …) only requires a new crate
///   that implements this trait — no change to the orchestrator logic.
///
/// # Thread safety
/// `Send + Sync` is required because the provider is shared across async tasks.
#[async_trait]
pub trait TrackerProvider: Send + Sync {
    /// Human-readable name of the tracker (e.g. "Redmine", "Jira").
    /// Used for logging and UI labels.
    fn name(&self) -> &'static str;

    /// Attempts to extract a ticket identifier from the MR title and/or description.
    ///
    /// Returns the raw ticket id string (e.g. "1234") when found, `None` otherwise.
    /// Implementations should prefer the title over the description when both match.
    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String>;

    /// Fetches the full ticket details for the given raw ticket id.
    ///
    /// Returns `None` on network error, authentication failure, or when the
    /// ticket does not exist. The caller is responsible for caching results.
    async fn fetch_ticket(&self, ticket_id: &str) -> Option<LinkedTicket>;

    /// Builds the direct URL to the ticket from its id.
    ///
    /// This is a pure helper that may be called without a network round-trip
    /// (e.g. to open the browser immediately on keypress while the fetch is pending).
    fn ticket_url(&self, ticket_id: &str) -> String;

    /// Returns the badge colour maps for tracker-type and priority labels.
    ///
    /// The returned strings are colour names or hex codes understood by the
    /// orchestrator's `parse_color` function (`"red"`, `"#ff6600"`, …).
    ///
    /// Default implementation returns empty maps — the renderer then falls back
    /// to its hard-coded default (dark_gray background, white text).
    /// Override this in your provider to expose your config file's colour settings.
    fn label_colors(&self) -> LabelColorMaps {
        LabelColorMaps::default()
    }

    /// Fetches the list of time-tracking activity categories available in the tracker.
    ///
    /// Called once at startup (or on first popup open) and cached in `App`.
    /// Default implementation returns an empty list (opt-in capability).
    async fn fetch_activities(&self) -> Vec<Activity> {
        vec![]
    }

    /// Fetches all time entries recorded on the given ticket.
    ///
    /// Displayed in the Inspector's TimeLog view. Default returns empty (opt-in).
    async fn fetch_time_entries(&self, _ticket_id: &str) -> Vec<TimeEntry> {
        vec![]
    }

    /// Submits a new time entry on the given ticket.
    ///
    /// Returns `Ok(())` on success, or an error message string suitable for
    /// displaying inline in the TUI. Default returns an unsupported error.
    async fn log_time(&self, _ticket_id: &str, _entry: TimeEntryRequest) -> Result<(), String> {
        Err("Time logging not supported by this tracker".into())
    }
}
