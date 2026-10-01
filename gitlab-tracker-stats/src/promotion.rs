//! Promotion lead time: how long a merged MR takes to reach the other tracked
//! branches (preproduction, master…) after its merge on its target branch.

use serde::{Deserialize, Serialize};

use crate::aggregator::percentile;

/// How an MR reached a branch other than its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrivalKind {
    /// Under the same SHA: merge, fast-forward, `reset --hard`.
    Merge,
    /// Copied with a `(cherry picked from commit …)` trailer.
    CherryPick,
    /// Copied without trailer (same author, author date and title).
    Copy,
}

impl ArrivalKind {
    /// Value stored in the `how` column.
    pub fn as_str(self) -> &'static str {
        match self {
            ArrivalKind::Merge => "merge",
            ArrivalKind::CherryPick => "cherry_pick",
            ArrivalKind::Copy => "copy",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "merge" => Some(ArrivalKind::Merge),
            "cherry_pick" => Some(ArrivalKind::CherryPick),
            "copy" => Some(ArrivalKind::Copy),
            _ => None,
        }
    }
}

/// First arrival of an MR on a tracked branch other than its target.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchArrival {
    pub project_id: String,
    pub mr_id: String,
    pub branch: String,
    pub arrived_at: chrono::DateTime<chrono::Utc>,
    pub how: ArrivalKind,
    /// GitLab `merged_at` (RFC 3339); `None` for an MR copied before its merge.
    pub merged_at: Option<String>,
}

/// Promotion lead time to one branch, over the arrivals of the report window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionLeadTime {
    pub branch: String,
    /// Arrivals with a known merge date (the lead time sample).
    pub arrivals: usize,
    pub median_hours: Option<f64>,
    pub p90_hours: Option<f64>,
    pub max_hours: Option<f64>,
    /// Share of those arrivals that were copies (cherry-pick, with or without `-x`).
    pub copied_share: f64,
}

/// Lead times per branch, sorted by branch name. An arrival before the merge
/// (copied from an open MR) counts as 0; one without merge date is skipped.
pub fn lead_times(arrivals: &[BranchArrival]) -> Vec<PromotionLeadTime> {
    let mut by_branch: std::collections::BTreeMap<&str, (Vec<f64>, usize)> = Default::default();
    for arrival in arrivals {
        let Some(merged) = arrival
            .merged_at
            .as_deref()
            .and_then(|m| chrono::DateTime::parse_from_rfc3339(m).ok())
        else {
            continue;
        };
        let hours =
            (arrival.arrived_at - merged.with_timezone(&chrono::Utc)).num_seconds() as f64 / 3600.0;
        let entry = by_branch.entry(&arrival.branch).or_default();
        entry.0.push(hours.max(0.0));
        entry.1 += usize::from(arrival.how != ArrivalKind::Merge);
    }
    by_branch
        .into_iter()
        .map(|(branch, (mut hours, copies))| {
            hours.sort_by(f64::total_cmp);
            PromotionLeadTime {
                branch: branch.to_string(),
                arrivals: hours.len(),
                median_hours: percentile(&hours, 50.0),
                p90_hours: percentile(&hours, 90.0),
                max_hours: hours.last().copied(),
                copied_share: copies as f64 / hours.len() as f64,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(
        branch: &str,
        merged: Option<&str>,
        arrived: &str,
        how: ArrivalKind,
    ) -> BranchArrival {
        BranchArrival {
            project_id: "p".into(),
            mr_id: "1".into(),
            branch: branch.into(),
            arrived_at: arrived.parse().unwrap(),
            how,
            merged_at: merged.map(str::to_string),
        }
    }

    #[test]
    fn lead_time_per_branch_from_merge_to_arrival() {
        use ArrivalKind::*;
        let rows = [
            arrival(
                "preprod",
                Some("2026-09-01T10:00:00Z"),
                "2026-09-01T12:00:00Z",
                Merge,
            ),
            arrival(
                "preprod",
                Some("2026-09-01T10:00:00+02:00"),
                "2026-09-01T14:00:00Z",
                Copy,
            ),
            // Copied from the open MR before its merge: 0, not negative.
            arrival(
                "preprod",
                Some("2026-09-02T10:00:00Z"),
                "2026-09-02T09:00:00Z",
                CherryPick,
            ),
            arrival("master", None, "2026-09-03T10:00:00Z", Merge), // no merge date: skipped
        ];

        let leads = lead_times(&rows);
        assert_eq!(leads.len(), 1);
        let preprod = &leads[0];
        assert_eq!((preprod.branch.as_str(), preprod.arrivals), ("preprod", 3));
        assert_eq!(preprod.max_hours, Some(6.0)); // 08:00Z → 14:00Z
        assert_eq!(preprod.median_hours, Some(2.0));
        assert!((preprod.copied_share - 2.0 / 3.0).abs() < 1e-9);
    }
}
