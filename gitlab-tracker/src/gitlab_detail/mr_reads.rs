//! The MR page's reads: merge checks, commits behind, conflicting files, approval rules, threads, deployments.

use super::*;

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
pub(super) async fn behind_commits(target: &Target) -> Option<Vec<BehindCommit>> {
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
pub(super) async fn conflict_files(target: &Target) -> Option<Vec<ConflictFile>> {
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
pub(super) fn changed_spans(patch: &str) -> Vec<(i64, i64)> {
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
pub(super) fn overlap(ours: &[(i64, i64)], theirs: &[(i64, i64)]) -> bool {
    ours.iter()
        .any(|&(a, b)| theirs.iter().any(|&(c, d)| a <= d + 2 && c <= b + 2))
}

pub(super) async fn fetch_rules(target: &Target) -> Option<Vec<Rule>> {
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

/// The threads and plain comments, every note but GitLab's system notes;
/// unresolved threads first.
pub(super) async fn fetch_threads(target: &Target) -> Result<Vec<Thread>, String> {
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
            if first.system {
                continue;
            }
            let position = first.position.as_ref();
            let resolvable = discussion.notes.iter().any(|n| n.resolvable);
            threads.push(Thread {
                resolvable,
                resolved: resolvable
                    && discussion
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
    threads.sort_by_key(|t| (t.resolved, !t.resolvable));
    Ok(threads)
}

/// The last successful deployment of each environment that deploys one of
/// `branches`, and whether it includes the MR's merge commit.
pub(super) async fn fetch_deployments(target: &Target, branches: &[String]) -> Vec<Deployment> {
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
pub(super) async fn contains(target: &Target, commit: &str, head: &str) -> Option<bool> {
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
