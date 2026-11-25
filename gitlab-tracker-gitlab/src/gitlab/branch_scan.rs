//! Branch commit scans: what each environment branch holds, cherry-picks included.

use super::*;

/// Original commit SHAs named by `(cherry picked from commit <sha>)` trailers, the
/// line added by `git cherry-pick -x` and by GitLab's "Cherry-pick" button.
pub fn cherry_pick_sources(message: &str) -> impl Iterator<Item = &str> {
    const MARKER: &str = "(cherry picked from commit ";
    message.match_indices(MARKER).filter_map(|(i, _)| {
        let start = i + MARKER.len();
        let sha = message.get(start..start + 40)?;
        sha.bytes().all(|b| b.is_ascii_hexdigit()).then_some(sha)
    })
}

/// A commit as returned by GitLab's commit list / detail endpoints.
#[derive(serde::Deserialize)]
pub struct CommitJson {
    pub id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    author_email: String,
    #[serde(default)]
    pub author_name: String,
    authored_date: Option<String>,
    /// When the commit was made on the branch (a copy's arrival date).
    committed_date: Option<String>,
}

impl CommitJson {
    /// `None` when GitLab did not return a parsable author date.
    pub fn fingerprint(&self) -> Option<CommitFingerprint> {
        let authored = chrono::DateTime::parse_from_rfc3339(self.authored_date.as_deref()?).ok()?;
        Some(CommitFingerprint {
            author_email: self.author_email.trim().to_lowercase(),
            authored_at: authored.timestamp(),
            title: normalize_title(&self.title),
        })
    }
}

/// Title as compared between copies: some tools wrap it in quotes when they
/// cherry-pick or reword (`'#1410856: Drupal core update'`), so surrounding
/// quotes and whitespace are dropped.
pub(super) fn normalize_title(title: &str) -> String {
    title.trim().trim_matches(['\'', '"']).trim().to_string()
}

/// Indexes a branch's commits by SHA, cherry-pick trailer and fingerprint, the
/// copy keys mapped to the commit date (the author date when GitLab omits it).
pub(super) fn index_branch_commits(commits: &[CommitJson]) -> BranchCommits {
    let mut index = BranchCommits::default();
    for c in commits {
        index.shas.insert(c.id.clone());
        let fingerprint = c.fingerprint();
        let committed_at = c
            .committed_date
            .as_deref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.timestamp())
            .or(fingerprint.as_ref().map(|fp| fp.authored_at))
            .unwrap_or_default();
        for source in cherry_pick_sources(&c.message) {
            index.picked_from.insert(source.to_owned(), committed_at);
        }
        if let Some(fp) = fingerprint {
            index.fingerprints.insert(fp, committed_at);
        }
    }
    index
}

/// How a branch scan reads the branch's commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchScan {
    /// Every commit since `since` (RFC 3339): the index is rebuilt.
    Full { since: String },
    /// Only the commits reachable from the branch and not from `from` (its head at
    /// the previous scan): added to the index kept in memory. Unlike a date filter,
    /// this also catches old commits that just arrived through a merge.
    Range { from: String },
}

/// What the kept index of a branch covers, to plan the next scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchScanScope {
    /// Oldest commit date the index covers.
    pub floor: chrono::DateTime<chrono::Utc>,
    /// Branch head at the last scan (`None`: no commit in range, rescan in full).
    pub head: Option<String>,
    /// Last full rebuild.
    pub full_at: chrono::DateTime<chrono::Utc>,
}

/// An incremental index is rebuilt from scratch this often, which also drops
/// commits a force-push removed.
pub const BRANCH_FULL_RESCAN: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// The scan to run for a branch whose index must reach back to `floor`.
pub fn plan_branch_scan(
    scope: Option<&BranchScanScope>,
    floor: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> BranchScan {
    match scope {
        Some(scope) if scope.floor <= floor && now - scope.full_at < BRANCH_FULL_RESCAN => {
            match &scope.head {
                Some(head) => BranchScan::Range { from: head.clone() },
                None => BranchScan::Full {
                    since: floor.to_rfc3339(),
                },
            }
        }
        _ => BranchScan::Full {
            since: floor.to_rfc3339(),
        },
    }
}

/// `repository/commits` URL for `scan` of `branch` (query string open for paging).
pub(super) fn branch_scan_url(ctx: &FetchContext, branch: &str, scan: &BranchScan) -> String {
    let filter = match scan {
        BranchScan::Full { since } => format!(
            "ref_name={}&since={}",
            urlencoding::encode(branch),
            urlencoding::encode(since)
        ),
        BranchScan::Range { from } => {
            format!(
                "ref_name={}",
                urlencoding::encode(&format!("{from}..{branch}"))
            )
        }
    };
    format!(
        "{}/api/v4/projects/{}/repository/commits?{filter}",
        ctx.base_url, ctx.project_id
    )
}

/// Spawns `scan` of `branch` and emits `GitlabEvent::BranchCommitsLoaded` with the
/// commits read (`None` when a page failed) and the branch head.
pub fn spawn_branch_commits_fetch<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    branch: String,
    scan: BranchScan,
    previous_head: Option<String>,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(async move {
        let _permit = semaphore.acquire().await;
        let url = branch_scan_url(&ctx, &branch, &scan);
        let commits = get_all_pages::<CommitJson>(&ctx, &url).await;
        // The branch tip comes first; an empty range leaves the head where it was.
        let head = match &commits {
            Some(list) => list.first().map(|c| c.id.clone()).or(match &scan {
                BranchScan::Range { .. } => previous_head,
                BranchScan::Full { .. } => None,
            }),
            None => previous_head,
        };
        let commits = commits.map(|list| index_branch_commits(&list));
        crate::events::emit(
            &tx,
            GitlabEvent::BranchCommitsLoaded {
                branch,
                commits,
                scan,
                head,
            },
        );
    });
}

pub(super) async fn fetch_mr_commits(ctx: &FetchContext, mr_id: &str) -> Option<Vec<MrCommit>> {
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/commits?",
        ctx.base_url, ctx.project_id, mr_id
    );
    let commits = get_all_pages::<CommitJson>(ctx, &url).await?;
    Some(
        commits
            .into_iter()
            .filter_map(|c| {
                let fingerprint = c.fingerprint()?;
                Some(MrCommit {
                    sha: c.id,
                    fingerprint,
                    author_name: c.author_name,
                })
            })
            .collect(),
    )
}

/// Fingerprint of commit `sha` (best-effort: `None` on any error).
pub(super) async fn fetch_commit_fingerprint(
    ctx: &FetchContext,
    sha: &str,
) -> Option<CommitFingerprint> {
    let url = format!(
        "{}/api/v4/projects/{}/repository/commits/{}",
        ctx.base_url, ctx.project_id, sha
    );
    let res = http_client()
        .get(url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
        .ok()
        .filter(|r| r.status().is_success())?;
    res.json::<CommitJson>().await.ok()?.fingerprint()
}
