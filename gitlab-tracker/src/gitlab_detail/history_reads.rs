//! Releases (or version tags), the merge history, deployments and branch pipelines.

use super::*;

/// How many releases and tags are read: the newest, enough for a cadence.
pub(super) const RELEASES_READ: usize = 50;

/// The project's releases, newest first: its GitLab releases when it has
/// some, else its tags named like a version. Whether the releases could not
/// be read for want of the permission (the tags are used then).
pub(super) async fn project_releases(target: &Target) -> (Vec<ReleaseRecord>, bool) {
    #[derive(serde::Deserialize)]
    struct RawRelease {
        tag_name: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        released_at: Option<String>,
        #[serde(default)]
        upcoming_release: bool,
        commit: Option<RawCommit>,
        #[serde(rename = "_links", default)]
        links: Option<RawLinks>,
    }
    #[derive(serde::Deserialize)]
    struct RawCommit {
        id: String,
        #[serde(default)]
        committed_date: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawLinks {
        #[serde(rename = "self", default)]
        this: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawTag {
        name: String,
        commit: RawCommit,
    }
    let releases = target
        .get::<Vec<RawRelease>>(&format!(
            "releases?order_by=released_at&sort=desc&per_page={RELEASES_READ}"
        ))
        .await;
    let denied = matches!(&releases, Err(e) if e.contains("403"));
    let list: Vec<ReleaseRecord> = releases
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.upcoming_release)
        .filter_map(|r| {
            let commit = r.commit?;
            Some(ReleaseRecord {
                name: r.name.filter(|n| !n.trim().is_empty() && *n != r.tag_name),
                sha: commit.id,
                committed_at: commit.committed_date.unwrap_or_default(),
                released_at: r.released_at,
                notes: r.description.filter(|d| !d.trim().is_empty()),
                web_url: r.links.and_then(|l| l.this).unwrap_or_default(),
                is_release: true,
                tag: r.tag_name,
            })
        })
        .collect();
    if !list.is_empty() {
        return (list, denied);
    }
    let Ok(tags) = target
        .get::<Vec<RawTag>>(&format!(
            "repository/tags?order_by=updated&sort=desc&per_page={RELEASES_READ}"
        ))
        .await
    else {
        return (Vec::new(), denied);
    };
    let mut list: Vec<ReleaseRecord> = tags
        .into_iter()
        .filter(|t| is_version_tag(&t.name))
        .map(|t| ReleaseRecord {
            // The page knows the project's address: it links the tag.
            web_url: String::new(),
            tag: t.name,
            name: None,
            sha: t.commit.id,
            committed_at: t.commit.committed_date.unwrap_or_default(),
            released_at: None,
            notes: None,
            is_release: false,
        })
        .collect();
    list.sort_by(|a, b| b.committed_at.cmp(&a.committed_at));
    (list, denied)
}

/// `known` brought up to date: the MRs updated since it was complete (with
/// [`MERGE_OVERLAP_MINUTES`] of overlap), the whole period when it never was
/// or is older than the period; merges older than the period dropped. Read in
/// update order, [`MERGE_HISTORY_PAGES`] at most: cut short, it is complete up
/// to the last MR read and the next read goes on from there. On an error,
/// `known` as it was.
pub(super) async fn project_merges(target: &Target, known: MergeHistory) -> MergeHistory {
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
    retain_period(&mut merged, now);
    MergeHistory {
        merged,
        read_at: Some(read_at),
    }
}

/// Only the merges of the last [`MERGE_HISTORY_DAYS`] days.
pub(super) fn retain_period(
    merged: &mut std::collections::BTreeMap<u64, String>,
    now: chrono::DateTime<chrono::Utc>,
) {
    let start = now - chrono::Duration::days(MERGE_HISTORY_DAYS);
    merged.retain(|_, at| {
        chrono::DateTime::parse_from_rfc3339(at)
            .is_ok_and(|t| t.with_timezone(&chrono::Utc) >= start)
    });
}

pub(super) async fn project_deployments(target: &Target) -> Vec<DeploymentRecord> {
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

pub(super) async fn branch_pipeline(target: &Target, branch: &str) -> Option<BranchPipeline> {
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
