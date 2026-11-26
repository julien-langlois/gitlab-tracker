//! A conflict to come: an open MR whose target took, since the MR's last push,
//! a merged MR that changed the same lines. GitLab sees no conflict yet (it
//! compares with the target as the branch last saw it); git stops at the next
//! rebase, merge or merge train. Read from the changes every tracked MR
//! already carries: no fetch of its own.
//!
//! Lines only, and the line numbers of two different bases: a rename or a
//! reformat may still surprise, and a merge the app does not track is not seen.

use super::*;
use crate::models::TrackedMr;

/// A merged MR the open one will conflict with.
#[derive(Debug, Clone, PartialEq)]
pub struct Ahead {
    /// The merged MR's id and title.
    pub merged: String,
    pub title: String,
    pub merged_at: DateTime<Utc>,
    /// The files both changed on the same lines.
    pub files: Vec<AheadFile>,
}

/// A file both MRs changed on the same lines: each one's hunks (first, last
/// line on the target).
#[derive(Debug, Clone, PartialEq)]
pub struct AheadFile {
    pub path: String,
    pub theirs: Vec<(u32, u32)>,
    pub mine: Vec<(u32, u32)>,
}

/// The merged MRs `mr` will conflict with, the first merged first; none for
/// an MR that is not open, already in conflict, or whose changes or last push
/// are not known.
pub fn ahead(app: &App, mr: &TrackedMr) -> Vec<Ahead> {
    if mr.state != GitlabMrState::Opened || mr.mergeability == MergeabilityStatus::Conflict {
        return Vec::new();
    }
    let Some(mine) = changed_files(mr) else {
        return Vec::new();
    };
    let Some(since) = super::super::stacks::last_push(mr) else {
        return Vec::new();
    };
    let mut out: Vec<Ahead> = app
        .mrs
        .iter()
        .filter(|m| {
            m.id != mr.id && m.state == GitlabMrState::Merged && m.target_branch == mr.target_branch
        })
        .filter_map(|m| {
            let merged_at = board::time_of(m.merged_at.as_deref()).filter(|at| *at > since)?;
            let theirs = changed_files(m)?;
            let files: Vec<AheadFile> = mine
                .iter()
                .filter(|f| !is_lock_file(&f.path))
                .filter_map(|f| {
                    // Both sides as on the target: my file's path there.
                    let path = f.old_path.as_deref().unwrap_or(&f.path);
                    let t = theirs.iter().find(|t| t.path == path)?;
                    (meet(f, t) == Level::SameLines).then(|| AheadFile {
                        path: path.to_string(),
                        theirs: t.hunks.clone(),
                        mine: f.hunks.clone(),
                    })
                })
                .collect();
            (!files.is_empty()).then(|| Ahead {
                merged: m.id.clone(),
                title: m.title.clone(),
                merged_at,
                files,
            })
        })
        .collect();
    out.sort_by_key(|a| a.merged_at);
    out
}

/// The files an MR changes, when GitLab listed them.
fn changed_files(mr: &TrackedMr) -> Option<&[ChangedFile]> {
    mr.diff_stats
        .as_ref()
        .filter(|s| s.files_listed)
        .map(|s| s.files.as_slice())
}

/// Hunks as "12–30, 41": the lines each covers on the target.
pub fn lines_text(hunks: &[(u32, u32)]) -> String {
    hunks
        .iter()
        .map(|(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}–{b}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
