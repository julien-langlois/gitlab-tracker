//! The project caches (the merges, the merged MRs' owners) and the settings: the window's.

use super::*;

impl StateDb {
    /// The project's key, its row created when absent.
    pub(super) async fn project_key_or_insert(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<i64> {
        sqlx::query(
            "INSERT INTO projects (gitlab_url, project_id) VALUES (?, ?)
             ON CONFLICT (gitlab_url, project_id) DO NOTHING",
        )
        .bind(gitlab_url)
        .bind(project_id)
        .execute(&self.pool)
        .await?;
        self.project_key(gitlab_url, project_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("project row missing"))
    }

    /// Up to when the project's read `what` is complete: `None` when it was
    /// never saved here.
    pub(super) async fn read_at(
        &self,
        key: i64,
        what: &str,
    ) -> anyhow::Result<Option<Option<String>>> {
        Ok(
            sqlx::query_scalar("SELECT read_at FROM project_reads WHERE project = ? AND what = ?")
                .bind(key)
                .bind(what)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub(super) async fn set_read_at(
        tx: &mut sqlx::SqliteConnection,
        key: i64,
        what: &str,
        read_at: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO project_reads (project, what, read_at) VALUES (?, ?, ?)
             ON CONFLICT (project, what) DO UPDATE SET read_at = excluded.read_at",
        )
        .bind(key)
        .bind(what)
        .bind(read_at)
        .execute(&mut *tx)
        .await?;
        Ok(())
    }

    /// A setting's JSON value, `None` when never saved.
    pub async fn setting(&self, name: &str) -> anyhow::Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM settings WHERE name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn set_setting(&self, name: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO settings (name, value) VALUES (?, ?)
             ON CONFLICT (name) DO UPDATE SET value = excluded.value",
        )
        .bind(name)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every merge of the project saved here, over any period; `None` when the
    /// merges were never saved.
    pub async fn load_merges(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<Option<gitlab_tracker_gitlab::history::MergeHistory>> {
        let Some(key) = self.project_key(gitlab_url, project_id).await? else {
            return Ok(None);
        };
        let Some(read_at) = self.read_at(key, "merges").await? else {
            return Ok(None);
        };
        let mut merged = BTreeMap::new();
        for row in sqlx::query("SELECT mr_iid, merged_at FROM project_merges WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            merged.insert(iid as u64, row.try_get("merged_at")?);
        }
        Ok(Some(gitlab_tracker_gitlab::history::MergeHistory {
            merged,
            read_at,
        }))
    }

    /// Saves the merges of `history` not in `known` (as they were before the
    /// read), and up to when it is complete. Merges out of the period stay.
    pub async fn save_merges(
        &self,
        gitlab_url: &str,
        project_id: &str,
        history: &gitlab_tracker_gitlab::history::MergeHistory,
        known: &gitlab_tracker_gitlab::history::MergeHistory,
    ) -> anyhow::Result<()> {
        let key = self.project_key_or_insert(gitlab_url, project_id).await?;
        let mut tx = self.pool.begin().await?;
        for (iid, at) in &history.merged {
            if known.merged.get(iid) == Some(at) {
                continue;
            }
            sqlx::query(
                "INSERT INTO project_merges (project, mr_iid, merged_at) VALUES (?, ?, ?)
                 ON CONFLICT (project, mr_iid) DO UPDATE SET merged_at = excluded.merged_at",
            )
            .bind(key)
            .bind(*iid as i64)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        }
        Self::set_read_at(&mut tx, key, "merges", history.read_at.as_deref()).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Every merged MR of the project saved here, with its people and files,
    /// over any period; `None` when they were never saved.
    pub async fn load_owners(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<Option<gitlab_tracker_gitlab::history::History>> {
        use gitlab_tracker_gitlab::history::{History, Merge};
        let Some(key) = self.project_key(gitlab_url, project_id).await? else {
            return Ok(None);
        };
        let Some(read_at) = self.read_at(key, "owners").await? else {
            return Ok(None);
        };
        let mut merges: BTreeMap<u64, Merge> = BTreeMap::new();
        for row in sqlx::query("SELECT mr_iid, merged_at, author FROM merged_mrs WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            merges.insert(
                iid as u64,
                Merge {
                    merged_at: row.try_get("merged_at")?,
                    author: row.try_get("author")?,
                    reviewers: Vec::new(),
                    approvers: Vec::new(),
                    paths: Vec::new(),
                },
            );
        }
        for row in sqlx::query(
            "SELECT mr_iid, role, login FROM merged_mr_people WHERE project = ?
             ORDER BY mr_iid, role, position",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            let role: String = row.try_get("role")?;
            if let Some(m) = merges.get_mut(&(iid as u64)) {
                let list = if role == "approver" {
                    &mut m.approvers
                } else {
                    &mut m.reviewers
                };
                list.push(row.try_get("login")?);
            }
        }
        for row in sqlx::query(
            "SELECT mr_iid, path FROM merged_mr_paths WHERE project = ? ORDER BY mr_iid, position",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            if let Some(m) = merges.get_mut(&(iid as u64)) {
                m.paths.push(row.try_get("path")?);
            }
        }
        let mut people = BTreeMap::new();
        for row in sqlx::query("SELECT login, name FROM people WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            people.insert(row.try_get("login")?, row.try_get("name")?);
        }
        Ok(Some(History {
            merges,
            people,
            read_at,
        }))
    }

    /// Saves the merged MRs and names of `history` that are new or changed
    /// since `known`, and up to when it is complete. Older ones stay.
    pub async fn save_owners(
        &self,
        gitlab_url: &str,
        project_id: &str,
        history: &gitlab_tracker_gitlab::history::History,
        known: &gitlab_tracker_gitlab::history::History,
    ) -> anyhow::Result<()> {
        let key = self.project_key_or_insert(gitlab_url, project_id).await?;
        let mut tx = self.pool.begin().await?;
        for (iid, m) in &history.merges {
            if known.merges.get(iid) == Some(m) {
                continue;
            }
            let iid = *iid as i64;
            sqlx::query(
                "INSERT INTO merged_mrs (project, mr_iid, merged_at, author) VALUES (?, ?, ?, ?)
                 ON CONFLICT (project, mr_iid) DO UPDATE SET
                     merged_at = excluded.merged_at, author = excluded.author",
            )
            .bind(key)
            .bind(iid)
            .bind(&m.merged_at)
            .bind(&m.author)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM merged_mr_people WHERE project = ? AND mr_iid = ?")
                .bind(key)
                .bind(iid)
                .execute(&mut *tx)
                .await?;
            let people = m
                .reviewers
                .iter()
                .enumerate()
                .map(|(i, l)| ("reviewer", i, l))
                .chain(
                    m.approvers
                        .iter()
                        .enumerate()
                        .map(|(i, l)| ("approver", i, l)),
                );
            for (role, position, login) in people {
                sqlx::query(
                    "INSERT INTO merged_mr_people (project, mr_iid, role, position, login)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(key)
                .bind(iid)
                .bind(role)
                .bind(position as i64)
                .bind(login)
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query("DELETE FROM merged_mr_paths WHERE project = ? AND mr_iid = ?")
                .bind(key)
                .bind(iid)
                .execute(&mut *tx)
                .await?;
            for (position, path) in m.paths.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO merged_mr_paths (project, mr_iid, position, path) VALUES (?, ?, ?, ?)",
                )
                .bind(key)
                .bind(iid)
                .bind(position as i64)
                .bind(path)
                .execute(&mut *tx)
                .await?;
            }
        }
        for (login, name) in &history.people {
            if known.people.get(login) == Some(name) {
                continue;
            }
            sqlx::query(
                "INSERT INTO people (project, login, name) VALUES (?, ?, ?)
                 ON CONFLICT (project, login) DO UPDATE SET name = excluded.name",
            )
            .bind(key)
            .bind(login)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        }
        Self::set_read_at(&mut tx, key, "owners", history.read_at.as_deref()).await?;
        tx.commit().await?;
        Ok(())
    }
}
