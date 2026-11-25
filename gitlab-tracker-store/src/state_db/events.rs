//! The events of an MR's history: what changed between two reads, and the history its data tells on its own.

use super::*;

/// What changed between the stored MR and the new one.
pub(super) fn mr_events(old: Option<&Stored>, new: &Stored, mr: &MrRow) -> Vec<Event> {
    let mut list = Vec::new();
    if old.is_none_or(|o| o.position.is_none()) {
        list.push(Event::new("tracked"));
    }
    let new_data: MrData = serde_json::from_str(&new.data).unwrap_or_default();
    let old_data: Option<MrData> = old
        .filter(|o| o.fetched && o.data != new.data)
        .and_then(|o| serde_json::from_str(&o.data).ok());
    match (&old_data, old) {
        (Some(old_data), _) => list.extend(data_events(old_data, &new_data)),
        // First real read (new, or after its placeholder): the history it tells.
        (None, o) if new.fetched && o.is_none_or(|o| !o.fetched) => {
            list.extend(history_of(&new_data))
        }
        _ => {}
    }

    let branches =
        |json: &str| -> BTreeSet<String> { serde_json::from_str(json).unwrap_or_default() };
    let before = old.map(|o| branches(&o.found)).unwrap_or_default();
    let after = branches(&new.found);
    for b in after.difference(&before) {
        list.push(Event::new("branch_reached").subject(b));
    }
    for b in before.difference(&after) {
        list.push(Event::new("branch_left").subject(b));
    }

    let old_m = old.and_then(|o| o.mergeability.as_deref());
    if new.mergeability.as_deref() != old_m
        && new_data.state == GitlabMrState::Opened
        && new.mergeability.is_some()
    {
        list.push(Event::new("mergeability").change(old_m, new.mergeability.as_deref()));
    }
    if old.is_some_and(|o| o.flagged != new.flagged) || (old.is_none() && new.flagged) {
        list.push(Event::new(if new.flagged {
            "flagged"
        } else {
            "unflagged"
        }));
    }
    if old.map(|o| o.snooze.is_some()).unwrap_or(false) != new.snooze.is_some() {
        list.push(Event::new(if new.snooze.is_some() {
            "snoozed"
        } else {
            "unsnoozed"
        }));
    }
    let old_ticket: Option<gitlab_tracker_core::LinkedTicket> = old
        .and_then(|o| o.ticket.as_deref())
        .and_then(|j| serde_json::from_str(j).ok());
    match (&old_ticket, &mr.linked_ticket) {
        (Some(a), Some(b)) if a.id == b.id && a.status != b.status => list.push(
            Event::new("ticket_status")
                .subject(&b.id)
                .change(Some(&a.status), Some(&b.status)),
        ),
        (a, Some(b)) if a.as_ref().map(|a| &a.id) != Some(&b.id) => list.push(
            Event::new("ticket_linked")
                .subject(&b.id)
                .change(a.as_ref().map(|a| a.id.as_str()), Some(&b.id)),
        ),
        (Some(a), None) => list.push(Event::new("ticket_unlinked").subject(&a.id)),
        _ => {}
    }
    list
}

/// The history an MR's data tells on its own, dated by GitLab: for an MR seen
/// for the first time (or imported).
pub fn history_of(d: &MrData) -> Vec<Event> {
    let mut list = vec![Event::new("opened")
        .at(d.created_at.as_deref())
        .actor(Some(&d.author))];
    if let Some(r) = &d.review {
        if let Some(at) = &r.ready_at {
            list.push(Event::new("ready").at(Some(at)));
        }
        for (login, at) in &r.review_requested_at {
            list.push(Event::new("review_requested").at(Some(at)).subject(login));
        }
        if let Some(at) = &r.first_review_at {
            list.push(Event::new("first_review").at(Some(at)));
        }
        if let Some(at) = &r.last_push_at {
            list.push(Event::new("pushed").at(Some(at)));
        }
        for (login, at) in &r.approvals {
            list.push(Event::new("approved").at(Some(at)).actor(Some(login)));
        }
    }
    // Approvals read from GitLab with no date (older reads).
    for login in &d.approved_by {
        if !d
            .review
            .as_ref()
            .is_some_and(|r| r.approvals.contains_key(login))
        {
            list.push(Event::new("approved").actor(Some(login)));
        }
    }
    for e in d.milestone_events.iter().flatten() {
        list.push(
            Event::new(if e.added {
                "milestone_set"
            } else {
                "milestone_removed"
            })
            .at(Some(&e.at))
            .actor(e.by.as_deref())
            .subject(e.milestone.as_deref().unwrap_or_default()),
        );
    }
    for p in &d.pipelines {
        list.push(
            Event::new("pipeline")
                .at(p.created_at.as_deref())
                .subject(p.id)
                .change(None, Some(&name_of(&p.status))),
        );
    }
    for t in &d.threads {
        list.push(
            Event::new("thread_opened")
                .at(Some(&t.created_at))
                .actor(Some(&t.author))
                .subject(t.note_id)
                .change(None, Some(&t.excerpt)),
        );
        if t.resolved {
            list.push(Event::new("thread_resolved").subject(t.note_id));
        }
    }
    match d.state {
        GitlabMrState::Merged => list.push(
            Event::new("merged")
                .at(d.merged_at.as_deref())
                .actor(d.merged_by.as_deref()),
        ),
        GitlabMrState::Closed => list.push(Event::new("closed").at(d.updated_at.as_deref())),
        _ => {}
    }
    list
}

/// What changed between two reads of an MR.
pub fn data_events(old: &MrData, new: &MrData) -> Vec<Event> {
    let mut list = Vec::new();
    let mut field = |kind: &'static str, a: Option<&str>, b: Option<&str>| {
        if a != b {
            list.push(Event::new(kind).change(a, b));
        }
    };
    field("title", Some(&old.title), Some(&new.title));
    field(
        "description",
        Some(&old.description),
        Some(&new.description),
    );
    field("assignee", old.assignee.as_deref(), new.assignee.as_deref());
    field(
        "milestone",
        old.milestone.as_deref(),
        new.milestone.as_deref(),
    );
    field(
        "target_branch",
        Some(&old.target_branch),
        Some(&new.target_branch),
    );
    field("head", old.sha.as_deref(), new.sha.as_deref());
    if old.auto_merge != new.auto_merge {
        list.push(Event::new(if new.auto_merge {
            "auto_merge_set"
        } else {
            "auto_merge_cancelled"
        }));
    }
    if old.state != new.state {
        let (kind, at, actor) = match new.state {
            GitlabMrState::Merged => ("merged", new.merged_at.as_deref(), new.merged_by.as_deref()),
            GitlabMrState::Closed => ("closed", new.updated_at.as_deref(), None),
            _ => ("reopened", new.updated_at.as_deref(), None),
        };
        list.push(
            Event::new(kind)
                .at(at)
                .actor(actor)
                .change(Some(&name_of(&old.state)), Some(&name_of(&new.state))),
        );
    }

    let set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
    let (a, b) = (set(&old.labels), set(&new.labels));
    for l in b.difference(&a) {
        list.push(Event::new("label_added").subject(l));
    }
    for l in a.difference(&b) {
        list.push(Event::new("label_removed").subject(l));
    }
    let (a, b) = (set(&old.reviewers), set(&new.reviewers));
    for l in b.difference(&a) {
        list.push(Event::new("reviewer_added").subject(l));
    }
    for l in a.difference(&b) {
        list.push(Event::new("reviewer_removed").subject(l));
    }
    let approved_at = |login: &str| {
        new.review
            .as_ref()
            .and_then(|r| r.approvals.get(login))
            .map(String::as_str)
    };
    let (a, b) = (set(&old.approved_by), set(&new.approved_by));
    for l in b.difference(&a) {
        list.push(Event::new("approved").at(approved_at(l)).actor(Some(l)));
    }
    for l in a.difference(&b) {
        list.push(Event::new("unapproved").actor(Some(l)));
    }

    let empty = Default::default();
    let (ra, rb) = (
        old.review.as_ref().unwrap_or(&empty),
        new.review.as_ref().unwrap_or(&empty),
    );
    for (login, at) in &rb.review_requested_at {
        if ra.review_requested_at.get(login) != Some(at) {
            list.push(Event::new("review_requested").at(Some(at)).subject(login));
        }
    }
    if ra.first_review_at.is_none() && rb.first_review_at.is_some() {
        list.push(Event::new("first_review").at(rb.first_review_at.as_deref()));
    }
    if rb.ready_at.is_some() && ra.ready_at != rb.ready_at {
        list.push(Event::new("ready").at(rb.ready_at.as_deref()));
    }
    if rb.last_push_at.is_some() && ra.last_push_at != rb.last_push_at {
        list.push(Event::new("pushed").at(rb.last_push_at.as_deref()));
    }

    let pipelines: BTreeMap<u64, &gitlab_tracker_gitlab::models::Pipeline> =
        old.pipelines.iter().map(|p| (p.id, p)).collect();
    for p in &new.pipelines {
        let status = name_of(&p.status);
        match pipelines.get(&p.id) {
            None => list.push(
                Event::new("pipeline")
                    .at(p.created_at.as_deref())
                    .subject(p.id)
                    .change(None, Some(&status)),
            ),
            Some(before) if before.status != p.status => list.push(
                Event::new("pipeline")
                    .at(p.updated_at.as_deref())
                    .subject(p.id)
                    .change(Some(&name_of(&before.status)), Some(&status)),
            ),
            _ => {}
        }
    }

    let threads: BTreeMap<u64, &gitlab_tracker_gitlab::models::MrThread> =
        old.threads.iter().map(|t| (t.note_id, t)).collect();
    for t in &new.threads {
        match threads.get(&t.note_id) {
            None => list.push(
                Event::new("thread_opened")
                    .at(Some(&t.created_at))
                    .actor(Some(&t.author))
                    .subject(t.note_id)
                    .change(None, Some(&t.excerpt)),
            ),
            Some(before) => {
                if t.notes > before.notes {
                    list.push(
                        Event::new("thread_replied")
                            .at(Some(&t.last_note_at))
                            .subject(t.note_id)
                            .change(Some(&before.notes.to_string()), Some(&t.notes.to_string())),
                    );
                }
                if t.resolved != before.resolved {
                    list.push(
                        Event::new(if t.resolved {
                            "thread_resolved"
                        } else {
                            "thread_reopened"
                        })
                        .at(Some(&t.last_note_at))
                        .subject(t.note_id),
                    );
                }
            }
        }
    }

    let size = |d: &MrData| {
        d.diff_stats.as_ref().map(|s| {
            format!(
                "+{} -{} · {} files · {} commits",
                s.additions, s.deletions, s.files_changed, s.commits_count
            )
        })
    };
    if let (Some(a), Some(b)) = (size(old), size(new)) {
        if a != b {
            list.push(Event::new("size").change(Some(&a), Some(&b)));
        }
    }

    let known: BTreeSet<(&str, bool)> = old
        .milestone_events
        .iter()
        .flatten()
        .map(|e| (e.at.as_str(), e.added))
        .collect();
    for e in new.milestone_events.iter().flatten() {
        if !known.contains(&(e.at.as_str(), e.added)) {
            list.push(
                Event::new(if e.added {
                    "milestone_set"
                } else {
                    "milestone_removed"
                })
                .at(Some(&e.at))
                .actor(e.by.as_deref())
                .subject(e.milestone.as_deref().unwrap_or_default()),
            );
        }
    }
    list
}
