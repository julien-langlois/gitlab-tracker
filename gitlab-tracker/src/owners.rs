//! Who owns and who knows the files an MR changes, to suggest its reviewers.
//!
//! Two sources, compared file by file:
//! * **Declared**: the project's `CODEOWNERS` (the first of `CODEOWNERS`,
//!   `docs/CODEOWNERS`, `.gitlab/CODEOWNERS`, read on the MR's target branch,
//!   as GitLab does). Applied whatever GitLab's tier: on Free, GitLab enforces
//!   nothing, but the file still says who answers for what.
//! * **Known**: who authored, reviewed or approved the project's MRs merged in
//!   the last [`HISTORY_DAYS`] days that touched each file ([`History`]).
//!
//! Without a `CODEOWNERS`, owners are deduced from the history (who did most
//! of the work in the file's folder), said so wherever they show, and given as
//! lines to start a `CODEOWNERS` from. With one, each suggested person says
//! how the two agree: an owner who knows the files, an owner who has not been
//! near them in the period (the file may be out of date), or someone who knows
//! them and is not in it.
//!
//! Matching follows GitLab's: a pattern without a leading `/` matches at any
//! depth (`/**/` before it), one ending in `/` everything under it (`**/*`
//! after it), then `fnmatch` with `FNM_PATHNAME | FNM_DOTMATCH`: `*` and `?`
//! stop at `/`, `**/` spans folders, a trailing `/**` is one level. In each
//! section the last matching entry wins; every section applies.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};

mod codeowners;
mod read;
mod suggest;

pub use codeowners::*;
pub use read::*;
pub use suggest::*;

/// Where GitLab looks for the file, in its order: the first found is used.
pub const LOCATIONS: [&str; 3] = ["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"];

// The merged MRs, as kept between launches (gitlab-tracker-gitlab).
pub use gitlab_tracker_gitlab::history::{History, Merge, HISTORY_DAYS};

#[cfg(test)]
mod tests;
