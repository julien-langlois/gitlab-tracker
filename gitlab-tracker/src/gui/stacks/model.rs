//! The stacks among the tracked MRs: each MR on its parent, and what is out of step.

use super::*;

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
                own_commits: mr.commits.len(),
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
