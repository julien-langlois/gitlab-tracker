-- v2: when each MR first reached each tracked branch other than its target, for
-- the promotion lead time (merge on the target → arrival on preproduction, master…).

CREATE TABLE IF NOT EXISTS mr_branch_arrivals (
    project_id TEXT NOT NULL,
    mr_id      TEXT NOT NULL,
    branch     TEXT NOT NULL,
    -- Canonical UTC (`format_timestamp`). For a copy, the date of the copied
    -- commit; otherwise the refresh that first saw the MR there.
    arrived_at TEXT NOT NULL,
    -- 'merge' (same SHA: merge, fast-forward, reset), 'cherry_pick' (-x trailer)
    -- or 'copy' (fingerprint match).
    how        TEXT NOT NULL,
    merged_at  TEXT,
    PRIMARY KEY (project_id, mr_id, branch)
);

CREATE INDEX IF NOT EXISTS idx_arrivals_project_date
    ON mr_branch_arrivals(project_id, arrived_at);
