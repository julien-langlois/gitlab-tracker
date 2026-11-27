//! What a promotion completes: the milestones it moves forward on the target
//! branch, and the tickets whose every MR it brings there. Read from the MRs
//! the page already has.

use super::*;

/// A line per milestone the picked MRs move forward on `to` ("v2.4.0: 5 → 8
/// of 14 on production"), then per ticket they bring entirely there.
pub(super) fn completes(app: &App, picked: &[&Row], to: &str) -> Vec<String> {
    let on_to = |mr: &crate::models::TrackedMr| {
        app.branch_presence(mr, to).is_some_and(|p| !p.is_missing())
    };
    let counted: Vec<&crate::models::TrackedMr> = app
        .mrs
        .iter()
        .filter(|m| !m.title.is_empty() && m.state != GitlabMrState::Closed)
        .collect();
    let is_picked = |id: &str| picked.iter().any(|r| r.id == id);
    let mut out = Vec::new();
    // Milestones, in the order the picked MRs name them.
    let mut milestones: Vec<&str> = Vec::new();
    for r in picked {
        let milestone = counted
            .iter()
            .find(|m| m.id == r.id)
            .and_then(|m| m.milestone.as_deref());
        if let Some(name) = milestone.filter(|n| !milestones.contains(n)) {
            milestones.push(name);
        }
    }
    for name in milestones {
        let of: Vec<&&crate::models::TrackedMr> = counted
            .iter()
            .filter(|m| m.milestone.as_deref() == Some(name))
            .collect();
        let before = of.iter().filter(|m| on_to(m)).count();
        let after = of.iter().filter(|m| on_to(m) || is_picked(&m.id)).count();
        out.push(tf(
            "milestone {name}: {before} → {after} of {all} on {to}",
            &[
                ("name", name),
                ("before", &before.to_string()),
                ("after", &after.to_string()),
                ("all", &of.len().to_string()),
                ("to", to),
            ],
        ));
    }
    // Tickets every MR of which reaches `to` with the promotion.
    let mut tickets: Vec<&gitlab_tracker_core::LinkedTicket> = Vec::new();
    for r in picked {
        let ticket = counted
            .iter()
            .find(|m| m.id == r.id)
            .and_then(|m| m.linked_ticket.as_ref());
        if let Some(t) = ticket.filter(|t| !tickets.iter().any(|x| x.id == t.id)) {
            tickets.push(t);
        }
    }
    for t in tickets {
        let all_there = counted
            .iter()
            .filter(|m| m.linked_ticket.as_ref().is_some_and(|x| x.id == t.id))
            .all(|m| on_to(m) || is_picked(&m.id));
        if all_there {
            out.push(tf(
                "ticket #{id} {subject}: every MR of it on {to} ({status} in the tracker)",
                &[
                    ("id", &t.id),
                    ("subject", &t.subject),
                    ("to", to),
                    ("status", &t.status),
                ],
            ));
        }
    }
    out
}
