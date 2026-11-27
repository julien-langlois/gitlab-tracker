//! The git commands that bring a stack back in step: each MR to retarget or
//! rebase, and every MR stacked on one of them, moved in merge order. The app
//! only reads: it hands the commands over, whoever copies them runs them.
//!
//! Each branch moves its own commits only (`HEAD~n`, n the MR's own commits as
//! GitLab lists them), so it works whether the parent's branch still exists or
//! was deleted at its merge, and on a tree of MRs as on a chain.

use super::*;

/// The commands for `stack`, or `None` when no MR of it has to move.
pub fn commands(stack: &Stack) -> Option<String> {
    let mut moved: Vec<&Node> = Vec::new();
    for node in &stack.nodes {
        if node.state != GitlabMrState::Opened {
            continue;
        }
        let parent_moved = node
            .parent
            .as_ref()
            .is_some_and(|p| moved.iter().any(|m| &m.id == p));
        if parent_moved || node.problems.iter().any(Problem::is_action) {
            moved.push(node);
        }
    }
    if moved.is_empty() {
        return None;
    }
    let mut out = vec!["git fetch origin".to_string()];
    let mut retargets = Vec::new();
    for node in &moved {
        let parent = node
            .parent
            .as_ref()
            .and_then(|p| stack.nodes.iter().find(|n| &n.id == p));
        let parent_moved = parent.is_some_and(|p| moved.iter().any(|m| m.id == p.id));
        let retarget = node.problems.iter().find_map(|p| match p {
            Problem::Retarget { base, .. } => Some(base.as_str()),
            _ => None,
        });
        let (onto, why) = match (parent, parent_moved, retarget) {
            (Some(p), true, _) => (
                p.source.clone(),
                tf(
                    "!{id}: onto !{parent}, moved above",
                    &[("id", &node.id), ("parent", &p.id)],
                ),
            ),
            (Some(p), false, Some(base)) => (
                format!("origin/{base}"),
                tf(
                    "!{id}: its parent !{parent} is merged, onto {base}",
                    &[("id", &node.id), ("parent", &p.id), ("base", base)],
                ),
            ),
            (Some(p), false, None) => (
                format!("origin/{}", p.source),
                tf(
                    "!{id}: onto the last commits of !{parent}",
                    &[("id", &node.id), ("parent", &p.id)],
                ),
            ),
            // A moved MR always sits on a parent of the stack.
            (None, ..) => continue,
        };
        out.push(String::new());
        out.push(format!("# {why}"));
        if node.own_commits == 0 {
            out.push(format!(
                "# {}",
                tl("its commits are not known yet: rebase it by hand")
            ));
            continue;
        }
        out.push(format!("git switch {}", node.source));
        out.push(format!("git merge --ff-only origin/{}", node.source));
        out.push(format!(
            "git rebase --onto {onto} HEAD~{}",
            node.own_commits
        ));
        if let Some(base) = retarget.filter(|_| !parent_moved) {
            retargets.push((node.id.clone(), base.to_string()));
        }
    }
    let branches: Vec<&str> = moved
        .iter()
        .filter(|n| n.own_commits > 0)
        .map(|n| n.source.as_str())
        .collect();
    if !branches.is_empty() {
        out.push(String::new());
        out.push(format!(
            "git push --force-with-lease origin {}",
            branches.join(" ")
        ));
    }
    for (id, base) in retargets {
        out.push(format!(
            "# {}",
            tf(
                "then set the target branch of !{id} to {base} in GitLab (Edit › Target branch)",
                &[("id", &id), ("base", &base)],
            )
        ));
    }
    Some(out.join("\n"))
}
