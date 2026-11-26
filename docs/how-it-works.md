# How it works

What the dashboard reads from GitLab to tell where an MR is, which branches it
follows, and when it notifies you.

- [Branch presence](#branch-presence)
- [Tracked branches](#tracked-branches)
- [Desktop notifications](#desktop-notifications)
- [The review timeline and drafts](#the-review-timeline-and-drafts)

## Branch presence

For each MR and each tracked branch, the checks run in this order and the first match wins:

1. **Same SHA (`🟢 PRESENT`).** The MR's merge commit, else its squash commit, else — for a merged fast-forward MR — its HEAD, is looked up with the GitLab Refs API (`/commits/:sha/refs?type=branch`): the branches *containing* that commit. This covers merges, fast-forwards and `git reset --hard origin/develop`. If GitLab answers 404 (commit garbage-collected after a force-push), the MR's target branch is assumed. If the call fails transiently (rate limit, 5xx, network), the branches found earlier are kept — never shown as absent because of an outage. At most 5 pages (500 branches containing the commit) are read; past that, the branches found earlier are kept too, and the copy scan below still finds the merge SHA on the branch.
2. **Copies (`🍒` / `≈` / `◐`).** After every refresh cycle, each branch that some MR is missing from is scanned: its commits since the oldest such MR was created, at most 90 days back. A branch with nothing missing costs no API call. Each scanned commit gives three keys: its SHA, the SHAs named by `(cherry picked from commit …)` trailers, and its **fingerprint** — author e-mail, author date and title. `git cherry-pick` and `git rebase` keep the author and the author date, so the fingerprint survives a copy even when conflicts changed the patch. Surrounding quotes added to titles by some tools (`'#1410856: …'`) are ignored.
   * First the MR's merge / squash commit is matched (by SHA, trailer, then fingerprint): the whole MR was copied at once.
   * Otherwise its own commits are matched **one by one** (the MR's commit list is fetched with the diff stats, behind the same `updated_at` cache): all found → `PRESENT` / `PICKED` / `COPIED` (the weakest proof wins), some found → `◐ PARTIAL`.
   * Open and closed MRs are checked too: an urgent fix can be cherry-picked from an open MR, or land through another MR.

**Limits.** A copy is not recognised when its author date was reset (`git rebase --ignore-date`, `--reset-author`, `git commit --amend --reset-author`), when its title was reworded, or when it is older than the 90-day scan window. `≈ COPIED` is an inference: a rewritten commit that kept the author, the author date and the title would match too. Squash-merging a branch of several MRs into another (e.g. a release branch) creates a new commit with a new author date, which none of them can be matched to. For an exact `🍒`, cherry-pick with `-x` or with GitLab's *Cherry-pick* button.

A desktop notification fires when an MR newly reaches a branch through a copy. The first scan of each session only records, so restarting never re-notifies.

## Tracked branches

Tracked branches (the columns shown in the MR table) are resolved in this priority order at startup:

1. **`tracked_branches`** in `projects.toml` — the canonical source, written by the app whenever you add or remove a branch.
2. **`branches`** in `tracker_<hash>.json` — legacy field from older versions, migrated silently to `projects.toml` on first startup and never written again (see [Local state](#local-state)).
3. **`default_branches`** in `projects.toml` — used on the very first run before any branch has been tracked interactively.

## Local state

The tracked MRs live in a SQLite database, `tracker.db`, in the config directory (`~/.config/gitlab-tracker/` on Linux), one set of tables for every project. It holds what the app needs to start with your MRs already there (their GitLab data, flags, snoozes, tickets, the branches they reached), and their history:

* **`mrs`**: one row per MR ever tracked, its main fields in columns and its full data as JSON. An MR removed from the dashboard keeps its row, out of the list, and comes back with its history when added again.
* **`mr_events`**: every change the app sees, appended, never rewritten: `approved`, `unapproved`, `merged`, `pipeline`, `label_added`, `reviewer_added`, `thread_opened`, `thread_resolved`, `branch_reached`, `mergeability`, `ticket_status`, `flagged`… Each one has the date the app saw it (`seen_at`) and, when GitLab gives it, the date it happened (`at`) and by whom. An MR seen for the first time gets the history its data already tells: opened, review requests, approvals, pipelines, threads, milestone moves, merge.
* **`mr_pipelines`**, **`mr_threads`**: every pipeline and review thread seen, kept after GitLab's lists move on; **`mr_labels`**, **`mr_people`**: current labels, reviewers and approvers, to query.
* **`project_merges`**: when each MR of the project merged, tracked or not (Delivery's pace). **`merged_mrs`**, **`merged_mr_people`**, **`merged_mr_paths`**, **`people`**: the project's merged MRs, their reviewers, approvers and changed files (the reviewer suggestions). The window reads them over their period (8 weeks, 90 days); the rows stay after it.
* **`settings`**: the window's preferences (page, theme, open tabs…).

Only the files you edit by hand stay files: `projects.toml` and `config.json`.

Read it with any SQLite client, e.g. the approvals of the last week, or who changed a directory most:

```sh
sqlite3 ~/.config/gitlab-tracker/tracker.db \
  "SELECT mr_id, actor, at FROM mr_events WHERE kind = 'approved' AND at >= date('now', '-7 days')"
sqlite3 ~/.config/gitlab-tracker/tracker.db \
  "SELECT m.author, COUNT(DISTINCT m.mr_iid) FROM merged_mrs m
   JOIN merged_mr_paths p USING (project, mr_iid)
   WHERE p.path LIKE 'src/api/%' GROUP BY m.author ORDER BY 2 DESC"
```

**From the JSON files:** the versions before `tracker.db` kept all this in JSON files: `tracker_<hash>.json`, `merges_<hash>.json`, `owners_<hash>.json` and `gui.json`. The first launch imports each one on its own, the first time it is needed, then renames it `….imported` (kept as a backup, no longer read). `gitlab-tracker import-state` does the same by hand for every project of `projects.toml` (or `--project <name>`), and prints what it imported.

## Sentinel

A full refresh reads every tracked MR, every `refresh_interval_secs` (15 min by default). In between, the **sentinel** asks GitLab what changed since its last check, every `sentinel_interval_secs` (2 min by default, `0` turns it off), and reads again only those MRs, through the same path as a refresh:

| Request | What it catches |
| :--- | :--- |
| `merge_requests?updated_after=…` | New MRs, pushes on an MR's branch, title, description, labels, assignee, reviewers, milestone, draft, merged / closed / reopened |
| `pipelines?updated_after=…` | A pipeline that starts, ends or is retried (it does not touch its MR's `updated_at`) |
| `events` | Comments and approvals (their discussions and approvers are read again), pushes on a tracked branch (branch scan) or on the target of open MRs (mergeability, commits behind) |

* **Linked tickets** (Redmine…) are read again only for the MRs the sentinel reads again; a ticket that changes on its own waits for the next full refresh.
* **Not seen by the sentinel**, left to the full refresh and to the hourly review read: a thread resolved or reopened, a mergeability GitLab recomputes on its own, a milestone's dates, a deleted MR, freeze periods, releases.
* **Clock:** the sentinel follows the GitLab server's clock (its `Date` header), with a 30-second overlap; what it already saw is skipped.
* **Gaps:** after a sleep or a network outage longer than 15 minutes, or when more changed than a check reads (500 per list), a full refresh runs instead of catching up.
* **Restart:** the sentinel starts from the launch, its watermark is not saved: the startup load reads every tracked MR again, and the review of every open MR last read over an hour ago.
* It never runs on top of a refresh, nor during the startup load. Settings: [configuration](configuration.md).

## Desktop notifications

Notifications fire for GitLab MR events (new branch, MR updated, mergeability changed, milestone changed, complexity changed, review reminders) and tracker-ticket events when a provider such as Redmine is active. Tracker notifications include ticket field changes detected during refreshes and successful automatic status transitions triggered by configured workflow mappings.

MR notifications include a clickable **"Open MR"** button; tracker notifications include **"Open ticket"** and open the linked ticket URL instead. Change notifications are suppressed during the initial sync to avoid spurious alerts on restart.

| Event | Trigger |
| :--- | :--- |
| 🌿 New branch | The MR's merge/squash SHA is found on a tracked branch it was not known on |
| 🕐 MR updated | `updated_at` changed since the previous refresh |
| 🔀 Mergeability changed | e.g. `MERGEABLE → CONFLICT` |
| 🏁 Milestone changed | e.g. `v2.4.0 → v2.5.0` |
| ⚠️ Complexity changed | The effort band changed (e.g. `🟢 EASY → 🔴 COMPLEX`) |
| 👁 Review waiting | An MR awaiting your review (same rule as *To review: me*) was requested `review_reminder_hours` ago or more; repeated every `review_reminder_hours`, and once again after a restart |
| 👁 New commits since your review | The author pushed after your last comment, reply or approval on an MR still awaiting your review; once per push |
| 🎫 Ticket field changed *(tracker)* | Priority, status, assignee, version or progress of the linked ticket |
| ✅ Status transitioned *(tracker)* | An automatic tracker status transition succeeded |

* **Never blocks the UI:** notifications are queued to one background worker thread that performs the (synchronous) D-Bus calls, so a slow or hung notification daemon cannot freeze the dashboard. At most 8 notifications wait for a click at once; beyond that they still show, but clicking does nothing.
* **Anti-spam:** change notifications are suppressed during the initial sync. An MR seen for the first time (no branch set persisted yet in `tracker.db`) records its branches silently instead of sending one toast per branch; once known, every new branch notifies, including changes made while the app was closed.
* **Platforms:** click-to-open uses D-Bus actions on Linux (GNOME, KDE…); without a notification daemon, clicks are ignored. Build with `--no-default-features` to drop the feature and `notify-rust` (`libdbus` is still needed on Linux, by the OS keyring).

## The review timeline and drafts

The review measures (where the time goes, time to first review, review rounds,
how long a reviewer takes to answer, how long a review has waited) are read from
each MR's discussions, system notes included: "requested review from @alice",
"approved this merge request", "added 2 commits", "marked this merge request as
**ready**"… GitLab writes them in English whatever your language. The older
wordings ("marked as a **Work In Progress**", "unmarked as a **Work In
Progress**") are read too.

Many teams open the MR **when the work starts**, as a draft, so everyone sees it
move day by day and can review it along the way instead of all at the end. The
dashboard follows that:

| Moment | What marks it |
| :--- | :--- |
| **Opened** | The MR's creation. |
| **Up for review** (the end of the development) | Its move out of draft (**Mark as ready**) or its first review request, whichever comes first: a review asked on a draft puts it up for review. An MR never in draft: its first review request. |
| **First review** | The first comment or approval by someone other than the author **once the MR is up for review**. An approval always counts, even on a draft. Bots (Renovate, Dependabot, project and group tokens, `*-bot`) never count. |
| **Approved**, **Merged**, **On production** | The approval, the merge, the arrival on the last tracked branch. |

- **Comments on a draft nobody was asked to review yet** are the review along
  the way: they are counted apart ("3 of 18 opened as drafts, 2 reviewed along the way" under *Where the
  time goes*) and are neither the first review nor rounds.
- **Review rounds** are the pushes after the first review: the pushes of the
  development do not count. An MR sent back to draft after its review and marked
  ready again keeps its first review; its rework pushes are rounds.
- **A reviewer asked on a draft** is asked for the review: many teams ask for
  it, and get the approval, before marking the MR ready. The development ends
  at that request, and the reviewer's wait (*Team*'s answer times, the next
  step) starts there. A reviewer named when the draft is opened, before the
  work is done, therefore makes the development look shorter: nothing tells
  the two apart.

Where each measure starts:

| Measure | From | To |
| :--- | :--- | :--- |
| *Delivery* · In development | opened | up for review |
| *Delivery* · Waiting for a review | up for review | first review |
| *Delivery* · In review | first review | approved |
| *Delivery* · Waiting for the merge / for production | approved / merged | merged / on production |
| *Cockpit*, *Flow* trend, *MR page*, a person's page · time to first review | up for review | first review |
| *Stats* · Median time to merge (the whole cycle) | opened | merged |
| *Stats* · Ready → merge | up for review | merged |
| *Delivery* · DORA lead time for changes | first commit | on production |

**What cannot be known.** An MR opened ready (never a draft) with its reviewers
asked at once gives no sign of when its work ended: its development counts as
*Waiting for a review*. *Where the time goes* counts these MRs in amber ("15
never drafts: development counted up to the review request"). To split the two,
open the MRs as drafts and mark them ready when the work is done.

**After an update.** When the way the timeline is read changes (drafts
followed, then a review asked on a draft taken as the review), each tracked
MR, merged ones included, has its discussions read once more at the next
refresh (one request per MR, then cached again). *Stats* keeps
its database: the MRs recorded before have their *Ready → merge* filled at the
next start, from what the refresh read.

