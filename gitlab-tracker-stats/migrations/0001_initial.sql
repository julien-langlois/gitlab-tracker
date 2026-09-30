-- v1: initial schema (identical to the pre-versioning inline schema, hence
-- IF NOT EXISTS: databases created before versioning are simply stamped v1).

CREATE TABLE IF NOT EXISTS mr_snapshots (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    recorded_at            TEXT    NOT NULL,
    -- Stores the calendar date (YYYY-MM-DD) of recorded_at for use in the UNIQUE
    -- constraint. Expressions (e.g. DATE()) are not allowed in UNIQUE constraints
    -- on older SQLite versions (< 3.37), so we materialise the value explicitly.
    recorded_date          TEXT    NOT NULL,
    trigger                TEXT    NOT NULL,
    mr_id                  TEXT    NOT NULL,
    project_id             TEXT    NOT NULL,
    title                  TEXT    NOT NULL,
    author                 TEXT    NOT NULL,
    assignee               TEXT,
    merged_by              TEXT,
    milestone              TEXT,
    target_branch          TEXT    NOT NULL,
    state                  TEXT    NOT NULL,
    created_at             TEXT,
    merged_at              TEXT,
    updated_at             TEXT,
    files_changed          INTEGER NOT NULL DEFAULT 0,
    additions              INTEGER NOT NULL DEFAULT 0,
    deletions              INTEGER NOT NULL DEFAULT 0,
    commits_count          INTEGER NOT NULL DEFAULT 0,
    diff_difficulty        REAL,
    user_notes_count       INTEGER NOT NULL DEFAULT 0,
    pipeline_count         INTEGER NOT NULL DEFAULT 0,
    pipeline_failure_count INTEGER NOT NULL DEFAULT 0,
    -- Idempotency: one snapshot per MR, per project, per calendar day, per trigger.
    UNIQUE(mr_id, project_id, recorded_date, trigger)
);

CREATE TABLE IF NOT EXISTS mr_labels (
    snapshot_id INTEGER NOT NULL REFERENCES mr_snapshots(id) ON DELETE CASCADE,
    label       TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS mr_reviewers (
    snapshot_id INTEGER NOT NULL REFERENCES mr_snapshots(id) ON DELETE CASCADE,
    reviewer    TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_snapshots_project_date
    ON mr_snapshots(project_id, recorded_date);

CREATE INDEX IF NOT EXISTS idx_snapshots_author
    ON mr_snapshots(author);

CREATE INDEX IF NOT EXISTS idx_snapshots_milestone
    ON mr_snapshots(milestone);

-- Label / reviewer lookups are keyed by snapshot_id; without these
-- every lookup is a full table scan.
CREATE INDEX IF NOT EXISTS idx_labels_snapshot
    ON mr_labels(snapshot_id);

CREATE INDEX IF NOT EXISTS idx_reviewers_snapshot
    ON mr_reviewers(snapshot_id);
