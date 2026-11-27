//! The four DORA measures month by month, for the Delivery page's trend:
//! deployment frequency and lead time for changes from the merges and branch
//! arrivals the database keeps, change failure rate and time to restore from
//! the deployments it keeps too (with reverts and hotfixes). A month the
//! database does not cover yet has no point; a figure on too few cases has
//! none either.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Months, NaiveDate, TimeZone, Utc};

use crate::db::{Deployment, SnapshotQuery, SqliteStatsDb, StatsError, StoredSnapshot};
use crate::promotion::BranchArrival;

/// Below this many cases in a month, a lead time or a failure rate says
/// nothing: the month has no point.
pub const MIN_CASES: usize = 3;

/// One month of the four measures.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DoraMonth {
    /// Its first day.
    pub start: NaiveDate,
    /// What reached production: deployments, else MRs (see `from_deployments`).
    pub shipped: usize,
    /// Read from the deployments kept, not from the MRs reaching production.
    pub from_deployments: bool,
    /// Deployments (or MRs) a week; `None` before the database covers the month.
    pub per_week: Option<f64>,
    /// Median days from an MR's opening to production, over `leads` MRs.
    pub lead: Option<f64>,
    pub leads: usize,
    /// Failed deployments, reverts and hotfixes, over the changes.
    pub failures: usize,
    pub changes: usize,
    pub failure_rate: Option<f64>,
    /// Median days from a failure to its fix, over `restores` incidents.
    pub restore: Option<f64>,
    pub restores: usize,
}

impl DoraMonth {
    /// The database covers the month: its figures are read, not missing.
    pub fn covered(&self) -> bool {
        self.per_week.is_some()
    }
}

/// What the months are read from.
pub struct TrendInput<'a> {
    pub snapshots: &'a [StoredSnapshot],
    pub arrivals: &'a [BranchArrival],
    pub deployments: &'a [Deployment],
    /// The production branch: the last tracked one.
    pub production: &'a str,
}

/// An MR merged, as the trend needs it.
struct Merged<'a> {
    title: &'a str,
    hotfix: bool,
    created: Option<DateTime<Utc>>,
    /// On production: merged into it, else arrived on it.
    delivered: Option<DateTime<Utc>>,
}

fn time(text: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn days_between(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    ((to - from).num_seconds() as f64 / 86_400.0).max(0.0)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

fn month_start(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

fn at_midnight(day: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap_or_default())
}

/// The `months` months up to `now`'s (the current one so far), oldest first.
pub fn monthly(input: &TrendInput, months: u32, now: DateTime<Utc>) -> Vec<DoraMonth> {
    let production = input.production;
    let arrived: HashMap<&str, DateTime<Utc>> = input
        .arrivals
        .iter()
        .filter(|a| a.branch == production)
        .map(|a| (a.mr_id.as_str(), a.arrived_at))
        .collect();
    let merged: Vec<Merged> = crate::aggregator::latest_per_mr(input.snapshots)
        .into_iter()
        .map(|i| &input.snapshots[i].snapshot)
        .filter(|s| s.state == "merged")
        .map(|s| Merged {
            title: &s.title,
            hotfix: s.labels.iter().any(|l| l.to_lowercase().contains("hotfix")),
            created: time(s.created_at.as_deref()),
            delivered: if s.target_branch == production {
                time(s.merged_at.as_deref())
            } else {
                arrived.get(s.mr_id.as_str()).copied()
            },
        })
        .collect();
    // Production's deployments: those of its branch, else of an environment
    // named like production (as the Delivery page reads them).
    let mut deployments: Vec<&Deployment> = input
        .deployments
        .iter()
        .filter(|d| d.reference == production)
        .collect();
    if deployments.is_empty() {
        deployments = input
            .deployments
            .iter()
            .filter(|d| d.environment.to_lowercase().contains("prod"))
            .collect();
    }
    deployments.sort_by_key(|d| d.created_at);
    // The database covers the months from that of the first merge, arrival
    // or deployment it holds; deployments count from the month after the first
    // one kept (that month may be cut).
    let first = merged
        .iter()
        .filter_map(|m| m.delivered.or(m.created))
        .chain(input.arrivals.iter().map(|a| a.arrived_at))
        .chain(deployments.iter().map(|d| d.created_at))
        .min();
    let deployments_from = deployments
        .first()
        .and_then(|d| month_start(d.created_at.date_naive()).checked_add_months(Months::new(1)));

    let this_month = month_start(now.date_naive());
    (0..months)
        .rev()
        .filter_map(|back| this_month.checked_sub_months(Months::new(back)))
        .map(|start| {
            let end = start
                .checked_add_months(Months::new(1))
                .unwrap_or(start)
                .min(now.date_naive().succ_opt().unwrap_or(start));
            let (from, to) = (at_midnight(start), at_midnight(end).min(now));
            let mut month = DoraMonth {
                start,
                shipped: 0,
                from_deployments: false,
                per_week: None,
                lead: None,
                leads: 0,
                failures: 0,
                changes: 0,
                failure_rate: None,
                restore: None,
                restores: 0,
            };
            if first.is_none_or(|first| month_start(first.date_naive()) > start) {
                return month;
            }
            let within = |t: DateTime<Utc>| t >= from && t < to;
            let delivered: Vec<&Merged> = merged
                .iter()
                .filter(|m| m.delivered.is_some_and(within))
                .collect();
            let reverts: Vec<&&Merged> = delivered
                .iter()
                .filter(|m| m.title.trim_start().to_lowercase().starts_with("revert"))
                .collect();
            let hotfixes = delivered
                .iter()
                .filter(|m| m.hotfix && !reverts.iter().any(|r| std::ptr::eq(**r, **m)))
                .count();
            month.from_deployments = deployments_from.is_some_and(|d| start >= d);
            let in_month: Vec<(usize, &&Deployment)> = deployments
                .iter()
                .enumerate()
                .filter(|(_, d)| within(d.created_at))
                .collect();
            let failed_deploys = if month.from_deployments {
                in_month.iter().filter(|(_, d)| d.failed()).count()
            } else {
                0
            };
            if month.from_deployments {
                month.shipped = in_month.iter().filter(|(_, d)| d.succeeded()).count();
                month.changes = month.shipped + failed_deploys;
            } else {
                month.shipped = delivered.len();
                month.changes = delivered.len();
            }
            let days = days_between(from, to).max(1.0);
            month.per_week = Some(month.shipped as f64 * 7.0 / days);
            let leads: Vec<f64> = delivered
                .iter()
                .filter_map(|m| Some(days_between(m.created?, m.delivered?)))
                .collect();
            month.leads = leads.len();
            if leads.len() >= MIN_CASES {
                month.lead = median(leads);
            }
            month.failures = failed_deploys + reverts.len() + hotfixes;
            if month.changes >= MIN_CASES {
                month.failure_rate = Some((month.failures as f64 / month.changes as f64).min(1.0));
            }
            // Time to restore: a failed deployment until the next good one of
            // its environment; a reverted MR's arrival until its revert's.
            let mut restores = Vec::new();
            if month.from_deployments {
                for (i, failed) in in_month.iter().filter(|(_, d)| d.failed()) {
                    if let Some(fixed) = deployments[i + 1..]
                        .iter()
                        .find(|d| d.environment == failed.environment && d.succeeded())
                    {
                        restores.push(days_between(failed.created_at, fixed.created_at));
                    }
                }
            }
            for revert in &reverts {
                let undone = revert
                    .title
                    .split_once('"')
                    .and_then(|(_, rest)| rest.rsplit_once('"'))
                    .map(|(quoted, _)| quoted);
                let broke = undone.and_then(|title| {
                    merged
                        .iter()
                        .filter(|m| m.title == title)
                        .filter_map(|m| m.delivered)
                        .max()
                });
                if let (Some(broke), Some(fixed)) = (broke, revert.delivered) {
                    restores.push(days_between(broke, fixed));
                }
            }
            month.restores = restores.len();
            month.restore = median(restores);
            month
        })
        .collect()
}

/// [`monthly`] read from `db` for `project_id`.
pub async fn load(
    db: &SqliteStatsDb,
    project_id: &str,
    production: &str,
    months: u32,
    now: DateTime<Utc>,
) -> Result<Vec<DoraMonth>, StatsError> {
    let first = month_start(now.date_naive())
        .checked_sub_months(Months::new(months.saturating_sub(1)))
        .unwrap_or_default();
    // A revert's MR may be older than the months shown.
    let from = at_midnight(first) - chrono::Duration::days(90);
    let snapshots = db
        .query(&SnapshotQuery {
            project_id: Some(project_id.to_string()),
            from_date: Some(crate::snapshot::format_timestamp(from)),
            ..SnapshotQuery::default()
        })
        .await?;
    let arrivals = db.query_branch_arrivals(project_id, None, None).await?;
    let deployments = db.query_deployments(project_id, Some(from)).await?;
    Ok(monthly(
        &TrendInput {
            snapshots: &snapshots,
            arrivals: &arrivals,
            deployments: &deployments,
            production,
        },
        months,
        now,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promotion::ArrivalKind;
    use crate::snapshot::{MrStatsSnapshot, SnapshotTrigger};

    fn t(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn merged(id: &str, title: &str, target: &str, created: &str, merged: &str) -> StoredSnapshot {
        StoredSnapshot {
            id: 0,
            recorded_at: merged.into(),
            snapshot: MrStatsSnapshot {
                mr_id: id.into(),
                project_id: "1".into(),
                title: title.into(),
                trigger: SnapshotTrigger::OnMerge,
                author: "ann".into(),
                assignee: None,
                reviewers: vec![],
                merged_by: None,
                milestone: None,
                labels: vec![],
                target_branch: target.into(),
                state: "merged".into(),
                created_at: Some(created.into()),
                merged_at: Some(merged.into()),
                updated_at: None,
                ready_at: None,
                files_changed: 1,
                additions: 1,
                deletions: 0,
                commits_count: 1,
                diff_difficulty: None,
                user_notes_count: 0,
                pipeline_count: 0,
                pipeline_failure_count: 0,
            },
        }
    }

    fn deploy(at: &str, status: &str) -> Deployment {
        Deployment {
            environment: "production".into(),
            reference: "master".into(),
            sha: at.into(),
            created_at: t(at),
            status: status.into(),
        }
    }

    #[test]
    fn each_month_reads_its_merges_and_deployments() {
        // September: three MRs merged into master (no deployment kept yet);
        // October: two deployments kept, one failed then fixed a day later,
        // and a revert of an MR shipped two days before.
        let snapshots = vec![
            merged(
                "1",
                "a",
                "master",
                "2026-09-01T00:00:00Z",
                "2026-09-03T00:00:00Z",
            ),
            merged(
                "2",
                "b",
                "master",
                "2026-09-02T00:00:00Z",
                "2026-09-06T00:00:00Z",
            ),
            merged(
                "3",
                "c",
                "master",
                "2026-09-10T00:00:00Z",
                "2026-09-11T00:00:00Z",
            ),
            merged(
                "4",
                "d",
                "main",
                "2026-10-01T00:00:00Z",
                "2026-10-02T00:00:00Z",
            ),
            merged(
                "5",
                "Revert \"d\"",
                "master",
                "2026-10-05T00:00:00Z",
                "2026-10-06T00:00:00Z",
            ),
        ];
        // !4 reached master by a promotion on the 4th.
        let arrivals = vec![BranchArrival {
            project_id: "1".into(),
            mr_id: "4".into(),
            branch: "master".into(),
            arrived_at: t("2026-10-04T00:00:00Z"),
            how: ArrivalKind::Merge,
            merged_at: None,
        }];
        let deployments = vec![
            deploy("2026-09-20T00:00:00Z", "success"),
            deploy("2026-10-10T00:00:00Z", "failed"),
            deploy("2026-10-11T00:00:00Z", "success"),
            deploy("2026-10-12T00:00:00Z", "success"),
        ];
        let input = TrendInput {
            snapshots: &snapshots,
            arrivals: &arrivals,
            deployments: &deployments,
            production: "master",
        };
        let months = monthly(&input, 4, t("2026-11-15T00:00:00Z"));
        let starts: Vec<String> = months.iter().map(|m| m.start.to_string()).collect();
        assert_eq!(
            starts,
            ["2026-08-01", "2026-09-01", "2026-10-01", "2026-11-01"]
        );
        // August: before anything the database holds.
        assert!(!months[0].covered());
        // September: from the MRs (its deployment is the first one kept).
        let sep = &months[1];
        assert!(!sep.from_deployments);
        assert_eq!((sep.shipped, sep.leads), (3, 3));
        assert_eq!(sep.lead, Some(2.0));
        assert_eq!((sep.failures, sep.failure_rate), (0, Some(0.0)));
        assert_eq!(sep.restore, None);
        // October: from the deployments; one failed, and a revert.
        let oct = &months[2];
        assert!(oct.from_deployments);
        assert_eq!((oct.shipped, oct.changes, oct.failures), (2, 3, 2));
        assert_eq!(oct.failure_rate, Some(2.0 / 3.0));
        // Two leads only (!4: 3 days, the revert: 1): too few for a point.
        assert_eq!((oct.leads, oct.lead), (2, None));
        // Restored in a day (the deployment) and two days (the revert).
        assert_eq!((oct.restores, oct.restore), (2, Some(1.5)));
        // November so far: covered, nothing shipped.
        assert_eq!(months[3].per_week, Some(0.0));
    }
}
