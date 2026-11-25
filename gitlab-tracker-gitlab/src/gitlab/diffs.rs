//! Commits behind the target, changed files and diff stats.

use super::*;

/// Fetches the number of commits the source branch is behind the target branch.
///
/// Uses `GET /projects/:id/repository/compare?from=<source>&to=<target>`.
/// The `commits` array in the response contains the commits present on `target`
/// but not yet on `source` — its length is the \"behind\" count.
///
/// Returns `None` on any network or parse error — this is a best-effort enrichment.
pub(super) async fn fetch_commits_behind(
    ctx: &FetchContext,
    source_branch: &str,
    target_branch: &str,
) -> Option<u32> {
    let client = http_client();
    // "from=source&to=target" returns commits present on `target` but not on `source`,
    // which is the number of commits the source branch is *behind* the target branch.
    let url = format!(
        "{}/api/v4/projects/{}/repository/compare?from={}&to={}&straight=true",
        ctx.base_url,
        ctx.project_id,
        urlencoding::encode(source_branch),
        urlencoding::encode(target_branch),
    );
    let res = client
        .get(&url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
        .ok()?;

    if !res.status().is_success() {
        tracing::warn!(
            "Compare API returned non-2xx for {source_branch}..{target_branch}: {}",
            res.status()
        );
        return None;
    }

    // `commits` is the list of commits on `target` but not on `source`; only their
    // number matters, so the entries themselves are not parsed.
    #[derive(serde::Deserialize)]
    struct Compare {
        commits: Vec<serde::de::IgnoredAny>,
    }
    let body: Compare = res.json().await.ok()?;
    Some(body.commits.len() as u32)
}

/// Body of `GET /merge_requests/:iid/changes` (only what the diff stats use).
#[derive(serde::Deserialize)]
pub(super) struct MrChanges {
    /// `true` when GitLab truncated `changes` (> 1 000 files).
    pub(super) overflow: Option<bool>,
    /// A string, sometimes with a trailing `+` when capped (e.g. `"1000+"`).
    pub(super) changes_count: Option<String>,
    /// `null` when the diff is too large for GitLab to compute inline.
    pub(super) changes: Option<Vec<MrChange>>,
}

#[derive(serde::Deserialize)]
pub(super) struct MrChange {
    /// Raw unified patch of one file.
    pub(super) diff: Option<String>,
    #[serde(default)]
    pub(super) new_path: String,
    #[serde(default)]
    pub(super) old_path: String,
    #[serde(default)]
    pub(super) new_file: bool,
    #[serde(default)]
    pub(super) deleted_file: bool,
    #[serde(default)]
    pub(super) renamed_file: bool,
}

/// The files of a Changes API body, with the target's lines each hunk touches.
pub(super) fn changed_files(body: &MrChanges) -> Vec<crate::models::ChangedFile> {
    let Some(changes) = &body.changes else {
        return Vec::new();
    };
    changes
        .iter()
        .map(|change| {
            let diff = change.diff.as_deref().unwrap_or("");
            let (added, deleted) = count_diff_lines(diff);
            let path = if change.new_path.is_empty() {
                change.old_path.clone()
            } else {
                change.new_path.clone()
            };
            crate::models::ChangedFile {
                path,
                old_path: change
                    .renamed_file
                    .then(|| change.old_path.clone())
                    .filter(|p| !p.is_empty()),
                added,
                deleted,
                hunks: hunk_ranges(diff),
                new_file: change.new_file,
                deleted_file: change.deleted_file,
            }
        })
        .collect()
}

/// The lines of the old file (first, last) each `@@ -a,b +c,d @@` hunk of a
/// unified patch replaces. A pure insertion (`b == 0`) covers line `a`, the
/// one it comes after.
pub fn hunk_ranges(diff: &str) -> Vec<(u32, u32)> {
    diff.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("@@ -")?;
            let old = rest.split_whitespace().next()?;
            let (start, count) = match old.split_once(',') {
                Some((start, count)) => (start.parse::<u32>().ok()?, count.parse::<u32>().ok()?),
                None => (old.parse::<u32>().ok()?, 1),
            };
            Some(if count == 0 {
                (start.max(1), start.max(1))
            } else {
                (start.max(1), start + count - 1)
            })
        })
        .collect()
}

/// `(files_changed, additions, deletions)` of a Changes API body.
///
/// Lines are counted from each file's patch. When the list is truncated
/// (`overflow`) or absent (`changes: null`), the server-side `changes_count` gives
/// the file total; additions/deletions then stay partial (best-effort) rather than
/// returning nothing and leaving the MR on "Loading".
pub(super) fn diff_totals(body: &MrChanges) -> (u32, u32, u32) {
    let changes_count = body
        .changes_count
        .as_deref()
        .map(|s| s.trim_end_matches('+'))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let Some(changes) = &body.changes else {
        return (changes_count, 0, 0);
    };
    let (mut additions, mut deletions) = (0, 0);
    for diff in changes.iter().filter_map(|c| c.diff.as_deref()) {
        let (added, deleted) = count_diff_lines(diff);
        additions += added;
        deletions += deleted;
    }
    let listed = changes.len() as u32;
    let files = if body.overflow.unwrap_or(false) && changes_count > 0 {
        changes_count
    } else {
        listed
    };
    (files, additions, deletions)
}

/// Fetches diff statistics for a merge request from the GitLab Changes API.
///
/// Calls `GET /projects/:id/merge_requests/:iid/changes` and aggregates
/// `additions` + `deletions` from each changed file entry.
///
/// Returns `None` on any network or parse error — diff stats are best-effort
/// and must not block the MR data from being displayed.
/// Diff totals from the Changes API; `commits_count` comes from the MR commit list
/// fetched alongside (0 when it failed: the cache then retries next cycle).
pub(super) async fn fetch_diff_stats(
    ctx: &FetchContext,
    mr_id: &str,
    commits_count: u32,
) -> Option<DiffStats> {
    let client = http_client();
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/changes",
        ctx.base_url, ctx.project_id, mr_id
    );
    let res = client
        .get(&url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
        .ok()?;

    if !res.status().is_success() {
        tracing::warn!(
            "Changes API returned non-2xx for MR {}: {}",
            mr_id,
            res.status()
        );
        return None;
    }

    let body: MrChanges = res.json().await.ok()?;
    let (files_changed, additions, deletions) = diff_totals(&body);

    Some(DiffStats {
        files_changed,
        additions,
        deletions,
        commits_count,
        commits_behind: None,
        files: changed_files(&body),
        files_listed: true,
    })
}
