-- v1: the tracker's state, which tracker_<hash>.json used to hold (the tracked
-- MRs, what each one is, where it landed), plus what the file never kept: every
-- change seen on them, day after day (mr_events), and every pipeline and thread
-- seen, kept after GitLab's lists move on.

CREATE TABLE projects (
    id                   INTEGER PRIMARY KEY,
    gitlab_url           TEXT    NOT NULL,
    project_id           TEXT    NOT NULL,
    -- Discovery adds only the MRs created after this (RFC 3339).
    discovery_started_at TEXT,
    -- When tracker_<hash>.json was imported; NULL for a project started here.
    imported_at          TEXT,
    -- Last save of its tracked MRs; NULL while only its caches are here (the
    -- state file, if any, is still to import).
    state_saved_at       TEXT,
    UNIQUE (gitlab_url, project_id)
);

-- One row per MR ever tracked: one removed from the dashboard keeps its row
-- (position NULL) and its history.
CREATE TABLE mrs (
    project        INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    mr_id          TEXT    NOT NULL,
    -- Its place in the dashboard, from 0; NULL once removed from it.
    position       INTEGER,
    -- Read from GitLab at least once (0: the placeholder shown while it loads).
    fetched        INTEGER NOT NULL DEFAULT 0,
    -- The main fields, to query; `data` holds them all.
    title          TEXT    NOT NULL,
    state          TEXT    NOT NULL,
    author         TEXT    NOT NULL,
    assignee       TEXT,
    source_branch  TEXT    NOT NULL,
    target_branch  TEXT    NOT NULL,
    milestone      TEXT,
    web_url        TEXT    NOT NULL,
    created_at     TEXT,
    updated_at     TEXT,
    merged_at      TEXT,
    merged_by      TEXT,
    -- Last mergeability read (`MergeabilityStatus`), NULL before any.
    mergeability   TEXT,
    flagged        INTEGER NOT NULL DEFAULT 0,
    -- JSON: the tracked branches it reached, its snooze, its ticket.
    found_branches TEXT    NOT NULL DEFAULT '[]',
    snooze         TEXT,
    linked_ticket  TEXT,
    -- JSON `MrData`, in full: what the app loads.
    data           TEXT    NOT NULL,
    first_seen_at  TEXT    NOT NULL,
    saved_at       TEXT    NOT NULL,
    PRIMARY KEY (project, mr_id)
);

CREATE INDEX idx_mrs_position ON mrs(project, position);

-- Current labels and people, to query (their changes are in mr_events).
CREATE TABLE mr_labels (
    project INTEGER NOT NULL,
    mr_id   TEXT    NOT NULL,
    label   TEXT    NOT NULL,
    PRIMARY KEY (project, mr_id, label),
    FOREIGN KEY (project, mr_id) REFERENCES mrs(project, mr_id) ON DELETE CASCADE
);

CREATE TABLE mr_people (
    project INTEGER NOT NULL,
    mr_id   TEXT    NOT NULL,
    -- 'reviewer' or 'approver'.
    role    TEXT    NOT NULL,
    login   TEXT    NOT NULL,
    PRIMARY KEY (project, mr_id, role, login),
    FOREIGN KEY (project, mr_id) REFERENCES mrs(project, mr_id) ON DELETE CASCADE
);

-- Every pipeline seen on an MR, with its last status: GitLab's MR keeps a few.
CREATE TABLE mr_pipelines (
    project     INTEGER NOT NULL,
    mr_id       TEXT    NOT NULL,
    pipeline_id INTEGER NOT NULL,
    status      TEXT    NOT NULL,
    sha         TEXT,
    created_at  TEXT,
    updated_at  TEXT,
    PRIMARY KEY (project, mr_id, pipeline_id),
    FOREIGN KEY (project, mr_id) REFERENCES mrs(project, mr_id) ON DELETE CASCADE
);

-- Every review thread seen, with its last state.
CREATE TABLE mr_threads (
    project      INTEGER NOT NULL,
    mr_id        TEXT    NOT NULL,
    note_id      INTEGER NOT NULL,
    author       TEXT    NOT NULL,
    excerpt      TEXT    NOT NULL,
    created_at   TEXT    NOT NULL,
    last_note_at TEXT    NOT NULL,
    notes        INTEGER NOT NULL,
    resolved     INTEGER NOT NULL,
    PRIMARY KEY (project, mr_id, note_id),
    FOREIGN KEY (project, mr_id) REFERENCES mrs(project, mr_id) ON DELETE CASCADE
);

-- What changed on an MR, as the app saw it: appended, never rewritten.
CREATE TABLE mr_events (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    project   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    mr_id     TEXT    NOT NULL,
    -- When the app saw it (RFC 3339).
    seen_at   TEXT    NOT NULL,
    -- When it happened, by GitLab, when known.
    at        TEXT,
    -- 'approved', 'merged', 'pipeline', 'label_added'… (see state_db.rs).
    kind      TEXT    NOT NULL,
    -- Who did it, when known.
    actor     TEXT,
    -- What it is about: a label, a login, a pipeline or note id, a branch.
    subject   TEXT,
    old_value TEXT,
    new_value TEXT
);

CREATE INDEX idx_events_mr   ON mr_events(project, mr_id, seen_at);
CREATE INDEX idx_events_date ON mr_events(project, seen_at);
CREATE INDEX idx_events_kind ON mr_events(kind);

-- The tracked branches each MR was on at the last refresh, as a JSON array: a
-- branch it reaches later notifies. An MR known on none has its row ('[]'):
-- one with no row is new, and its branches are recorded silently.
CREATE TABLE known_branches (
    project  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    mr_id    TEXT    NOT NULL,
    branches TEXT    NOT NULL DEFAULT '[]',
    PRIMARY KEY (project, mr_id)
);

-- MRs removed by hand: discovery does not add them again.
CREATE TABLE dismissed_mrs (
    project INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    mr_id   TEXT    NOT NULL,
    PRIMARY KEY (project, mr_id)
);
