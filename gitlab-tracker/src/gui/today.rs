//! The Today page: what waits for me across every open tab — the reviews asked of
//! me, my MRs that need a fix, those ready to merge, those still waiting, and
//! what I merged today. "Me" is the project's `gitlab_username`, read from GitLab
//! when the tab opens. A click shows the MR in its project's tab.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, Vec2};

use super::widgets::{avatar, card, mono, muted, page_title, split_person};
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
    /// Since when it waits, and the usual wait when known: "6 d · usually 1 d".
    pub since: Option<String>,
    /// Waits too long for that step: the since in amber.
    pub late: bool,
    /// A message to remind whoever it waits on, for the MRs waiting on the others.
    pub reminder: Option<String>,
    /// The tab of the MR's page that helps with it, and the button's name.
    pub action: Option<(super::mr_page::Tab, &'static str)>,
}

/// Where each MR of `app` stands for `username`; the MRs that are none of my
/// business are left out.
pub fn classify(app: &App, username: &str, tab: usize, today: chrono::NaiveDate) -> Vec<Item> {
    use crate::next_step::{days_text, first_name, Actor};
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let usual_review = crate::next_step::usual_first_review_days(app);
    let now = chrono::Utc::now();
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
        // Who it waits on, since when, and (waiting on the others) a reminder.
        let step = crate::next_step::of(app, mr, Some(username));
        let waited = step.waited_days(now);
        let usual = (step.actor == Actor::Reviewers)
            .then_some(usual_review)
            .flatten();
        let late = match (waited, usual) {
            (Some(w), Some(u)) => w > u * 1.5 && w - u > 0.5,
            (Some(w), None) => w > 7.0,
            _ => false,
        };
        let since = waited.map(|w| match usual {
            Some(u) => tf(
                "{time} · usually {usual}",
                &[("time", &days_text(w)), ("usual", &days_text(u))],
            ),
            None => days_text(w),
        });
        let mut reminder = None;
        if kind == Some(Kind::Waiting) && !step.mine && !step.people.is_empty() {
            let names: Vec<&str> = step.people.iter().map(|p| first_name(p)).collect();
            let what = step
                .todos
                .first()
                .map(|t| t.text().to_lowercase())
                .unwrap_or_default();
            reasons.push(tf(
                "Waiting on {who}: {what}",
                &[("who", &names.join(", ")), ("what", &what)],
            ));
            reminder = Some(tf(
                "Hi {who}, could you have a look at !{id} \"{title}\" when you can? {url} (waiting for {time})",
                &[
                    ("who", &names.join(", ")),
                    ("id", &mr.id),
                    ("title", &mr.title),
                    ("url", &mr.web_url),
                    ("time", &waited.map(days_text).unwrap_or_default()),
                ],
            ));
        }
        use crate::next_step::Todo;
        let action = match kind {
            Some(Kind::Review) => Some(if step.todos.contains(&Todo::Review { again: true }) {
                (super::mr_page::Tab::Commits, tl("See the new commits"))
            } else {
                (super::mr_page::Tab::Changes, tl("Review it"))
            }),
            Some(Kind::Fix)
                if step
                    .todos
                    .iter()
                    .any(|t| matches!(t, Todo::AnswerThreads { .. })) =>
            {
                Some((super::mr_page::Tab::Discussions, tl("Open the threads")))
            }
            Some(Kind::Fix) if step.todos.contains(&Todo::FixPipeline) => {
                Some((super::mr_page::Tab::Pipelines, tl("See the pipeline")))
            }
            _ => None,
        };
        let shows_since = matches!(
            kind,
            Some(Kind::Review | Kind::Fix | Kind::Waiting | Kind::Ready | Kind::Assigned)
        );
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
                since: since.filter(|_| shows_since),
                late: late && shows_since,
                reminder,
                action,
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
    let mut standup_copied = false;
    ui.horizontal(|ui| {
        page_title(ui, &theme, tl("Today"), subtitle);
        if !users.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                standup_copied = super::widgets::button(
                    ui,
                    &theme,
                    Some(super::icons::Icon::Copy),
                    tl("Copy my stand-up"),
                    false,
                )
                .on_hover_text(tl(
                    "What you did since the last working day, what is on today, and what waits on others, as text for the daily meeting",
                ))
                .clicked();
            });
        }
    });
    if standup_copied {
        ui.ctx().copy_text(standup(g, &items, chrono::Local::now()));
        g.app
            .flash(tl("Stand-up copied: paste it in the team's chat"), false);
    }
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
    let mut copied = false;
    let mut seen = false;
    let changes = feed(g);
    let wide = ui.available_width() >= 1100.0;
    type Pick = Option<(usize, String, Option<super::mr_page::Tab>)>;
    let lists = |ui: &mut Ui, pick: &mut Pick, copied: &mut bool| {
        let mut empty = Vec::new();
        for kind in Kind::ALL {
            let list: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
            // The quiet sections only when they have something.
            if list.is_empty() && matches!(kind, Kind::Assigned | Kind::Waiting) {
                continue;
            }
            // The empty ones in a line at the end: they take no room.
            if list.is_empty() {
                empty.push(kind);
                continue;
            }
            let title = format!("{} · {}", kind.title(), list.len());
            card(ui, &theme, &title, |ui| {
                if kind == Kind::Waiting {
                    ui.label(muted(
                        &theme,
                        tl("Your MRs blocked by someone else: a reminder can unblock them."),
                    ));
                    ui.add_space(4.0);
                }
                ui.spacing_mut().item_spacing.y = 2.0;
                for item in list {
                    match row(ui, &theme, item, several) {
                        Some(RowClick::Open) => *pick = Some((item.tab, item.id.clone(), None)),
                        Some(RowClick::Page(tab)) => {
                            *pick = Some((item.tab, item.id.clone(), Some(tab)))
                        }
                        Some(RowClick::Copied) => *copied = true,
                        None => {}
                    }
                }
            });
            ui.add_space(10.0);
        }
        if !empty.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for kind in empty {
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0, theme.palette.border))
                        .corner_radius(CornerRadius::same(theme.radii.medium))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(muted(
                                &theme,
                                tf("{section} · nothing yet", &[("section", kind.title())]),
                            ))
                            .on_hover_text(kind.empty());
                        });
                }
            });
        }
    };
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if wide {
                ui.horizontal_top(|ui| {
                    let side = 360.0;
                    let left = ui.available_width() - side - 16.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(left, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(left);
                            lists(ui, &mut pick, &mut copied);
                        },
                    );
                    ui.add_space(16.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(side, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(side);
                            match feed_card(ui, &theme, &changes, several) {
                                Some(FeedClick::Open(tab, id)) => pick = Some((tab, id, None)),
                                Some(FeedClick::Seen) => seen = true,
                                None => {}
                            }
                        },
                    );
                });
            } else {
                lists(ui, &mut pick, &mut copied);
                ui.add_space(10.0);
                match feed_card(ui, &theme, &changes, several) {
                    Some(FeedClick::Open(tab, id)) => pick = Some((tab, id, None)),
                    Some(FeedClick::Seen) => seen = true,
                    None => {}
                }
            }
        });
    if copied {
        g.app.flash(
            tl("Reminder copied: paste it in a chat or a comment"),
            false,
        );
    }
    if seen {
        g.feed_seen_at = Some(chrono::Local::now());
        g.app.last_changes.clear();
        for slot in &mut g.tabs {
            if let SlotState::Parked(tab) = &mut slot.state {
                tab.app.last_changes.clear();
            }
        }
    }
    match pick {
        Some((tab, id, None)) => {
            g.activate(tab);
            g.show_mr(&id);
        }
        Some((tab, id, Some(page_tab))) => {
            g.activate(tab);
            g.open_mr(&id);
            g.mr_tab = page_tab;
        }
        None => {}
    }
}

/// Where the stand-up's "since" starts: the last working day, at midnight
/// (Friday on a Monday, else yesterday).
fn last_working_day(now: chrono::DateTime<chrono::Local>) -> chrono::DateTime<chrono::Utc> {
    use chrono::{Datelike, Weekday};
    let back = match now.weekday() {
        Weekday::Mon => 3,
        Weekday::Sun => 2,
        _ => 1,
    };
    let day = now.date_naive() - chrono::Duration::days(back);
    day.and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
        .map_or_else(
            || now.with_timezone(&chrono::Utc) - chrono::Duration::days(back),
            |t| t.with_timezone(&chrono::Utc),
        )
}

/// My stand-up as text: what I did since the last working day (merged,
/// reviewed, pushed, opened), what is on today (reviews, fixes, merges) and
/// what waits on others; across every open tab.
fn standup(g: &GuiApp, items: &[Item], now: chrono::DateTime<chrono::Local>) -> String {
    use std::fmt::Write;
    let since = last_working_day(now);
    let after = |at: Option<&str>| {
        at.and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
            .is_some_and(|t| t.with_timezone(&chrono::Utc) >= since)
    };
    let line = |id: &str, title: &str, project: Option<&str>| match project {
        Some(p) => format!("!{id} {title} ({p})"),
        None => format!("!{id} {title}"),
    };
    let several = g
        .tabs
        .iter()
        .filter(|t| matches!(t.state, SlotState::Active | SlotState::Parked(_)))
        .count()
        > 1;
    // Each tab's links: its MRs and tickets, clickable once pasted.
    let links: Vec<Option<super::board::Links>> = g
        .tabs
        .iter()
        .map(|slot| match &slot.state {
            SlotState::Active => Some(super::board::Links::of(&g.app)),
            SlotState::Parked(tab) => Some(super::board::Links::of(&tab.app)),
            _ => None,
        })
        .collect();
    let linked = |tab: usize, text: String| match links.get(tab).and_then(Option::as_ref) {
        Some(l) => l.apply(&text),
        None => text,
    };
    let (mut merged, mut reviewed, mut pushed, mut opened) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (index, slot) in g.tabs.iter().enumerate() {
        let app = match &slot.state {
            SlotState::Active => &g.app,
            SlotState::Parked(tab) => &tab.app,
            _ => continue,
        };
        let Some(me) = app.config.gitlab_username.as_deref() else {
            continue;
        };
        let project = app
            .project_name
            .clone()
            .unwrap_or_else(|| app.project_id.clone());
        let project = several.then_some(project.as_str());
        for mr in app.mrs.iter().filter(|mr| !mr.title.is_empty()) {
            let mine = matches_gitlab_username(&mr.author, me) || mr.author == me;
            let text = linked(index, line(&mr.id, &mr.title, project));
            if mine {
                if mr.state == GitlabMrState::Merged && after(mr.merged_at.as_deref()) {
                    merged.push(text);
                } else if after(mr.created_at.as_deref()) {
                    opened.push(text);
                } else if mr.state == GitlabMrState::Opened
                    && after(mr.review.as_ref().and_then(|r| r.last_push_at.as_deref()))
                {
                    pushed.push(text);
                }
            } else if after(
                mr.review
                    .as_ref()
                    .and_then(|r| r.last_activity_by.get(me))
                    .map(String::as_str),
            ) {
                reviewed.push(text);
            }
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "**{}**",
        tf(
            "Stand-up · {date}",
            &[("date", &now.format("%Y-%m-%d").to_string())]
        )
    );
    let section = |out: &mut String, title: &str, lines: &[String]| {
        if lines.is_empty() {
            return;
        }
        let _ = writeln!(out, "{title}");
        for l in lines {
            let _ = writeln!(out, "- {l}");
        }
    };
    let _ = writeln!(
        out,
        "\n_{}_",
        tf(
            "Since {day}",
            &[(
                "day",
                &since
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d")
                    .to_string()
            )]
        )
    );
    let done_before = out.len();
    section(&mut out, tl("Merged:"), &merged);
    section(&mut out, tl("Reviewed:"), &reviewed);
    section(&mut out, tl("Pushed to:"), &pushed);
    section(&mut out, tl("Opened:"), &opened);
    if out.len() == done_before {
        let _ = writeln!(out, "- {}", tl("Nothing on GitLab."));
    }
    let _ = writeln!(out, "\n_{}_", tl("Today"));
    let today_before = out.len();
    let with_reasons = |kind: Kind| -> Vec<String> {
        items
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| {
                let base = linked(
                    i.tab,
                    line(&i.id, &i.title, several.then_some(i.project.as_str())),
                );
                if i.reasons.is_empty() {
                    base
                } else {
                    format!("{base} — {}", i.reasons.join(", "))
                }
            })
            .collect()
    };
    section(&mut out, tl("To review:"), &with_reasons(Kind::Review));
    section(&mut out, tl("To fix:"), &with_reasons(Kind::Fix));
    section(&mut out, tl("To merge:"), &with_reasons(Kind::Ready));
    if out.len() == today_before {
        let _ = writeln!(out, "- {}", tl("Nothing waits for me."));
    }
    let waiting = with_reasons(Kind::Waiting);
    if !waiting.is_empty() {
        let _ = writeln!(out, "\n_{}_", tl("Waiting on others"));
        for l in waiting {
            let _ = writeln!(out, "- {l}");
        }
    }
    out
}

/// A line of the feed: about which MR (and tab), what, when.
struct Change {
    tab: usize,
    project: String,
    id: String,
    title: String,
    text: String,
    color: egui::Color32,
    /// `None`: found at startup (while the app was closed).
    when: Option<chrono::DateTime<chrono::Local>>,
}

/// What changed: while the app was closed (each tab's startup), then during the
/// session (the bell's notifications), newest first.
fn feed(g: &GuiApp) -> Vec<Change> {
    use crate::app::ChangeKind;
    let t = &g.theme.tones;
    let mut session = Vec::new();
    let mut startup = Vec::new();
    for (index, slot) in g.tabs.iter().enumerate() {
        let (app, toasts) = match &slot.state {
            SlotState::Active => (&g.app, &g.toasts),
            SlotState::Parked(tab) => (&tab.app, &tab.toasts),
            _ => continue,
        };
        let project = app
            .project_name
            .clone()
            .unwrap_or_else(|| app.project_id.clone());
        for past in &toasts.history {
            if g.feed_seen_at.is_some_and(|seen| past.when <= seen) {
                continue;
            }
            session.push(Change {
                tab: index,
                project: project.clone(),
                id: past.id.clone(),
                title: past.title.clone(),
                text: past.text.clone(),
                color: past.tone.fg,
                when: Some(past.when),
            });
        }
        for change in &app.last_changes {
            let (text, color) = match &change.kind {
                ChangeKind::Merged => (tl("merged").to_string(), t.purple.fg),
                ChangeKind::Closed => (tl("closed").to_string(), t.gray.fg),
                ChangeKind::Arrived(branch) => {
                    (tf("reached {branch}", &[("branch", branch)]), t.green.fg)
                }
                ChangeKind::CiFailing => (tl("pipeline failed").to_string(), t.red.fg),
            };
            startup.push(Change {
                tab: index,
                project: project.clone(),
                id: change.id.clone(),
                title: change.title.clone(),
                text: format!("!{} {text}", change.id),
                color,
                when: None,
            });
        }
    }
    session.sort_by_key(|c| std::cmp::Reverse(c.when));
    session.extend(startup);
    session
}

/// The feed in a card; the MR clicked, with its tab.
/// What a click in the feed asked for.
enum FeedClick {
    Open(usize, String),
    Seen,
}

fn feed_card(ui: &mut Ui, theme: &Theme, changes: &[Change], several: bool) -> Option<FeedClick> {
    let mut picked = None;
    card(ui, theme, tl("Since your last visit"), |ui| {
        if changes.is_empty() {
            ui.label(muted(
                theme,
                tl("Nothing changed on your tracked MRs since the app was last opened."),
            ));
            return;
        }
        let mut heading = None;
        for change in changes {
            let group = if change.when.is_some() {
                tl("This session")
            } else {
                tl("While the app was closed")
            };
            if heading != Some(group) {
                if heading.is_some() {
                    ui.add_space(6.0);
                }
                ui.label(muted(theme, group).size(theme.text_size - 2.0));
                ui.add_space(2.0);
                heading = Some(group);
            }
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            ui.painter().circle_filled(
                egui::pos2(rect.left() + 8.0, rect.top() + 12.0),
                4.0,
                change.color,
            );
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(
                        rect.shrink2(Vec2::new(0.0, 3.0))
                            .with_min_x(rect.left() + 20.0),
                    )
                    .layout(Layout::top_down(Align::Min)),
            );
            inner.style_mut().interaction.selectable_labels = false;
            inner.spacing_mut().item_spacing.y = 1.0;
            inner.add(
                egui::Label::new(
                    RichText::new(&change.text).font(fonts::medium(theme.text_size - 1.0)),
                )
                .truncate(),
            );
            let mut detail = change.title.clone();
            if several {
                detail = format!("{} · {detail}", change.project);
            }
            if let Some(when) = change.when {
                detail = format!("{} · {detail}", when.format("%H:%M"));
            }
            inner
                .add(egui::Label::new(muted(theme, detail).size(theme.text_size - 2.0)).truncate());
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tl("Show it in its project's tab"))
                .clicked()
            {
                picked = Some(FeedClick::Open(change.tab, change.id.clone()));
            }
        }
        ui.add_space(8.0);
        if super::widgets::button(ui, theme, None, tl("Mark all as seen"), false).clicked() {
            picked = Some(FeedClick::Seen);
        }
    });
    picked
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

/// The project and id columns of a row.
const PROJECT_WIDTH: f32 = 92.0;
const ID_WIDTH: f32 = 44.0;

/// What a click on a row asked for.
enum RowClick {
    Open,
    /// The MR's page, on this tab.
    Page(super::mr_page::Tab),
    /// Its reminder was copied.
    Copied,
}

/// One MR, on two lines as in the mockup: its title, then why it is here in the
/// section's tone; on the right since when it waits (amber when late) and the
/// button that helps (the threads, the new commits, a reminder).
fn row(ui: &mut Ui, theme: &Theme, item: &Item, show_project: bool) -> Option<RowClick> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    // A line between rows, as in the mockup's list.
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    let tone = item.kind.tone(theme);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 11.0), Vec2::new(3.0, 28.0)),
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
    line.spacing_mut().item_spacing.x = 10.0;
    if show_project {
        // A column of its own, so that the titles line up.
        line.allocate_ui_with_layout(
            Vec2::new(PROJECT_WIDTH, 20.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(PROJECT_WIDTH);
                ui.add(
                    Chip::new(&item.project, theme.tones.gray)
                        .font(fonts::medium(theme.text_size - 2.5)),
                );
            },
        );
    }
    line.allocate_ui_with_layout(
        Vec2::new(ID_WIDTH, 20.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(ID_WIDTH);
            ui.label(
                mono(format!("!{}", item.id), theme.text_size - 1.0)
                    .color(theme.palette.text_secondary),
            );
        },
    );
    let (name, _) = split_person(&item.author);
    avatar(&mut line, theme, name, 18.0).on_hover_text(name);
    let mut click = None;
    line.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if let Some(reminder) = &item.reminder {
            if super::widgets::button(ui, theme, None, tl("Copy a reminder"), false)
                .on_hover_text(reminder)
                .clicked()
            {
                ui.ctx().copy_text(reminder.clone());
                click = Some(RowClick::Copied);
            }
        } else if let Some((tab, label)) = item.action {
            if super::widgets::button(ui, theme, None, label, false).clicked() {
                click = Some(RowClick::Page(tab));
            }
        }
        if let Some(since) = &item.since {
            let color = if item.late {
                theme.tones.amber.fg
            } else {
                theme.palette.text_muted
            };
            ui.label(
                RichText::new(since)
                    .size(theme.text_size - 1.5)
                    .color(color),
            )
            .on_hover_text(tl("Since when it waits"));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            super::widgets::centered_block(ui, ("today", &item.project, &item.id), |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&item.title).font(fonts::semibold(theme.text_size)),
                    )
                    .truncate(),
                );
                // Why: the reasons, then the status, in the section's tone.
                let (status, _) = tones::status(item.state, &item.mergeability, &theme.tones);
                let mut why: Vec<String> = item.reasons.clone();
                why.push(status.to_string());
                ui.add(
                    egui::Label::new(
                        RichText::new(why.join(" · "))
                            .size(theme.text_size - 1.5)
                            .color(tone.fg),
                    )
                    .truncate(),
                );
            });
        });
    });
    if click.is_some() {
        return click;
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Show it in its project's tab"))
        .clicked()
        .then_some(RowClick::Open)
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

    #[test]
    fn the_stand_up_looks_back_to_the_last_working_day() {
        use chrono::TimeZone;
        let at = |y, m, d| {
            chrono::Local
                .with_ymd_and_hms(y, m, d, 9, 30, 0)
                .earliest()
                .unwrap()
        };
        let day = |t: chrono::DateTime<chrono::Utc>| t.with_timezone(&chrono::Local).date_naive();
        // Monday 2026-11-09 → Friday; Wednesday → Tuesday.
        assert_eq!(
            day(last_working_day(at(2026, 11, 9))),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
        );
        assert_eq!(
            day(last_working_day(at(2026, 11, 11))),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
        );
    }
}
