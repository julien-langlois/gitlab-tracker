//! What the window runs beside the frames: the update check, ticket statuses and moves, tracker sync.

use super::*;

impl GuiApp {
    /// Looks for a newer release in the background (GitHub, at most once a day).
    pub(super) fn check_for_update(&mut self) {
        let (done, rx) = std::sync::mpsc::channel();
        self.update_rx = Some(rx);
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            if let Some(release) = crate::update_check::newer_release().await {
                let _ = done.send(release);
                ctx.request_repaint();
            }
        });
    }

    /// Takes the check's answer: the title bar's pill, and one notification.
    pub(super) fn poll_update(&mut self, now: f64) {
        let Some(release) = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.update_rx = None;
        if self.update_check {
            self.toasts
                .push_update(&release, self.theme.tones.blue, now);
            self.update = Some(release);
        }
    }

    /// Settings › Interface: turning the check off hides what it found.
    pub(crate) fn set_update_check(&mut self, on: bool) {
        self.update_check = on;
        if on {
            if self.update.is_none() && self.update_rx.is_none() && !self.demo {
                self.check_for_update();
            }
        } else {
            self.update = None;
        }
    }

    /// Fetches the tracker's statuses once (for the board's columns).
    pub(super) fn ensure_statuses(&mut self) {
        if self.statuses.is_some() || self.statuses_rx.is_some() {
            return;
        }
        let Some(transitioner) = self.app.ticket_transitioner.clone() else {
            return;
        };
        let (done, rx) = std::sync::mpsc::channel();
        self.statuses_rx = Some(rx);
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            let statuses = match transitioner.fetch_transition_targets().await {
                Ok(statuses) => statuses,
                Err(e) => {
                    tracing::warn!(error = %e, "Could not fetch the tracker's statuses");
                    Vec::new()
                }
            };
            let _ = done.send(statuses);
            ctx.request_repaint();
        });
    }

    /// Moves `ticket` to the status `target` (named `label`) in the tracker, then
    /// refreshes; the demo only says it is read-only.
    pub(super) fn move_ticket(&mut self, ticket: String, target: String, label: String) {
        if self.app.read_only {
            self.app
                .flash(tl("Read-only: the ticket stays where it is"), true);
            return;
        }
        let Some(transitioner) = self.app.ticket_transitioner.clone() else {
            return;
        };
        self.app.flash(
            tf(
                "Moving #{id} to {status}…",
                &[("id", &ticket), ("status", &label)],
            ),
            false,
        );
        let done = self.moves.0.clone();
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            let result = match transitioner
                .transition_ticket_status(&ticket, &target)
                .await
            {
                Ok(()) => Ok(tf(
                    "#{id} moved to {status}",
                    &[("id", &ticket), ("status", &label)],
                )),
                Err(e) => Err(format!("#{ticket}: {e}")),
            };
            let _ = done.send(result);
            ctx.request_repaint();
        });
    }

    /// Takes the tracker's answers: the statuses, the tickets moved.
    pub(super) fn poll_tracker(&mut self) {
        if let Some(statuses) = self.statuses_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.statuses = Some(statuses);
            self.statuses_rx = None;
        }
        while let Ok(result) = self.moves.1.try_recv() {
            match result {
                Ok(message) => {
                    self.app.flash(format!("✔ {message}"), false);
                    self.pending.push(Action::Refresh);
                }
                Err(message) => self.app.flash(format!("✘ {message}"), true),
            }
        }
    }

    /// Notes when the fetches in flight are all back: "updated N min ago".
    pub(super) fn track_sync(&mut self) {
        let syncing = !self.app.pending_initial_fetches.is_empty()
            || !self.app.pending_refresh_fetches.is_empty();
        let first = self.last_sync.is_none() && !self.app.mrs.is_empty();
        if !syncing && (self.was_syncing || first) {
            self.last_sync = Some(chrono::Local::now());
        }
        self.was_syncing = syncing;
    }
}
