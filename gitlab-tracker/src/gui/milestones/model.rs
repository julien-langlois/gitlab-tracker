//! Each milestone's MRs and where they stand: delivered, on the way, to merge, in review, on the author, draft.

use super::*;

/// Where an MR of a milestone stands, from the closest to done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stand {
    Delivered,
    OnTheWay,
    ToMerge,
    InReview,
    OnAuthor,
    Draft,
}

impl Stand {
    pub(super) const ALL: [Stand; 6] = [
        Stand::Delivered,
        Stand::OnTheWay,
        Stand::ToMerge,
        Stand::InReview,
        Stand::OnAuthor,
        Stand::Draft,
    ];

    pub(super) fn label(self, last: &str) -> String {
        match self {
            Stand::Delivered => tf("On {branch}", &[("branch", last)]),
            Stand::OnTheWay => tf("Merged, not on {branch} yet", &[("branch", last)]),
            Stand::ToMerge => tl("Ready to merge").into(),
            Stand::InReview => tl("In review").into(),
            Stand::OnAuthor => tl("Back on their author").into(),
            Stand::Draft => tl("Drafts").into(),
        }
    }

    pub(super) fn short(self) -> &'static str {
        tl(match self {
            Stand::Delivered => "Delivered",
            Stand::OnTheWay => "On the way",
            Stand::ToMerge => "To merge",
            Stand::InReview => "In review",
            Stand::OnAuthor => "On the author",
            Stand::Draft => "Draft",
        })
    }

    pub(super) fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Stand::Delivered => t.green,
            Stand::OnTheWay => t.purple,
            Stand::ToMerge => t.teal,
            Stand::InReview => t.blue,
            Stand::OnAuthor => t.red,
            Stand::Draft => t.gray,
        }
    }
}

/// An MR of a milestone.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub author: String,
    pub stand: Stand,
    pub step: NextStep,
    pub created: Option<DateTime<Utc>>,
    /// When it reached the last branch of the flow.
    pub delivered: Option<DateTime<Utc>>,
    /// Its presence on each tracked branch, in flow order.
    pub branches: Vec<Option<BranchPresence>>,
    pub ticket: Option<(String, Option<u32>, Option<u32>)>,
}

/// A milestone and its MRs.
#[derive(Debug, Clone)]
pub struct Milestone {
    /// Empty for the MRs without a milestone.
    pub title: String,
    pub due: Option<NaiveDate>,
    pub description: Option<String>,
    pub items: Vec<Item>,
}

impl Milestone {
    pub(super) fn delivered(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.stand == Stand::Delivered)
            .count()
    }

    pub(super) fn done(&self) -> bool {
        !self.items.is_empty() && self.delivered() == self.items.len()
    }

    /// Every MR of it is on the last branch.
    pub fn is_done(&self) -> bool {
        self.done()
    }

    pub(super) fn name(&self) -> String {
        if self.title.is_empty() {
            tl("No milestone").to_string()
        } else {
            self.title.clone()
        }
    }
}

/// The last branch of the flow (production), and the tracked branches in order.
pub(in crate::gui) fn flow_of(app: &App) -> (String, Vec<String>) {
    let flow = app.branch_flow();
    let ordered: Vec<String> = flow.in_order().iter().map(|b| b.name.clone()).collect();
    (ordered.last().cloned().unwrap_or_default(), ordered)
}

/// When `mr` reached `branch`: its recorded arrival, else its merge when
/// `branch` is its target.
pub(in crate::gui) fn arrived(app: &App, mr: &TrackedMr, branch: &str) -> Option<DateTime<Utc>> {
    app.branch_arrivals
        .get(&(mr.id.clone(), branch.to_string()))
        .copied()
        .or_else(|| {
            (mr.target_branch == branch)
                .then(|| board::time_of(mr.merged_at.as_deref()))
                .flatten()
        })
}

pub(super) fn stand_of(app: &App, mr: &TrackedMr, step: &NextStep, last: &str) -> Stand {
    let on_last = app
        .branch_presence(mr, last)
        .is_some_and(|p| !p.is_missing());
    if mr.state == GitlabMrState::Merged {
        return if on_last || last.is_empty() {
            Stand::Delivered
        } else {
            Stand::OnTheWay
        };
    }
    if mr.mergeability == MergeabilityStatus::Draft {
        return Stand::Draft;
    }
    match step.actor {
        Actor::Merger | Actor::Gitlab => Stand::ToMerge,
        Actor::Reviewers => Stand::InReview,
        _ => Stand::OnAuthor,
    }
}

/// The milestones of `app`: those GitLab lists and those its MRs carry, with
/// their MRs (closed ones left out), the soonest due first, then the MRs
/// without a milestone.
pub fn milestones(app: &App) -> Vec<Milestone> {
    let me = app.config.gitlab_username.as_deref();
    let (last, ordered) = flow_of(app);
    let mut by_title: BTreeMap<String, Milestone> = BTreeMap::new();
    for m in &app.milestones {
        by_title.insert(
            m.title.clone(),
            Milestone {
                title: m.title.clone(),
                due: m.due_date.as_deref().and_then(|d| d.parse().ok()),
                description: m.description.clone().filter(|d| !d.trim().is_empty()),
                items: Vec::new(),
            },
        );
    }
    for mr in &app.mrs {
        if mr.title.is_empty() || mr.state == GitlabMrState::Closed {
            continue;
        }
        let title = mr.milestone.clone().unwrap_or_default();
        let entry = by_title.entry(title.clone()).or_insert_with(|| Milestone {
            title: title.clone(),
            due: None,
            description: None,
            items: Vec::new(),
        });
        if entry.due.is_none() {
            entry.due = mr
                .milestone_due_date
                .as_deref()
                .and_then(|d| d.parse().ok());
        }
        if entry.description.is_none() {
            entry.description = mr
                .milestone_description
                .clone()
                .filter(|d| !d.trim().is_empty());
        }
        let step = next_step::of(app, mr, me);
        let stand = stand_of(app, mr, &step, &last);
        entry.items.push(Item {
            id: mr.id.clone(),
            title: mr.title.clone(),
            author: mr.author.clone(),
            stand,
            created: board::time_of(mr.created_at.as_deref()),
            delivered: (stand == Stand::Delivered)
                .then(|| {
                    arrived(app, mr, &last).or_else(|| board::time_of(mr.merged_at.as_deref()))
                })
                .flatten(),
            branches: ordered.iter().map(|b| app.branch_presence(mr, b)).collect(),
            ticket: mr
                .linked_ticket
                .as_ref()
                .map(|t| (t.status.clone(), t.time_spent, t.time_estimate)),
            step,
        });
    }
    let mut list: Vec<Milestone> = by_title.into_values().collect();
    for m in &mut list {
        m.items
            .sort_by(|a, b| a.stand.cmp(&b.stand).then(a.id.cmp(&b.id)));
    }
    list.sort_by(|a, b| {
        (a.title.is_empty(), a.due.is_none(), a.due, a.title.clone()).cmp(&(
            b.title.is_empty(),
            b.due.is_none(),
            b.due,
            b.title.clone(),
        ))
    });
    list
}
