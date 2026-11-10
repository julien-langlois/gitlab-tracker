# GitLab requests: REST, GraphQL and the request budget

> For **contributors**. How the app keeps the number of requests it sends to
> GitLab (and Redmine) low, what already uses GraphQL, what else could, and what
> cannot. See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the rest of the internals.

---

## Where the requests go

A refresh cycle (every `refresh_interval_secs`, 15 min by default) reads:

| What | How | Requests |
| :--- | :--- | :--- |
| MR base data | `GET merge_requests?iids[]=…` (`gitlab::list_mrs`) | 1 per 100 MRs |
| Commits behind of an open MR that cannot merge | its own page (`diverged_commits_count` is not in the list) | 1 per such MR |
| Discussions + approvals of an open MR | only on a sign of change, at least hourly (`gitlab::review_stale`) | 2 per MR read |
| Commits + changed files (patches) | when `updated_at` moved | 2 per changed MR |
| **Pipelines + jobs** | **GraphQL** (`gitlab_graphql`), REST as a fallback | **1 query per 10 MRs** (REST: 1 + 1 per pipeline, per MR) |
| Branches containing a merged MR | `GET commits/:sha/refs` | 1+ per merged MR not yet on every branch |
| Branch scans (copies, cherry-picks) | `GET repository/commits?ref_name=<old head>..<branch>` | 1 per branch with a missing MR |
| New MRs (`discover_new_mrs`) | `GET merge_requests?created_after=…` | 1 |
| Redmine tickets | `GET issues.json?issue_id=…&status_id=*` | 1 per 100 tickets |

Every request goes through `gitlab_tracker_core::pacing`: projects on the same
instance share one concurrency limit (3) and one pause when the server answers
`429` / `503` or reports a spent budget (`Retry-After`, `RateLimit-Reset`).

---

## GraphQL today: pipelines and jobs (step 1)

REST needs `GET merge_requests/:iid/pipelines` and then `GET pipelines/:id/jobs`
for each of up to 5 pipelines: up to **6 requests per MR**, every cycle while a
pipeline runs, and every cycle for an MR without any pipeline (an empty cache is
never reusable). One GraphQL query now reads them for a batch of MRs:

```graphql
query($project: ID!, $iids: [String!], $first: Int) {
  project(fullPath: $project) {           # projects(ids: [gid]) for a numeric project id
    mergeRequests(iids: $iids, first: $first) { nodes { ...MrPipelines } }
  }
}
fragment MrPipelines on MergeRequest {
  iid
  pipelines(first: 5) { nodes { id status createdAt updatedAt sha
    jobs(first: 50, retried: false) { nodes {
      id name status duration queuedDuration stage { name } } } } }
}
```

* **Same data as REST**: same windows (5 pipelines, 50 jobs, retried jobs left
  out), statuses lowercased, global IDs (`gid://gitlab/Ci::Pipeline/123`) turned
  into numbers, newest first. The rest of the app does not know which API it came
  from.
* **Which MRs**: those listed this cycle whose pipeline cache is not reusable
  (`gitlab::pipelines_due`: same rule as before — MR changed, a pipeline still
  running, or no pipeline cached).
* **Too complex**: GitLab caps query complexity and depth. The batch (10 MRs to
  start with) is split in two and retried, and the smaller size is kept for that
  instance for the session.
* **Older GitLab**: an unknown field or argument (`queuedDuration`, `retried`…)
  fails the whole query. GraphQL pipelines are then turned off for that instance
  for the session, and REST is used.
* **Any other failure** (network, 5xx, partial data with errors): those MRs read
  over REST this cycle; nothing is learnt.
* **Off switch**: `GITLAB_TRACKER_GRAPHQL=off` keeps everything on REST.
* **Auth**: `Authorization: Bearer <token>`; the `read_api` scope is enough.
* **Counter**: the status bar counts one query per batch for the pipelines due
  (`gitlab_graphql::queries_for`), and the requests this saves in the `−X%`.

---

## Estimated gain on 50 tracked MRs

Assumptions, for one project:

* 20 open MRs (8 that cannot merge as is), 20 merged waiting to reach other
  branches, 10 closed; 3 tracked branches; `discover_new_mrs` on; 15-min refresh.
* Per cycle: 5 open MRs changed, 3 others with a running pipeline, 5 MRs without
  any pipeline. An MR has 3 pipelines on average in its 5-run window (so a REST
  pipelines read costs 1 + 3 requests).
* Open MRs that did not change re-read their review once an hour (¼ per cycle).

Requests to GitLab per refresh cycle:

| | Before batching | Batched REST | **Batched + GraphQL pipelines** |
| :--- | ---: | ---: | ---: |
| MR base data (pages / list) | 50 | 1 + 8 | 1 + 8 |
| Discussions + approvals | 40 | ≈ 18 | ≈ 18 |
| Commits + changes | 10 | 10 | 10 |
| **Pipelines + jobs** | **37** | **37** | **2** |
| `/refs` (merged MRs) | 20 | 20 | 20 |
| Branch scans | ≈ 15 | 3 | 3 |
| Discovery | 1 | 1 | 1 |
| **Total per cycle** | **≈ 173** | **≈ 98** | **≈ 62** |
| Per 8-hour day (32 cycles) | ≈ 5 500 | ≈ 3 100 | ≈ 2 000 |
| Redmine, 40 linked MRs (30 tickets) | 40 | 1 | 1 |

* Pipelines alone: **37 → 2 requests per cycle (−95 %)**, the whole cycle
  **−36 %** compared with batched REST.
* First launch, nothing cached: the pipelines of 50 MRs take **5 queries instead
  of ≈ 185 requests**.
* The busier the CI (several running pipelines, long-lived MRs with 5 runs), the
  bigger the share pipelines had, and the bigger the gain.
* Server side, the database work is about the same — GraphQL groups it — but rate
  limits count requests, and a cycle needs far fewer round trips (faster on a slow
  or distant instance).

The estimate follows the same rules as the status-bar counter. With
`RUST_LOG=info`, the "Refresh cycle complete" log line gives each cycle's
estimated GitLab requests and how long it took (the requests actually sent are not
counted).

---

## What else GraphQL could take over

Check field availability on your instance with its GraphiQL explorer
(`<gitlab>/-/graphql-explorer`): fields come and go with GitLab versions, and an
unknown one fails the whole query. Every step below keeps REST as a fallback, as
step 1 does.

### Step 2 — the rest of the MR refresh (biggest remaining gain)

One query per batch of MRs could also bring:

| REST today | GraphQL on `MergeRequest` | Notes |
| :--- | :--- | :--- |
| `merge_requests?iids[]=` list | `iid title state updatedAt detailedMergeStatus conflicts draft labels milestone author reviewers assignees mergeCommitSha squashCommitSha …` | Replaces the list (1 request per 100 MRs, small gain). |
| Single page for `diverged_commits_count` | `divergedFromTargetBranch` (boolean), `commitsBehind`-like counts depending on version | **Check the schema**: the exact count may not exist; without it the page read stays. Up to 8 requests per cycle here. |
| `/discussions` (paginated) | `discussions { nodes { resolvable resolved notes { nodes { system body author createdAt } } } }` | Nested pagination (notes per discussion) counts heavily in complexity: smaller batches, follow-up pages for busy MRs. |
| `/approvals` | `approvedBy { nodes { username name } }`, `approvalState { rules { … } }` | |
| `/commits` (paginated) | `commits(first: 100) { nodes { sha title message authorEmail authoredDate committedDate } }` | `message` is needed for cherry-pick trailers. |
| `/changes` (counts only) | `diffStatsSummary { additions deletions fileCount }`, `diffStats { path additions deletions }` | **No patch text**: the hunks used by Overlaps still need REST `/changes` (or `/diffs`). |

Gain on the 50-MR example: discussions + approvals (≈ 18) and commits (5) become
part of 1–3 queries: **≈ 62 → ≈ 40 requests per cycle**, more if the commits-behind
count is available (≈ 32). More valuable than the count: reading every open MR's
threads and approvals at each cycle becomes cheap, so the `review_stale`
heuristics and the hourly safety net can go — threads and approvals always fresh.

### Step 3 — on-demand and project-level reads (GUI)

These run when a page opens, not every cycle; the gain is latency more than load.

| REST today (`gitlab_detail.rs`) | GraphQL |
| :--- | :--- |
| MR page: overview + approval rules + threads + deployments (4–6 requests) | One query: `mergeRequest(iid:) { detailedMergeStatus mergeabilityChecks approvalState discussions … }`; deployments through `project.environments { lastDeployment … }`. |
| Project overview: deployments, one pipeline per tracked branch, merged MRs history | `project { environments … pipelines(ref:, first: 1) … mergeRequests(state: merged, mergedAfter:) … }` — one query instead of 2 + one per branch. |
| Labels, milestones, discovery | `project { labels … milestones … mergeRequests(createdAfter:) }` — 1 request each today, negligible. |

### What stays on REST

| Need | Why |
| :--- | :--- |
| Branches containing a commit (`/commits/:sha/refs`) | No GraphQL equivalent. The largest remaining item (≈ 20 per cycle in the example); see below. |
| Branch scans (`repository/commits?ref_name=a..b`) | No commit list by revision range in GraphQL. |
| Patch text (`/changes`, `/diffs`), file diffs | GraphQL gives per-file counts, not the patches. |
| Conflicts, `compare`, job logs | REST only. |

### Other leads, not GraphQL

* **`/refs` of merged MRs** (≈ 20 per cycle): the branch scans already index each
  branch's commits; an MR found there by its merge SHA could skip `/refs` and only
  fall back to it when absent. Biggest remaining REST item.
* **MRs without any pipeline**: an empty pipeline cache is never reusable, so they
  are asked again every cycle (now in the batched query, but still listed). It could
  be reused while `updated_at` is unchanged (a push moves it; a pipeline started by
  hand from the UI might not — check before changing).
* **`/changes` → `/diffs`**: `/changes` is deprecated upstream; `/diffs` is
  paginated and the same weight.
