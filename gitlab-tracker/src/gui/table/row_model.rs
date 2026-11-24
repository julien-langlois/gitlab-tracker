//! What a row shows, computed once per frame from the MR: next step, signatures, branches.

use super::*;

/// What one table row shows, copied out of the app so the table can record actions.
pub(super) struct RowView {
    pub(super) index: usize,
    pub(super) id: String,
    pub(super) title: String,
    pub(super) author: String,
    pub(super) updated: String,
    pub(super) labels: Vec<(String, Tone)>,
    pub(super) flagged: bool,
    pub(super) fresh: bool,
    pub(super) selected: bool,
    /// Picked with Ctrl / Shift + click, for the bulk bar.
    pub(super) marked: bool,
    /// Bulk mode: a box replaces the flag, a click picks the row.
    pub(super) bulk_mode: bool,
    pub(super) status: (&'static str, Tone),
    pub(super) effort: Option<(&'static str, Tone)>,
    pub(super) threads: u32,
    pub(super) activity: (&'static str, Tone),
    /// The color of the "updated" date: amber once slowing, red once stale.
    pub(super) age: Option<egui::Color32>,
    /// The title is not loaded yet, and a fetch is on its way.
    pub(super) loading: bool,
    pub(super) branches: Vec<BranchCell>,
    pub(super) milestone: Option<String>,
    pub(super) target: String,
    pub(super) behind: Option<u32>,
    /// Still open: what it lacks can be listed.
    pub(super) opened: bool,
    pub(super) ticket: Option<(String, String)>,
    pub(super) has_ticket: bool,
    /// Who it waits on, for the Next step column.
    pub(super) next: Option<NextView>,
    /// The latest pipeline: its state, `None` without one.
    pub(super) pipeline: Option<crate::models::PipelineState>,
    /// Commits whose signature the project's policy rejects, when it enforces
    /// signed commits and there are some.
    pub(super) signatures: Option<SignatureBadge>,
    /// Its place in the merge queue: "2/4", its tone and what it means.
    pub(super) queue: Option<(String, Tone, String)>,
}

/// The shield next to the author: how many commits lack a valid signature.
#[derive(Debug, Clone)]
pub(super) struct SignatureBadge {
    pub(super) count: usize,
    pub(super) tone: Tone,
    pub(super) tip: String,
}

impl SignatureBadge {
    pub(super) fn of(
        summary: &crate::signatures::Summary,
        tones: &bevel_theme::Tones,
    ) -> Option<Self> {
        let count = summary.anomalies();
        if count == 0 {
            return None;
        }
        let mut lines = Vec::new();
        if !summary.unsigned.is_empty() {
            lines.push(tn(
                summary.unsigned.len() as u64,
                "{n} commit without a signature",
                "{n} commits without a signature",
            ));
        }
        if !summary.system.is_empty() {
            lines.push(tn(
                summary.system.len() as u64,
                "{n} commit signed by GitLab only",
                "{n} commits signed by GitLab only",
            ));
        }
        if !summary.unverified.is_empty() {
            lines.push(tn(
                summary.unverified.len() as u64,
                "{n} signature not verified",
                "{n} signatures not verified",
            ));
        }
        if summary.rebased > 0 {
            lines.push(tn(
                summary.rebased as u64,
                "{n} signed before a rebase from GitLab: tolerated",
                "{n} signed before a rebase from GitLab: tolerated",
            ));
        }
        let unsigned = summary.unsigned.len() + summary.system.len();
        Some(Self {
            count,
            tone: if unsigned > 0 { tones.red } else { tones.amber },
            tip: lines.join("\n"),
        })
    }
}

/// The signature badge: a shield and the count, its detail on hover.
pub(super) fn signature_chip(ui: &mut Ui, theme: &Theme, badge: &SignatureBadge) {
    let response = egui::Frame::new()
        .fill(badge.tone.bg)
        .corner_radius(CornerRadius::same(theme.radii.small))
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Shield, badge.tone.fg);
            ui.label(
                RichText::new(badge.count.to_string())
                    .font(fonts::semibold(theme.text_size - 2.5))
                    .color(badge.tone.fg),
            );
        })
        .response;
    response.rich(Tip::new().text(&badge.tip));
}

/// The Next step cell: who, what, since when.
#[derive(Debug, Clone)]
pub struct NextView {
    /// "You", "Alex", "Deploy"…
    pub who: String,
    /// "rebase, 2 threads"
    pub what: String,
    /// "waiting 12 d", "on staging for 4 d"
    pub since: String,
    pub tone: Tone,
    /// I have to act: drawn stronger.
    pub mine: bool,
    /// The todos in full, for the hover.
    pub hint: String,
}

impl NextView {
    pub fn of(step: &crate::next_step::NextStep, theme: &Theme) -> Option<Self> {
        use crate::next_step::{days_text, Actor};
        let tones = &theme.tones;
        let now = chrono::Utc::now();
        let waited = step.waited_days(now).map(days_text);
        let (tone, since) = match step.actor {
            Actor::Unknown => return None,
            Actor::Nobody if step.closed => (tones.gray, String::new()),
            Actor::Nobody => (
                tones.green,
                step.reached
                    .as_ref()
                    .map(|b| tf("on {branch}", &[("branch", b)]))
                    .unwrap_or_default(),
            ),
            Actor::Deploy => (
                tones.blue,
                match (&step.reached, &waited) {
                    (Some(b), Some(w)) => {
                        tf("on {branch} for {time}", &[("branch", b), ("time", w)])
                    }
                    (Some(b), None) => tf("on {branch}", &[("branch", b)]),
                    _ => String::new(),
                },
            ),
            _ => (
                if step.mine { tones.orange } else { tones.gray },
                waited
                    .map(|w| tf("waiting {time}", &[("time", &w)]))
                    .unwrap_or_default(),
            ),
        };
        let mut hint: Vec<String> = step
            .todos
            .iter()
            .map(|t| format!("• {}", t.text()))
            .collect();
        if !step.people.is_empty() && !step.mine {
            hint.insert(
                0,
                step.people
                    .iter()
                    .map(|p| split_person(p).0.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !step.then.is_empty() {
            hint.push(tf("Then: {steps}", &[("steps", &step.then.join(" → "))]));
        }
        Some(Self {
            who: step.who(),
            what: step.summary(),
            since,
            tone,
            mine: step.mine,
            hint: hint.join("\n"),
        })
    }
}

/// A branch cell: where the MR stands on it, or why that is not known yet.
#[derive(Debug, Clone, Copy)]
pub(super) enum BranchCell {
    Known(&'static str, &'static str, Tone),
    /// A fetch is on its way.
    Loading,
    /// Not fetched, and nothing on its way: the next refresh tells.
    Unknown,
}

/// The rows of `indexes` (into the visible list): only those in view are copied
/// out, so a frame costs the same whatever the number of MRs.
pub(super) fn row_views(g: &GuiApp, indexes: &[usize]) -> Vec<RowView> {
    let app = &g.app;
    let theme = &g.theme;
    let tones = &theme.tones;
    let selected = app.table_state.selected();
    let fresh_window = app.update_highlight_ticks > 0;
    let me = app.config.gitlab_username.as_deref();
    let order = branch_order(app);
    let mut views: Vec<RowView> = app
        .visible_mrs()
        .enumerate()
        .filter(|(index, _)| indexes.contains(index))
        .map(|(index, mr)| {
            let failed = matches!(mr.status, MrStatus::Error(_));
            let fetching = app.pending_initial_fetches.contains(&mr.id)
                || app.pending_refresh_fetches.contains(&mr.id);
            let activity = activity(app, mr.updated_at.as_deref(), theme);
            RowView {
                index,
                id: mr.id.clone(),
                title: if mr.title.is_empty() {
                    tl("Loading…").into()
                } else {
                    mr.title.clone()
                },
                loading: mr.title.is_empty() && fetching,
                age: [tones.amber, tones.red]
                    .iter()
                    .find(|t| t.fg == activity.1.fg)
                    .map(|t| t.fg),
                author: mr.author.clone(),
                updated: mr
                    .updated_at
                    .as_deref()
                    .map(crate::utils::format_relative_date)
                    .unwrap_or_default(),
                labels: mr
                    .labels
                    .iter()
                    .filter(|l| app.config.is_table_label(l))
                    .map(|l| (l.clone(), label_tone(app, l, theme)))
                    .collect(),
                flagged: mr.flagged,
                fresh: mr.recently_updated && fresh_window,
                selected: selected == Some(index),
                marked: g.marked.contains(&mr.id),
                bulk_mode: g.bulk_mode,
                status: tones::status(mr.state, &mr.mergeability, tones),
                effort: mr.diff_stats.as_ref().map(|s| {
                    let effort = Effort::from_score(s.difficulty(&app.config.complexity_profile));
                    tones::effort(effort, tones)
                }),
                threads: mr.user_notes_count,
                activity,
                branches: order
                    .iter()
                    .map(|(b, _)| match app.branch_presence(mr, b) {
                        None if !failed && fetching => BranchCell::Loading,
                        None if !failed => BranchCell::Unknown,
                        presence => {
                            let (glyph, label, tone) = tones::presence(presence, failed, tones);
                            BranchCell::Known(glyph, label, tone)
                        }
                    })
                    .collect(),
                milestone: mr.milestone.clone(),
                target: mr.target_branch.clone(),
                behind: mr.diff_stats.as_ref().and_then(|s| s.commits_behind),
                opened: mr.state == GitlabMrState::Opened,
                ticket: mr
                    .linked_ticket
                    .as_ref()
                    .map(|t| (t.id.clone(), t.status.clone())),
                has_ticket: mr.linked_ticket.is_some() && app.tracker.is_some(),
                next: NextView::of(&crate::next_step::of(app, mr, me), theme),
                pipeline: mr.pipelines.first().map(|p| p.status),
                signatures: app
                    .signature_report(mr)
                    .and_then(|report| SignatureBadge::of(&report.summary(), tones)),
                queue: super::super::merge_queue::cell(&g.queues, &mr.id, theme),
            }
        })
        .collect();
    // In the order asked for (the groups reorder the list).
    views.sort_by_key(|v| indexes.iter().position(|i| *i == v.index));
    views
}
