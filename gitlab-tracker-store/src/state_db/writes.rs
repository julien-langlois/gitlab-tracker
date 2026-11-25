//! Writing one MR: its row, its details, its new events.

use super::*;

pub(super) async fn write_mr(
    tx: &mut sqlx::SqliteConnection,
    key: i64,
    id: &str,
    s: &Stored,
    now: &str,
) -> anyhow::Result<()> {
    let d: MrData = serde_json::from_str(&s.data).unwrap_or_default();
    sqlx::query(
        "INSERT INTO mrs (project, mr_id, position, fetched, title, state, author, assignee,
             source_branch, target_branch, milestone, web_url, created_at, updated_at,
             merged_at, merged_by, mergeability, flagged, found_branches, snooze,
             linked_ticket, data, first_seen_at, saved_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (project, mr_id) DO UPDATE SET
             position = excluded.position, fetched = excluded.fetched,
             title = excluded.title, state = excluded.state, author = excluded.author,
             assignee = excluded.assignee, source_branch = excluded.source_branch,
             target_branch = excluded.target_branch, milestone = excluded.milestone,
             web_url = excluded.web_url, created_at = excluded.created_at,
             updated_at = excluded.updated_at, merged_at = excluded.merged_at,
             merged_by = excluded.merged_by, mergeability = excluded.mergeability,
             flagged = excluded.flagged, found_branches = excluded.found_branches,
             snooze = excluded.snooze, linked_ticket = excluded.linked_ticket,
             data = excluded.data, saved_at = excluded.saved_at",
    )
    .bind(key)
    .bind(id)
    .bind(s.position)
    .bind(s.fetched)
    .bind(&d.title)
    .bind(name_of(&d.state))
    .bind(&d.author)
    .bind(&d.assignee)
    .bind(&d.source_branch)
    .bind(&d.target_branch)
    .bind(&d.milestone)
    .bind(&d.web_url)
    .bind(&d.created_at)
    .bind(&d.updated_at)
    .bind(&d.merged_at)
    .bind(&d.merged_by)
    .bind(&s.mergeability)
    .bind(s.flagged)
    .bind(&s.found)
    .bind(&s.snooze)
    .bind(&s.ticket)
    .bind(&s.data)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// The queryable copies of an MR's data: current labels and people, and every
/// pipeline and thread seen (kept when GitLab's lists drop them).
pub(super) async fn write_details(
    tx: &mut sqlx::SqliteConnection,
    key: i64,
    id: &str,
    s: &Stored,
) -> anyhow::Result<()> {
    let d: MrData = serde_json::from_str(&s.data).unwrap_or_default();
    sqlx::query("DELETE FROM mr_labels WHERE project = ? AND mr_id = ?")
        .bind(key)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for label in d.labels.iter().collect::<BTreeSet<_>>() {
        sqlx::query("INSERT INTO mr_labels (project, mr_id, label) VALUES (?, ?, ?)")
            .bind(key)
            .bind(id)
            .bind(label)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM mr_people WHERE project = ? AND mr_id = ?")
        .bind(key)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let people = d
        .reviewers
        .iter()
        .map(|l| ("reviewer", l))
        .chain(d.approved_by.iter().map(|l| ("approver", l)))
        .collect::<BTreeSet<_>>();
    for (role, login) in people {
        sqlx::query("INSERT INTO mr_people (project, mr_id, role, login) VALUES (?, ?, ?, ?)")
            .bind(key)
            .bind(id)
            .bind(role)
            .bind(login)
            .execute(&mut *tx)
            .await?;
    }
    for p in &d.pipelines {
        sqlx::query(
            "INSERT INTO mr_pipelines (project, mr_id, pipeline_id, status, sha, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (project, mr_id, pipeline_id) DO UPDATE SET
                 status = excluded.status, sha = excluded.sha,
                 created_at = excluded.created_at, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(id)
        .bind(i64::try_from(p.id).unwrap_or(i64::MAX))
        .bind(name_of(&p.status))
        .bind(&p.sha)
        .bind(&p.created_at)
        .bind(&p.updated_at)
        .execute(&mut *tx)
        .await?;
    }
    for t in &d.threads {
        sqlx::query(
            "INSERT INTO mr_threads (project, mr_id, note_id, author, excerpt, created_at,
                 last_note_at, notes, resolved)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (project, mr_id, note_id) DO UPDATE SET
                 excerpt = excluded.excerpt, last_note_at = excluded.last_note_at,
                 notes = excluded.notes, resolved = excluded.resolved",
        )
        .bind(key)
        .bind(id)
        .bind(i64::try_from(t.note_id).unwrap_or(i64::MAX))
        .bind(&t.author)
        .bind(&t.excerpt)
        .bind(&t.created_at)
        .bind(&t.last_note_at)
        .bind(t.notes)
        .bind(t.resolved)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

pub(super) async fn write_events(
    tx: &mut sqlx::SqliteConnection,
    key: i64,
    id: &str,
    events: &[Event],
    now: &str,
) -> anyhow::Result<()> {
    for e in events {
        sqlx::query(
            "INSERT INTO mr_events (project, mr_id, seen_at, at, kind, actor, subject,
                 old_value, new_value)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(key)
        .bind(id)
        .bind(now)
        .bind(&e.at)
        .bind(e.kind)
        .bind(&e.actor)
        .bind(&e.subject)
        .bind(&e.old)
        .bind(&e.new)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}
