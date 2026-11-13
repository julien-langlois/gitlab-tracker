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
use super::widgets::{button, muted, semibold, split_person};
use super::GuiApp;
use crate::app::App;
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{GitlabMrState, PipelineState, TrackedMr};
use gitlab_tracker_core::MergeabilityStatus;

/// The stacks page's choices.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The stack shown, by the id of its first MR; `None`: all of them.
    pub stack: Option<String>,
    /// The MR picked, by id.
    pub selected: Option<String>,
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
fn last_push(mr: &TrackedMr) -> Option<DateTime<Utc>> {
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

/// The stacks among `app`'s MRs, the one with the worst problem first.
pub fn stacks(app: &App) -> Vec<Stack> {
    let mrs: Vec<&TrackedMr> = app
        .mrs
        .iter()
        .filter(|mr| !mr.title.is_empty() && mr.state != GitlabMrState::Closed)
        .collect();
    // An MR's parent: the MR whose source branch it targets, an open one first.
    let parent_of = |mr: &TrackedMr| -> Option<&TrackedMr> {
        let mut found: Vec<&&TrackedMr> = mrs
            .iter()
            .filter(|p| p.id != mr.id && !p.source_branch.is_empty())
            .filter(|p| p.source_branch == mr.target_branch)
            .collect();
        found.sort_by_key(|p| (p.state != GitlabMrState::Opened, p.id.clone()));
        found.first().map(|p| **p)
    };
    let mut parents: HashMap<String, &TrackedMr> = HashMap::new();
    let mut children: BTreeMap<String, Vec<&TrackedMr>> = BTreeMap::new();
    for mr in mrs.iter().filter(|mr| mr.state == GitlabMrState::Opened) {
        if let Some(p) = parent_of(mr) {
            parents.insert(mr.id.clone(), p);
            children.entry(p.id.clone()).or_default().push(mr);
        }
    }
    // The first MR of a stack: open with children and no open parent, or open
    // on a merged parent's branch. A merged parent leads its stack, dimmed.
    let mut roots: Vec<&TrackedMr> = Vec::new();
    for mr in &mrs {
        let parent = parents.get(&mr.id);
        let open_parent = parent.is_some_and(|p| p.state == GitlabMrState::Opened);
        let leads = match mr.state {
            GitlabMrState::Opened => {
                !open_parent && (children.contains_key(&mr.id) || parent.is_some())
            }
            // A merged MR leads only when an open one still targets its branch.
            GitlabMrState::Merged => children.contains_key(&mr.id),
            GitlabMrState::Closed => false,
        };
        // An open MR on a merged parent is shown under it.
        let under_merged = mr.state == GitlabMrState::Opened
            && parent.is_some_and(|p| p.state == GitlabMrState::Merged);
        if leads && !under_merged {
            roots.push(mr);
        }
    }
    let mut out = Vec::new();
    for root in roots {
        // A merged first MR is on its target already: the stack ends there too.
        let base = root.target_branch.clone();
        let mut nodes = Vec::new();
        let mut todo = vec![(root, 0_usize)];
        while let Some((mr, depth)) = todo.pop() {
            let parent = parents.get(&mr.id).copied().filter(|_| depth > 0);
            let mut problems = Vec::new();
            if let Some(p) = parent {
                if p.state == GitlabMrState::Merged {
                    problems.push(Problem::Retarget {
                        parent: p.id.clone(),
                        base: p.target_branch.clone(),
                    });
                } else {
                    if let (Some(pp), Some(own)) = (last_push(p), last_push(mr)) {
                        if pp > own {
                            problems.push(Problem::Rebase {
                                parent: p.id.clone(),
                            });
                        }
                    }
                    if let Some(why) = holding(p) {
                        problems.push(Problem::HeldBy {
                            parent: p.id.clone(),
                            why,
                        });
                    }
                }
            }
            let node = Node {
                id: mr.id.clone(),
                title: mr.title.clone(),
                author: mr.author.clone(),
                source: mr.source_branch.clone(),
                target: mr.target_branch.clone(),
                state: mr.state,
                mergeability: mr.mergeability,
                approved: !mr.approved_by.is_empty(),
                ci_failed: mr
                    .pipelines
                    .first()
                    .is_some_and(|p| p.status == PipelineState::Failed),
                behind: mr.diff_stats.as_ref().and_then(|d| d.commits_behind),
                threads: mr.user_notes_count,
                depth,
                parent: parent.map(|p| p.id.clone()),
                problems,
            };
            if node.ready() && parent.is_some_and(|p| p.state == GitlabMrState::Opened) {
                let parent = node.parent.clone().unwrap_or_default();
                let mut node = node;
                node.problems.push(Problem::LandsInParent { parent });
                nodes.push(node);
            } else {
                nodes.push(node);
            }
            // Children in id order: pushed reversed onto the stack.
            if let Some(kids) = children.get(&mr.id) {
                let mut kids = kids.clone();
                kids.sort_by(|a, b| b.id.cmp(&a.id));
                for kid in kids {
                    todo.push((kid, depth + 1));
                }
            }
        }
        for node in &mut nodes {
            node.problems.sort();
        }
        let stack = Stack { base, nodes };
        if stack.open() >= 1 && stack.nodes.len() >= 2 {
            out.push(stack);
        }
    }
    out.sort_by(|a, b| {
        let worst = |s: &Stack| s.worst().cloned();
        match (worst(a), worst(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then(a.root().id.cmp(&b.root().id))
    });
    out
}

// ── The page ─────────────────────────────────────────────────────────────────

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let all = stacks(&g.app);
    if g.stacks
        .stack
        .as_ref()
        .is_some_and(|s| !all.iter().any(|st| &st.root().id == s))
    {
        g.stacks.stack = None;
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

fn stack_tone(stack: &Stack, theme: &Theme) -> Color32 {
    stack
        .worst()
        .map_or(theme.tones.green.fg, |p| p.tone(theme).fg)
}

fn nav(g: &mut GuiApp, ui: &mut Ui, all: &[Stack]) {
    let theme = g.theme;
    let mrs: usize = all.iter().map(Stack::open).sum();
    board::nav_header(
        ui,
        &theme,
        tl("Stacked MRs"),
        &format!(
            "{} · {}",
            tn(all.len() as u64, "{n} stack", "{n} stacks"),
            tn(mrs as u64, "{n} open MR", "{n} open MRs")
        ),
    );
    let mut picked = None;
    board::nav_group(ui, &theme, tl("Stacks"), |ui| {
        if board::nav_item(
            ui,
            &theme,
            tl("All the stacks"),
            &all.len().to_string(),
            None,
            g.stacks.stack.is_none(),
        )
        .clicked()
        {
            picked = Some(None);
        }
        for stack in all {
            let root = stack.root();
            let label = format!("!{} {}", root.id, root.title);
            if board::nav_item(
                ui,
                &theme,
                &label,
                &stack.nodes.len().to_string(),
                Some(stack_tone(stack, &theme)),
                g.stacks.stack.as_deref() == Some(root.id.as_str()),
            )
            .clicked()
            {
                picked = Some(Some(root.id.clone()));
            }
        }
    });
    if let Some(stack) = picked {
        g.stacks.stack = stack;
    }
    board::nav_note(
        ui,
        &theme,
        tl("A stack is an MR opened on another MR's branch, to split a change into steps that review well. They merge from the top: each one after the one it sits on."),
    );
}

fn main(g: &mut GuiApp, ui: &mut Ui, all: &[Stack]) {
    let theme = g.theme;
    board::heading(
        ui,
        &theme,
        tl("Stacked MRs"),
        tl("MRs built on each other, the order to merge them, and what is out of step"),
        |_| {},
    );
    ui.add_space(14.0);
    let count = |pred: fn(&Problem) -> bool| {
        all.iter()
            .flat_map(|s| &s.nodes)
            .filter(|n| n.problems.iter().any(pred))
            .count()
    };
    let retarget = count(|p| matches!(p, Problem::Retarget { .. }));
    let rebase = count(|p| matches!(p, Problem::Rebase { .. }));
    let deepest = all
        .iter()
        .map(|s| s.nodes.iter().map(|n| n.depth + 1).max().unwrap_or(0))
        .max()
        .unwrap_or(0);
    let t = &theme.tones;
    let figures = [
        Figure {
            label: tl("Stacks").into(),
            value: all.len().to_string(),
            value_color: None,
            caption: tn(
                deepest as u64,
                "{n} MR deep at most",
                "{n} MRs deep at most",
            ),
            caption_color: None,
        },
        Figure {
            label: tl("MRs in a stack").into(),
            value: all.iter().map(Stack::open).sum::<usize>().to_string(),
            value_color: None,
            caption: tl("open").to_string(),
            caption_color: None,
        },
        Figure {
            label: tl("To retarget").into(),
            value: retarget.to_string(),
            value_color: (retarget > 0).then_some(t.red.fg),
            caption: tl("their parent is merged").to_string(),
            caption_color: None,
        },
        Figure {
            label: tl("To rebase").into(),
            value: rebase.to_string(),
            value_color: (rebase > 0).then_some(t.orange.fg),
            caption: tl("their parent changed since").to_string(),
            caption_color: None,
        },
    ];
    board::figures(ui, &theme, &figures);
    if all.is_empty() {
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("No tracked MR targets another MR's branch: no stack to follow."),
            );
        });
        return;
    }
    let shown: Vec<&Stack> = all
        .iter()
        .filter(|s| g.stacks.stack.as_ref().is_none_or(|id| &s.root().id == id))
        .collect();
    let mut picked = None;
    let mut open = None;
    for stack in shown {
        let root = stack.root();
        board::card(
            ui,
            &theme,
            &format!("!{} {}", root.id, root.title),
            &tf(
                "into {base} · {n}",
                &[
                    ("base", &stack.base),
                    ("n", &tn(stack.nodes.len() as u64, "{n} MR", "{n} MRs")),
                ],
            ),
            0.0,
            |ui| {
                stack_body(
                    ui,
                    &theme,
                    stack,
                    g.stacks.selected.as_deref(),
                    &mut picked,
                    &mut open,
                );
            },
        );
        ui.add_space(GAP);
    }
    if let Some(id) = picked {
        g.stacks.selected = Some(id);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

/// Width of a level of the tree.
const INDENT: f32 = 26.0;

/// The base branch, then each MR under the one it sits on, joined by lines.
fn stack_body(
    ui: &mut Ui,
    theme: &Theme,
    stack: &Stack,
    selected: Option<&str>,
    picked: &mut Option<String>,
    open: &mut Option<String>,
) {
    // The base branch.
    let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let w = board::paint_tag(
        ui,
        theme,
        egui::pos2(line.left(), line.center().y),
        &stack.base,
        theme.tones.green,
        true,
    );
    board::paint_text(
        ui,
        egui::pos2(line.left() + w + 10.0, line.center().y),
        Align2::LEFT_CENTER,
        tl("where the stack ends"),
        FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
        line.width() - w - 10.0,
    );
    let mut anchors: HashMap<String, egui::Pos2> = HashMap::new();
    let base_anchor = egui::pos2(line.left() + 10.0, line.bottom());
    let mut order = 0;
    for node in &stack.nodes {
        let height = if node.problems.is_empty() { 50.0 } else { 70.0 };
        let (rect, response) = board::row(ui, theme, height, selected == Some(node.id.as_str()));
        let x = rect.left() + 10.0 + node.depth as f32 * INDENT;
        let dot = egui::pos2(x, rect.top() + 17.0);
        // The line from the parent (or the base) to this MR.
        let from = node
            .parent
            .as_ref()
            .and_then(|p| anchors.get(p).copied())
            .unwrap_or(base_anchor);
        let line_color = theme.palette.border;
        ui.painter().line_segment(
            [from, egui::pos2(from.x, dot.y)],
            Stroke::new(1.5, line_color),
        );
        if (from.x - dot.x).abs() > 0.5 {
            ui.painter().line_segment(
                [egui::pos2(from.x, dot.y), dot],
                Stroke::new(1.5, line_color),
            );
        }
        anchors.insert(node.id.clone(), egui::pos2(dot.x, dot.y + 6.0));
        let merged = node.state == GitlabMrState::Merged;
        let dot_color = if merged {
            theme.tones.purple.fg
        } else if let Some(p) = node.problems.first() {
            p.tone(theme).fg
        } else if node.ready() {
            theme.tones.green.fg
        } else {
            theme.tones.blue.fg
        };
        ui.painter()
            .circle(dot, 6.0, dot_color, Stroke::new(2.0, theme.palette.panel));
        // First line: the merge order, the id, the title, the author.
        let left = x + 16.0;
        let order_text = if merged {
            tl("merged").to_string()
        } else {
            order += 1;
            format!("{order}.")
        };
        let ow = board::paint_text(
            ui,
            egui::pos2(left, dot.y),
            Align2::LEFT_CENTER,
            &order_text,
            fonts::semibold(theme.text_size - 1.5),
            theme.palette.text_muted,
            60.0,
        );
        let iw = board::paint_tag(
            ui,
            theme,
            egui::pos2(left + ow + 8.0, dot.y),
            &format!("!{}", node.id),
            board::id_tone(theme),
            true,
        );
        let author = split_person(&node.author).0;
        let aw = board::paint_text(
            ui,
            egui::pos2(rect.right() - 10.0, dot.y),
            Align2::RIGHT_CENTER,
            author,
            FontId::proportional(theme.text_size - 1.5),
            theme.palette.text_muted,
            140.0,
        );
        let title_x = left + ow + iw + 16.0;
        board::paint_text(
            ui,
            egui::pos2(title_x, dot.y),
            Align2::LEFT_CENTER,
            &node.title,
            FontId::proportional(theme.text_size - 0.5),
            if merged {
                theme.palette.text_muted
            } else {
                theme.palette.text
            },
            rect.right() - 20.0 - aw - title_x,
        );
        // Second line: the branch, then its state.
        let y2 = rect.top() + 38.0;
        let mut cx = left;
        cx += board::paint_text(
            ui,
            egui::pos2(cx, y2),
            Align2::LEFT_CENTER,
            &node.source,
            FontId::monospace(theme.text_size - 2.5),
            theme.palette.text_muted,
            (rect.right() - cx) * 0.4,
        ) + 10.0;
        for (text, tone) in state_tags(node, theme) {
            if cx > rect.right() - 60.0 {
                break;
            }
            cx += board::paint_tag_within(
                ui,
                theme,
                egui::pos2(cx, y2),
                &text,
                tone,
                false,
                rect.right() - 10.0 - cx,
            ) + 6.0;
        }
        // Third line: the worst problem.
        if let Some(problem) = node.problems.first() {
            let y3 = rect.top() + 58.0;
            let extra = node.problems.len() - 1;
            let text = if extra > 0 {
                format!("{} (+{extra})", problem.text())
            } else {
                problem.text()
            };
            let w = board::paint_text(
                ui,
                egui::pos2(left, y3),
                Align2::LEFT_CENTER,
                &text,
                fonts::medium(theme.text_size - 1.5),
                problem.tone(theme).fg,
                rect.right() - 10.0 - left,
            );
            // The parent it names, by its title.
            ui.interact(
                egui::Rect::from_min_size(egui::pos2(left, y3 - 9.0), Vec2::new(w, 18.0)),
                ui.scope_id().with(("problem", node.id.as_str())),
                Sense::hover(),
            )
            .on_hover_text(stack.named(problem.parent()));
        }
        if super::widgets::double_clicked(ui, &response, ("stack", &node.id)) {
            *open = Some(node.id.clone());
        } else if response.clicked() {
            *picked = Some(node.id.clone());
        }
    }
}

/// The short tags of an MR's state: approved, CI, conflicts, behind, threads.
fn state_tags(node: &Node, theme: &Theme) -> Vec<(String, Tone)> {
    let t = &theme.tones;
    let mut out = Vec::new();
    if node.state == GitlabMrState::Merged {
        out.push((tc("state", "Merged").to_string(), t.purple));
        return out;
    }
    match node.mergeability {
        MergeabilityStatus::Draft => out.push((tc("state", "Draft").to_string(), t.gray)),
        MergeabilityStatus::Conflict => out.push((tl("Conflict").to_string(), t.red)),
        _ => {}
    }
    if node.ci_failed {
        out.push((tl("CI failed").to_string(), t.red));
    }
    if node.approved {
        out.push((tl("Approved").to_string(), t.green));
    }
    if let Some(n) = node.behind.filter(|n| *n > 0) {
        out.push((tn(u64::from(n), "{n} behind", "{n} behind"), t.amber));
    }
    if node.threads > 0 {
        out.push((
            tn(u64::from(node.threads), "{n} thread", "{n} threads"),
            t.amber,
        ));
    }
    out
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(g: &mut GuiApp, ui: &mut Ui, all: &[Stack]) {
    let theme = g.theme;
    let found = g.stacks.selected.as_deref().and_then(|id| {
        all.iter()
            .find_map(|s| s.nodes.iter().find(|n| n.id == id).map(|n| (s, n)))
    });
    let Some((stack, node)) = found else {
        board::inspector_empty(
            ui,
            &theme,
            tl("Pick an MR of a stack: where it sits, what it waits on, and the order to merge the stack."),
        );
        return;
    };
    if board::inspector_title(ui, &theme, &format!("!{}", node.id)) {
        g.stacks.selected = None;
        return;
    }
    let mut open = false;
    board::section(ui, &theme, "", "", |ui| {
        ui.add(egui::Label::new(semibold(&node.title, theme.text_size)).wrap());
        ui.add_space(4.0);
        ui.label(muted(&theme, split_person(&node.author).0));
        ui.add_space(8.0);
        open = button(ui, &theme, None, tl("Open the MR"), false).clicked();
    });
    board::section(ui, &theme, tl("Where it sits"), "", |ui| {
        let open_nodes: Vec<&Node> = stack
            .nodes
            .iter()
            .filter(|n| n.state == GitlabMrState::Opened)
            .collect();
        let position = open_nodes
            .iter()
            .position(|n| n.id == node.id)
            .map(|i| {
                tf(
                    "{i} of {n}",
                    &[
                        ("i", &(i + 1).to_string()),
                        ("n", &open_nodes.len().to_string()),
                    ],
                )
            })
            .unwrap_or_else(|| tl("merged").to_string());
        board::facts(
            ui,
            &theme,
            &[
                (tl("Branch"), node.source.clone()),
                (tl("Targets"), node.target.clone()),
                (tl("Stack ends in"), stack.base.clone()),
                (tl("Merge order"), position),
                (
                    tl("Sits on"),
                    node.parent
                        .as_ref()
                        .map_or_else(|| stack.base.clone(), |p| stack.named(p)),
                ),
            ],
        );
    });
    if !node.problems.is_empty() {
        board::section(ui, &theme, tl("To look at"), "", |ui| {
            for problem in &node.problems {
                let tone = problem.tone(&theme);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    super::widgets::dot(ui, tone.fg, 8.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(problem.text())
                                .size(theme.text_size - 1.0)
                                .color(if problem.is_action() {
                                    theme.palette.text
                                } else {
                                    theme.palette.text_secondary
                                }),
                        )
                        .wrap(),
                    )
                    .on_hover_text(stack.named(problem.parent()));
                });
                ui.add_space(4.0);
            }
        });
    }
    board::section(
        ui,
        &theme,
        tl("Merge order"),
        tl("each after the one it sits on"),
        |ui| {
            let mut i = 0;
            for n in &stack.nodes {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                let merged = n.state == GitlabMrState::Merged;
                let label = if merged {
                    "✓".to_string()
                } else {
                    i += 1;
                    format!("{i}.")
                };
                let color = if n.id == node.id {
                    theme.palette.text
                } else {
                    theme.palette.text_secondary
                };
                board::paint_text(
                    ui,
                    egui::pos2(line.left(), line.center().y),
                    Align2::LEFT_CENTER,
                    &label,
                    fonts::semibold(theme.text_size - 1.5),
                    if merged {
                        theme.tones.purple.fg
                    } else {
                        theme.palette.text_muted
                    },
                    22.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(line.left() + 26.0, line.center().y),
                    Align2::LEFT_CENTER,
                    &format!("!{} {}", n.id, n.title),
                    FontId::proportional(theme.text_size - 1.0),
                    color,
                    line.width() - 26.0,
                );
            }
        },
    );
    if open {
        let id = node.id.clone();
        g.open_mr(&id);
    }
}

// ── The rail's badge ─────────────────────────────────────────────────────────

/// The MRs of a stack to retarget (red) or rebase (orange).
pub fn badge(g: &GuiApp) -> Option<Badge> {
    let all = stacks(&g.app);
    let nodes: Vec<&Node> = all
        .iter()
        .flat_map(|s| &s.nodes)
        .filter(|n| n.problems.iter().any(Problem::is_action))
        .collect();
    if nodes.is_empty() {
        return None;
    }
    let retarget = nodes.iter().any(|n| {
        n.problems
            .iter()
            .any(|p| matches!(p, Problem::Retarget { .. }))
    });
    let ids: Vec<String> = nodes.iter().map(|n| format!("!{}", n.id)).collect();
    Some(Badge {
        n: nodes.len(),
        tone: if retarget {
            g.theme.tones.red
        } else {
            g.theme.tones.orange
        },
        hint: rail_badges::listed(
            tn(
                nodes.len() as u64,
                "{n} stacked MR to retarget or rebase",
                "{n} stacked MRs to retarget or rebase",
            ),
            rail_badges::names(&ids, 4),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_demo_stacks() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let all = stacks(&app);
        // Push notifications ← preferences ← mute, and the PKCE verifier on
        // the merged PKCE MR.
        let iid = crate::demo::iid;
        let chain = all
            .iter()
            .find(|s| s.root().id == iid("push-notifications"))
            .expect("the notifications stack");
        let ids: Vec<String> = chain.nodes.iter().map(|n| n.id.clone()).collect();
        assert_eq!(
            ids,
            [
                iid("push-notifications"),
                iid("notif-preferences"),
                iid("notif-mute")
            ]
        );
        assert_eq!(chain.base, "main");
        let mute = &chain.nodes[2];
        assert_eq!(mute.depth, 2);
        assert_eq!(mute.parent, Some(iid("notif-preferences")));
        let on_merged = all
            .iter()
            .find(|s| s.root().id == iid("oauth-pkce"))
            .expect("the stack on a merged MR");
        assert!(on_merged.nodes[1]
            .problems
            .iter()
            .any(|p| matches!(p, Problem::Retarget { .. })));
        // The stack to retarget comes first.
        assert_eq!(all[0].root().id, iid("oauth-pkce"));
    }
}
