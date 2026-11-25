//! One MR read in full: the request sequence behind `MrLoaded`.

use super::*;

/// Fetches everything the UI needs for one MR.
///
/// A `semaphore` permit is held for the whole fetch (bounding concurrent API
/// traffic), except while sleeping between mergeability retries: an idle wait
/// must not block other MRs from being fetched.
pub async fn fetch_gitlab_data<E: From<GitlabEvent> + Send + 'static>(
    ctx: &FetchContext,
    mr_id: &str,
    cached: CachedMrData,
    prefetched: Prefetched,
    semaphore: &Semaphore,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) -> Result<MrLoadedData, GitlabError> {
    let acquire = || async {
        semaphore
            .acquire()
            .await
            .map_err(|_| GitlabError::Cancelled)
    };
    let mut _permit = acquire().await?;
    let client = http_client();

    // updated_at is always fetched fresh — never served from cache — so we always
    // know the real last-update timestamp regardless of the cache hit path.
    let mr_url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}?with_merge_status_recheck=true&include_diverged_commits_count=true",
        ctx.base_url, ctx.project_id, mr_id
    );

    // The list entry stands in for the MR's own page unless that page has more to
    // say (`needs_single_read`): one request saved per MR.
    let mut mr: GitLabMr = match prefetched.mr.filter(|mr| !needs_single_read(mr)) {
        Some(mr) => mr,
        None => fetch_single_mr(client, ctx, &mr_url).await?,
    };

    for attempt in 0..MERGEABILITY_RETRY_ATTEMPTS {
        if !is_transient_mergeability_status(mr.detailed_merge_status.as_deref()) {
            break;
        }

        crate::events::emit(
            &tx,
            GitlabEvent::MrMergeabilityRetrying {
                id: mr_id.to_string(),
            },
        );

        tracing::debug!(
            mr_id = %mr_id,
            attempt = attempt + 1,
            max_attempts = MERGEABILITY_RETRY_ATTEMPTS,
            detailed_merge_status = ?mr.detailed_merge_status,
            "GitLab mergeability is still transient — retrying after delay",
        );

        drop(_permit);
        sleep(Duration::from_secs(MERGEABILITY_RETRY_DELAY_SECS)).await;
        _permit = acquire().await?;
        mr = fetch_single_mr(client, ctx, &mr_url).await?;
    }

    let updated_at = mr.updated_at.clone();
    let created_at = mr.created_at.clone();
    // Always read the state fresh from the API response — never served from cache.
    let state = mr.state.unwrap_or_default();
    // Resolve mergeability with the following priority:
    //   1. `has_conflicts: true` always wins — manual intervention is required regardless
    //      of what `detailed_merge_status` says. GitLab can return "need_rebase" AND
    //      has_conflicts: true simultaneously when the rebase would produce conflicts.
    //   2. `detailed_merge_status` (GitLab ≥ 15.6) — exhaustive mapping of all known values.
    //   3. `merge_status` (legacy, GitLab < 15.6) — coarse-grained last resort.
    let mergeability = if mr.has_conflicts == Some(true) {
        MergeabilityStatus::Conflict
    } else {
        match mr.detailed_merge_status.as_deref() {
            // ── Mergeable ────────────────────────────────────────────────────────────────
            // The MR can be merged cleanly with no further action required.
            Some("mergeable") => MergeabilityStatus::Mergeable,

            // ── NeedsRebase ──────────────────────────────────────────────────────────────
            // The source branch is behind the target branch but there are no conflicts;
            // a simple rebase (or merge commit) is sufficient.
            // "need_rebase" is the canonical value; "behind_target_branch" is its alias
            // returned by some GitLab versions.
            Some("need_rebase") | Some("behind_target_branch") => MergeabilityStatus::NeedsRebase,

            // ── Conflict ─────────────────────────────────────────────────────────────────
            // Merge conflicts that require manual resolution before the MR can progress.
            Some("merge_conflict") | Some("conflict") => MergeabilityStatus::Conflict,
            // GitLab was unable to compute the merge status — treat as a blocking conflict
            // to avoid falsely showing the MR as ready.
            Some("broken_status") => MergeabilityStatus::Conflict,
            // A security policy is violated — blocks merge, treat as conflict-level blocker.
            Some("security_policy_violations") => MergeabilityStatus::Conflict,

            // ── NotOpen ──────────────────────────────────────────────────────────────────
            // The MR is not open (already merged or closed in GitLab).
            Some("not_open") => MergeabilityStatus::NotOpen,

            // ── Draft ────────────────────────────────────────────────────────────────────
            // The MR is a draft — intentionally not ready to merge.
            Some("draft_status") => MergeabilityStatus::Draft,

            // ── DiscussionsNotResolved ────────────────────────────────────────────────────
            // There are unresolved discussion threads that must be resolved before merging.
            Some("discussions_not_resolved") => MergeabilityStatus::DiscussionsNotResolved,

            // ── CiMustPass ───────────────────────────────────────────────────────────────
            // A required CI pipeline must pass before this MR can be merged.
            Some("ci_must_pass") => MergeabilityStatus::CiMustPass,

            // ── CiStillRunning ───────────────────────────────────────────────────────────
            // A CI pipeline is currently running — outcome not yet known.
            Some("ci_still_running") => MergeabilityStatus::CiStillRunning,

            // ── NotApproved ──────────────────────────────────────────────────────────────
            // Required approval rules are not yet satisfied.
            // "approvals_syncing" is a transient state while GitLab recomputes approvals.
            Some("not_approved") | Some("approvals_syncing") => MergeabilityStatus::NotApproved,

            // ── RequestedChanges ─────────────────────────────────────────────────────────
            // A reviewer has explicitly requested changes on the MR.
            Some("requested_changes") => MergeabilityStatus::RequestedChanges,

            // ── SyncFailed (unresolved transient GitLab states) ──────────────────────────
            // GitLab was still computing the merge status after the bounded retry window.
            // Surface this as a failed sync for this run; the next manual/auto refresh can retry.
            Some("checking") | Some("unchecked") | Some("preparing") => {
                MergeabilityStatus::SyncFailed
            }
            // External status checks (e.g. deployment gates) have not yet passed.
            Some("external_status_checks") => MergeabilityStatus::Unknown,
            // A required Jira issue association is missing.
            Some("jira_association_missing") => MergeabilityStatus::Unknown,
            // Commit message format or signature requirements are not met.
            Some("commits_status") => MergeabilityStatus::Unknown,

            // ── Fallback ─────────────────────────────────────────────────────────────────
            // Unknown or future detailed_merge_status values: fall back to the legacy field.
            _ => match mr.merge_status.as_deref() {
                Some("can_be_merged") => MergeabilityStatus::Mergeable,
                Some("cannot_be_merged") => MergeabilityStatus::Conflict,
                _ => MergeabilityStatus::Unknown,
            },
        }
    };

    let state_unchanged = cached.cached_state.as_ref().is_some_and(|s| s == &state);
    let updated_at_unchanged = updated_at.is_some() && updated_at == cached.updated_at;
    // Discussions (unresolved threads, review timeline) and approvers. Once the MR
    // is merged or closed they are frozen (read once more on the transition). While
    // it is open they are read again only when something says they moved: see
    // `review_stale` — `updated_at` alone is not enough (resolving a thread does not
    // touch it), hence GitLab's note count, the blocking-threads flag, the
    // mergeability and, as a safety net, their age.
    let read_age = cached
        .review_read_at
        .as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map(|at| chrono::Utc::now().signed_duration_since(at));
    let review_fresh = if state == GitlabMrState::Opened {
        !review_stale(
            &cached,
            updated_at_unchanged && mr.user_notes_count == cached.notes_seen,
            mr.blocking_discussions_resolved,
            mergeability,
            read_age,
        )
    } else {
        !cached.cache_policy.notes_stale() && review_current(&cached) && state_unchanged
    };
    let (user_notes_count, threads, review, review_read_at) = if review_fresh {
        (
            cached.user_notes_count,
            cached.threads.clone(),
            cached.review.clone(),
            cached.review_read_at.clone(),
        )
    } else {
        match fetch_discussions(ctx, mr_id).await {
            Some(discussions) => {
                let threads = review_threads(&discussions);
                let unresolved = threads.iter().filter(|t| !t.resolved).count();
                let author = mr.author.as_ref().map_or("", |a| a.username.as_str());
                (
                    u32::try_from(unresolved).unwrap_or(u32::MAX),
                    threads,
                    Some(review_activity(
                        &discussions,
                        author,
                        mr.draft
                            .unwrap_or(mergeability == MergeabilityStatus::Draft),
                    )),
                    Some(chrono::Utc::now().to_rfc3339()),
                )
            }
            // A failed page says nothing about the threads: keep what was known.
            None => (
                cached.user_notes_count,
                cached.threads.clone(),
                cached.review.clone(),
                cached.review_read_at.clone(),
            ),
        }
    };
    // Approvers change while the MR is open (approvals, resets on push): read with
    // the discussions, once more when it leaves open (approved and merged between
    // two refreshes), frozen afterwards.
    let fetched = if !review_fresh {
        fetch_approved_by(ctx, mr_id).await
    } else {
        None
    };
    let (approved_by, approvers): (Vec<String>, Vec<String>) = match fetched {
        Some(users) => users
            .into_iter()
            .map(|u| {
                let display = if u.name.trim().is_empty() {
                    u.username.clone()
                } else {
                    format!("{} (@{})", u.name, u.username)
                };
                (u.username, display)
            })
            .unzip(),
        None => (cached.approved_by.clone(), cached.approvers.clone()),
    };
    // A closed MR whose approvals were never read (merged before a refresh saw it
    // approved) still has them in its system notes: frozen there, they no longer
    // move with a push.
    let approved_by = match review.as_ref() {
        Some(r) if approved_by.is_empty() && state != GitlabMrState::Opened => {
            r.approvals.keys().cloned().collect()
        }
        _ => approved_by,
    };
    // Recorded only with a successful read, so a failed one is retried next cycle.
    let notes_seen = if review_read_at != cached.review_read_at {
        mr.user_notes_count
    } else {
        cached.notes_seen
    };

    // Milestone is always read fresh from the GitLab API response — never served from cache.
    // This is the authoritative source: a MR may be attached or detached from a milestone
    // at any time, and the cache would silently hold a stale value.
    let milestone_due_date = mr.milestone.as_ref().and_then(|m| m.due_date.clone());
    let milestone_description = mr.milestone.as_ref().and_then(|m| m.description.clone());
    let milestone = mr.milestone.map(|m| m.title);
    let milestone_events = if milestone_events_due(&cached, milestone.as_deref()) {
        // `None` on a failed read: read again at the next refresh.
        fetch_milestone_events(ctx, mr_id).await
    } else {
        cached.milestone_events.clone()
    };

    // Reviewers are always read fresh — they can be added or removed at any time.
    // Format: "Full Name (username)" — mirrors the author/assignee display convention.
    let reviewers = mr
        .reviewers
        .unwrap_or_default()
        .into_iter()
        .map(|u| format!("{} (@{})", u.name, u.username))
        .collect::<Vec<_>>();

    // merged_by and merged_at are only populated for merged MRs.
    let merged_by = mr
        .merged_by
        .map(|u| format!("{} (@{})", u.name, u.username));
    let merged_at = mr.merged_at.clone();

    let source_branch = mr
        .source_branch
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Always read the merge SHA from the fresh API response — never from cache.
    //
    // `merge_sha`: the commit used to detect branch presence via /refs?type=branch.
    //   Priority: merge_commit_sha → squash_commit_sha → sha (HEAD, fast-forward fallback).
    //   The HEAD SHA (mr.sha) is only a valid branch-detection signal when the MR has been
    //   merged via fast-forward: in that case GitLab sets no merge_commit_sha/squash_commit_sha
    //   but the HEAD commit lands directly on the target branch.
    //   For still-open MRs the HEAD lives on the source branch only — using it would produce
    //   false positives (the commit is NOT on develop, it is on the feature branch).
    //
    // `stored_sha`: the value persisted into TrackedMr.sha and used by the auto-refresh
    //   skip guard (`mr.sha.is_some()` in app.rs). We only store a SHA once the MR is
    //   truly merged (merge_commit_sha or squash_commit_sha present) so open MRs keep
    //   sha = None and are never incorrectly frozen by the skip guard.
    let merge_sha = mr
        .merge_commit_sha
        .clone()
        .or_else(|| mr.squash_commit_sha.clone())
        .or_else(|| {
            // Use the HEAD SHA only when the MR is confirmed merged by GitLab — this
            // covers fast-forward merges where no dedicated merge commit is created.
            if state == GitlabMrState::Merged {
                mr.sha.clone()
            } else {
                None
            }
        });
    // stored_sha: only the "real" merge commit; keeps open-MR sha = None.
    let sha = mr.merge_commit_sha.or(mr.squash_commit_sha);

    // Title, description and labels: serve from cache when `updated_at` is unchanged
    // (GitLab bumps `updated_at` on any edit to these fields), refresh otherwise.
    // This avoids holding a stale "Draft: TITLE" after the draft prefix is removed,
    // while still saving the extra JSON parsing work on unchanged MRs.
    let title = if updated_at_unchanged {
        cached
            .title
            // State files written before `MrStatus::Error(String)` stored the fetch
            // error in the title: never reuse such a cached title.
            .filter(|t| !t.contains("⚠️ ERROR"))
            .unwrap_or(mr.title)
    } else {
        mr.title
    };

    let description = if updated_at_unchanged {
        cached
            .description
            .unwrap_or_else(|| mr.description.unwrap_or_default())
    } else {
        mr.description.unwrap_or_default()
    };

    let labels = if updated_at_unchanged {
        cached
            .labels
            .unwrap_or_else(|| mr.labels.unwrap_or_default())
    } else {
        mr.labels.unwrap_or_default()
    };

    // Author and web_url are immutable after MR creation — always served from cache
    // when available to avoid redundant formatting work.
    let (author, web_url) = match (cached.author, cached.web_url) {
        (Some(a), Some(w)) if !w.is_empty() => (a, w),
        _ => {
            let auth = mr
                .author
                .map(|u| format!("{} (@{})", u.name, u.username))
                .unwrap_or_else(|| "unknown".to_string());
            (auth, mr.web_url.unwrap_or_default())
        }
    };
    // The assignee can change at any time: always take it from the fresh payload.
    let assignee = mr.assignee.map(|u| format!("{} (@{})", u.name, u.username));

    let found_branches = match &merge_sha {
        Some(commit_sha) => match branches_containing(client, ctx, commit_sha).await {
            RefsLookup::Found(found) => found,
            RefsLookup::CommitGone => {
                // GitLab answers 404 when the commit was garbage-collected (force-push
                // after merge…). A merged MR was at least on its target branch.
                tracing::warn!(
                    mr_id = %mr_id,
                    commit_sha = %commit_sha,
                    "Commit not found on /refs — falling back to the target branch",
                );
                mr.target_branch
                    .iter()
                    .filter(|tb| ctx.branches.contains(tb))
                    .cloned()
                    .collect()
            }
            RefsLookup::Failed(partial) => {
                // A transient failure (rate limit, 5xx, network) says nothing about
                // the branches: keep what was known rather than showing "absent".
                tracing::warn!(
                    mr_id = %mr_id,
                    commit_sha = %commit_sha,
                    "Branch detection failed — keeping the previously found branches",
                );
                partial.union(&cached.branches).cloned().collect()
            }
        },
        None => {
            tracing::debug!(
                mr_id = %mr_id,
                state = ?state,
                "No merge SHA available — skipping branch detection (MR not yet merged)",
            );
            HashSet::new()
        }
    };

    // Fetch diff statistics (files changed, additions, deletions) from the Changes API.
    // We cache this behind the same `updated_at` guard as pipelines to avoid hammering
    // the API: if the MR hasn't changed, the diff hasn't changed either.
    // The MR commits are cached with them (same guard): one list call gives both
    // the commit count and the fingerprints used to recognise copies.
    let cached_diff_stats_valid =
        diff_stats_cache_valid(cached.diff_stats.as_ref(), &cached.commits);
    let (diff_stats, commits) =
        if updated_at.is_some() && updated_at == cached.updated_at && cached_diff_stats_valid {
            (cached.diff_stats.clone(), cached.commits.clone())
        } else {
            let commits = fetch_mr_commits(ctx, mr_id).await.unwrap_or_default();
            let count = u32::try_from(commits.len()).unwrap_or(u32::MAX);
            (fetch_diff_stats(ctx, mr_id, count).await, commits)
        };

    // The merge / squash commit never changes for a given SHA: fetched once.
    let landed_fingerprint = match &sha {
        Some(s) if cached.sha.as_ref() == Some(s) && cached.landed_fingerprint.is_some() => {
            cached.landed_fingerprint.clone()
        }
        Some(s) => fetch_commit_fingerprint(ctx, s).await,
        None => None,
    };

    // Resolve the number of commits the source branch is behind the target branch.
    //
    // Strategy (in priority order):
    //   1. `diverged_commits_count` from the MR response — free, no extra API call.
    //      Populated when the request includes `include_diverged_commits_count=true`.
    //      GitLab may return `null` for this field even with the param when the MR is
    //      Mergeable or when the computation has not yet been triggered server-side.
    //   2. Implicit `Some(0)` when mergeability is `Mergeable` — up to date by definition.
    //   3. `/repository/compare` fallback — one extra call, used only when GitLab did
    //      not provide `diverged_commits_count` for a non-Mergeable open MR.
    //   4. `None` for merged/closed MRs — not applicable.
    let commits_behind: Option<u32> = if state == GitlabMrState::Opened {
        match mergeability {
            MergeabilityStatus::Mergeable => Some(0),
            MergeabilityStatus::Retrying
            | MergeabilityStatus::SyncFailed
            | MergeabilityStatus::Unknown => {
                // GitLab has not computed the status yet — also skip the compare call
                // to avoid a spurious request while the MR is still being checked.
                None
            }
            _ => {
                // Non-Mergeable open MR: prefer the free field from the MR response.
                // Fall back to /compare only when GitLab did not provide the count.
                match mr.diverged_commits_count {
                    Some(n) => {
                        tracing::debug!(
                            mr_id = %mr_id,
                            commits_behind = n,
                            "Using diverged_commits_count from MR response",
                        );
                        Some(n)
                    }
                    None => {
                        tracing::debug!(
                            mr_id = %mr_id,
                            "diverged_commits_count absent — falling back to /compare",
                        );
                        let tb = mr.target_branch.as_deref().unwrap_or("main");
                        fetch_commits_behind(ctx, &source_branch, tb).await
                    }
                }
            }
        }
    } else {
        // Merged / Closed — not applicable.
        None
    };

    // Inject commits_behind into the diff_stats so it is persisted alongside the rest.
    let diff_stats = diff_stats.map(|mut s| {
        s.commits_behind = commits_behind;
        s
    });

    // Only re-fetch pipelines if the MR has been updated since the last cycle.
    // If `updated_at` is unchanged, reuse the cached pipeline data to avoid
    // hammering the GitLab API with redundant requests (rate-limit friendly).
    // Also re-fetch if cached pipelines exist but none have jobs — this handles
    // stale cache entries written before jobs were persisted.
    // Also re-fetch if any cached pipeline is missing `created_at` — this
    // transparently enriches state files written before that field was added.
    //
    // IMPORTANT: always re-fetch when any cached pipeline is in a transient state
    // (running, pending, created, waiting). A pipeline can transition to success/failed
    // without the MR's `updated_at` changing, so the equality check alone is not
    // sufficient to detect stale pipeline data.
    let pipelines = if !cached.cache_policy.pipelines_stale()
        && updated_at.is_some()
        && updated_at == cached.updated_at
        && pipelines_cache_reusable(&cached.pipelines)
    {
        cached.pipelines
    } else if let Some(pipelines) = prefetched.pipelines {
        pipelines // read with other MRs' over GraphQL
    } else {
        fetch_pipelines(ctx, mr_id).await
    };

    let target_branch = mr.target_branch.unwrap_or_else(|| "unknown".to_string());
    let auto_merge = mr.auto_merge_enabled.or(mr.merge_when_pipeline_succeeds) == Some(true);

    Ok(MrLoadedData {
        id: mr_id.to_string(),
        branches: found_branches,
        mergeability,
        data: crate::models::MrData {
            title,
            sha,
            description,
            author,
            assignee,
            reviewers,
            milestone,
            milestone_due_date,
            milestone_description,
            web_url,
            labels,
            updated_at,
            created_at,
            source_branch,
            target_branch,
            state,
            merged_by,
            merged_at,
            pipelines,
            user_notes_count,
            diff_stats,
            commits,
            landed_fingerprint,
            threads,
            review,
            approved_by,
            approvers,
            review_read_at,
            notes_seen,
            milestone_events,
            signatures: None,
            auto_merge,
        },
    })
}
