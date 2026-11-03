//! What the window's MR page reads from GitLab when it opens, on top of what the
//! refresh cycle keeps: why the MR cannot merge, its approval rules, its threads
//! in full, and the deployments of the tracked branches (`gui` feature).
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

/// Everything the page reads when it opens.
#[derive(Debug, Clone)]
pub struct PageData {
    pub overview: Option<Overview>,
    /// `None`: no approval rules on this tier, or not readable.
    pub rules: Option<Vec<Rule>>,
    pub threads: Result<Vec<Thread>, String>,
    pub deployments: Vec<Deployment>,
}

/// Where to read: the instance, the token, the project and the MR.
#[derive(Clone)]
pub struct Target {
    pub base_url: String,
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
    pub mr_id: String,
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
    PageData {
        overview: overview.ok(),
        rules,
        threads,
        deployments,
    }
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
