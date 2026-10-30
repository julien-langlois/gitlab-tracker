//! Which theme tone and label each MR state, branch cell and effort gets.

use bevel_theme::{Tone, Tones};

use crate::i18n::tl;
use crate::models::{BranchPresence, Effort, GitlabMrState, MergeabilityStatus, PipelineState};

/// Label and tone of the status badge. Open MRs show their mergeability, as the
/// TUI badge does once its animation settles.
pub fn status(
    state: GitlabMrState,
    mergeability: &MergeabilityStatus,
    tones: &Tones,
) -> (&'static str, Tone) {
    match state {
        GitlabMrState::Merged => (tl("MERGED"), tones.purple),
        GitlabMrState::Closed => (tl("CLOSED"), tones.gray),
        GitlabMrState::Opened => {
            let label = match mergeability {
                // Shorter than the TUI badges, to fit the column.
                MergeabilityStatus::RequestedChanges => "CHANGES REQ.",
                MergeabilityStatus::CiStillRunning => "CI RUNNING",
                _ => crate::ui::table::mergeability_badge(mergeability).0,
            };
            let tone = match mergeability {
                MergeabilityStatus::Mergeable | MergeabilityStatus::Unknown => tones.green,
                MergeabilityStatus::Conflict
                | MergeabilityStatus::RequestedChanges
                | MergeabilityStatus::SyncFailed
                | MergeabilityStatus::NotOpen => tones.red,
                MergeabilityStatus::NeedsRebase
                | MergeabilityStatus::NotApproved
                | MergeabilityStatus::CiMustPass => tones.amber,
                MergeabilityStatus::CiStillRunning => tones.orange,
                MergeabilityStatus::DiscussionsNotResolved => tones.blue,
                MergeabilityStatus::Retrying => tones.teal,
                MergeabilityStatus::Draft => tones.gray,
            };
            (tl(label), tone)
        }
    }
}

/// Glyph, label and tone of a branch cell (`None`: loading, or `failed`).
/// The glyphs differ as well as the hues, so cells read without color.
pub fn presence(
    presence: Option<BranchPresence>,
    failed: bool,
    tones: &Tones,
) -> (&'static str, &'static str, Tone) {
    match presence {
        Some(BranchPresence::Present) => ("●", tl("Present"), tones.green),
        Some(BranchPresence::CherryPicked) => ("◆", tl("Picked"), tones.amber),
        Some(BranchPresence::Copied) => ("≈", tl("Copied"), tones.teal),
        Some(BranchPresence::Partial) => ("◐", tl("Partial"), tones.purple),
        Some(BranchPresence::Absent) => ("○", tl("Absent"), tones.red),
        None if failed => ("✕", tl("Failed"), tones.red),
        None => ("…", tl("Loading"), tones.gray),
    }
}

pub fn effort(effort: Effort, tones: &Tones) -> (&'static str, Tone) {
    match effort {
        Effort::Easy => (tl("Easy"), tones.green),
        Effort::Medium => (tl("Medium"), tones.amber),
        Effort::Complex => (tl("Complex"), tones.red),
    }
}

pub fn pipeline(state: PipelineState, tones: &Tones) -> (&'static str, Tone) {
    let (label, tone) = match state {
        PipelineState::Success => ("Passed", tones.green),
        PipelineState::Failed => ("Failed", tones.red),
        PipelineState::Running => ("Running", tones.orange),
        PipelineState::Pending | PipelineState::Created => ("Pending", tones.amber),
        PipelineState::Canceled => ("Canceled", tones.gray),
        PipelineState::Skipped => ("Skipped", tones.gray),
        PipelineState::Unknown => ("Unknown", tones.gray),
    };
    (tl(label), tone)
}

/// Tone of a label chip: scoped labels by scope, the rest neutral.
pub fn label(name: &str, tones: &Tones) -> Tone {
    match name.split_once("::").map(|(scope, _)| scope) {
        Some("deploy") => tones.purple,
        Some("review") => tones.blue,
        Some(_) => tones.teal,
        None => tones.gray,
    }
}

/// Tone of a tracker status, guessed from its name (trackers name their own):
/// done in green, in progress in blue, in review in purple, blocked in red.
pub fn ticket_status(status: &str, tones: &Tones) -> Tone {
    let status = status.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| status.contains(w));
    if has(&["reject", "block", "bloqu", "cancel", "annul"]) {
        tones.red
    } else if has(&[
        "clos", "resol", "résol", "done", "termin", "fermé", "livr", "deliver",
    ]) {
        tones.green
    } else if has(&["review", "valid", "test", "recette", "qa"]) {
        tones.purple
    } else if has(&["progress", "cours", "doing", "develop", "dév"]) {
        tones.blue
    } else if has(&["feedback", "wait", "attente", "pending"]) {
        tones.amber
    } else {
        tones.gray
    }
}
