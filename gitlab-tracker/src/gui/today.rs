//! The Today page: what waits for me across every open tab — the reviews asked of
//! me, my MRs that need a fix, those ready to merge, those still waiting, and
//! what I merged today. "Me" is the project's `gitlab_username`, read from GitLab
//! when the tab opens. A click shows the MR in its project's tab.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, Vec2};

use super::widgets::{avatar, badge, card, mono, muted, page_title, split_person};
use super::{tones, GuiApp, SlotState};
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState};
use crate::utils::{awaiting_review_by, matches_gitlab_username};
use gitlab_tracker_core::MergeabilityStatus;

/// Where an MR stands for me; each MR shows under the first that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Someone asked for my review, and I have not approved yet.
    Review,
    /// Mine, and something needs me: CI failed, conflicts, threads, changes asked.
    Fix,
    /// Mine, approved and mergeable.
    Ready,
    /// Assigned to me, someone else's.
    Assigned,
    /// Mine, waiting on the others.
    Waiting,
    /// Mine, merged today.
    Merged,
}

impl Kind {
    const ALL: [Kind; 6] = [
        Kind::Review,
        Kind::Fix,
        Kind::Ready,
        Kind::Assigned,
        Kind::Waiting,
        Kind::Merged,
    ];

    fn title(self) -> &'static str {
        match self {
            Kind::Review => tl("Waiting for my review"),
            Kind::Fix => tl("My MRs that need me"),
            Kind::Ready => tl("Ready to merge"),
            Kind::Assigned => tl("Assigned to me"),
            Kind::Waiting => tl("Waiting on the others"),
            Kind::Merged => tl("Merged today"),
        }
    }

    fn empty(self) -> &'static str {
        match self {
            Kind::Review => tl("No review waits for you."),
            Kind::Fix => tl("None of your MRs is failing, in conflict or awaiting an answer."),
            Kind::Ready => tl("None of your MRs is approved and mergeable yet."),
            Kind::Assigned => tl("Nothing else is assigned to you."),
            Kind::Waiting => tl("None of your MRs waits on a reviewer."),
            Kind::Merged => tl("Nothing of yours merged today yet."),
        }
    }

    fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Kind::Review => t.amber,
            Kind::Fix => t.red,
            Kind::Ready => t.green,
            Kind::Assigned => t.blue,
            Kind::Waiting => t.teal,
            Kind::Merged => t.purple,
        }
    }
}

/// An MR on the page.
#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    /// The tab it is in.
    pub tab: usize,
    pub project: String,
    pub id: String,
    pub title: String,
    pub author: String,
    /// Why it is here, shortest first: "CI failed", "2 threads"…
    pub reasons: Vec<String>,
    pub state: GitlabMrState,
    pub mergeability: MergeabilityStatus,
}

/// Where each MR of `app` stands for `username`; the MRs that are none of my
/// business are left out.
pub fn classify(app: &App, username: &str, tab: usize, today: chrono::NaiveDate) -> Vec<Item> {
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let mut items = Vec::new();
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let open = mr.state == GitlabMrState::Opened;
        let mine = matches_gitlab_username(&mr.author, username) || mr.author == username;
        let assigned = mr
            .assignee
            .as_deref()
            .is_some_and(|a| matches_gitlab_username(a, username) || a == username);
        let mut reasons = Vec::new();
        let kind = if awaiting_review_by(
            mr.state,
            mr.mergeability,
            &mr.reviewers,
            &mr.approved_by,
            username,
        ) && !mine
        {
            Some(Kind::Review)
        } else if mine && open {
            if mr
                .pipelines
                .first()
                .is_some_and(|p| p.status == PipelineState::Failed)
            {
                reasons.push(tl("CI failed").to_string());
            }
            // Said by the status badge already: no chip of its own.
            let blocked = matches!(
                mr.mergeability,
                MergeabilityStatus::Conflict
                    | MergeabilityStatus::NeedsRebase
                    | MergeabilityStatus::RequestedChanges
            );
            if mr.user_notes_count > 0 {
                reasons.push(tn(
                    u64::from(mr.user_notes_count),
                    "{n} thread to answer",
                    "{n} threads to answer",
                ));
            }
            if blocked || !reasons.is_empty() {
                Some(Kind::Fix)
            } else if mr.mergeability == MergeabilityStatus::Mergeable && !mr.approved_by.is_empty()
            {
                reasons.push(tn(
                    mr.approved_by.len() as u64,
                    "{n} approval",
                    "{n} approvals",
                ));
                Some(Kind::Ready)
            } else {
                if mr.mergeability == MergeabilityStatus::Draft {
                    reasons.push(tl("Still a draft").to_string());
                } else if mr.reviewers.is_empty() {
                    reasons.push(tl("No reviewer").to_string());
                }
                Some(Kind::Waiting)
            }
        } else if assigned && open {
            Some(Kind::Assigned)
        } else if mine
            && mr.state == GitlabMrState::Merged
            && mr
                .merged_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| at.with_timezone(&chrono::Local).date_naive() == today)
        {
            Some(Kind::Merged)
        } else {
            None
        };
        if let Some(kind) = kind {
            items.push(Item {
                kind,
                tab,
                project: project.clone(),
                id: mr.id.clone(),
                title: mr.title.clone(),
                author: mr.author.clone(),
                reasons,
                state: mr.state,
                mergeability: mr.mergeability,
            });
        }
    }
    items
}

/// What waits for me now (reviews and fixes), for the rail's badge.
pub fn waiting_count(g: &GuiApp) -> usize {
    collect(g)
        .0
        .iter()
        .filter(|i| matches!(i.kind, Kind::Review | Kind::Fix))
        .count()
}

/// The MRs of every loaded tab, and the usernames they were read as.
fn collect(g: &GuiApp) -> (Vec<Item>, Vec<String>) {
    let today = chrono::Local::now().date_naive();
    let mut items = Vec::new();
    let mut users = Vec::new();
    for (index, slot) in g.tabs.iter().enumerate() {
        let app = match &slot.state {
            SlotState::Active => &g.app,
            SlotState::Parked(tab) => &tab.app,
            _ => continue,
        };
        let Some(username) = app.config.gitlab_username.as_deref() else {
            continue;
        };
        if !users.iter().any(|u| u == username) {
            users.push(username.to_string());
        }
        items.extend(classify(app, username, index, today));
    }
    (items, users)
}

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let (items, users) = collect(g);
    let projects = g
        .tabs
        .iter()
        .filter(|t| matches!(t.state, SlotState::Active | SlotState::Parked(_)))
        .count();
    let subtitle = if users.is_empty() {
        tl("Your GitLab username is not known yet: it is read from GitLab when a project opens.")
            .to_string()
    } else {
        let who = users
            .iter()
            .map(|u| format!("@{u}"))
            .collect::<Vec<_>>()
            .join(", ");
        tf(
            "Across {n} open projects · as {user}",
            &[("n", &projects.to_string()), ("user", &who)],
        )
    };
    page_title(ui, &theme, tl("Today"), subtitle);
    ui.add_space(14.0);

    // The counts, one tile per kind that matters for the day.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for kind in [Kind::Review, Kind::Fix, Kind::Ready, Kind::Merged] {
            let n = items.iter().filter(|i| i.kind == kind).count();
            tile(ui, &theme, kind, n);
        }
    });
    ui.add_space(12.0);

    let several = projects > 1;
    let mut pick = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for kind in Kind::ALL {
                let list: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
                // The quiet sections only when they have something.
                if list.is_empty() && matches!(kind, Kind::Assigned | Kind::Waiting) {
                    continue;
                }
                let title = format!("{} · {}", kind.title(), list.len());
                card(ui, &theme, &title, |ui| {
                    if list.is_empty() {
                        ui.label(muted(&theme, kind.empty()));
                        return;
                    }
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for item in list {
                        if row(ui, &theme, item, several) {
                            pick = Some((item.tab, item.id.clone()));
                        }
                    }
                });
                ui.add_space(10.0);
            }
        });
    if let Some((tab, id)) = pick {
        g.activate(tab);
        g.show_mr(&id);
    }
}

/// A count of one kind, in its tone.
fn tile(ui: &mut Ui, theme: &Theme, kind: Kind, n: usize) {
    let tone = kind.tone(theme);
    egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_min_width(170.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(n.to_string())
                        .font(fonts::semibold(theme.text_size + 10.0))
                        .color(if n > 0 {
                            tone.fg
                        } else {
                            theme.palette.text_secondary
                        }),
                );
                ui.label(muted(theme, kind.title()));
            });
        });
}

/// One MR; `true` when clicked.
fn row(ui: &mut Ui, theme: &Theme, item: &Item, show_project: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let tone = item.kind.tone(theme);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 6.0), Vec2::new(3.0, 24.0)),
        CornerRadius::same(2),
        tone.fg,
    );
    let mut line = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The row takes the click: its texts, selectable, would take it over them.
    line.style_mut().interaction.selectable_labels = false;
    line.spacing_mut().item_spacing.x = 8.0;
    if show_project {
        line.add(
            Chip::new(&item.project, theme.tones.gray).font(fonts::medium(theme.text_size - 2.5)),
        );
    }
    line.label(
        mono(format!("!{}", item.id), theme.text_size - 1.0).color(theme.palette.text_secondary),
    );
    let (name, _) = split_person(&item.author);
    avatar(&mut line, theme, name, 18.0).on_hover_text(name);
    line.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (status, status_tone) = tones::status(item.state, &item.mergeability, &theme.tones);
        badge(ui, theme, status, status_tone);
        for reason in item.reasons.iter().rev() {
            ui.add(Chip::new(reason, tone).font(fonts::medium(theme.text_size - 2.5)));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&item.title).font(fonts::medium(theme.text_size)))
                    .truncate(),
            );
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Show it in its project's tab"))
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_demo_sorts_into_the_days_lists() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let today = chrono::Local::now().date_naive();
        let items = classify(&app, "thomas_db", 0, today);
        // Nothing is listed twice, and nothing of someone else's that does not
        // concern me shows.
        let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
        for item in &items {
            let mr = app.mrs.iter().find(|m| m.id == item.id).unwrap();
            let concerns_me = mr.author.contains("@thomas_db")
                || mr.author == "thomas_db"
                || mr
                    .assignee
                    .as_deref()
                    .is_some_and(|a| a.contains("@thomas_db"))
                || mr.reviewers.iter().any(|r| r.contains("@thomas_db"));
            assert!(concerns_me, "!{} is not about thomas_db", item.id);
        }
        // Someone with nothing to do gets nothing.
        assert!(classify(&app, "nobody_here", 0, today).is_empty());
    }
}
