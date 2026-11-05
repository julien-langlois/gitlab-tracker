//! Which theme tone and label each MR state, branch cell and effort gets.

use bevel_theme::{Tone, Tones};
use egui::Color32;

use crate::branch_flow::Place;
use crate::i18n::tl;
use crate::models::{BranchPresence, Effort, GitlabMrState, MergeabilityStatus, PipelineState};
use crate::status::StatusKind;

/// Label and tone of the status badge. Open MRs show their mergeability, as the
/// TUI badge does once its animation settles.
pub fn status(
    state: GitlabMrState,
    mergeability: &MergeabilityStatus,
    tones: &Tones,
) -> (&'static str, Tone) {
    let kind = StatusKind::of(state, mergeability);
    (tl(kind.short_badge()), status_tone(kind, tones))
}

/// The color of a status's swatch (its segment of the status bar, its dot):
/// its own hue, a little deeper on a light background.
pub fn status_swatch(kind: StatusKind, tones: &Tones) -> Color32 {
    let (r, g, b) = kind.rgb();
    let deeper = |c: u8| {
        if tones.gray.bg.a() == 255 {
            (c as f32 * 0.8) as u8
        } else {
            c
        }
    };
    Color32::from_rgb(deeper(r), deeper(g), deeper(b))
}

/// The tone of a status: its own hue (see [`StatusKind::rgb`]), as the theme's
/// tones are made: a tint of it under it on a dark background; on a light one,
/// darkened over a pale tint.
pub fn status_tone(kind: StatusKind, tones: &Tones) -> Tone {
    // The light theme's tones are opaque, the dark one's tints.
    let light = tones.gray.bg.a() == 255;
    let (r, g, b) = kind.rgb();
    if light {
        let (fr, fg, fb) = kind.rgb_on_light();
        let pale = |c: u8| (255.0 - (255.0 - c as f32) * 0.2) as u8;
        Tone {
            bg: Color32::from_rgb(pale(r), pale(g), pale(b)),
            fg: Color32::from_rgb(fr, fg, fb),
        }
    } else {
        Tone {
            bg: Color32::from_rgba_unmultiplied(r, g, b, 41),
            fg: Color32::from_rgb(r, g, b),
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

/// [`presence`] of a branch at its `place` in an MR's flow: a branch to sync
/// not reached yet is behind (amber, a rebase is due), not a failure; one off
/// the MR's path, absent, does not concern it (gray).
pub fn flow_presence(
    presence: Option<BranchPresence>,
    failed: bool,
    place: Place,
    tones: &Tones,
) -> (&'static str, &'static str, Tone) {
    match (presence, place) {
        (Some(BranchPresence::Absent), Place::Sync) => ("○", tl("Not synced"), tones.amber),
        (Some(BranchPresence::Absent), Place::Off) => ("○", tl("Absent"), tones.gray),
        _ => self::presence(presence, failed, tones),
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
