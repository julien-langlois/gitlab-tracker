//! Moving around the window: theme, pages, focus, filters, the MR and person pages, bulk actions.

use super::*;

impl GuiApp {
    /// Applies `mode` to the window and to the text the terminal views produce.
    pub(super) fn set_theme(&mut self, mode: bevel_theme::Mode) {
        self.theme = bevel_theme::Theme::for_mode(mode);
        self.theme.apply(&self.ctx);
        let light = mode == bevel_theme::Mode::Light;
        crate::ui::theme::set_light(light);
        self.app.theme = crate::ui::theme::Palette::for_mode(if light {
            crate::ui::theme::ThemeMode::Light
        } else {
            crate::ui::theme::ThemeMode::Dark
        });
    }

    /// Whether the stats view replaces the merge requests.
    #[cfg(feature = "stats")]
    pub(super) fn on_stats(&self) -> bool {
        matches!(self.app.input_mode, crate::app::InputMode::Stats)
    }

    /// The shortcuts to the sections of the page shown, when it has some.
    pub(super) fn page_jumps(&mut self) -> Option<&mut jump::Jumps> {
        match self.page() {
            Page::Mr => Some(&mut self.mr_jumps),
            Page::Delivery => Some(&mut self.delivery.jumps),
            Page::Ci => Some(&mut self.ci.jumps),
            _ => None,
        }
    }

    /// The page shown: the stats while they are open, else the one picked last.
    pub(super) fn page(&self) -> Page {
        #[cfg(feature = "stats")]
        if self.on_stats() {
            return Page::Stats;
        }
        match self.page {
            Page::Tracker if self.app.tracker.is_none() => Page::MergeRequests,
            Page::Mr if self.app.selected_mr().is_none() => Page::MergeRequests,
            Page::Person if self.team.selected.is_none() => Page::Team,
            page => page,
        }
    }

    /// Shows `page`, opening or closing the stats as the TUI's `G` does.
    pub(super) fn open_page(&mut self, page: Page) {
        history::visit(self, page);
        #[cfg(feature = "stats")]
        {
            let on_stats = self.on_stats();
            if page == Page::Stats {
                if !on_stats {
                    self.pending.push(Action::OpenStats);
                }
                return;
            }
            if on_stats {
                self.app.input_mode = crate::app::InputMode::Normal;
            }
        }
        self.page = page;
    }

    /// Lists the MRs behind the cockpit counter `key` on the merge requests page.
    pub(super) fn focus_on(&mut self, key: &str, label: impl Into<String>) {
        let ids = members_of(&self.app, &self.members, key)
            .into_iter()
            .collect();
        self.next_focus = Some(Some(Focus {
            key: key.to_string(),
            label: label.into(),
            ids,
        }));
        self.open_page(Page::MergeRequests);
    }

    /// Lists the MRs `ids` on the merge requests page, under `label`: a focus
    /// that keeps those MRs (no counter behind it to follow).
    pub(super) fn focus_on_ids(&mut self, label: impl Into<String>, ids: Vec<String>) {
        self.next_focus = Some(Some(Focus {
            key: String::new(),
            label: label.into(),
            ids: ids.into_iter().collect(),
        }));
        self.open_page(Page::MergeRequests);
    }

    /// Lists the MRs carrying `label` alone, on the merge requests page: the
    /// label filter on it and no other, no cockpit focus, no search.
    pub(super) fn filter_by_label(&mut self, label: &str) {
        self.filter_by("label", label);
    }

    /// Lists the MRs of the release (milestone) `title`, as [`Self::filter_by_label`].
    pub(super) fn filter_by_milestone(&mut self, title: &str) {
        self.filter_by("milestone", title);
    }

    /// The merge requests page with the filter `id` on `query` alone.
    pub(super) fn filter_by(&mut self, id: &str, query: &str) {
        let Some(index) = self.app.filter_defs.iter().position(|def| def.id == id) else {
            return;
        };
        if self.app.focus.is_some() {
            self.next_focus = Some(None);
        }
        self.app.input.clear();
        self.app.milestone_suggestions.clear();
        if self.app.status_pick.is_some() {
            self.pending.push(Action::SetStatusPick(None));
        }
        self.pending.push(Action::SetFilter {
            index: 0,
            query: String::new(),
        });
        self.pending
            .push(Action::SetFilters(vec![crate::app::ActiveFilter {
                index,
                query: query.to_string(),
            }]));
        self.open_page(Page::MergeRequests);
    }

    /// Deep link: the MR `id`'s page, out of the view or search that hides it
    /// (merged MRs hidden by `M` stay hidden: [`GuiApp::apply_picks`] says so).
    pub(super) fn open_mr(&mut self, id: &str) {
        let visible = self.app.visible_mrs().any(|mr| mr.id == id);
        if !visible {
            if self.app.focus.is_some() {
                self.next_focus = Some(None);
            }
            self.pending.push(Action::SetFilter {
                index: 0,
                query: String::new(),
            });
            self.pending.push(Action::SetFilters(Vec::new()));
            if self.app.status_pick.is_some() {
                self.pending.push(Action::SetStatusPick(None));
            }
            self.app.input.clear();
        }
        self.select_id = Some(id.to_string());
        self.open_page(Page::Mr);
    }

    /// The page of the team member `login`.
    pub(super) fn open_person(&mut self, login: &str) {
        history::visit_person(self, login);
        self.team.selected = Some(login.to_string());
        self.team.step = 0;
        self.open_page(Page::Person);
    }

    /// Deep link across tabs: the MR `id` of the tab `tab`, on its page.
    pub(super) fn open_link(&mut self, tab: usize, id: &str) {
        self.activate(tab);
        self.open_mr(id);
    }

    /// Counts the MRs behind each counter, before the frame's snapshot: the
    /// focus aside, and on the cockpit without it. Keeps the focus up to date.
    pub(super) fn count_members(&mut self) {
        self.app.focus_suspended = true;
        // Reading every MR's counters is the frame's heaviest work: kept while
        // the same MRs are listed and nothing changed (see `memo.rs`).
        let listed = self.app.compute_visible_indices();
        let app = &self.app;
        self.members = self.memo.members.get(&app.project_id, listed, || {
            crate::ui::cockpit::metric_members(app)
        });
        self.app.focus_suspended = self.page() == Page::Cockpit;
        let key = self.app.focus.as_ref().map(|f| f.key.clone());
        if let Some(key) = key.filter(|k| !k.is_empty()) {
            let ids = members_of(&self.app, &self.members, &key)
                .into_iter()
                .collect();
            if let Some(focus) = &mut self.app.focus {
                focus.ids = ids;
            }
        }
        let complete = self.app.pending_initial_fetches.is_empty() && !self.app.mrs.is_empty();
        if self.baseline.is_none() && complete {
            self.baseline = Some(Baseline {
                counts: self
                    .members
                    .iter()
                    .map(|(k, v)| (k.clone(), v.len()))
                    .collect(),
                at: chrono::Local::now().format("%H:%M").to_string(),
            });
        }
    }

    /// Applies what the bulk bar asked for to every picked MR.
    pub(super) fn apply_bulk(&mut self) {
        let Some(bulk) = self.bulk.take() else {
            return;
        };
        let ids = self.marked.clone();
        match bulk {
            table::Bulk::Flag(on) => {
                for mr in self.app.mrs.iter_mut().filter(|m| ids.contains(&m.id)) {
                    mr.flagged = on;
                }
                memo::changed();
                if !self.demo && !self.app.read_only {
                    self.save_state();
                }
            }
            table::Bulk::StopTracking => {
                for id in &ids {
                    self.pending.push(Action::SubmitInput(format!("-{id}")));
                }
                self.marked.clear();
                self.dispatch_pending();
            }
            table::Bulk::CopyChecklist => {
                // The checklist copies the listed MRs: list the picked ones for it.
                let focus = self.app.focus.take();
                let selected = self.app.table_state.selected();
                self.app.set_focus(Some(Focus {
                    key: String::new(),
                    label: String::new(),
                    ids,
                }));
                self.pending.push(Action::CopyChecklist);
                self.dispatch_pending();
                self.app.focus = focus;
                self.app.table_state.select(selected);
            }
        }
    }

    /// Applies what the frame picked: the focus, then the MR to select.
    pub(super) fn apply_picks(&mut self) {
        self.apply_bulk();
        if let Some(focus) = self.next_focus.take() {
            self.app.focus_suspended = false;
            self.app.set_focus(focus);
        }
        if let Some(id) = self.select_id.take() {
            let found = self.app.visible_mrs().position(|mr| mr.id == id);
            if let Some(index) = found {
                self.app.table_state.select(Some(index));
                self.app.layout.inspector.reset();
            } else if self.page == Page::Mr {
                // Not in the list (merged MRs hidden, or no longer tracked): not
                // another MR's page instead.
                self.page = Page::MergeRequests;
                self.app.flash(
                    tf(
                        "!{id} is not in the list: merged MRs may be hidden (M)",
                        &[("id", &id)],
                    ),
                    true,
                );
            }
        }
    }
}
