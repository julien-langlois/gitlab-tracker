//! Tickets out of step with the code: closed while an MR of theirs is open or
//! not on production yet, or still open a week after all their MRs reached
//! production. "Closed" is the tracker's own word (Redmine's `is_closed`
//! statuses, read with the board's columns): a status the tracker does not
//! mark closed, such as Redmine's default "Resolved", is not. Read from the
//! tickets and branches the app already has: nothing fetched, nothing changed
//! in the tracker.

use chrono::{DateTime, Utc};
use gitlab_tracker_core::{LinkedTicket, TicketTransitionTarget};

use super::super::board;
use super::super::milestones::{arrived, flow_of};
use crate::app::App;
use crate::i18n::{tf, tn};
use crate::models::{GitlabMrState, TrackedMr};

/// Days on production after which a ticket still open is out of step: the
/// team's own check of what shipped comes first.
pub const SHIPPED_DAYS: i64 = 7;

/// How a ticket is out of step with its MRs, the most pressing first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Drift {
    /// Closed, while these MRs of it are still open.
    ClosedMrOpen(Vec<String>),
    /// Closed, while these merged MRs of it are not on production yet.
    ClosedNotShipped(Vec<String>),
    /// Every MR of it on production for that many days, the ticket still open.
    ShippedStillOpen(i64),
}

/// A ticket out of step, and how.
#[derive(Debug, Clone)]
pub struct Drifting {
    pub ticket: LinkedTicket,
    pub drift: Drift,
}

impl Drifting {
    /// What is out of step, in a sentence: "Closed, but its MR !223 is still open".
    pub fn text(&self, production: &str) -> String {
        let ids = |mrs: &[String]| {
            mrs.iter()
                .map(|id| format!("!{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let status = &self.ticket.status;
        match &self.drift {
            Drift::ClosedMrOpen(mrs) => tf(
                "{status}, but {mrs} still open",
                &[("status", status), ("mrs", &ids(mrs))],
            ),
            Drift::ClosedNotShipped(mrs) => tf(
                "{status}, but {mrs} not on {branch} yet",
                &[
                    ("status", status),
                    ("mrs", &ids(mrs)),
                    ("branch", production),
                ],
            ),
            Drift::ShippedStillOpen(days) => tf(
                "{status}, but its MRs are on {branch} for {days}",
                &[
                    ("status", status),
                    ("branch", production),
                    ("days", &tn(*days as u64, "{n} day", "{n} days")),
                ],
            ),
        }
    }
}

/// The tickets of `app`'s MRs that are out of step, the most pressing first;
/// none while the tracker's statuses are not known.
pub fn drifts(app: &App, statuses: &[TicketTransitionTarget], now: DateTime<Utc>) -> Vec<Drifting> {
    let closed = |status: &str| {
        statuses
            .iter()
            .any(|s| s.closed && s.label.eq_ignore_ascii_case(status))
    };
    let (production, _) = flow_of(app);
    if production.is_empty() {
        return Vec::new();
    }
    // The tickets, each with its MRs (a closed, unmerged MR does not count).
    let mut tickets: Vec<(&LinkedTicket, Vec<&TrackedMr>)> = Vec::new();
    for mr in app.mrs.iter().filter(|m| m.state != GitlabMrState::Closed) {
        let Some(ticket) = &mr.linked_ticket else {
            continue;
        };
        match tickets.iter_mut().find(|(t, _)| t.id == ticket.id) {
            Some((_, mrs)) => mrs.push(mr),
            None => tickets.push((ticket, vec![mr])),
        }
    }
    let shipped = |mr: &TrackedMr| {
        app.branch_presence(mr, &production)
            .is_some_and(|p| !p.is_missing())
    };
    let mut out: Vec<Drifting> = tickets
        .into_iter()
        .filter_map(|(ticket, mrs)| {
            let ids = |keep: &dyn Fn(&TrackedMr) -> bool| -> Vec<String> {
                mrs.iter()
                    .filter(|m| keep(m))
                    .map(|m| m.id.clone())
                    .collect()
            };
            let drift = if closed(&ticket.status) {
                let open = ids(&|m| m.state == GitlabMrState::Opened);
                let waiting = ids(&|m| m.state == GitlabMrState::Merged && !shipped(m));
                if !open.is_empty() {
                    Drift::ClosedMrOpen(open)
                } else if !waiting.is_empty() {
                    Drift::ClosedNotShipped(waiting)
                } else {
                    return None;
                }
            } else {
                if !mrs
                    .iter()
                    .all(|m| m.state == GitlabMrState::Merged && shipped(m))
                {
                    return None;
                }
                // Since the last of them arrived (else was merged).
                let since = mrs
                    .iter()
                    .filter_map(|m| {
                        arrived(app, m, &production)
                            .or_else(|| board::time_of(m.merged_at.as_deref()))
                    })
                    .max()?;
                let days = (now - since).num_days();
                if days < SHIPPED_DAYS {
                    return None;
                }
                Drift::ShippedStillOpen(days)
            };
            Some(Drifting {
                ticket: ticket.clone(),
                drift,
            })
        })
        .collect();
    out.sort_by(|a, b| a.drift.cmp(&b.drift).then(a.ticket.id.cmp(&b.ticket.id)));
    out
}

/// The card of the tickets out of step, over the list; returns the URL of the
/// ticket clicked.
pub(super) fn card(
    ui: &mut egui::Ui,
    theme: &bevel_theme::Theme,
    list: &[Drifting],
    production: &str,
) -> Option<String> {
    use super::super::tip::{Tip, TipExt};
    use super::super::widgets::{mono, muted, semibold};
    use crate::i18n::tl;
    use egui::{CornerRadius, Margin, RichText, Sense, Stroke};
    let mut clicked = None;
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.tones.orange.fg.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(semibold(
                    tf(
                        "Out of step with the code · {n}",
                        &[("n", &list.len().to_string())],
                    ),
                    theme.text_size,
                ));
                ui.label(
                    muted(
                        theme,
                        tf(
                            "the tracker's closed statuses, against {branch}",
                            &[("branch", production)],
                        ),
                    )
                    .size(theme.text_size - 1.5),
                );
            });
            ui.add_space(6.0);
            for d in list {
                let tone = match d.drift {
                    Drift::ClosedMrOpen(_) => theme.tones.red,
                    Drift::ClosedNotShipped(_) => theme.tones.orange,
                    Drift::ShippedStillOpen(_) => theme.tones.blue,
                };
                let row = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(
                            mono(format!("#{}", d.ticket.id), theme.text_size - 0.5)
                                .color(theme.palette.accent),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(&d.ticket.subject)
                                    .font(bevel_theme::fonts::medium(theme.text_size)),
                            )
                            .truncate()
                            .show_tooltip_when_elided(false),
                        );
                        ui.label(RichText::new(d.text(production)).color(tone.fg));
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if row
                    .rich(
                        Tip::new()
                            .title(format!("#{} {}", d.ticket.id, d.ticket.subject))
                            .hint(tl("Open the ticket")),
                    )
                    .clicked()
                {
                    clicked = Some(d.ticket.url.clone());
                }
            }
        });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The demo's Redmine statuses, Closed and Rejected closing a ticket.
    fn statuses() -> Vec<TicketTransitionTarget> {
        crate::demo::tracker::STATUSES
            .iter()
            .map(|label| TicketTransitionTarget {
                id: (*label).into(),
                label: (*label).into(),
                closed: crate::demo::tracker::CLOSED.contains(label),
            })
            .collect()
    }

    #[tokio::test]
    async fn the_demo_has_one_ticket_of_each_drift() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let iid = crate::demo::iid;
        let found = drifts(&app, &statuses(), Utc::now());
        let got: Vec<(&str, &Drift)> = found
            .iter()
            .map(|d| (d.ticket.id.as_str(), &d.drift))
            .collect();
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(
            got[0],
            ("4350", &Drift::ClosedMrOpen(vec![iid("webhook-docs")]))
        );
        assert_eq!(
            got[1],
            ("4310", &Drift::ClosedNotShipped(vec![iid("csv-export")]))
        );
        assert_eq!(got[2].0, "4102");
        assert!(matches!(got[2].1, Drift::ShippedStillOpen(days) if *days >= SHIPPED_DAYS));
    }

    #[tokio::test]
    async fn nothing_is_out_of_step_without_the_trackers_closed_statuses() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let mut open_only = statuses();
        for s in &mut open_only {
            s.closed = false;
        }
        // Only the shipped ticket still open remains: closing needs the tracker's word.
        let found = drifts(&app, &open_only, Utc::now());
        assert!(found
            .iter()
            .all(|d| matches!(d.drift, Drift::ShippedStillOpen(_))));
        // Looked at a month earlier: not on production long enough yet.
        let soon = drifts(&app, &statuses(), Utc::now() - chrono::Duration::days(30));
        assert!(!soon
            .iter()
            .any(|d| matches!(d.drift, Drift::ShippedStillOpen(_))));
    }
}
