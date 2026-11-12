//! Reads from GitLab what [`crate::signatures`] needs to check an MR: its
//! commits' signatures, its versions (only when a commit may have lost its
//! signature in a rebase) and the patches of the commits matched across a
//! rebase. Whatever was read is kept with the MR ([`SignatureData`]): a SHA's
//! signature and patch never change, so each is read once.
//!
//! Calls, for an MR whose commits are all signed: one per commit, once. A
//! commit without a signature adds the versions list, one commit list per
//! version (`base..head`, no diffs) and, for each copy matched, its signature
//! and the diffs of both commits.

use std::sync::Arc;

use futures::future::join_all;
use gitlab_tracker_core::pacing::SendPaced;
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::gitlab::{get_all_pages, http_client, CommitJson, FetchContext};
use crate::models::{AppEvent, MrCommit};
use crate::signatures::{
    classify, patch_id, DiffFile, Policy, Report, Signature, SignatureData, Status, Verdict,
    Version, VersionCommit,
};

/// Versions read at most, the newest: a signed original further back is rare.
const MAX_VERSIONS: usize = 20;
/// Rounds of reading, each one what the previous classification still lacked
/// (a rebase traced back one version per round at worst).
const MAX_ROUNDS: usize = 8;

/// One MR to check.
#[derive(Debug, Clone)]
pub struct CheckRequest {
    pub mr_id: String,
    /// The GitLab username that opened the MR.
    pub author: String,
    /// The MR's commits now.
    pub commits: Vec<MrCommit>,
    /// What was read before.
    pub data: SignatureData,
    pub policy: Policy,
}

/// Whether `data` lacks something to check `commits` under `policy`: never
/// read, read for other commits, or missing a signature, a patch or the
/// versions.
pub fn needs_check(
    data: Option<&SignatureData>,
    commits: &[MrCommit],
    author: &str,
    policy: &Policy,
) -> bool {
    if commits.is_empty() {
        return false;
    }
    let Some(data) = data else {
        return true;
    };
    if !data.is_current(commits.iter().map(|c| c.sha.as_str())) {
        return true;
    }
    let report = classify(&data.versions, author, &data.facts, policy);
    !report.complete() || wants_versions(data, &report, policy)
}

/// Checks `request` in the background and emits [`AppEvent::SignaturesChecked`].
pub fn spawn_signature_check(
    ctx: FetchContext,
    request: CheckRequest,
    semaphore: Arc<Semaphore>,
    tx: UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let _permit = semaphore.acquire().await;
        let mr_id = request.mr_id.clone();
        let data = check(&ctx, request).await;
        let _ = tx.send(AppEvent::SignaturesChecked {
            mr_id,
            data: Box::new(data),
        });
    });
}

/// Reads what is missing, round after round, and returns all that is known.
/// What could not be read stays missing: the next refresh tries again.
pub async fn check(ctx: &FetchContext, request: CheckRequest) -> SignatureData {
    let CheckRequest {
        mr_id,
        author,
        commits,
        mut data,
        policy,
    } = request;
    if !data.is_current(commits.iter().map(|c| c.sha.as_str())) {
        data.versions = vec![current_version(&commits)];
        data.versions_read = false;
    }
    for _ in 0..MAX_ROUNDS {
        let report = classify(&data.versions, &author, &data.facts, &policy);
        if wants_versions(&data, &report, &policy) {
            match fetch_versions(ctx, &mr_id).await {
                Some(versions)
                    if versions.last().is_some_and(|last| {
                        SignatureData {
                            versions: vec![last.clone()],
                            ..SignatureData::default()
                        }
                        .is_current(commits.iter().map(|c| c.sha.as_str()))
                    }) =>
                {
                    data.versions = versions;
                    data.versions_read = true;
                    continue;
                }
                // Pushed meanwhile, or not readable: the next refresh tries again.
                _ => break,
            }
        }
        if report.missing.is_empty() {
            break;
        }
        let signatures = join_all(
            report
                .missing
                .signatures
                .iter()
                .map(|sha| async move { (sha.clone(), fetch_signature(ctx, sha).await) }),
        )
        .await;
        let patches = join_all(
            report
                .missing
                .patch_ids
                .iter()
                .map(|sha| async move { (sha.clone(), fetch_patch_id(ctx, sha).await) }),
        )
        .await;
        let mut progress = false;
        for (sha, signature) in signatures {
            if let Some(signature) = signature {
                data.facts.signatures.insert(sha, signature);
                progress = true;
            }
        }
        for (sha, patch) in patches {
            if let Some(patch) = patch {
                data.facts.patch_ids.insert(sha, patch);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    data
}

/// The versions are worth reading: a commit has no signature, or the
/// instance's, and only the history can tell whether a rebase took it.
fn wants_versions(data: &SignatureData, report: &Report, policy: &Policy) -> bool {
    policy.tolerate_gitlab_rebase
        && !data.versions_read
        && report
            .commits
            .iter()
            .any(|c| !c.accepted && matches!(c.verdict, Verdict::Unsigned | Verdict::SystemSigned))
}

/// The MR's commits as a version of their own, before its versions are read.
fn current_version(commits: &[MrCommit]) -> Version {
    Version {
        base_sha: String::new(),
        commits: commits
            .iter()
            .map(|c| VersionCommit {
                sha: c.sha.clone(),
                fingerprint: c.fingerprint.clone(),
            })
            .collect(),
    }
}

fn project_url(ctx: &FetchContext) -> String {
    format!("{}/api/v4/projects/{}", ctx.base_url, ctx.project_id)
}

/// A commit's signature: GitLab answers 404 for a commit without one. `None`
/// when GitLab could not be read.
async fn fetch_signature(ctx: &FetchContext, sha: &str) -> Option<Signature> {
    #[derive(serde::Deserialize)]
    struct SignatureJson {
        #[serde(default)]
        verification_status: String,
    }
    let url = format!(
        "{}/repository/commits/{}/signature",
        project_url(ctx),
        urlencoding::encode(sha)
    );
    let res = http_client()
        .get(url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
        .ok()?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Some(Signature::Unsigned);
    }
    if !res.status().is_success() {
        tracing::warn!(sha, status = %res.status(), "Could not read a commit's signature");
        return None;
    }
    let json: SignatureJson = res.json().await.ok()?;
    Some(Signature::Signed(Status::parse(&json.verification_status)))
}

/// A commit's patch-id (`Some(None)`: read, but GitLab left a diff out).
async fn fetch_patch_id(ctx: &FetchContext, sha: &str) -> Option<Option<String>> {
    let url = format!(
        "{}/repository/commits/{}/diff?",
        project_url(ctx),
        urlencoding::encode(sha)
    );
    let files = get_all_pages::<DiffFile>(ctx, &url).await?;
    Some(patch_id(&files))
}

/// The MR's versions, oldest first, each with its commits.
async fn fetch_versions(ctx: &FetchContext, mr_id: &str) -> Option<Vec<Version>> {
    #[derive(serde::Deserialize)]
    struct VersionJson {
        base_commit_sha: Option<String>,
        head_commit_sha: Option<String>,
    }
    let url = format!("{}/merge_requests/{}/versions?", project_url(ctx), mr_id);
    // Newest first.
    let mut listed = get_all_pages::<VersionJson>(ctx, &url).await?;
    listed.retain(|v| v.base_commit_sha.is_some() && v.head_commit_sha.is_some());
    listed.truncate(MAX_VERSIONS);
    listed.reverse();
    let versions = join_all(listed.into_iter().map(|v| async move {
        let base = v.base_commit_sha.unwrap_or_default();
        let head = v.head_commit_sha.unwrap_or_default();
        let commits = fetch_range(ctx, &base, &head).await?;
        Some(Version {
            base_sha: base,
            commits,
        })
    }))
    .await;
    versions.into_iter().collect()
}

/// The commits of `base..head`: a version's own commits, without its diffs.
async fn fetch_range(ctx: &FetchContext, base: &str, head: &str) -> Option<Vec<VersionCommit>> {
    let url = format!(
        "{}/repository/commits?ref_name={}",
        project_url(ctx),
        urlencoding::encode(&format!("{base}..{head}"))
    );
    let commits = get_all_pages::<CommitJson>(ctx, &url).await?;
    Some(
        commits
            .into_iter()
            .filter_map(|c| {
                let fingerprint = c.fingerprint()?;
                Some(VersionCommit {
                    sha: c.id,
                    fingerprint,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitlab::tests::{fake_gitlab, NOT_FOUND};
    use crate::models::CommitFingerprint;

    fn commit(sha: &str, title: &str) -> MrCommit {
        MrCommit {
            sha: sha.into(),
            fingerprint: CommitFingerprint {
                author_email: "alice@example.com".into(),
                authored_at: 1_700_000_000,
                title: title.into(),
            },
        }
    }

    fn ctx(url: String) -> FetchContext {
        FetchContext {
            base_url: url,
            token: Arc::new(zeroize::Zeroizing::new("token".into())),
            project_id: "7".into(),
            branches: Vec::new(),
        }
    }

    fn request(commits: Vec<MrCommit>) -> CheckRequest {
        CheckRequest {
            mr_id: "12".into(),
            author: "alice".into(),
            commits,
            data: SignatureData::default(),
            policy: Policy::default(),
        }
    }

    fn json_commit(sha: &str, title: &str) -> serde_json::Value {
        serde_json::json!({"id": sha, "title": title, "author_email": "alice@example.com",
                           "authored_date": "2023-11-14T22:13:20Z"})
    }

    /// v1 (`a` signed) on base `x`; v2, rebased from GitLab's UI on `y`: `a2`
    /// unsigned. Same patch for both.
    fn rebased_route(path: &str, _body: &str) -> String {
        let p = path.split('?').next().unwrap();
        let query = path.split('?').nth(1).unwrap_or_default();
        if p.ends_with("/commits/a/signature") {
            r#"{"verification_status":"verified"}"#.into()
        } else if p.ends_with("/signature") {
            NOT_FOUND.into()
        } else if p.ends_with("/merge_requests/12/versions") {
            serde_json::json!([
                {"base_commit_sha": "y", "head_commit_sha": "a2"},
                {"base_commit_sha": "x", "head_commit_sha": "a"},
            ])
            .to_string()
        } else if p.ends_with("/repository/commits") && query.contains("x..a") {
            serde_json::json!([json_commit("a", "one")]).to_string()
        } else if p.ends_with("/repository/commits") && query.contains("y..a2") {
            serde_json::json!([json_commit("a2", "one")]).to_string()
        } else if p.ends_with("/diff") {
            serde_json::json!([{"old_path": "f.rs", "new_path": "f.rs",
                                "diff": "@@ -1 +1 @@\n-a\n+b\n"}])
            .to_string()
        } else {
            "[]".into()
        }
    }

    #[tokio::test]
    async fn a_rebase_from_gitlab_is_traced_through_the_versions() {
        let (url, seen) = fake_gitlab(rebased_route).await;
        let data = check(&ctx(url), request(vec![commit("a2", "one")])).await;
        assert!(data.versions_read);
        assert_eq!(data.versions.len(), 2);
        let report = classify(&data.versions, "alice", &data.facts, &Policy::default());
        assert_eq!(
            report.commits[0].verdict,
            Verdict::SignedBeforeRebase {
                original: "a".into(),
                version: 1
            }
        );
        assert!(report.complete());

        // Read once: a second check reads nothing.
        let calls = seen.lock().unwrap().len();
        let again = check(
            &ctx(String::from("http://127.0.0.1:9")),
            CheckRequest {
                data: data.clone(),
                ..request(vec![commit("a2", "one")])
            },
        )
        .await;
        assert_eq!(again, data);
        assert_eq!(seen.lock().unwrap().len(), calls);
        assert!(!needs_check(
            Some(&data),
            &[commit("a2", "one")],
            "alice",
            &Policy::default()
        ));
    }

    #[tokio::test]
    async fn signed_commits_need_one_call_each() {
        fn route(path: &str, _body: &str) -> String {
            if path.contains("/signature") {
                r#"{"verification_status":"verified"}"#.into()
            } else {
                panic!("unexpected call: {path}")
            }
        }
        let (url, seen) = fake_gitlab(route).await;
        let data = check(
            &ctx(url),
            request(vec![commit("a", "one"), commit("b", "two")]),
        )
        .await;
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(!data.versions_read, "no version read for signed commits");
        let report = classify(&data.versions, "alice", &data.facts, &Policy::default());
        assert!(report
            .commits
            .iter()
            .all(|c| c.verdict == Verdict::Verified));
    }

    #[tokio::test]
    async fn an_unreachable_gitlab_leaves_the_commits_pending() {
        let data = check(
            &ctx("http://127.0.0.1:9".into()),
            request(vec![commit("a", "one")]),
        )
        .await;
        let report = classify(&data.versions, "alice", &data.facts, &Policy::default());
        assert_eq!(report.commits[0].verdict, Verdict::Pending);
        assert!(needs_check(
            Some(&data),
            &[commit("a", "one")],
            "alice",
            &Policy::default()
        ));
    }

    #[test]
    fn new_commits_need_a_new_check() {
        let data = SignatureData {
            versions: vec![current_version(&[commit("a", "one")])],
            versions_read: false,
            facts: crate::signatures::Facts {
                signatures: [("a".to_string(), Signature::Signed(Status::Verified))].into(),
                ..Default::default()
            },
        };
        let policy = Policy::default();
        assert!(!needs_check(
            Some(&data),
            &[commit("a", "one")],
            "alice",
            &policy
        ));
        assert!(needs_check(
            Some(&data),
            &[commit("a", "one"), commit("b", "two")],
            "alice",
            &policy
        ));
        assert!(needs_check(None, &[commit("a", "one")], "alice", &policy));
        assert!(!needs_check(None, &[], "alice", &policy));
    }
}
