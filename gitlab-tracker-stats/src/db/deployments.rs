//! The project's deployments, kept as read for the monthly DORA trend.

use chrono::{DateTime, Utc};

use super::{SqliteStatsDb, StatsError};
use crate::snapshot::format_timestamp;

/// A deployment of the project, as GitLab lists it (who started it is not kept).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Deployment {
    pub environment: String,
    /// The branch (or tag) deployed.
    pub reference: String,
    pub sha: String,
    pub created_at: DateTime<Utc>,
    /// `success`, `failed`, `running`, `canceled`…
    pub status: String,
}

impl Deployment {
    pub fn succeeded(&self) -> bool {
        self.status == "success"
    }

    pub fn failed(&self) -> bool {
        self.status == "failed"
    }
}

impl SqliteStatsDb {
    /// Keeps `deployments` of `project_id`: new ones are added, a known one
    /// takes its latest status (a running deployment that ended). Returns how
    /// many rows were written.
    pub async fn record_deployments(
        &self,
        project_id: &str,
        deployments: &[Deployment],
    ) -> Result<u64, StatsError> {
        let mut written = 0;
        let mut tx = self.pool.begin().await?;
        for d in deployments {
            let result = sqlx::query(
                "INSERT INTO deployments (project_id, environment, reference, sha, created_at, status)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT (project_id, environment, sha, created_at)
                 DO UPDATE SET status = excluded.status WHERE status <> excluded.status",
            )
            .bind(project_id)
            .bind(&d.environment)
            .bind(&d.reference)
            .bind(&d.sha)
            .bind(format_timestamp(d.created_at))
            .bind(&d.status)
            .execute(&mut *tx)
            .await?;
            written += result.rows_affected();
        }
        tx.commit().await?;
        Ok(written)
    }

    /// The deployments of `project_id` from `from` on (`None`: all), oldest
    /// first. Rows with an unreadable date are skipped.
    pub async fn query_deployments(
        &self,
        project_id: &str,
        from: Option<DateTime<Utc>>,
    ) -> Result<Vec<Deployment>, StatsError> {
        let from = from.map(format_timestamp);
        let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT environment, reference, sha, created_at, status FROM deployments
             WHERE project_id = ? AND (? IS NULL OR created_at >= ?)
             ORDER BY created_at",
        )
        .bind(project_id)
        .bind(&from)
        .bind(&from)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(environment, reference, sha, created_at, status)| {
                Some(Deployment {
                    environment,
                    reference,
                    sha,
                    created_at: created_at.parse().ok()?,
                    status,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deployment(at: &str, status: &str) -> Deployment {
        Deployment {
            environment: "production".into(),
            reference: "master".into(),
            sha: format!("sha-{at}"),
            created_at: at.parse().unwrap(),
            status: status.into(),
        }
    }

    #[tokio::test]
    async fn a_deployment_is_kept_once_with_its_latest_status() {
        let db = SqliteStatsDb::open(":memory:").await.unwrap();
        let running = deployment("2026-10-01T10:00:00Z", "running");
        assert_eq!(
            db.record_deployments("1", std::slice::from_ref(&running))
                .await
                .unwrap(),
            1
        );
        // Read again, unchanged: nothing written.
        assert_eq!(
            db.record_deployments("1", std::slice::from_ref(&running))
                .await
                .unwrap(),
            0
        );
        let done = Deployment {
            status: "success".into(),
            ..running
        };
        let older = deployment("2026-09-01T10:00:00Z", "failed");
        db.record_deployments("1", &[done.clone(), older.clone()])
            .await
            .unwrap();
        let all = db.query_deployments("1", None).await.unwrap();
        assert_eq!(all, vec![older, done.clone()]);
        let since = "2026-09-15T00:00:00Z".parse().ok();
        assert_eq!(db.query_deployments("1", since).await.unwrap(), vec![done]);
        // Another project's are its own.
        assert!(db.query_deployments("2", None).await.unwrap().is_empty());
    }
}
