//! What changed while the app was closed: the "since last session" summary.

use super::*;

/// What a restored MR looked like when the app was last closed.
#[derive(Debug, Clone)]
pub(super) struct MrBaseline {
    pub(super) state: GitlabMrState,
    pub(super) branches: HashSet<String>,
    pub(super) pipeline: Option<PipelineState>,
}

/// What happened to an MR while the app was closed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub enum ChangeKind {
    Merged,
    Closed,
    /// Reached this branch.
    Arrived(String),
    CiFailing,
}

/// One change of [`session_changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub struct SessionChange {
    pub id: String,
    pub title: String,
    pub kind: ChangeKind,
}

/// What changed between the saved `baseline` and the freshly fetched `mrs`, one
/// change per line: what [`session_digest`] counts.
pub(super) fn session_changes(
    baseline: &HashMap<String, MrBaseline>,
    mrs: &[TrackedMr],
) -> Vec<SessionChange> {
    let mut changes = Vec::new();
    let failed = Some(PipelineState::Failed);
    for mr in mrs {
        let Some(before) = baseline.get(&mr.id) else {
            continue;
        };
        let mut push = |kind| {
            changes.push(SessionChange {
                id: mr.id.clone(),
                title: mr.title.clone(),
                kind,
            })
        };
        if mr.state != before.state {
            match mr.state {
                GitlabMrState::Merged => push(ChangeKind::Merged),
                GitlabMrState::Closed => push(ChangeKind::Closed),
                GitlabMrState::Opened => {}
            }
        }
        if let MrStatus::MergedIn(found) = &mr.status {
            let mut arrived: Vec<&String> = found.difference(&before.branches).collect();
            arrived.sort();
            for branch in arrived {
                push(ChangeKind::Arrived(branch.clone()));
            }
        }
        if mr.pipelines.first().map(|p| p.status) == failed && before.pipeline != failed {
            push(ChangeKind::CiFailing);
        }
    }
    changes
}

/// One-line summary of what changed between the saved `baseline` and the freshly
/// fetched `mrs`, or `None` when nothing worth reporting did.
pub(super) fn session_digest(
    baseline: &HashMap<String, MrBaseline>,
    mrs: &[TrackedMr],
) -> Option<String> {
    let (mut merged, mut closed, mut arrivals, mut ci_failing) = (0, 0, 0, 0);
    let failed = Some(PipelineState::Failed);
    for mr in mrs {
        let Some(before) = baseline.get(&mr.id) else {
            continue;
        };
        if mr.state != before.state {
            match mr.state {
                GitlabMrState::Merged => merged += 1,
                GitlabMrState::Closed => closed += 1,
                GitlabMrState::Opened => {}
            }
        }
        if let MrStatus::MergedIn(found) = &mr.status {
            arrivals += found.difference(&before.branches).count();
        }
        if mr.pipelines.first().map(|p| p.status) == failed && before.pipeline != failed {
            ci_failing += 1;
        }
    }
    // Same words in English for one or many, but not in every language
    // ("1 fusionnée", "2 fusionnées").
    let parts: Vec<String> = [
        (merged, "{n} merged"),
        (closed, "{n} closed"),
        (arrivals, "{n} new on a branch"),
        (ci_failing, "{n} CI failing"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, label)| crate::i18n::tn(n as u64, label, label))
    .collect();
    (!parts.is_empty()).then(|| {
        tf(
            "Since last session: {changes}",
            &[("changes", &parts.join(" · "))],
        )
    })
}

/// What an MR looked like when the app was last closed: its id, state,
/// branches and last pipeline.
pub(crate) type LastSeen = (
    String,
    GitlabMrState,
    HashSet<String>,
    Option<PipelineState>,
);

impl App {
    /// The startup's "since last session" changes, against what the MRs looked
    /// like at the last session (the demo's history gives it).
    pub(crate) fn changes_since(&mut self, before: Vec<LastSeen>) {
        let baseline: HashMap<String, MrBaseline> = before
            .into_iter()
            .map(|(id, state, branches, pipeline)| {
                (
                    id,
                    MrBaseline {
                        state,
                        branches,
                        pipeline,
                    },
                )
            })
            .collect();
        self.last_changes = session_changes(&baseline, &self.mrs);
        if let Some(digest) = session_digest(&baseline, &self.mrs) {
            self.last_digest = Some(digest.clone());
            self.flash_for(digest, false, DIGEST_TICKS);
        }
    }
}
