//! What the window's MR page reads from GitLab when it opens, on top of what the
//! refresh cycle keeps: why the MR cannot merge (with the files in conflict), its
//! approval rules, its threads in full, and the deployments of the tracked
//! branches (`gui` feature).
//!
//! Every read is best-effort: a failure leaves its part of the page out (or says
//! so), never the rest. Approval rules need GitLab Premium: on other tiers the
//! endpoint answers 403/404 and the page falls back to the approvers.

use crate::gitlab::http_client;

/// The MR as GitLab sees it now: the merge checks the list does not keep.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Overview {
    /// `mergeable`, `conflict`, `ci_must_pass`, `discussions_not_resolved`,
    /// `not_approved`, `need_rebase`, `draft_status`, `requested_changes`…
    #[serde(default)]
    pub detailed_merge_status: String,
    #[serde(default)]
    pub has_conflicts: bool,
    #[serde(default)]
    pub blocking_discussions_resolved: bool,
    #[serde(default)]
    pub draft: bool,
    /// Commits the target branch has that the source lacks.
    pub diverged_commits_count: Option<u32>,
    #[serde(default)]
    pub force_remove_source_branch: Option<bool>,
}

/// An approval rule and where it stands.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Rule {
    pub name: String,
    #[serde(default)]
    pub approvals_required: u32,
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub approved_by: Vec<User>,
    #[serde(default)]
    pub eligible_approvers: Vec<User>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct User {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub username: String,
}

/// A thread with all its notes.
#[derive(Debug, Clone)]
pub struct Thread {
    pub resolved: bool,
    /// The file and line it is about; `None` for a general thread.
    pub path: Option<String>,
    pub line: Option<u32>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub author: String,
    pub username: String,
    pub body: String,
    pub created_at: String,
}

/// The last successful deployment of an environment.
#[derive(Debug, Clone)]
pub struct Deployment {
    pub environment: String,
    /// The branch (or tag) deployed.
    pub reference: String,
    pub sha: String,
    pub created_at: String,
    /// Whether the MR's merge commit is in what was deployed (`None`: unknown,
    /// e.g. the MR reached the branch as a copy).
    pub includes: Option<bool>,
}

/// A file the MR and its target branch both changed since they parted.
///
/// GitLab's API does not list the files in conflict (its "Resolve conflicts"
/// screen reads a route that wants a browser session): they are worked out
/// from the two sides' patches instead, as git would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
    /// Both sides changed the same or adjacent lines, one side deleted the file,
    /// both added it, or a patch is missing (binary, too large): git stops on
    /// it. Otherwise the file most likely merges on its own.
    pub clash: bool,
}

/// Everything the page reads when it opens.
#[derive(Debug, Clone)]
pub struct PageData {
    pub overview: Option<Overview>,
    /// The files changed on both sides, clashes first; `None` when the MR is
    /// not in conflict or the patches could not be read.
    pub conflicts: Option<Vec<ConflictFile>>,
    /// `None`: no approval rules on this tier, or not readable.
    pub rules: Option<Vec<Rule>>,
    pub threads: Result<Vec<Thread>, String>,
    pub deployments: Vec<Deployment>,
    /// What the target branch has that the source lacks, on the target's first
    /// parents (its merges and direct pushes), newest first; `None` when the MR
    /// is not behind or the commits could not be read.
    pub behind: Option<Vec<BehindCommit>>,
}

/// A commit of the target branch the source branch does not have yet.
#[derive(Debug, Clone, Default)]
pub struct BehindCommit {
    pub sha: String,
    /// The commit's title; for a merge commit, the MR's title it names.
    pub title: String,
    pub author: String,
    pub created_at: String,
    pub web_url: String,
    /// The MR it merged (`See merge request group/project!12`), as its iid.
    pub mr_iid: Option<String>,
}

/// How many commits the "behind" list reads at most.
const BEHIND_READ: usize = 100;

/// Where to read: the instance, the token, the project and the MR.
#[derive(Clone)]
pub struct Target {
    pub base_url: String,
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
    pub mr_id: String,
    pub source_branch: String,
    pub target_branch: String,
    /// The MR's merge (or squash) commit, for the deployments.
    pub sha: Option<String>,
}

impl Target {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/api/v4/projects/{}/{}",
            self.base_url.trim_end_matches('/'),
            urlencoding::encode(&self.project_id),
            path
        )
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let res = http_client()
            .get(self.url(path))
            .header("PRIVATE-TOKEN", self.token.as_str())
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if !res.status().is_success() {
            return Err(format!("GitLab answered {}", res.status()));
        }
        res.json().await.map_err(|e| e.to_string())
    }
}

/// Reads the page's data, the parts at once.
pub async fn fetch(target: Target, deployed_branches: Vec<String>) -> PageData {
    let path = format!(
        "merge_requests/{}?include_diverged_commits_count=true",
        target.mr_id
    );
    let overview = target.get::<Overview>(&path);
    let rules = fetch_rules(&target);
    let threads = fetch_threads(&target);
    let deployments = fetch_deployments(&target, &deployed_branches);
    let (overview, rules, threads, deployments) =
        tokio::join!(overview, rules, threads, deployments);
    let overview = overview.ok();
    // Two more reads, only for an MR in conflict; one for an MR behind.
    let in_conflict = overview.as_ref().is_some_and(|o| o.has_conflicts);
    let is_behind = overview
        .as_ref()
        .and_then(|o| o.diverged_commits_count)
        .is_some_and(|n| n > 0);
    let (conflicts, behind) = tokio::join!(
        async {
            if in_conflict {
                conflict_files(&target).await
            } else {
                None
            }
        },
        async {
            if is_behind {
                behind_commits(&target).await
            } else {
                None
            }
        }
    );
    PageData {
        overview,
        conflicts,
        rules,
        threads,
        deployments,
        behind,
    }
}

/// The target's commits the source lacks (`source..target`), first parents
/// only: each merge once, not the commits of the branch it merged.
async fn behind_commits(target: &Target) -> Option<Vec<BehindCommit>> {
    #[derive(serde::Deserialize)]
    struct Commit {
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        message: String,
        #[serde(default)]
        author_name: String,
        #[serde(default)]
        created_at: String,
        #[serde(default)]
        web_url: String,
    }
    let range = format!("{}..{}", target.source_branch, target.target_branch);
    let path = format!(
        "repository/commits?ref_name={}&first_parent=true&per_page={BEHIND_READ}",
        urlencoding::encode(&range)
    );
    match target.get::<Vec<Commit>>(&path).await {
        Ok(commits) => Some(
            commits
                .into_iter()
                .map(|c| {
                    let (title, mr_iid) = read_merge_message(&c.title, &c.message);
                    BehindCommit {
                        sha: c.id,
                        title,
                        author: c.author_name,
                        created_at: c.created_at,
                        web_url: c.web_url,
                        mr_iid,
                    }
                })
                .collect(),
        ),
        Err(e) => {
            tracing::warn!(mr = %target.mr_id, error = %e, "Could not read the commits behind the target");
            None
        }
    }
}

/// The title to show for a commit, and the MR it names. GitLab's merge commits
/// read `Merge branch 'a' into 'b'`, the MR's title, then `See merge request
/// group/project!12`; a squash commit may end with the same trailer.
pub fn read_merge_message(title: &str, message: &str) -> (String, Option<String>) {
    let mr_iid = message.lines().rev().find_map(|line| {
        let rest = line.trim().strip_prefix("See merge request ")?;
        let (_, iid) = rest.rsplit_once('!')?;
        let iid = iid.trim_end_matches('.');
        (!iid.is_empty() && iid.bytes().all(|b| b.is_ascii_digit())).then(|| iid.to_string())
    });
    let is_merge =
        title.starts_with("Merge branch ") || title.starts_with("Merge remote-tracking branch ");
    let named = is_merge
        .then(|| {
            message
                .split("\n\n")
                .nth(1)
                .map(|p| p.lines().next().unwrap_or_default().trim())
                .filter(|p| !p.is_empty() && !p.starts_with("See merge request "))
        })
        .flatten();
    (named.unwrap_or(title).to_string(), mr_iid)
}

/// The MR's patches next to what its target changed since the merge base
/// (`/compare` from the source to the target, not `straight`).
async fn conflict_files(target: &Target) -> Option<Vec<ConflictFile>> {
    #[derive(serde::Deserialize)]
    struct Compare {
        #[serde(default)]
        diffs: Vec<crate::gitlab::FileDiff>,
    }
    let path = format!(
        "repository/compare?from={}&to={}",
        urlencoding::encode(&target.source_branch),
        urlencoding::encode(&target.target_branch)
    );
    let (ours, theirs) = tokio::join!(
        crate::gitlab::fetch_file_diffs(
            &target.base_url,
            target.token.as_str(),
            &target.project_id,
            &target.mr_id,
        ),
        target.get::<Compare>(&path),
    );
    match (ours, theirs) {
        (Ok(ours), Ok(theirs)) => Some(changed_on_both_sides(&ours, &theirs.diffs)),
        (ours, theirs) => {
            tracing::warn!(
                mr = %target.mr_id,
                ours = ?ours.err(),
                theirs = ?theirs.err(),
                "Could not read the patches to list the files in conflict"
            );
            None
        }
    }
}

/// The files both patch sets touch, clashes first, then by path.
pub fn changed_on_both_sides(
    ours: &[crate::gitlab::FileDiff],
    theirs: &[crate::gitlab::FileDiff],
) -> Vec<ConflictFile> {
    let mut files: Vec<ConflictFile> = ours
        .iter()
        .filter_map(|o| {
            let t = theirs
                .iter()
                .find(|t| t.old_path == o.old_path || t.new_path == o.new_path)?;
            let clash = o.deleted_file
                || t.deleted_file
                || (o.new_file && t.new_file)
                || o.diff.is_empty()
                || t.diff.is_empty()
                || overlap(&changed_spans(&o.diff), &changed_spans(&t.diff));
            Some(ConflictFile {
                path: o.new_path.clone(),
                clash,
            })
        })
        .collect();
    files.sort_by(|a, b| b.clash.cmp(&a.clash).then_with(|| a.path.cmp(&b.path)));
    files
}

/// The runs of lines a patch changes, in the base file. Twice the line number,
/// so that an insertion before line `l` sits at `2l - 1`, between its
/// neighbours.
fn changed_spans(patch: &str) -> Vec<(i64, i64)> {
    let mut spans = Vec::new();
    let mut run: Option<(i64, i64)> = None;
    // The next line of the base file; `None` before the first hunk (headers).
    let mut old: Option<i64> = None;
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("@@ -") {
            spans.extend(run.take());
            let range = header.split(' ').next().unwrap_or_default();
            let (start, count) = range.split_once(',').unwrap_or((range, "1"));
            let start: i64 = start.parse().unwrap_or(0);
            // `-a,0`: nothing removed, the lines go after line `a`.
            old = Some(if count == "0" { start + 1 } else { start });
            continue;
        }
        let Some(at) = old.as_mut() else { continue };
        let point = match line.as_bytes().first() {
            Some(b'-') => {
                *at += 1;
                2 * (*at - 1)
            }
            Some(b'+') => 2 * *at - 1,
            Some(b'\\') => continue,
            _ => {
                *at += 1;
                spans.extend(run.take());
                continue;
            }
        };
        run = Some(run.map_or((point, point), |(a, b)| (a.min(point), b.max(point))));
    }
    spans.extend(run);
    spans
}

/// Whether two sides' changes touch the same or adjacent lines: git does not
/// merge those on its own.
fn overlap(ours: &[(i64, i64)], theirs: &[(i64, i64)]) -> bool {
    ours.iter()
        .any(|&(a, b)| theirs.iter().any(|&(c, d)| a <= d + 2 && c <= b + 2))
}

async fn fetch_rules(target: &Target) -> Option<Vec<Rule>> {
    #[derive(serde::Deserialize)]
    struct State {
        #[serde(default)]
        rules: Vec<Rule>,
    }
    let state: State = target
        .get(&format!("merge_requests/{}/approval_state", target.mr_id))
        .await
        .ok()?;
    // Rules nobody has to meet (0 required, no approver) say nothing.
    let rules: Vec<Rule> = state
        .rules
        .into_iter()
        .filter(|r| r.approvals_required > 0 || !r.approved_by.is_empty())
        .collect();
    (!rules.is_empty()).then_some(rules)
}

/// The resolvable threads, every note but GitLab's system notes; unresolved first.
async fn fetch_threads(target: &Target) -> Result<Vec<Thread>, String> {
    #[derive(serde::Deserialize)]
    struct Discussion {
        #[serde(default)]
        notes: Vec<RawNote>,
    }
    #[derive(serde::Deserialize)]
    struct RawNote {
        #[serde(default)]
        body: String,
        author: Option<User>,
        #[serde(default)]
        created_at: String,
        #[serde(default)]
        system: bool,
        #[serde(default)]
        resolvable: bool,
        #[serde(default)]
        resolved: bool,
        position: Option<Position>,
    }
    #[derive(serde::Deserialize)]
    struct Position {
        new_path: Option<String>,
        old_path: Option<String>,
        new_line: Option<u32>,
        old_line: Option<u32>,
    }
    let mut threads = Vec::new();
    for page in 1..=5 {
        let batch: Vec<Discussion> = target
            .get(&format!(
                "merge_requests/{}/discussions?per_page=100&page={page}",
                target.mr_id
            ))
            .await?;
        let len = batch.len();
        for discussion in batch {
            let Some(first) = discussion.notes.first() else {
                continue;
            };
            if first.system || !first.resolvable {
                continue;
            }
            let position = first.position.as_ref();
            threads.push(Thread {
                resolved: discussion
                    .notes
                    .iter()
                    .filter(|n| n.resolvable)
                    .all(|n| n.resolved),
                path: position.and_then(|p| p.new_path.clone().or_else(|| p.old_path.clone())),
                line: position.and_then(|p| p.new_line.or(p.old_line)),
                notes: discussion
                    .notes
                    .into_iter()
                    .filter(|n| !n.system)
                    .map(|n| Note {
                        author: n
                            .author
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default(),
                        username: n.author.map(|a| a.username).unwrap_or_default(),
                        body: n.body,
                        created_at: n.created_at,
                    })
                    .collect(),
            });
        }
        if len < 100 {
            break;
        }
    }
    threads.sort_by_key(|t| t.resolved);
    Ok(threads)
}

/// The last successful deployment of each environment that deploys one of
/// `branches`, and whether it includes the MR's merge commit.
async fn fetch_deployments(target: &Target, branches: &[String]) -> Vec<Deployment> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(rename = "ref", default)]
        reference: String,
        #[serde(default)]
        sha: String,
        #[serde(default)]
        created_at: String,
        environment: Option<Env>,
    }
    #[derive(serde::Deserialize)]
    struct Env {
        name: String,
    }
    let Ok(raw) = target
        .get::<Vec<Raw>>("deployments?status=success&order_by=created_at&sort=desc&per_page=100")
        .await
    else {
        return Vec::new();
    };
    let mut latest: Vec<Deployment> = Vec::new();
    for d in raw {
        let Some(env) = d.environment else { continue };
        if !branches.contains(&d.reference) || latest.iter().any(|l| l.environment == env.name) {
            continue;
        }
        latest.push(Deployment {
            environment: env.name,
            reference: d.reference,
            sha: d.sha,
            created_at: d.created_at,
            includes: None,
        });
    }
    if let Some(sha) = &target.sha {
        for deployment in &mut latest {
            deployment.includes = contains(target, sha, &deployment.sha).await;
        }
    }
    latest
}

/// Whether `commit` is an ancestor of `head` (their merge base is `commit`).
async fn contains(target: &Target, commit: &str, head: &str) -> Option<bool> {
    #[derive(serde::Deserialize)]
    struct Base {
        id: String,
    }
    if commit == head {
        return Some(true);
    }
    let base: Base = target
        .get(&format!(
            "repository/merge_base?refs[]={}&refs[]={}",
            urlencoding::encode(commit),
            urlencoding::encode(head)
        ))
        .await
        .ok()?;
    Some(base.id == commit)
}

// ── The project's environments and branch pipelines ─────────────────────────

/// A deployment of an environment, as the environments page lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct DeploymentRecord {
    pub environment: String,
    /// The branch (or tag) deployed.
    pub reference: String,
    pub sha: String,
    pub created_at: String,
    /// `success`, `failed`, `running`, `canceled`…
    pub status: String,
    /// Who started it, by name.
    pub user: String,
}

/// The latest pipeline of a tracked branch.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchPipeline {
    pub branch: String,
    pub id: u64,
    /// The commit it ran on: the branch's head when it started.
    pub sha: String,
    pub status: crate::models::PipelineState,
    pub created_at: String,
    pub updated_at: String,
    pub web_url: String,
    /// The jobs that failed in it, by name.
    pub failed_jobs: Vec<String>,
}

/// What the environments and CI pages read about the project itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectOverview {
    /// Newest first.
    pub deployments: Vec<DeploymentRecord>,
    /// One per tracked branch that has a pipeline.
    pub pipelines: Vec<BranchPipeline>,
    /// The project's merges, tracked or not: the team's pace for the
    /// milestones' forecast.
    pub merges: MergeHistory,
}

/// When the project's MRs merged over the last [`MERGE_HISTORY_DAYS`] days,
/// kept between reads and between launches (`merges_<hash>.json` beside the
/// project's state file): the first read takes the whole period, the next ones
/// only what was merged (or updated) since, and are merged into it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MergeHistory {
    /// Merge date by MR iid.
    #[serde(default)]
    pub merged: std::collections::BTreeMap<u64, String>,
    /// Up to when it is complete (RFC 3339): every MR updated before is in.
    #[serde(default)]
    pub read_at: Option<String>,
}

impl MergeHistory {
    fn read_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let at = chrono::DateTime::parse_from_rfc3339(self.read_at.as_deref()?).ok()?;
        Some(at.with_timezone(&chrono::Utc))
    }

    /// Complete up to the last day: fit to draw a pace from. A first read cut
    /// short (more merges than its pages hold) has only the oldest weeks yet,
    /// and is not.
    pub fn is_current(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.read_at()
            .is_some_and(|at| now - at <= chrono::Duration::days(1))
    }
}

/// How far back the project's merges are read.
pub const MERGE_HISTORY_DAYS: i64 = 56;

/// Pages of 100 merged MRs read at most in one go; a read cut short goes on
/// from where it stopped at the next one.
const MERGE_HISTORY_PAGES: u32 = 5;

/// Overlap with the last read, for clocks and MRs updated while it ran.
const MERGE_OVERLAP_MINUTES: i64 = 10;

/// Where to read a project-level overview.
#[derive(Clone)]
pub struct ProjectTarget {
    pub base_url: String,
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
}

impl ProjectTarget {
    fn as_target(&self) -> Target {
        Target {
            base_url: self.base_url.clone(),
            token: self.token.clone(),
            project_id: self.project_id.clone(),
            mr_id: String::new(),
            source_branch: String::new(),
            target_branch: String::new(),
            sha: None,
        }
    }
}

/// Reads the project's latest deployments (100 at most), the latest pipeline
/// of each of `branches` with the jobs that failed in it, and its merges since
/// `merges` was last read (all of the period on the first read). A part that
/// cannot be read is left empty; the merges keep what they had.
pub async fn fetch_project(
    target: ProjectTarget,
    branches: Vec<String>,
    merges: MergeHistory,
    cache: Option<std::path::PathBuf>,
) -> ProjectOverview {
    let target = target.as_target();
    let deployments = project_deployments(&target);
    let pipelines = futures::future::join_all(
        branches
            .iter()
            .map(|branch| branch_pipeline(&target, branch)),
    );
    let merges = async {
        // Not known in this session yet: what the last launch left on disk.
        let known = match (&merges.read_at, &cache) {
            (None, Some(path)) => load_merges(path).await.unwrap_or(merges),
            _ => merges,
        };
        let updated = project_merges(&target, known.clone()).await;
        if let Some(path) = cache.filter(|_| updated != known) {
            save_merges(&path, &updated).await;
        }
        updated
    };
    let (deployments, pipelines, merges) = tokio::join!(deployments, pipelines, merges);
    ProjectOverview {
        deployments,
        pipelines: pipelines.into_iter().flatten().collect(),
        merges,
    }
}

/// `known` brought up to date: the MRs updated since it was complete (with
/// [`MERGE_OVERLAP_MINUTES`] of overlap), the whole period when it never was
/// or is older than the period; merges older than the period dropped. Read in
/// update order, [`MERGE_HISTORY_PAGES`] at most: cut short, it is complete up
/// to the last MR read and the next read goes on from there. On an error,
/// `known` as it was.
async fn project_merges(target: &Target, known: MergeHistory) -> MergeHistory {
    #[derive(serde::Deserialize)]
    struct Raw {
        iid: u64,
        merged_at: Option<String>,
        updated_at: Option<String>,
    }
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::days(MERGE_HISTORY_DAYS);
    let since = known
        .read_at()
        .map(|at| at - chrono::Duration::minutes(MERGE_OVERLAP_MINUTES))
        .filter(|at| *at > start)
        .unwrap_or(start);
    let since_text = since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut merged = known.merged.clone();
    let mut read_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    for page in 1..=MERGE_HISTORY_PAGES {
        // Updated after: an MR merged since was updated since.
        let path = format!(
            "merge_requests?state=merged&updated_after={}&order_by=updated_at&sort=asc&per_page=100&page={page}",
            urlencoding::encode(&since_text)
        );
        let Ok(raw) = target.get::<Vec<Raw>>(&path).await else {
            return known;
        };
        let full = raw.len() == 100;
        let last_updated = raw.last().and_then(|r| r.updated_at.clone());
        merged.extend(raw.into_iter().filter_map(|r| Some((r.iid, r.merged_at?))));
        if !full {
            break;
        }
        // More than the pages hold: complete up to the last MR read.
        if page == MERGE_HISTORY_PAGES {
            if let Some(at) = last_updated {
                read_at = at;
            }
        }
    }
    merged.retain(|_, at| {
        chrono::DateTime::parse_from_rfc3339(at)
            .is_ok_and(|t| t.with_timezone(&chrono::Utc) >= start)
    });
    MergeHistory {
        merged,
        read_at: Some(read_at),
    }
}

/// The merge history a previous launch saved, if any (a file that cannot be
/// read is ignored: the next read starts over).
async fn load_merges(path: &std::path::Path) -> Option<MergeHistory> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    match serde_json::from_str(&text) {
        Ok(history) => Some(history),
        Err(e) => {
            tracing::warn!(error = %e, path = ?path, "Unreadable merge history, read again");
            None
        }
    }
}

async fn save_merges(path: &std::path::Path, history: &MergeHistory) {
    match serde_json::to_string(history) {
        Ok(text) => {
            if let Err(e) = crate::storage::write_atomic(path, &text).await {
                tracing::warn!(error = %e, path = ?path, "Could not save the merge history");
            }
        }
        Err(e) => tracing::warn!(error = %e, "Could not serialise the merge history"),
    }
}

async fn project_deployments(target: &Target) -> Vec<DeploymentRecord> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(rename = "ref", default)]
        reference: String,
        #[serde(default)]
        sha: String,
        #[serde(default)]
        created_at: String,
        #[serde(default)]
        status: String,
        environment: Option<Env>,
        user: Option<User>,
    }
    #[derive(serde::Deserialize)]
    struct Env {
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct User {
        #[serde(default)]
        name: String,
    }
    let Ok(raw) = target
        .get::<Vec<Raw>>("deployments?order_by=created_at&sort=desc&per_page=100")
        .await
    else {
        return Vec::new();
    };
    raw.into_iter()
        .filter_map(|d| {
            Some(DeploymentRecord {
                environment: d.environment?.name,
                reference: d.reference,
                sha: d.sha,
                created_at: d.created_at,
                status: d.status,
                user: d.user.map(|u| u.name).unwrap_or_default(),
            })
        })
        .collect()
}

async fn branch_pipeline(target: &Target, branch: &str) -> Option<BranchPipeline> {
    #[derive(serde::Deserialize)]
    struct Raw {
        id: u64,
        #[serde(default)]
        sha: String,
        #[serde(default)]
        status: crate::models::PipelineState,
        #[serde(default)]
        created_at: String,
        #[serde(default)]
        updated_at: String,
        #[serde(default)]
        web_url: String,
    }
    #[derive(serde::Deserialize)]
    struct Job {
        #[serde(default)]
        name: String,
    }
    let list: Vec<Raw> = target
        .get(&format!(
            "pipelines?ref={}&per_page=1&order_by=id&sort=desc",
            urlencoding::encode(branch)
        ))
        .await
        .ok()?;
    let raw = list.into_iter().next()?;
    let failed_jobs = if raw.status == crate::models::PipelineState::Failed {
        target
            .get::<Vec<Job>>(&format!(
                "pipelines/{}/jobs?scope[]=failed&per_page=50",
                raw.id
            ))
            .await
            .map(|jobs| jobs.into_iter().map(|j| j.name).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Some(BranchPipeline {
        branch: branch.to_string(),
        id: raw.id,
        sha: raw.sha,
        status: raw.status,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        web_url: raw.web_url,
        failed_jobs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitlab::FileDiff;

    #[test]
    fn the_merge_history_round_trips_and_knows_when_it_is_current() {
        let now = chrono::Utc::now();
        let history = MergeHistory {
            merged: [(12, "2026-11-01T10:00:00Z".to_string())].into(),
            read_at: Some((now - chrono::Duration::hours(3)).to_rfc3339()),
        };
        let text = serde_json::to_string(&history).unwrap();
        assert_eq!(
            serde_json::from_str::<MergeHistory>(&text).unwrap(),
            history
        );
        assert!(history.is_current(now));
        // A read cut short two days ago: only its oldest weeks are known.
        let behind = MergeHistory {
            read_at: Some((now - chrono::Duration::days(2)).to_rfc3339()),
            ..history.clone()
        };
        assert!(!behind.is_current(now));
        assert!(!MergeHistory::default().is_current(now));
        // An older file without some fields still loads.
        assert_eq!(
            serde_json::from_str::<MergeHistory>("{}").unwrap(),
            MergeHistory::default()
        );
    }

    fn file(path: &str, diff: &str) -> FileDiff {
        FileDiff {
            old_path: path.into(),
            new_path: path.into(),
            new_file: false,
            renamed_file: false,
            deleted_file: false,
            diff: diff.into(),
        }
    }

    #[test]
    fn merge_commits_name_their_mr_and_its_title() {
        let message = "Merge branch 'feat/login' into 'main'\n\nfeat(auth): Login with SSO\n\nCloses #12\n\nSee merge request group/sub/project!42";
        assert_eq!(
            read_merge_message("Merge branch 'feat/login' into 'main'", message),
            ("feat(auth): Login with SSO".into(), Some("42".into()))
        );
        // A squash commit keeps its title, the trailer still names the MR.
        assert_eq!(
            read_merge_message(
                "fix: Typo",
                "fix: Typo\n\nSee merge request group/project!7"
            ),
            ("fix: Typo".into(), Some("7".into()))
        );
        // A direct push names none.
        assert_eq!(
            read_merge_message("docs: README", "docs: README\n"),
            ("docs: README".into(), None)
        );
    }

    #[test]
    fn spans_follow_the_base_file() {
        // Line 11 replaced, then one line inserted before line 14.
        let patch =
            "--- a/f\n+++ b/f\n@@ -10,5 +10,6 @@\n ctx\n-old\n+new\n ctx\n ctx\n+added\n ctx";
        assert_eq!(changed_spans(patch), [(22, 23), (27, 27)]);
        // A new file: everything goes before line 1.
        assert_eq!(changed_spans("@@ -0,0 +1,2 @@\n+a\n+b"), [(1, 1)]);
    }

    #[test]
    fn only_close_changes_clash() {
        let ours = [
            file("same_line.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+B\n c"),
            file("adjacent.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+B\n c"),
            file("far_apart.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+B\n c"),
            file("binary.png", ""),
            file("only_ours.rs", "@@ -1 +1 @@\n-a\n+b"),
        ];
        let theirs = [
            file("same_line.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+X\n c"),
            file("adjacent.rs", "@@ -6,3 +6,3 @@\n b\n-c\n+C\n d"),
            file("far_apart.rs", "@@ -40,3 +40,3 @@\n a\n-b\n+B\n c"),
            file("binary.png", ""),
        ];
        let files = changed_on_both_sides(&ours, &theirs);
        let clash = |p: &str| files.iter().find(|f| f.path == p).map(|f| f.clash);
        assert_eq!(clash("same_line.rs"), Some(true));
        assert_eq!(clash("adjacent.rs"), Some(true));
        assert_eq!(clash("far_apart.rs"), Some(false));
        assert_eq!(clash("binary.png"), Some(true));
        assert_eq!(clash("only_ours.rs"), None);
        // Clashes first.
        assert_eq!(files.last().map(|f| f.path.as_str()), Some("far_apart.rs"));
    }
}
