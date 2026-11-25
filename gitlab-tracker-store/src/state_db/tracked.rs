//! The tracked MRs' state: loading, saving, importing it, and each MR's history of events.

use super::*;

impl StateDb {
    /// The project's tracked MRs, in dashboard order, and its project-wide state;
    /// `None` when the project was never saved here.
    pub async fn load(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<Option<(Vec<SavedMr>, ProjectState)>> {
        let Some(key) = self.state_key(gitlab_url, project_id).await? else {
            return Ok(None);
        };
        let stored = self.read_stored(key).await?;
        let mut tracked: Vec<(&String, &Stored)> = stored
            .iter()
            .filter(|(_, s)| s.position.is_some())
            .collect();
        tracked.sort_by_key(|(_, s)| s.position);
        let mut mrs = Vec::with_capacity(tracked.len());
        for (id, s) in tracked {
            let data: MrData = match serde_json::from_str(&s.data) {
                Ok(data) => data,
                Err(e) => {
                    tracing::error!(mr = %id, error = %e, "Unreadable MR in tracker.db: skipped");
                    continue;
                }
            };
            mrs.push(SavedMr {
                id: id.clone(),
                found_branches: serde_json::from_str(&s.found).unwrap_or_default(),
                flagged: s.flagged,
                snooze: s
                    .snooze
                    .as_deref()
                    .and_then(|j| serde_json::from_str(j).ok()),
                linked_ticket: s
                    .ticket
                    .as_deref()
                    .and_then(|j| serde_json::from_str(j).ok()),
                data,
            });
        }

        let discovery_started_at: Option<String> =
            sqlx::query_scalar("SELECT discovery_started_at FROM projects WHERE id = ?")
                .bind(key)
                .fetch_one(&self.pool)
                .await?;
        let mut last_known_branches: HashMap<String, HashSet<String>> = HashMap::new();
        for row in sqlx::query("SELECT mr_id, branches FROM known_branches WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            let branches: String = row.try_get("branches")?;
            last_known_branches.insert(
                row.try_get("mr_id")?,
                serde_json::from_str(&branches).unwrap_or_default(),
            );
        }
        let dismissed_mr_ids: HashSet<String> =
            sqlx::query_scalar("SELECT mr_id FROM dismissed_mrs WHERE project = ?")
                .bind(key)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .collect();
        self.stored.lock().await.insert(key, stored);
        Ok(Some((
            mrs,
            ProjectState {
                last_known_branches,
                discovery_started_at,
                dismissed_mr_ids,
            },
        )))
    }

    /// Every stored MR of the project, tracked or not.
    pub(super) async fn read_stored(&self, key: i64) -> anyhow::Result<HashMap<String, Stored>> {
        let rows = sqlx::query(
            "SELECT mr_id, position, fetched, flagged, snooze, linked_ticket, found_branches,
                    mergeability, data
             FROM mrs WHERE project = ?",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?;
        let mut stored = HashMap::with_capacity(rows.len());
        for row in rows {
            stored.insert(
                row.try_get("mr_id")?,
                Stored {
                    position: row.try_get("position")?,
                    fetched: row.try_get("fetched")?,
                    flagged: row.try_get("flagged")?,
                    snooze: row.try_get("snooze")?,
                    ticket: row.try_get("linked_ticket")?,
                    found: row.try_get("found_branches")?,
                    mergeability: row.try_get("mergeability")?,
                    data: row.try_get("data")?,
                },
            );
        }
        Ok(stored)
    }

    /// Saves the project's tracked MRs (in dashboard order) and its state, and
    /// appends what changed since the last save. Returns the number of events.
    pub async fn save(
        &self,
        gitlab_url: &str,
        project_id: &str,
        mrs: &[MrRow],
        project: &ProjectState,
        now: &str,
    ) -> anyhow::Result<usize> {
        self.save_inner(gitlab_url, project_id, mrs, project, now, false)
            .await
    }

    /// Imports a `tracker_<hash>.json` state, marking the project imported.
    pub async fn import(
        &self,
        gitlab_url: &str,
        project_id: &str,
        state: SavedState,
        now: &str,
    ) -> anyhow::Result<ImportReport> {
        let project = ProjectState {
            last_known_branches: state.last_known_branches,
            discovery_started_at: state.discovery_started_at,
            dismissed_mr_ids: state.dismissed_mr_ids,
        };
        let rows: Vec<MrRow> = state.mrs.into_iter().map(MrRow::from).collect();
        let events = self
            .save_inner(gitlab_url, project_id, &rows, &project, now, true)
            .await?;
        Ok(ImportReport {
            mrs: rows.len(),
            events,
        })
    }

    pub(super) async fn save_inner(
        &self,
        gitlab_url: &str,
        project_id: &str,
        mrs: &[MrRow],
        project: &ProjectState,
        now: &str,
        import: bool,
    ) -> anyhow::Result<usize> {
        // One save at a time: each compares with what the previous one stored.
        let mut cache = self.stored.lock().await;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO projects (gitlab_url, project_id) VALUES (?, ?)
             ON CONFLICT (gitlab_url, project_id) DO NOTHING",
        )
        .bind(gitlab_url)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
        let key: i64 =
            sqlx::query_scalar("SELECT id FROM projects WHERE gitlab_url = ? AND project_id = ?")
                .bind(gitlab_url)
                .bind(project_id)
                .fetch_one(&mut *tx)
                .await?;
        // Taken out while saving: a failed save leaves it to be read again.
        let mut stored = match cache.remove(&key) {
            Some(stored) => stored,
            None => self.read_stored(key).await?,
        };
        let mut events = 0;

        let mut seen = HashSet::new();
        for (position, mr) in mrs.iter().enumerate() {
            if !seen.insert(mr.id.as_str()) {
                continue;
            }
            let old = stored.get(&mr.id);
            let new = stored_row(mr, position as i64, old);
            if old == Some(&new) {
                continue;
            }
            let list = mr_events(old, &new, mr);
            let data_changed = old.is_none_or(|o| o.data != new.data);
            write_mr(&mut tx, key, &mr.id, &new, now).await?;
            if data_changed {
                write_details(&mut tx, key, &mr.id, &new).await?;
            }
            events += list.len();
            write_events(&mut tx, key, &mr.id, &list, now).await?;
            stored.insert(mr.id.clone(), new);
        }
        // Off the dashboard: the row stays, out of it.
        for (id, s) in stored.iter_mut() {
            if s.position.is_some() && !seen.contains(id.as_str()) {
                sqlx::query(
                    "UPDATE mrs SET position = NULL, saved_at = ? WHERE project = ? AND mr_id = ?",
                )
                .bind(now)
                .bind(key)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                write_events(&mut tx, key, id, &[Event::new("untracked")], now).await?;
                events += 1;
                s.position = None;
            }
        }

        sqlx::query(
            "UPDATE projects SET discovery_started_at = ?, state_saved_at = ? WHERE id = ?",
        )
        .bind(&project.discovery_started_at)
        .bind(now)
        .bind(key)
        .execute(&mut *tx)
        .await?;
        if import {
            sqlx::query("UPDATE projects SET imported_at = ? WHERE id = ?")
                .bind(now)
                .bind(key)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM known_branches WHERE project = ?")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        for (id, branches) in &project.last_known_branches {
            let branches = serde_json::to_string(&branches.iter().collect::<BTreeSet<_>>())?;
            sqlx::query("INSERT INTO known_branches (project, mr_id, branches) VALUES (?, ?, ?)")
                .bind(key)
                .bind(id)
                .bind(branches)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM dismissed_mrs WHERE project = ?")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        for id in &project.dismissed_mr_ids {
            sqlx::query("INSERT INTO dismissed_mrs (project, mr_id) VALUES (?, ?)")
                .bind(key)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        cache.insert(key, stored);
        Ok(events)
    }

    /// The events of an MR, oldest seen first: for tests and the import report.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn events_of(
        &self,
        gitlab_url: &str,
        project_id: &str,
        mr_id: &str,
    ) -> anyhow::Result<Vec<Event>> {
        let Some(key) = self.project_key(gitlab_url, project_id).await? else {
            return Ok(vec![]);
        };
        let rows = sqlx::query(
            "SELECT kind, at, actor, subject, old_value, new_value FROM mr_events
             WHERE project = ? AND mr_id = ? ORDER BY id",
        )
        .bind(key)
        .bind(mr_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let kind: String = row.try_get("kind")?;
                Ok(Event {
                    kind: Box::leak(kind.into_boxed_str()),
                    at: row.try_get("at")?,
                    actor: row.try_get("actor")?,
                    subject: row.try_get("subject")?,
                    old: row.try_get("old_value")?,
                    new: row.try_get("new_value")?,
                })
            })
            .collect()
    }
}
