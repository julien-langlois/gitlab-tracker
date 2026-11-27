-- v4: the project's deployments as the app reads them, kept for the monthly
-- DORA trend (change failure rate, time to restore): GitLab's list only goes
-- back so far. Who deployed is not kept.

CREATE TABLE IF NOT EXISTS deployments (
    project_id  TEXT NOT NULL,
    environment TEXT NOT NULL,
    -- The branch (or tag) deployed.
    reference   TEXT NOT NULL,
    sha         TEXT NOT NULL,
    -- Canonical UTC (`format_timestamp`).
    created_at  TEXT NOT NULL,
    -- GitLab's: success, failed, running, canceled…; updated when read again.
    status      TEXT NOT NULL,
    PRIMARY KEY (project_id, environment, sha, created_at)
);

CREATE INDEX IF NOT EXISTS idx_deployments_project_date
    ON deployments(project_id, created_at);
