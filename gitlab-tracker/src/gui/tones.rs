//! Which theme tone and label each MR state, branch cell and effort gets.

use bevel_theme::{Theme, Tone, Tones};
use egui::Color32;

use super::colors::{contrast, lerp};
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
/// darkened over a pale tint. The text is then moved toward the theme's text
/// color as far as needed to read (WCAG AA, 4.5:1) on its tint and on the bare
/// surfaces it is also drawn on.
pub fn status_tone(kind: StatusKind, tones: &Tones) -> Tone {
    // The light theme's tones are opaque, the dark one's tints.
    let light = tones.gray.bg.a() == 255;
    let (r, g, b) = kind.rgb();
    if light {
        let (fr, fg, fb) = kind.rgb_on_light();
        let pale = |c: u8| (255.0 - (255.0 - c as f32) * 0.2) as u8;
        let bg = Color32::from_rgb(pale(r), pale(g), pale(b));
        let p = Theme::light().palette;
        Tone {
            bg,
            fg: readable(Color32::from_rgb(fr, fg, fb), &[bg, p.content], p.text),
        }
    } else {
        let bg = Color32::from_rgba_unmultiplied(r, g, b, 41);
        let p = Theme::dark().palette;
        // The tint over the lightest surface it lies on: the least contrast.
        let on_raised = blend(bg, p.raised);
        Tone {
            bg,
            fg: readable(Color32::from_rgb(r, g, b), &[on_raised, p.raised], p.text),
        }
    }
}

/// WCAG AA for normal-size text.
const AA: f32 = 4.5;

/// `fg`, moved toward `text` in small steps until it reads on every one of
/// `backgrounds`: as close to its own hue as legibility allows.
fn readable(fg: Color32, backgrounds: &[Color32], text: Color32) -> Color32 {
    let reads = |c: Color32| backgrounds.iter().all(|&bg| contrast(c, bg) >= AA);
    (0..=20)
        .map(|step| lerp(fg, text, step as f32 * 0.05))
        .find(|&c| reads(c))
        .unwrap_or(text)
}

/// `over` (a translucent tint) composited on the opaque `under`.
fn blend(over: Color32, under: Color32) -> Color32 {
    let [r, g, b, a] = over.to_srgba_unmultiplied();
    let a = f32::from(a) / 255.0;
    let mix = |o: u8, u: u8| (f32::from(o) * a + f32::from(u) * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(r, under.r()), mix(g, under.g()), mix(b, under.b()))
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
        // Not there yet is an MR's usual state, not an alarm: gray. What is
        // late says so elsewhere (the next step, "Not synced" in amber).
        Some(BranchPresence::Absent) => ("○", tl("Absent"), tones.gray),
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

/// Glyph, label and tone of a commit's signature. The glyphs differ as well
/// as the hues, so they read without color.
pub fn signature(
    check: &crate::signatures::CommitCheck,
    tones: &Tones,
) -> (&'static str, &'static str, Tone) {
    use crate::signatures::Verdict;
    match &check.verdict {
        Verdict::Verified => ("✓", tl("Verified"), tones.green),
        Verdict::SignedBeforeRebase { .. } => ("↻", tl("Signed before a rebase"), tones.teal),
        Verdict::SystemSigned if check.accepted => ("✓", tl("Signed by GitLab"), tones.green),
        Verdict::SystemSigned => ("!", tl("Signed by GitLab"), tones.amber),
        Verdict::Unverified(_) if check.accepted => ("✓", tl("Not verified"), tones.gray),
        Verdict::Unverified(_) => ("!", tl("Not verified"), tones.amber),
        Verdict::Unsigned => ("✕", tl("Unsigned"), tones.red),
        Verdict::BotExempt => ("–", tl("Exempted bot"), tones.gray),
        Verdict::Pending => ("…", tl("Checking"), tones.gray),
    }
}

/// Why a commit's signature is what it is, for its tooltip.
pub fn signature_detail(check: &crate::signatures::CommitCheck) -> String {
    use crate::i18n::tf;
    use crate::signatures::{Status, Verdict};
    match &check.verdict {
        Verdict::Verified => tl("Signed, and verified by GitLab.").into(),
        Verdict::SignedBeforeRebase { original, version } => tf(
            "Rebased from GitLab, which does not sign: its copy {sha} in version {version} \
             of the MR was signed and verified, with the same changes.",
            &[
                ("sha", original.get(..8).unwrap_or(original)),
                ("version", &version.to_string()),
            ],
        ),
        Verdict::SystemSigned => {
            tl("Signed with the GitLab instance's key, not its author's.").into()
        }
        Verdict::Unverified(status) => {
            let reason = match status {
                Status::UnknownKey => {
                    tl("GitLab does not know the key: its author must add it to their profile.")
                }
                Status::RevokedKey => tl("The key was revoked."),
                Status::OtherUser => tl("The key belongs to another GitLab user."),
                Status::SameUserDifferentEmail => {
                    tl("The commit's e-mail is not one of the key owner's verified e-mails.")
                }
                Status::UnverifiedKey => tl("The key's e-mail is not verified in GitLab."),
                Status::MultipleSignatures => tl("The commit carries several signatures."),
                _ => tl("GitLab cannot verify the signature."),
            };
            reason.into()
        }
        Verdict::Unsigned => {
            tl("No signature, and no earlier version of the MR held a signed copy of it.").into()
        }
        Verdict::BotExempt => tl("From an exempted bot: no signature needed.").into(),
        Verdict::Pending => tl("Being read from GitLab.").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_mr_not_on_a_branch_yet_is_gray_not_red() {
        let theme = Theme::dark();
        let t = &theme.tones;
        let absent = Some(BranchPresence::Absent);
        assert_eq!(presence(absent, false, t).2.fg, t.gray.fg);
        // A branch to sync it has not reached stays amber, a failed read red.
        assert_eq!(
            flow_presence(absent, false, Place::Sync, t).2.fg,
            t.amber.fg
        );
        assert_eq!(presence(None, true, t).2.fg, t.red.fg);
    }

    /// bevel-theme 0.1's palette pairs below 4.5:1, to be fixed in bevel itself
    /// (with text on the selection highlight: muted and accent, both themes).
    /// The test fails once one is fixed, so the list only shrinks.
    const KNOWN_IN_BEVEL: &[&str] = &["Dark text_muted on raised", "Light accent on chrome"];

    /// Every text color and tone the window draws reads at WCAG AA (4.5:1) on
    /// every surface, in both themes, but for [`KNOWN_IN_BEVEL`].
    #[test]
    fn text_and_tones_read_at_aa_in_both_themes() {
        let mut failures = Vec::new();
        for theme in [Theme::dark(), Theme::light()] {
            let p = theme.palette;
            let surfaces = [
                ("chrome", p.chrome),
                ("rail", p.rail),
                ("panel", p.panel),
                ("content", p.content),
                ("raised", p.raised),
                ("input", p.input),
            ];
            let texts = [
                ("text", p.text),
                ("text_secondary", p.text_secondary),
                ("text_muted", p.text_muted),
                ("accent", p.accent),
            ];
            let t = theme.tones;
            let mut tones: Vec<(String, Tone)> = [
                ("green", t.green),
                ("purple", t.purple),
                ("red", t.red),
                ("amber", t.amber),
                ("teal", t.teal),
                ("gray", t.gray),
                ("blue", t.blue),
                ("orange", t.orange),
            ]
            .into_iter()
            .map(|(n, tone)| (n.to_string(), tone))
            .collect();
            tones.extend(
                StatusKind::ALL
                    .iter()
                    .map(|&k| (format!("status {}", k.name()), status_tone(k, &t))),
            );
            let mut check = |what: String, fg: Color32, bg: Color32| {
                let c = contrast(fg, bg);
                let what = format!("{:?} {what}", theme.mode);
                let known = KNOWN_IN_BEVEL.contains(&what.as_str());
                if (c < AA) != known {
                    let tag = if known {
                        "fixed, drop from KNOWN_IN_BEVEL"
                    } else {
                        ""
                    };
                    failures.push(format!("{what}: {c:.2} {tag}"));
                }
            };
            for (s, bg) in surfaces {
                for (n, fg) in texts {
                    check(format!("{n} on {s}"), fg, bg);
                }
            }
            for (s, bg) in [
                ("content", p.content),
                ("panel", p.panel),
                ("raised", p.raised),
            ] {
                for (n, tone) in &tones {
                    check(format!("{n} chip on {s}"), tone.fg, blend(tone.bg, bg));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "contrast (AA = 4.5:1):\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn a_status_hue_already_legible_is_kept() {
        let tones = Theme::dark().tones;
        let (r, g, b) = StatusKind::Mergeable.rgb();
        assert_eq!(
            status_tone(StatusKind::Mergeable, &tones).fg,
            Color32::from_rgb(r, g, b)
        );
    }
}
