//! The events of the open tabs' MRs since a date, newest first.

use super::*;

/// The events of one tab's MRs since `since`.
pub fn events_of(app: &App, tab: usize, since: DateTime<Utc>, theme: &Theme) -> Vec<Event> {
    let t = &theme.tones;
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let mut out = Vec::new();
    let names: HashMap<&str, String> = app
        .mrs
        .iter()
        .flat_map(|mr| {
            std::iter::once(&mr.author)
                .chain(&mr.reviewers)
                .chain(&mr.approvers)
                .chain(mr.merged_by.iter())
        })
        .map(|d| (login(d), name(d)))
        .collect();
    let person = |login: &str| -> String {
        names
            .get(login)
            .cloned()
            .unwrap_or_else(|| login.to_string())
    };
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let author = login(&mr.author).to_string();
        let mut push = |kind: Kind,
                        at: Option<&str>,
                        what: String,
                        chip: Option<(String, Tone)>,
                        detail: String,
                        glyph: &'static str,
                        tone: Option<Tone>,
                        people: Vec<String>| {
            let Some(when) = board::time_of(at).filter(|w| *w >= since) else {
                return;
            };
            out.push(Event {
                tab,
                project: project.clone(),
                id: mr.id.clone(),
                title: mr.title.clone(),
                url: mr.web_url.clone(),
                kind,
                when,
                what,
                chip,
                detail,
                glyph,
                tone,
                people,
            });
        };
        push(
            Kind::Activity,
            mr.created_at.as_deref(),
            tf("opened by {name}", &[("name", &name(&mr.author))]),
            None,
            tf(
                "{source} → {target}",
                &[("source", &mr.source_branch), ("target", &mr.target_branch)],
            ),
            "+",
            None,
            vec![author.clone()],
        );
        if let Some(review) = &mr.review {
            // Out of draft: the work is done, the review can start.
            if let Some(at) = &review.ready_at {
                push(
                    Kind::Review,
                    Some(at),
                    tl("marked ready for review").to_string(),
                    Some((tl("Ready").to_string(), t.teal)),
                    tf("by {name}", &[("name", &name(&mr.author))]),
                    "▸",
                    Some(t.teal),
                    vec![author.clone()],
                );
            }
            for (reviewer, at) in &review.review_requested_at {
                push(
                    Kind::Review,
                    Some(at),
                    tf("review asked of {name}", &[("name", &person(reviewer))]),
                    None,
                    tf("by {name}", &[("name", &name(&mr.author))]),
                    "?",
                    None,
                    vec![reviewer.clone(), author.clone()],
                );
            }
            if let Some(at) = &review.approved_at {
                let who = if mr.approvers.is_empty() {
                    String::new()
                } else {
                    mr.approvers
                        .iter()
                        .map(|a| name(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                push(
                    Kind::Review,
                    Some(at),
                    tl("approved").to_string(),
                    Some((tl("Approved").to_string(), t.green)),
                    if who.is_empty() {
                        String::new()
                    } else {
                        tf("by {names}", &[("names", &who)])
                    },
                    "✓",
                    Some(t.green),
                    mr.approved_by.clone(),
                );
            }
            if let Some(at) = &review.last_push_at {
                push(
                    Kind::Activity,
                    Some(at),
                    tl("new commits pushed").to_string(),
                    None,
                    tf("by {name}", &[("name", &name(&mr.author))]),
                    "↑",
                    None,
                    vec![author.clone()],
                );
            }
        }
        for thread in &mr.threads {
            let state = if thread.resolved {
                Some((tl("Resolved").to_string(), t.green))
            } else {
                Some((tl("Open").to_string(), t.amber))
            };
            push(
                Kind::Review,
                Some(&thread.created_at),
                tf(
                    "thread opened by {name}",
                    &[("name", &person(&thread.author))],
                ),
                state,
                thread.excerpt.clone(),
                "“",
                None,
                vec![thread.author.clone(), author.clone()],
            );
        }
        for pipeline in &mr.pipelines {
            let (label, tone) = super::super::tones::pipeline(pipeline.status, t);
            let failed: Vec<&str> = pipeline
                .jobs
                .iter()
                .filter(|j| j.status == "failed")
                .map(|j| j.name.as_str())
                .collect();
            let detail = if failed.is_empty() {
                tn(
                    pipeline.jobs.len() as u64,
                    "#{id} · {n} job",
                    "#{id} · {n} jobs",
                )
                .replace("{id}", &pipeline.id.to_string())
            } else {
                tf(
                    "#{id} · failed: {jobs}",
                    &[
                        ("id", &pipeline.id.to_string()),
                        ("jobs", &failed.join(", ")),
                    ],
                )
            };
            let (what, glyph) = match pipeline.status {
                PipelineState::Failed => (tl("pipeline failed").to_string(), "✕"),
                PipelineState::Success => (tl("pipeline passed").to_string(), "✓"),
                _ => (tl("pipeline started").to_string(), "▸"),
            };
            push(
                Kind::Pipeline,
                pipeline.created_at.as_deref(),
                what,
                Some((label.to_string(), tone)),
                detail,
                glyph,
                Some(tone),
                vec![author.clone()],
            );
        }
        match mr.state {
            GitlabMrState::Merged => {
                let by = mr.merged_by.as_deref().map(name);
                push(
                    Kind::Merge,
                    mr.merged_at.as_deref(),
                    match &by {
                        Some(by) => tf("merged by {name}", &[("name", by)]),
                        None => tl("merged").to_string(),
                    },
                    None,
                    tf("into {target}", &[("target", &mr.target_branch)]),
                    "⇥",
                    None,
                    mr.merged_by
                        .iter()
                        .map(|m| login(m).to_string())
                        .chain([author.clone()])
                        .collect(),
                );
            }
            GitlabMrState::Closed => push(
                Kind::Merge,
                mr.updated_at.as_deref(),
                tl("closed without merging").to_string(),
                None,
                String::new(),
                "×",
                Some(t.gray),
                vec![author.clone()],
            ),
            _ => {}
        }
        for branch in &app.branches {
            let Some(at) = app.branch_arrivals.get(&(mr.id.clone(), branch.clone())) else {
                continue;
            };
            if *at < since {
                continue;
            }
            let presence = app.branch_presence(mr, branch);
            let (glyph, label, tone) = super::super::tones::presence(presence, false, t);
            let how = match presence {
                Some(crate::models::BranchPresence::CherryPicked) => {
                    tl("cherry-picked with its trailer")
                }
                Some(crate::models::BranchPresence::Copied) => {
                    tl("copied without trailer: inferred from the author, date and title")
                }
                Some(crate::models::BranchPresence::Partial) => tl("only some of its commits"),
                _ => tl("its own commits"),
            };
            out.push(Event {
                tab,
                project: project.clone(),
                id: mr.id.clone(),
                title: mr.title.clone(),
                url: mr.web_url.clone(),
                kind: Kind::Branch,
                when: *at,
                what: tf("reached {branch}", &[("branch", branch)]),
                chip: Some((format!("{glyph} {label}"), tone)),
                detail: how.to_string(),
                glyph,
                tone: Some(tone),
                people: vec![author.clone()],
            });
        }
    }
    out
}

/// The events of every open tab since `since`, newest first.
pub(super) fn collect(g: &GuiApp, since: DateTime<Utc>) -> (Vec<Event>, Vec<(usize, String)>) {
    let mut events = Vec::new();
    let mut tabs = Vec::new();
    for (index, slot) in g.tabs.iter().enumerate() {
        let app = match &slot.state {
            SlotState::Active => &g.app,
            SlotState::Parked(tab) => &tab.app,
            _ => continue,
        };
        tabs.push((
            index,
            app.project_name
                .clone()
                .unwrap_or_else(|| app.project_id.clone()),
        ));
        events.extend(events_of(app, index, since, &g.theme));
    }
    events.sort_by(|a, b| b.when.cmp(&a.when).then(a.kind.cmp(&b.kind)));
    (events, tabs)
}
