//! Reading from GitLab: the CODEOWNERS GitLab would use, and the merged MRs' history (GraphQL, or REST without it).

use super::*;

use crate::gitlab::http_client;
use crate::gitlab_detail::ProjectTarget;
use gitlab_tracker_core::pacing::SendPaced;

/// MRs per GraphQL page, until the instance finds it too complex.
pub(super) const PAGE: usize = 50;
/// GraphQL pages read in one go: a first read of a busy project goes on at
/// the next one.
pub(super) const PAGES: usize = 10;
/// MRs whose files are read one by one over REST in one go, when GraphQL
/// cannot be used.
pub(super) const REST_MRS: usize = 40;
/// Read again from this far before the last read: an MR merged while it ran.
pub(super) const OVERLAP_HOURS: i64 = 24;

pub(super) fn api_url(target: &ProjectTarget, path: &str) -> String {
    format!(
        "{}/api/v4/projects/{}/{}",
        target.base_url.trim_end_matches('/'),
        urlencoding::encode(&target.project_id),
        path
    )
}

pub(super) async fn get(target: &ProjectTarget, path: &str) -> Result<reqwest::Response, String> {
    http_client()
        .get(api_url(target, path))
        .header("PRIVATE-TOKEN", target.token.as_str())
        .send_paced()
        .await
        .map_err(|e| e.without_url().to_string())
}

/// The `CODEOWNERS` GitLab would use for an MR into `branch`.
pub async fn read_declared(target: &ProjectTarget, branch: &str) -> Result<Declared, String> {
    for location in LOCATIONS {
        let path = format!(
            "repository/files/{}/raw?ref={}",
            urlencoding::encode(location),
            urlencoding::encode(branch)
        );
        let res = get(target, &path).await?;
        match res.status() {
            s if s.is_success() => {
                let text = res.text().await.map_err(|e| e.to_string())?;
                return Ok(Declared::Found(CodeOwners::parse(location, &text)));
            }
            reqwest::StatusCode::NOT_FOUND => continue,
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::UNAUTHORIZED => {
                return Ok(Declared::Denied)
            }
            s => return Err(format!("GitLab answered {s}")),
        }
    }
    Ok(Declared::Absent)
}

#[derive(serde::Deserialize)]
pub(super) struct Person {
    #[serde(default)]
    pub(super) username: String,
    #[serde(default)]
    pub(super) name: String,
}

/// What one read brought: the merges, the names, and up to when it is complete.
pub(super) struct Read {
    pub(super) merges: Vec<(u64, Merge)>,
    pub(super) people: Vec<Person>,
    pub(super) complete_to: DateTime<Utc>,
}

/// The project's merged MRs since the last read (the whole period the first
/// time), added to `known`; `known` as it was when GitLab cannot be read. The
/// result is saved at `cache` when it changed; a first read starts from what
/// `cache` holds.
pub async fn read_history(
    target: ProjectTarget,
    known: History,
    cache: Option<crate::storage::ProjectStore>,
) -> History {
    let now = Utc::now();
    let known = match (&known.read_at, &cache) {
        (None, Some(store)) => store
            .load_owners()
            .await
            .map(|mut h| {
                // The database keeps them all: the period's only.
                h.trim(now);
                h
            })
            .unwrap_or(known),
        _ => known,
    };
    let start = now - chrono::Duration::days(HISTORY_DAYS);
    let since = known
        .read_at()
        .map(|at| at - chrono::Duration::hours(OVERLAP_HOURS))
        .filter(|at| *at > start)
        .unwrap_or(start);
    let read = match crate::gitlab_graphql::support(&target.base_url) {
        crate::gitlab_graphql::Support::Batch(_) => match graphql(&target, since, now).await {
            Ok(read) => Ok(read),
            Err(e) => {
                tracing::warn!(error = %e, "Merged MRs over GraphQL failed — REST");
                rest(&target, since, start, now).await
            }
        },
        crate::gitlab_graphql::Support::Off => rest(&target, since, start, now).await,
    };
    let read = match read {
        Ok(read) => read,
        Err(e) => {
            tracing::warn!(error = %e, "Could not read the merged MRs");
            return known;
        }
    };
    let mut history = known.clone();
    history.merges.extend(read.merges);
    for p in read.people.into_iter().filter(|p| !p.username.is_empty()) {
        if !p.name.is_empty() {
            history.people.insert(p.username, p.name);
        }
    }
    history.read_at = Some(
        read.complete_to
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    history.trim(now);
    if let Some(store) = cache.filter(|_| history != known) {
        store.save_owners(&history, &known).await;
    }
    history
}

/// The query of a page of merged MRs, oldest merge first.
pub(super) fn query(
    project_id: &str,
    since: &str,
    after: Option<&str>,
    first: usize,
) -> serde_json::Value {
    let mrs = "mergeRequests(state: merged, mergedAfter: $since, sort: MERGED_AT_ASC, \
               first: $first, after: $after) { pageInfo { hasNextPage endCursor } \
               nodes { iid mergedAt author { username name } \
               reviewers { nodes { username name } } \
               approvedBy { nodes { username name } } diffStats { path } } }";
    let (head, body, project) = match crate::gitlab_graphql::project_ref(project_id) {
        crate::gitlab_graphql::ProjectRef::Path(path) => (
            "$project: ID!",
            format!("project(fullPath: $project) {{ {mrs} }}"),
            serde_json::json!({ "project": path }),
        ),
        crate::gitlab_graphql::ProjectRef::Id(gid) => (
            "$ids: [ID!]",
            format!("projects(ids: $ids) {{ nodes {{ {mrs} }} }}"),
            serde_json::json!({ "ids": [gid] }),
        ),
    };
    let mut variables = serde_json::json!({ "since": since, "after": after, "first": first });
    if let (Some(v), Some(p)) = (variables.as_object_mut(), project.as_object()) {
        v.extend(p.clone());
    }
    serde_json::json!({
        "query": format!("query({head}, $since: Time, $after: String, $first: Int) {{ {body} }}"),
        "variables": variables,
    })
}

/// A page of the answer: its MRs, their people, and the cursor of the next one.
pub(super) type Page = (Vec<(u64, Merge)>, Vec<Person>, Option<String>);

/// A page of the answer: its MRs, and the cursor of the next one.
pub(super) fn page_of(body: &serde_json::Value) -> Result<Page, String> {
    if let Some(errors) = body.get("errors").and_then(|e| e.as_array()) {
        if let Some(first) = errors.first() {
            let message = first.get("message").and_then(|m| m.as_str()).unwrap_or("");
            return Err(message.to_string());
        }
    }
    let data = body.get("data").ok_or("no data")?;
    let project = data
        .get("project")
        .or_else(|| data.pointer("/projects/nodes/0"))
        .ok_or("project not found or not visible")?;
    let mrs = project.get("mergeRequests").ok_or("no mergeRequests")?;
    let people_of = |node: &serde_json::Value, key: &str| -> Vec<Person> {
        node.pointer(&format!("/{key}/nodes"))
            .and_then(|n| serde_json::from_value(n.clone()).ok())
            .unwrap_or_default()
    };
    let mut merges = Vec::new();
    let mut people = Vec::new();
    for node in mrs
        .get("nodes")
        .and_then(|n| n.as_array())
        .into_iter()
        .flatten()
    {
        let Some(iid) = node
            .get("iid")
            .and_then(|i| i.as_str())
            .and_then(|i| i.parse().ok())
        else {
            continue;
        };
        let Some(merged_at) = node.get("mergedAt").and_then(|m| m.as_str()) else {
            continue;
        };
        let author: Option<Person> = node
            .get("author")
            .and_then(|a| serde_json::from_value(a.clone()).ok());
        let reviewers = people_of(node, "reviewers");
        let approvers = people_of(node, "approvedBy");
        let paths = node
            .get("diffStats")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|d| d.get("path")?.as_str().map(str::to_string))
            .collect();
        merges.push((
            iid,
            Merge {
                merged_at: merged_at.to_string(),
                author: author
                    .as_ref()
                    .map(|a| a.username.clone())
                    .unwrap_or_default(),
                reviewers: reviewers.iter().map(|p| p.username.clone()).collect(),
                approvers: approvers.iter().map(|p| p.username.clone()).collect(),
                paths,
            },
        ));
        people.extend(author);
        people.extend(reviewers);
        people.extend(approvers);
    }
    let next = mrs
        .pointer("/pageInfo/hasNextPage")
        .and_then(|h| h.as_bool())
        .filter(|h| *h)
        .and_then(|_| {
            mrs.pointer("/pageInfo/endCursor")?
                .as_str()
                .map(str::to_string)
        });
    Ok((merges, people, next))
}

pub(super) async fn graphql(
    target: &ProjectTarget,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Read, String> {
    let url = format!("{}/api/graphql", target.base_url.trim_end_matches('/'));
    let since_text = since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut read = Read {
        merges: Vec::new(),
        people: Vec::new(),
        complete_to: now,
    };
    let mut after: Option<String> = None;
    let mut size = PAGE;
    let mut pages = 0;
    while pages < PAGES {
        let res = http_client()
            .post(&url)
            .bearer_auth(target.token.as_str())
            .json(&query(
                &target.project_id,
                &since_text,
                after.as_deref(),
                size,
            ))
            .send_paced()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if !res.status().is_success() {
            return Err(format!("HTTP {}", res.status()));
        }
        let body: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        let (merges, people, next) = match page_of(&body) {
            Ok(page) => page,
            Err(message) => {
                let lower = message.to_lowercase();
                let too_big = ["complexity", "depth", "too many", "max_", "maximum"]
                    .iter()
                    .any(|w| lower.contains(w));
                if too_big && size > 5 {
                    size /= 2;
                    continue;
                }
                return Err(message);
            }
        };
        pages += 1;
        read.merges.extend(merges);
        read.people.extend(people);
        match next {
            Some(cursor) => after = Some(cursor),
            None => return Ok(read),
        }
    }
    // More than the pages hold: complete up to the last merge read.
    if let Some(at) = read
        .merges
        .iter()
        .filter_map(|(_, m)| time(&m.merged_at))
        .max()
    {
        read.complete_to = at;
    }
    Ok(read)
}

/// Without GraphQL: the merged MRs over REST, then each one's files
/// ([`REST_MRS`] at most in one go). Approvals are not read this way.
pub(super) async fn rest(
    target: &ProjectTarget,
    since: DateTime<Utc>,
    start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Read, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        iid: u64,
        merged_at: Option<String>,
        updated_at: Option<String>,
        author: Option<Person>,
        #[serde(default)]
        reviewers: Vec<Person>,
    }
    #[derive(serde::Deserialize)]
    struct Changes {
        #[serde(default)]
        changes: Vec<Change>,
    }
    #[derive(serde::Deserialize)]
    struct Change {
        #[serde(default)]
        new_path: String,
    }
    let since_text = since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let path = format!(
        "merge_requests?state=merged&updated_after={}&order_by=updated_at&sort=asc&per_page=100",
        urlencoding::encode(&since_text)
    );
    let res = get(target, &path).await?;
    if !res.status().is_success() {
        return Err(format!("GitLab answered {}", res.status()));
    }
    let list: Vec<Raw> = res.json().await.map_err(|e| e.to_string())?;
    let full = list.len() == 100;
    let mut read = Read {
        merges: Vec::new(),
        people: Vec::new(),
        complete_to: now,
    };
    let mut fetched = 0;
    let mut last_updated: Option<String> = None;
    for raw in list {
        let Some(merged_at) = raw.merged_at.clone() else {
            continue;
        };
        if time(&merged_at).is_none_or(|t| t < start) {
            continue;
        }
        if fetched == REST_MRS {
            // The rest at the next read.
            if let Some(at) = last_updated.as_deref().and_then(time) {
                read.complete_to = at;
            }
            return Ok(read);
        }
        fetched += 1;
        let res = get(target, &format!("merge_requests/{}/changes", raw.iid)).await?;
        if !res.status().is_success() {
            return Err(format!("GitLab answered {}", res.status()));
        }
        let changes: Changes = res.json().await.map_err(|e| e.to_string())?;
        read.merges.push((
            raw.iid,
            Merge {
                merged_at,
                author: raw
                    .author
                    .as_ref()
                    .map(|a| a.username.clone())
                    .unwrap_or_default(),
                reviewers: raw.reviewers.iter().map(|p| p.username.clone()).collect(),
                approvers: Vec::new(),
                paths: changes.changes.into_iter().map(|c| c.new_path).collect(),
            },
        ));
        read.people.extend(raw.author);
        read.people.extend(raw.reviewers);
        last_updated = raw.updated_at;
    }
    if full {
        if let Some(at) = last_updated.as_deref().and_then(time) {
            read.complete_to = at;
        }
    }
    Ok(read)
}
