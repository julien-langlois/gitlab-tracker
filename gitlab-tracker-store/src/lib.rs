//! *Internal crate of [gitlab-tracker](https://crates.io/crates/gitlab-tracker),
//! released with it under the same version: its API may change in any release.*
//!
//! The tracker's local state in SQLite (`tracker.db`): the tracked MRs of every
//! project, their day-to-day history, the project caches and the window's
//! settings. See [`state_db`].

use std::path::Path;

pub mod state_db;

/// Creates `dir` if needed and, on Unix, makes it private to its owner (`0700`):
/// its logs, caches and `.env` describe the user's projects and may hold tokens
/// (`.env`), so other local accounts must not read them. Best effort: a failure is
/// logged later by the code that needs the directory.
pub fn restrict_to_owner(dir: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
}
