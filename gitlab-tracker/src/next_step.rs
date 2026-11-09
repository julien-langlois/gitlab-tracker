//! The next step of an MR: who has to act on it now, what they have to do, and
//! since when the MR waits on them.
//!
//! Read from what the refreshes already bring: the mergeability, the latest
//! pipeline, the unresolved threads, the review timeline (review requests, last
//! push, each person's latest comment or approval), the approvals and, once
//! merged, where the MR is on the branches of its flow (see
//! [`crate::branch_flow`]).
//!
//! An open MR goes, in this order, to its author (a draft to finish, conflicts, a
//! rebase, a failed pipeline, threads to answer, changes asked, no reviewer),
//! then to its reviewers (a first review, or a new one after a push), then to
//! whoever merges it (its assignee, else its author). A merged MR waits on a
//! deployment until it reached the last branch of its promotion path.

use chrono::{DateTime, Utc};

use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, MrStatus, MrThread, PipelineState, TrackedMr};
use crate::utils::matches_gitlab_username;
use gitlab_tracker_core::MergeabilityStatus;

/// Who the MR waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Actor {
    /// Its author: something to fix or to answer.
    Author,
    /// Its reviewers: a review, or an approval.
    Reviewers,
    /// Approved and mergeable: whoever merges it.
    Merger,
    /// Merged, not yet on every branch up its flow.
    Deploy,
    /// Nothing left: on every branch of its flow, or closed.
    Nobody,
    /// Not loaded yet, or its fetch failed.
    Unknown,
}

/// The groups the window lists MRs in, from what they wait on and for whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bucket {
    /// I have to act: my MR to fix, a review asked of me, an MR of mine to merge.
    OnMe,
    OnAuthor,
    OnReviewers,
    ToMerge,
    ToDeploy,
    Done,
    Unknown,
}

impl Bucket {
    pub const ALL: [Bucket; 7] = [
        Bucket::OnMe,
        Bucket::OnAuthor,
        Bucket::OnReviewers,
        Bucket::ToMerge,
        Bucket::ToDeploy,
        Bucket::Done,
        Bucket::Unknown,
    ];

    pub fn label(self) -> &'static str {
        tl(match self {
            Bucket::OnMe => "On me",
            Bucket::OnAuthor => "On the author",
            Bucket::OnReviewers => "On the reviewers",
            Bucket::ToMerge => "To merge",
            Bucket::ToDeploy => "To deploy",
            Bucket::Done => "Done",
            Bucket::Unknown => "Not known yet",
        })
    }

    /// The buckets behind a focus key: a bucket's own, or `next:others` (what
    /// waits on someone else than me, before the merge).
    pub fn of_key(key: &str) -> Vec<Bucket> {
        if key == OTHERS_KEY {
            return vec![Bucket::OnAuthor, Bucket::OnReviewers, Bucket::ToMerge];
        }
        Bucket::from_key(key).into_iter().collect()
    }

    pub fn from_key(key: &str) -> Option<Bucket> {
        Bucket::ALL.into_iter().find(|b| b.key() == key)
    }

    /// A stable key, for the focus and the groups.
    pub fn key(self) -> &'static str {
        match self {
            Bucket::OnMe => "next:me",
            Bucket::OnAuthor => "next:author",
            Bucket::OnReviewers => "next:reviewers",
            Bucket::ToMerge => "next:merge",
            Bucket::ToDeploy => "next:deploy",
            Bucket::Done => "next:done",
            Bucket::Unknown => "next:unknown",
        }
    }
}

/// The focus key of the MRs waiting on someone else than me.
pub const OTHERS_KEY: &str = "next:others";

/// One thing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Todo {
    FinishDraft,
    /// Conflicts with the target (`true`), or only behind it.
    Rebase {
        conflicts: bool,
        behind: Option<u32>,
    },
    FixPipeline,
    /// Unresolved threads waiting for the author's answer, by whom they wait.
    AnswerThreads {
        count: usize,
        from: Vec<String>,
    },
    AddressChanges,
    AskReview,
    /// A first review, or a new one after the author pushed (`again`).
    Review {
        again: bool,
    },
    /// Unresolved threads the author answered: back to their authors.
    ReadAnswers {
        count: usize,
    },
    Merge,
    /// Up to the next branch of its flow.
    Promote {
        to: String,
    },
}

impl Todo {
    /// One line, to list the todos.
    pub fn text(&self) -> String {
        match self {
            Todo::FinishDraft => tl("Finish the draft").into(),
            Todo::Rebase {
                conflicts: true, ..
            } => tl("Resolve the conflicts with the target").into(),
            Todo::Rebase {
                behind: Some(n), ..
            } => tn(
                u64::from(*n),
                "Rebase: {n} commit behind the target",
                "Rebase: {n} commits behind the target",
            ),
            Todo::Rebase { .. } => tl("Rebase on the target").into(),
            Todo::FixPipeline => tl("Fix the failed pipeline").into(),
            Todo::AnswerThreads { count, from } if !from.is_empty() => tf(
                "{threads} ({who})",
                &[
                    (
                        "threads",
                        &tn(*count as u64, "Answer {n} thread", "Answer {n} threads"),
                    ),
                    (
                        "who",
                        &from
                            .iter()
                            .map(|p| p.split(" (@").next().unwrap_or(p))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ],
            ),
            Todo::AnswerThreads { count, .. } => {
                tn(*count as u64, "Answer {n} thread", "Answer {n} threads")
            }
            Todo::AddressChanges => tl("Address the changes asked").into(),
            Todo::AskReview => tl("Ask for a review").into(),
            Todo::Review { again: false } => tl("Review it").into(),
            Todo::Review { again: true } => tl("Review the new commits").into(),
            Todo::ReadAnswers { count } => tn(
                *count as u64,
                "Read the answer to {n} thread",
                "Read the answers to {n} threads",
            ),
            Todo::Merge => tl("Merge it").into(),
            Todo::Promote { to } => tf("Deploy to {branch}", &[("branch", to)]),
        }
    }

    /// A word or two, for the table's cell.
    pub fn short(&self) -> String {
        match self {
            Todo::FinishDraft => tl("draft").into(),
            Todo::Rebase {
                conflicts: true, ..
            } => tl("conflicts").into(),
            Todo::Rebase { .. } => tl("rebase").into(),
            Todo::FixPipeline => tl("fix CI").into(),
            Todo::AnswerThreads { count, .. } => tn(*count as u64, "{n} thread", "{n} threads"),
            Todo::AddressChanges => tl("changes asked").into(),
            Todo::AskReview => tl("ask a review").into(),
            Todo::Review { again: false } => tl("review").into(),
            Todo::Review { again: true } => tl("review again").into(),
            Todo::ReadAnswers { count } => tn(*count as u64, "{n} answer", "{n} answers"),
            Todo::Merge => tl("merge").into(),
            Todo::Promote { to } => tf("to {branch}", &[("branch", to)]),
        }
    }
}

/// Where an unresolved thread stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// The author has not answered since its last note.
    Author,
    /// The author's activity is the thread's latest: back to the others.
    Others,
}

/// The next step of an MR.
#[derive(Debug, Clone)]
pub struct NextStep {
    pub actor: Actor,
    /// Who has to act, as display strings ("Full Name (@login)"); empty for a
    /// deployment.
    pub people: Vec<String>,
    /// I am among them.
    pub mine: bool,
    pub todos: Vec<Todo>,
    /// Since when it waits on them.
    pub since: Option<DateTime<Utc>>,
    /// What comes after, in order ("approvals", "merge", the branches…).
    pub then: Vec<String>,
    /// Merged: the last branch of its flow it reached (its target at least).
    pub reached: Option<String>,
    /// Closed without being merged.
    pub closed: bool,
}

impl NextStep {
    pub fn bucket(&self) -> Bucket {
        if self.mine {
            return Bucket::OnMe;
        }
        match self.actor {
            Actor::Author => Bucket::OnAuthor,
            Actor::Reviewers => Bucket::OnReviewers,
            Actor::Merger => Bucket::ToMerge,
            Actor::Deploy => Bucket::ToDeploy,
            Actor::Nobody => Bucket::Done,
            Actor::Unknown => Bucket::Unknown,
        }
    }

    /// Who, in a word: "You", the author's name, "Reviewers", "Deploy"…
    pub fn who(&self) -> String {
        if self.mine {
            return tl("You").into();
        }
        match self.actor {
            Actor::Author | Actor::Merger => self
                .people
                .first()
                .map(|p| first_name(p).to_string())
                .unwrap_or_else(|| tl("Author").into()),
            Actor::Reviewers => match self.people.as_slice() {
                [one] => first_name(one).to_string(),
                [] => tl("Reviewers").into(),
                many => tn(many.len() as u64, "{n} reviewer", "{n} reviewers"),
            },
            Actor::Deploy => tl("Deploy").into(),
            Actor::Nobody if self.closed => tl("Closed").into(),
            Actor::Nobody => tl("Done").into(),
            Actor::Unknown => "—".into(),
        }
    }

    /// The todos in a few words: "rebase, 2 threads".
    pub fn summary(&self) -> String {
        match self.actor {
            Actor::Nobody if self.closed => tl("not merged").into(),
            Actor::Nobody => tl("nothing left").into(),
            Actor::Unknown => String::new(),
            _ => self
                .todos
                .iter()
                .map(Todo::short)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// Days (fractional) it has waited, at `now`.
    pub fn waited_days(&self, now: DateTime<Utc>) -> Option<f64> {
        self.since
            .map(|since| (now - since).num_minutes().max(0) as f64 / (60.0 * 24.0))
    }
}

/// The login of a "Full Name (@login)" display string, else the string.
pub fn login(display: &str) -> &str {
    display
        .split_once("(@")
        .map_or(display, |(_, rest)| rest.trim_end_matches(')'))
}

/// The first word of a display name ("Alex" for "Alex Devries (@alex_dev)").
pub fn first_name(display: &str) -> &str {
    let name = display.split(" (@").next().unwrap_or(display);
    name.split_whitespace().next().unwrap_or(name)
}

fn parse(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn is(display: &str, me: Option<&str>) -> bool {
    me.is_some_and(|me| matches_gitlab_username(display, me) || display == me)
}

/// Whose turn an unresolved `thread` of `mr` is: the author's when nothing of
/// theirs (comment, approval, push) came after the thread's last note.
pub fn thread_turn(mr: &TrackedMr, thread: &MrThread) -> Turn {
    let author = login(&mr.author);
    let Some(last_note) = parse(&thread.last_note_at) else {
        return Turn::Author;
    };
    let review = mr.review.as_ref();
    let acted = review
        .and_then(|r| r.last_activity_by.get(author))
        .and_then(|at| parse(at));
    match acted {
        // Their latest activity is this thread's last note, or came after it.
        Some(at) if at >= last_note => Turn::Others,
        _ => Turn::Author,
    }
}

/// The next step of `mr` in `app`, for `me` (a GitLab username).
pub fn of(app: &App, mr: &TrackedMr, me: Option<&str>) -> NextStep {
    let mut step = NextStep {
        actor: Actor::Unknown,
        people: Vec::new(),
        mine: false,
        todos: Vec::new(),
        since: None,
        then: Vec::new(),
        reached: None,
        closed: false,
    };
    if mr.title.is_empty() || matches!(mr.status, MrStatus::Error(_)) && mr.author.is_empty() {
        return step;
    }
    match mr.state {
        GitlabMrState::Closed => {
            step.actor = Actor::Nobody;
            step.closed = true;
            step.since = mr.updated_at.as_deref().and_then(parse);
            step
        }
        GitlabMrState::Merged => merged(app, mr, step),
        GitlabMrState::Opened => opened(app, mr, me, step),
    }
}

fn merged(app: &App, mr: &TrackedMr, mut step: NextStep) -> NextStep {
    let path = app.branch_flow().path(&mr.target_branch);
    let merged_at = mr.merged_at.as_deref().and_then(parse);
    // The last branch reached up the flow, and when.
    let mut reached_at = merged_at;
    let mut reached = mr.target_branch.clone();
    let mut missing = Vec::new();
    for branch in &path.promotion {
        if !app.branches.contains(branch) {
            continue;
        }
        match app.branch_presence(mr, branch) {
            None => {
                // Loading: the step is not known yet.
                step.actor = Actor::Unknown;
                return step;
            }
            Some(p) if p.is_missing() => missing.push(branch.clone()),
            Some(_) => {
                if missing.is_empty() {
                    reached = branch.clone();
                    if let Some(at) = app.branch_arrivals.get(&(mr.id.clone(), branch.clone())) {
                        reached_at = Some(*at);
                    }
                }
            }
        }
    }
    step.reached = Some(reached);
    match missing.split_first() {
        Some((next, rest)) => {
            step.actor = Actor::Deploy;
            step.todos.push(Todo::Promote { to: next.clone() });
            step.then = rest.to_vec();
            step.since = reached_at;
        }
        None => {
            step.actor = Actor::Nobody;
            step.since = reached_at;
        }
    }
    step
}

fn opened(app: &App, mr: &TrackedMr, me: Option<&str>, mut step: NextStep) -> NextStep {
    let review = mr.review.as_ref();
    let author_login = login(&mr.author).to_string();
    let last_push = review
        .and_then(|r| r.last_push_at.as_deref())
        .and_then(parse);
    let created = mr.created_at.as_deref().and_then(parse);
    let updated = mr.updated_at.as_deref().and_then(parse);

    // The author's part.
    let mut todos = Vec::new();
    let mut since: Vec<DateTime<Utc>> = Vec::new();
    match mr.mergeability {
        MergeabilityStatus::Draft => todos.push(Todo::FinishDraft),
        MergeabilityStatus::Conflict => todos.push(Todo::Rebase {
            conflicts: true,
            behind: mr.diff_stats.as_ref().and_then(|d| d.commits_behind),
        }),
        MergeabilityStatus::NeedsRebase => todos.push(Todo::Rebase {
            conflicts: false,
            behind: mr.diff_stats.as_ref().and_then(|d| d.commits_behind),
        }),
        _ => {}
    }
    if let Some(p) = mr
        .pipelines
        .first()
        .filter(|p| p.status == PipelineState::Failed)
    {
        todos.push(Todo::FixPipeline);
        since.extend(p.created_at.as_deref().and_then(parse));
    }
    let unresolved: Vec<&MrThread> = mr.threads.iter().filter(|t| !t.resolved).collect();
    let (to_answer, answered): (Vec<&MrThread>, Vec<&MrThread>) = unresolved
        .iter()
        .partition(|t| thread_turn(mr, t) == Turn::Author);
    if !to_answer.is_empty() {
        let mut from: Vec<String> = Vec::new();
        for t in &to_answer {
            if t.author != author_login && !from.contains(&t.author) {
                from.push(t.author.clone());
            }
            since.extend(parse(&t.last_note_at));
        }
        let from = from.iter().map(|l| display_of(mr, l)).collect();
        todos.push(Todo::AnswerThreads {
            count: to_answer.len(),
            from,
        });
    } else if mr.threads.is_empty() && mr.user_notes_count > 0 {
        // The threads were not read: their count only.
        todos.push(Todo::AnswerThreads {
            count: mr.user_notes_count as usize,
            from: Vec::new(),
        });
    }
    // Changes asked: the author's until they push after the reviewers' latest word.
    let reviewers_last = mr
        .reviewers
        .iter()
        .filter_map(|r| {
            review
                .and_then(|rv| rv.last_activity_by.get(login(r)))
                .and_then(|at| parse(at))
        })
        .max();
    let pushed_since = matches!((last_push, reviewers_last), (Some(p), Some(r)) if p > r);
    if mr.mergeability == MergeabilityStatus::RequestedChanges && !pushed_since {
        todos.push(Todo::AddressChanges);
    }
    if todos.is_empty() && mr.reviewers.is_empty() && mr.approved_by.is_empty() {
        todos.push(Todo::AskReview);
    }
    if !todos.is_empty() {
        step.actor = Actor::Author;
        step.people = vec![mr.author.clone()];
        step.mine = is(&mr.author, me);
        step.todos = todos;
        step.since = since
            .into_iter()
            .max()
            .or(last_push)
            .or(created)
            .or(updated);
        step.then = after_author(mr);
        return step;
    }

    // The reviewers' part: those who did not approve.
    let approved = |display: &str| {
        let l = login(display);
        mr.approved_by.iter().any(|a| a == l)
    };
    let pending: Vec<&String> = mr.reviewers.iter().filter(|r| !approved(r)).collect();
    let needs_approval = matches!(
        mr.mergeability,
        MergeabilityStatus::NotApproved
            | MergeabilityStatus::Unknown
            | MergeabilityStatus::RequestedChanges
    ) || mr.approved_by.is_empty();
    if !pending.is_empty() && needs_approval || !answered.is_empty() && !pending.is_empty() {
        let again = pending.iter().any(|r| {
            let seen = review
                .and_then(|rv| rv.last_activity_by.get(login(r)))
                .and_then(|at| parse(at));
            matches!((seen, last_push), (Some(seen), Some(push)) if push > seen)
        });
        step.actor = Actor::Reviewers;
        step.people = pending.iter().map(|r| (*r).clone()).collect();
        step.mine = pending.iter().any(|r| is(r, me));
        step.todos.push(Todo::Review { again });
        if !answered.is_empty() {
            step.todos.push(Todo::ReadAnswers {
                count: answered.len(),
            });
        }
        let asked = pending
            .iter()
            .filter_map(|r| {
                review
                    .and_then(|rv| rv.review_requested_at.get(login(r)))
                    .and_then(|at| parse(at))
            })
            .max();
        let answered_at = answered.iter().filter_map(|t| parse(&t.last_note_at)).max();
        step.since = [asked, last_push, answered_at]
            .into_iter()
            .flatten()
            .max()
            .or(created);
        step.then = vec![tl("merge").into()];
        step.then.extend(promotion(app, mr));
        return step;
    }
    if mr.approved_by.is_empty() && mr.mergeability == MergeabilityStatus::NotApproved {
        // Approvals missing, and no reviewer named: the author has to find one.
        step.actor = Actor::Author;
        step.people = vec![mr.author.clone()];
        step.mine = is(&mr.author, me);
        step.todos.push(Todo::AskReview);
        step.since = last_push.or(created);
        step.then = after_author(mr);
        return step;
    }

    // Approved: whoever merges it.
    let merger = mr.assignee.clone().unwrap_or_else(|| mr.author.clone());
    step.actor = Actor::Merger;
    step.mine = is(&merger, me);
    step.people = vec![merger];
    step.todos.push(Todo::Merge);
    step.since = review
        .and_then(|r| r.approved_at.as_deref())
        .and_then(parse)
        .or(updated);
    step.then = promotion(app, mr);
    step
}

/// The display string of `login` among the MR's people, else the login.
fn display_of(mr: &TrackedMr, login_name: &str) -> String {
    std::iter::once(&mr.author)
        .chain(mr.reviewers.iter())
        .chain(mr.assignee.iter())
        .chain(mr.approvers.iter())
        .find(|d| login(d) == login_name)
        .cloned()
        .unwrap_or_else(|| login_name.to_string())
}

fn after_author(mr: &TrackedMr) -> Vec<String> {
    let mut then = Vec::new();
    if mr.approved_by.is_empty() {
        then.push(tl("approvals").into());
    }
    then.push(tl("merge").into());
    then
}

/// The tracked branches above the MR's target, nearest first.
fn promotion(app: &App, mr: &TrackedMr) -> Vec<String> {
    app.branch_flow()
        .path(&mr.target_branch)
        .promotion
        .into_iter()
        .filter(|b| app.branches.contains(b))
        .collect()
}

/// The next step of every MR of `app` with a title, by id.
pub fn all(app: &App) -> std::collections::HashMap<String, NextStep> {
    let me = app.config.gitlab_username.as_deref();
    app.mrs
        .iter()
        .map(|mr| (mr.id.clone(), of(app, mr, me)))
        .collect()
}

/// The median of `values`, `None` when empty.
pub fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// How long a first review usually takes in `app`, in days: the median from
/// creation to the first review, over the MRs that had one.
pub fn usual_first_review_days(app: &App) -> Option<f64> {
    median(
        app.mrs
            .iter()
            .filter_map(|mr| {
                let created = parse(mr.created_at.as_deref()?)?;
                let first = parse(mr.review.as_ref()?.first_review_at.as_deref()?)?;
                Some((first - created).num_minutes().max(0) as f64 / 1440.0)
            })
            .collect(),
    )
}

/// A duration in days, short: "3 h", "1.5 d", "12 d".
pub fn days_text(days: f64) -> String {
    if days < 1.0 {
        let hours = (days * 24.0).round().max(1.0) as u64;
        tf("{n} h", &[("n", &hours.to_string())])
    } else if days < 10.0 {
        let text = format!("{days:.1}");
        let text = text.strip_suffix(".0").unwrap_or(&text);
        tf("{n} d", &[("n", text)])
    } else {
        tf("{n} d", &[("n", &format!("{days:.0}"))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::iid;
    use crate::models::ReviewActivity;

    async fn demo() -> App {
        let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        app.config.gitlab_username = Some("thomas_db".into());
        app
    }

    fn mr<'a>(app: &'a App, id: &str) -> &'a TrackedMr {
        app.mrs.iter().find(|m| m.id == id).unwrap()
    }

    #[tokio::test]
    async fn my_conflicting_mr_waits_on_me() {
        let app = demo().await;
        let step = of(&app, mr(&app, &iid("pool-deadlock")), Some("thomas_db"));
        assert_eq!(step.actor, Actor::Author);
        assert!(step.mine);
        assert_eq!(step.bucket(), Bucket::OnMe);
        assert!(matches!(
            step.todos.first(),
            Some(Todo::Rebase {
                conflicts: true,
                ..
            })
        ));
        // Someone else sees it as the author's.
        let other = of(&app, mr(&app, &iid("pool-deadlock")), Some("nobody"));
        assert_eq!(other.bucket(), Bucket::OnAuthor);
    }

    #[tokio::test]
    async fn a_merged_mr_waits_on_the_deployment_until_the_last_branch() {
        let app = demo().await;
        // On main and staging, not production.
        let step = of(&app, mr(&app, &iid("oauth-pkce")), None);
        assert_eq!(step.actor, Actor::Deploy);
        assert_eq!(
            step.todos,
            vec![Todo::Promote {
                to: "production".into()
            }]
        );
        // On every branch: done.
        assert_eq!(
            of(&app, mr(&app, &iid("eta-timezone")), None).actor,
            Actor::Nobody
        );
        // Closed: nothing left.
        assert_eq!(
            of(&app, mr(&app, &iid("juniper-prototype")), None).actor,
            Actor::Nobody
        );
    }

    #[tokio::test]
    async fn a_review_asked_of_me_after_a_push_is_a_review_again() {
        let mut app = demo().await;
        let now = Utc::now();
        let at = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339();
        let m = app
            .mrs
            .iter_mut()
            .find(|m| m.id == iid("tokio-ecosystem"))
            .unwrap();
        m.reviewers = vec!["Thomas Dubosc (@thomas_db)".into()];
        m.mergeability = MergeabilityStatus::NotApproved;
        m.review = Some(ReviewActivity {
            last_push_at: Some(at(1)),
            last_activity_by: [("thomas_db".to_string(), at(3))].into(),
            ..Default::default()
        });
        let step = of(&app, mr(&app, &iid("tokio-ecosystem")), Some("thomas_db"));
        assert_eq!(step.actor, Actor::Reviewers);
        assert!(step.mine);
        assert_eq!(step.todos, vec![Todo::Review { again: true }]);
        let waited = step.waited_days(now).unwrap();
        assert!((0.9..1.1).contains(&waited), "{waited}");
    }

    #[tokio::test]
    async fn a_thread_answered_by_the_author_goes_back_to_the_reviewer() {
        let mut app = demo().await;
        let now = Utc::now();
        let at = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339();
        let m = app
            .mrs
            .iter_mut()
            .find(|m| m.id == iid("tokio-ecosystem"))
            .unwrap();
        m.reviewers = vec!["Alex Devries (@alex_dev)".into()];
        m.mergeability = MergeabilityStatus::NotApproved;
        let thread = |answered: bool| MrThread {
            note_id: 1,
            author: "alex_dev".into(),
            excerpt: "Why?".into(),
            created_at: at(5),
            last_note_at: at(if answered { 2 } else { 1 }),
            notes: 2,
            resolved: false,
        };
        m.threads = vec![thread(true)];
        m.review = Some(ReviewActivity {
            last_activity_by: [("renovate_bot".to_string(), at(2))].into(),
            ..Default::default()
        });
        let step = of(&app, mr(&app, &iid("tokio-ecosystem")), None);
        assert_eq!(step.actor, Actor::Reviewers);
        assert!(step.todos.contains(&Todo::ReadAnswers { count: 1 }));
        // A later note from the reviewer: the author's turn again.
        let m = app
            .mrs
            .iter_mut()
            .find(|m| m.id == iid("tokio-ecosystem"))
            .unwrap();
        m.threads = vec![thread(false)];
        let step = of(&app, mr(&app, &iid("tokio-ecosystem")), None);
        assert_eq!(step.actor, Actor::Author);
        assert!(
            matches!(&step.todos[0], Todo::AnswerThreads { count: 1, from } if from[0].contains("Alex"))
        );
    }

    #[test]
    fn logins_and_first_names_read_from_display_strings() {
        assert_eq!(login("Alex Devries (@alex_dev)"), "alex_dev");
        assert_eq!(login("alex_dev"), "alex_dev");
        assert_eq!(first_name("Alex Devries (@alex_dev)"), "Alex");
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![1.0, 2.0, 3.0, 4.0]), Some(2.5));
        assert_eq!(median(Vec::new()), None);
    }
}
