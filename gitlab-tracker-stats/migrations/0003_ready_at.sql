-- v3: when each MR was put up for review (out of draft, else its first review
-- request), for the "ready → merge" time next to the full time to merge: an
-- MR opened as a draft when the work starts has its development in the latter.
-- NULL for rows written before, until the startup backfill fills them.

ALTER TABLE mr_snapshots ADD COLUMN ready_at TEXT;
