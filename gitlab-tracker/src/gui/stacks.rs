//! The stacked MRs page: the MRs opened on top of another MR's branch (its
//! target is the other one's source), as chains from the branch they all end
//! in. For each, the order to merge them, and what goes wrong in a stack: a
//! parent merged while its child still targets its branch, a parent pushed to
//! after its child (a rebase is due), a child ready before its parent (merging
//! it lands in the parent's branch), a parent holding its children back.

use std::collections::{BTreeMap, HashMap};

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Utc};
use egui::{Align2, Color32, FontId, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, semibold, split_person};
use super::GuiApp;
use crate::app::App;
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState, TrackedMr};
use gitlab_tracker_core::MergeabilityStatus;

mod commands;
mod model;
mod page;
mod side_panel;

pub use commands::*;
pub use model::*;
use page::*;
pub use side_panel::*;

/// The stacks page's choices.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The stack shown, by the id of its first MR; `None`: all of them.
    pub stack: Option<String>,
    /// The MR picked, by id.
    pub selected: Option<String>,
    /// Whether an MR was picked for the page's first look (the inspector is
    /// never empty when there is a stack; a closed one stays closed).
    pub opened: bool,
}

/// What goes wrong with an MR of a stack, the worst first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    /// Its parent is merged, it still targets the parent's branch.
    Retarget { parent: String, base: String },
    /// Its parent was pushed to after it: it lacks the parent's last commits.
    Rebase { parent: String },
    /// Its parent is a draft, failing or in conflict: it cannot go before.
    HeldBy { parent: String, why: String },
    /// Ready before its parent: merging it now lands in the parent's branch.
    LandsInParent { parent: String },
}

impl Problem {
    /// The parent MR the problem names.
    fn parent(&self) -> &str {
        match self {
            Problem::Retarget { parent, .. }
            | Problem::Rebase { parent }
            | Problem::HeldBy { parent, .. }
            | Problem::LandsInParent { parent } => parent,
        }
    }

    fn text(&self) -> String {
        match self {
            Problem::Retarget { parent, base } => tf(
                "!{parent} is merged: retarget it to {base}",
                &[("parent", parent), ("base", base)],
            ),
            Problem::Rebase { parent } => tf(
                "!{parent} changed since: rebase it on its branch",
                &[("parent", parent)],
            ),
            Problem::HeldBy { parent, why } => tf(
                "Held back by !{parent}: {why}",
                &[("parent", parent), ("why", why)],
            ),
            Problem::LandsInParent { parent } => tf(
                "Ready before !{parent}: merging it now lands in !{parent}'s branch",
                &[("parent", parent)],
            ),
        }
    }

    fn tone(&self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Problem::Retarget { .. } => t.red,
            Problem::Rebase { .. } => t.orange,
            Problem::HeldBy { .. } => t.gray,
            Problem::LandsInParent { .. } => t.amber,
        }
    }

    /// Something to do (not only to know).
    fn is_action(&self) -> bool {
        matches!(self, Problem::Retarget { .. } | Problem::Rebase { .. })
    }
}

/// An MR of a stack.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub author: String,
    pub source: String,
    pub target: String,
    pub state: GitlabMrState,
    pub mergeability: MergeabilityStatus,
    pub approved: bool,
    pub ci_failed: bool,
    pub behind: Option<u32>,
    pub threads: u32,
    /// 0 for the first MR of the stack.
    pub depth: usize,
    pub parent: Option<String>,
    pub problems: Vec<Problem>,
    /// Its own commits, as GitLab lists them (0: not read yet).
    pub own_commits: usize,
}

impl Node {
    fn ready(&self) -> bool {
        self.state == GitlabMrState::Opened
            && self.approved
            && self.mergeability == MergeabilityStatus::Mergeable
            && !self.ci_failed
    }
}

/// MRs on top of each other, from the one on the base branch.
#[derive(Debug, Clone)]
pub struct Stack {
    /// The branch the whole stack ends in.
    pub base: String,
    /// In merge order: each MR after its parent.
    pub nodes: Vec<Node>,
}

impl Stack {
    fn root(&self) -> &Node {
        &self.nodes[0]
    }

    /// An MR of the stack named by its id and title: "!12 · its title".
    fn named(&self, id: &str) -> String {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .map_or_else(|| format!("!{id}"), |n| format!("!{id} · {}", n.title))
    }

    /// An MR of the stack by its id and title, for a tooltip.
    fn named_tip(&self, id: &str) -> Tip {
        let title = self
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map_or("", |n| &n.title);
        Tip::new().mr(id, title)
    }

    fn open(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.state == GitlabMrState::Opened)
            .count()
    }

    fn worst(&self) -> Option<&Problem> {
        self.nodes.iter().flat_map(|n| &n.problems).min()
    }
}

/// When `mr` last got new commits: its last push, else its newest commit.
pub(in crate::gui) fn last_push(mr: &TrackedMr) -> Option<DateTime<Utc>> {
    mr.review
        .as_ref()
        .and_then(|r| board::time_of(r.last_push_at.as_deref()))
        .or_else(|| {
            mr.commits
                .iter()
                .map(|c| c.fingerprint.authored_at)
                .max()
                .and_then(|s| DateTime::from_timestamp(s, 0))
        })
}

/// Why a parent holds its children back, if it does.
fn holding(mr: &TrackedMr) -> Option<String> {
    if mr.mergeability == MergeabilityStatus::Draft {
        Some(tl("a draft").to_string())
    } else if mr
        .pipelines
        .first()
        .is_some_and(|p| p.status == PipelineState::Failed)
    {
        Some(tl("its pipeline failed").to_string())
    } else if mr.mergeability == MergeabilityStatus::Conflict {
        Some(tl("in conflict").to_string())
    } else if mr.mergeability == MergeabilityStatus::RequestedChanges {
        Some(tl("changes requested").to_string())
    } else {
        None
    }
}

// ── The page ─────────────────────────────────────────────────────────────────

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let all = g.memo.stacks.get(&g.app.project_id, (), || stacks(&g.app));
    if g.stacks
        .stack
        .as_ref()
        .is_some_and(|s| !all.iter().any(|st| &st.root().id == s))
    {
        g.stacks.stack = None;
    }
    if !g.stacks.opened && g.stacks.selected.is_none() {
        if let Some(id) = first_pick(&all) {
            g.stacks.selected = Some(id);
            g.stacks.opened = true;
        }
    }
    board::screen(
        g,
        ui,
        sizes,
        "stacks",
        |g, ui| nav(g, ui, &all),
        |g, ui| inspector(g, ui, &all),
        |g, ui| main(g, ui, &all),
    );
}

/// The MR the inspector shows first: the first open MR with something out of
/// step, else the first open one.
fn first_pick(all: &[Stack]) -> Option<String> {
    let open = || {
        all.iter()
            .flat_map(|s| &s.nodes)
            .filter(|n| n.state == GitlabMrState::Opened)
    };
    open()
        .find(|n| !n.problems.is_empty())
        .or_else(|| open().next())
        .map(|n| n.id.clone())
}

fn stack_tone(stack: &Stack, theme: &Theme) -> Color32 {
    stack
        .worst()
        .map_or(theme.tones.green.fg, |p| p.tone(theme).fg)
}

#[cfg(test)]
mod tests;
