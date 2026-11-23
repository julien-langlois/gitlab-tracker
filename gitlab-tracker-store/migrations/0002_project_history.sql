-- v2: what the other JSON files held. merges_<hash>.json: when the project's MRs
-- merged (the pace on Delivery). owners_<hash>.json: who wrote, reviewed and
-- approved its merged MRs and the files they changed (the reviewer
-- suggestions). gui.json: the window's preferences. The app reads the merges
-- over its period (8 weeks, 90 days); the rows stay after it: the history.

-- Up to when each read is complete (RFC 3339): 'merges', 'owners'.
CREATE TABLE project_reads (
    project INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    what    TEXT    NOT NULL,
    read_at TEXT,
    PRIMARY KEY (project, what)
);

-- When each MR of the project merged (every merged MR, tracked or not).
CREATE TABLE project_merges (
    project   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    mr_iid    INTEGER NOT NULL,
    merged_at TEXT    NOT NULL,
    PRIMARY KEY (project, mr_iid)
);

-- The merged MRs as far as ownership goes.
CREATE TABLE merged_mrs (
    project   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    mr_iid    INTEGER NOT NULL,
    merged_at TEXT    NOT NULL,
    author    TEXT    NOT NULL,
    PRIMARY KEY (project, mr_iid)
);

CREATE INDEX idx_merged_mrs_date ON merged_mrs(project, merged_at);

-- Their reviewers and approvers, in GitLab's order.
CREATE TABLE merged_mr_people (
    project  INTEGER NOT NULL,
    mr_iid   INTEGER NOT NULL,
    -- 'reviewer' or 'approver'.
    role     TEXT    NOT NULL,
    position INTEGER NOT NULL,
    login    TEXT    NOT NULL,
    PRIMARY KEY (project, mr_iid, role, position),
    FOREIGN KEY (project, mr_iid) REFERENCES merged_mrs(project, mr_iid) ON DELETE CASCADE
);

CREATE INDEX idx_merged_people_login ON merged_mr_people(project, login);

-- The files each one changed, in GitLab's order.
CREATE TABLE merged_mr_paths (
    project  INTEGER NOT NULL,
    mr_iid   INTEGER NOT NULL,
    position INTEGER NOT NULL,
    path     TEXT    NOT NULL,
    PRIMARY KEY (project, mr_iid, position),
    FOREIGN KEY (project, mr_iid) REFERENCES merged_mrs(project, mr_iid) ON DELETE CASCADE
);

CREATE INDEX idx_merged_paths_path ON merged_mr_paths(project, path);

-- Names by login, as GitLab gave them.
CREATE TABLE people (
    project INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    login   TEXT    NOT NULL,
    name    TEXT    NOT NULL,
    PRIMARY KEY (project, login)
);

-- The app's own settings, not by project: 'gui' (the window's preferences), as JSON.
CREATE TABLE settings (
    name  TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
