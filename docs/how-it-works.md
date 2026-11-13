# How it works

What the dashboard reads from GitLab to tell where an MR is, which branches it
follows, and when it notifies you.

- [Branch presence](#branch-presence)
- [Tracked branches](#tracked-branches)
- [Desktop notifications](#desktop-notifications)

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
2. **`branches`** in `tracker_<hash>.json` — legacy field from older versions, migrated silently to `projects.toml` on first startup and never written again.
3. **`default_branches`** in `projects.toml` — used on the very first run before any branch has been tracked interactively.

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
* **Anti-spam:** change notifications are suppressed during the initial sync. An MR seen for the first time (no branch set persisted yet in `tracker_<hash>.json`) records its branches silently instead of sending one toast per branch; once known, every new branch notifies, including changes made while the app was closed.
* **Platforms:** click-to-open uses D-Bus actions on Linux (GNOME, KDE…); without a notification daemon, clicks are ignored. Build with `--no-default-features` to drop the feature and `notify-rust` (`libdbus` is still needed on Linux, by the OS keyring).
