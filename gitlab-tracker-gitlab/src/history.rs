//! What a project merged over a period, read from GitLab and kept between
//! launches (in tracker.db): the merge dates that give the team's pace, and the
//! merges as far as file ownership goes (who wrote, reviewed, approved what).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

/// How far back the project's merges are read.
pub const MERGE_HISTORY_DAYS: i64 = 56;

/// How far back the merges that tell who knows a file are kept.
pub const HISTORY_DAYS: i64 = 90;

/// When the project's MRs merged over the last [`MERGE_HISTORY_DAYS`] days,
/// kept between reads and between launches (`merges_<hash>.json` beside the
/// project's state file): the first read takes the whole period, the next ones
/// only what was merged (or updated) since, and are merged into it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MergeHistory {
    /// Merge date by MR iid.
    #[serde(default)]
    pub merged: std::collections::BTreeMap<u64, String>,
    /// Up to when it is complete (RFC 3339): every MR updated before is in.
    #[serde(default)]
    pub read_at: Option<String>,
}

impl MergeHistory {
    pub fn read_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let at = chrono::DateTime::parse_from_rfc3339(self.read_at.as_deref()?).ok()?;
        Some(at.with_timezone(&chrono::Utc))
    }

    /// Complete up to the last day: fit to draw a pace from. A first read cut
    /// short (more merges than its pages hold) has only the oldest weeks yet,
    /// and is not.
    pub fn is_current(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.read_at()
            .is_some_and(|at| now - at <= chrono::Duration::days(1))
    }
}

/// One merged MR, as far as ownership goes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Merge {
    /// RFC 3339.
    pub merged_at: String,
    /// Logins.
    pub author: String,
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub approvers: Vec<String>,
    /// The files it changed.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The project's merged MRs of the period, kept on disk between launches and
/// read again from where they stopped.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct History {
    /// By MR iid.
    #[serde(default)]
    pub merges: BTreeMap<u64, Merge>,
    /// Names by login.
    #[serde(default)]
    pub people: BTreeMap<String, String>,
    /// Up to when it is complete (RFC 3339): every MR merged before is in.
    #[serde(default)]
    pub read_at: Option<String>,
}

impl History {
    pub fn read_at(&self) -> Option<DateTime<Utc>> {
        time(self.read_at.as_deref()?)
    }

    /// Merges older than the period dropped.
    pub fn trim(&mut self, now: DateTime<Utc>) {
        let start = now - chrono::Duration::days(HISTORY_DAYS);
        self.merges
            .retain(|_, m| time(&m.merged_at).is_some_and(|t| t >= start));
    }
}

fn time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}
