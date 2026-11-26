//! Events, saves and the open projects' tabs: switching, loading, restoring, closing.

use super::*;

impl GuiApp {
    /// Applies the events received since the last frame, as the TUI loop does.
    pub(super) fn apply_events(&mut self) {
        if self.demo {
            while let Ok(event) = self.events.try_recv() {
                crate::demo::apply_demo_event(&mut self.app, event);
                memo::changed();
            }
            return;
        }
        let needs_save = drain_events(
            &mut self.app,
            &self.events,
            &self.api_semaphore,
            &self.tx,
            &mut self.last_known_branches,
        );
        if self
            .save_clock
            .ready(needs_save, syncing(&self.app), std::time::Instant::now())
        {
            self.save_state();
        }
    }

    /// Applies the language picked in the settings dialog and saves it (not in
    /// the demo, which never writes to disk).
    pub(super) fn save_language(&mut self) {
        if self.language_draft == self.language {
            return;
        }
        self.language = self.language_draft.clone();
        i18n::set_language(self.language.as_deref());
        if !self.demo && !self.app.read_only {
            block_on(crate::storage::set_language_async(self.language.clone()));
        }
    }

    /// Applies the rail and start page picked in the settings dialog, and saves
    /// them with the window's preferences (not in the demo, which never writes).
    pub(super) fn save_rail(&mut self) {
        if self.rail_draft == self.rail {
            return;
        }
        self.rail = self.rail_draft.clone();
        if !self.demo {
            prefs::save(&prefs::current(self));
        }
    }

    /// Shows `key` in the rail again (from Ctrl+K), saved at once.
    pub(super) fn show_in_rail(&mut self, key: &str) {
        if !self.rail.shows(key) {
            self.rail.toggle(key);
        }
        self.rail_draft = self.rail.clone();
        if !self.demo {
            prefs::save(&prefs::current(self));
        }
        self.app.flash(
            format!(
                "✔ {}",
                tf(
                    "{page} is back in your rail",
                    &[("page", persona::page_label(key))]
                )
            ),
            false,
        );
    }

    /// Saves the state in the background: the frame never waits on the disk.
    pub(super) fn save_state(&self) {
        save_state(&self.app, &self.last_known_branches, &self.state_writer);
    }

    /// Changes the MRs of the tab at `index` (the active one or a hidden one)
    /// and saves that tab's state; the demo, read-only, keeps it in memory.
    pub(crate) fn edit_tab(&mut self, index: usize, edit: impl FnOnce(&mut App)) {
        let save = !self.demo;
        // The edit changes MRs without an event: the memos read them again.
        memo::changed();
        match self.tabs.get_mut(index).map(|slot| &mut slot.state) {
            Some(SlotState::Active) => {
                edit(&mut self.app);
                if save && !self.app.read_only {
                    self.save_state();
                }
            }
            Some(SlotState::Parked(tab)) => {
                edit(&mut tab.app);
                if save && !tab.app.read_only {
                    save_state(&tab.app, &tab.last_known_branches, &tab.state_writer);
                }
            }
            _ => {}
        }
    }

    /// Ends the snoozes whose time came or whose MR moved, in every tab, and
    /// says which MRs are back in Today.
    pub(super) fn wake_snoozed(&mut self) {
        let now = chrono::Utc::now();
        let due = |app: &App| {
            app.mrs.iter().any(|mr| {
                mr.snooze
                    .as_ref()
                    .is_some_and(|s| s.wake(mr, now).is_some())
            })
        };
        let tabs: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| match &self.tabs[i].state {
                SlotState::Active => due(&self.app),
                SlotState::Parked(tab) => due(&tab.app),
                _ => false,
            })
            .collect();
        let mut woken = Vec::new();
        for index in tabs {
            self.edit_tab(index, |app| {
                woken.extend(crate::snooze::wake_due(&mut app.mrs, now));
            });
        }
        if let Some((id, _, why)) = woken.first() {
            let text = if woken.len() == 1 {
                tf(
                    "!{id} is back in Today: {why}",
                    &[("id", id), ("why", why.text())],
                )
            } else {
                tf(
                    "{n} snoozed MRs are back in Today",
                    &[("n", &woken.len().to_string())],
                )
            };
            self.app.flash(text, false);
        }
    }

    pub(super) fn dispatch_pending(&mut self) {
        let mut ctx = ActionContext {
            api_semaphore: &self.api_semaphore,
            tx: &self.tx,
            last_known_branches: &mut self.last_known_branches,
            state_writer: Some(&self.state_writer),
        };
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            // An action may change MRs (flag, snooze, dismiss…) without an event.
            memo::changed();
        }
        for action in pending {
            block_on(dispatch(action, &mut self.app, &mut ctx));
        }
    }

    /// Opens the project the project picker asked for (`App::switch_to`).
    pub(super) fn take_switch(&mut self) {
        if let Some(next) = self.app.switch_to.take() {
            self.open_project(next);
        }
    }

    /// Shows `project`: its tab when it is open, else a new tab, loaded in the
    /// background (the token lookup waits on the keyring) and shown once ready.
    pub(super) fn open_project(&mut self, project: ProjectEntry) {
        if let Some(index) = self.tab_of(&project.gitlab_url, &project.project_id) {
            match &mut self.tabs[index].state {
                SlotState::Loading { show, .. } => *show = true,
                _ => self.activate(index),
            }
            return;
        }
        let name = project
            .name
            .clone()
            .unwrap_or_else(|| project.project_id.clone());
        self.app
            .flash(tf("Loading {project}…", &[("project", &name)]), false);
        let slot = self.load_slot(project, true);
        self.tabs.push(slot);
    }

    /// A tab for `project`, loading in the background.
    pub(super) fn load_slot(&self, project: ProjectEntry, show: bool) -> Slot {
        let (done, loaded) = std::sync::mpsc::channel();
        let global_config = self.global_config.clone();
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        let skip_redmine = project
            .tracker
            .as_ref()
            .is_some_and(|t| self.skip_redmine.contains(t.url.trim_end_matches('/')));
        let slot = Slot {
            url: project.gitlab_url.trim_end_matches('/').to_string(),
            id: project.project_id.clone(),
            name: project
                .name
                .clone()
                .unwrap_or_else(|| project.project_id.clone()),
            state: SlotState::Loading { rx: loaded, show },
        };
        tokio::spawn(async move {
            let _ = done.send(load_project(&global_config, project, &shared, skip_redmine).await);
            ctx.request_repaint();
        });
        slot
    }

    /// Reopens the tabs open when the window was last closed, in their order, the
    /// project the window started on among them. Projects no longer in
    /// `projects.toml` are left out.
    pub(super) fn restore_tabs(&mut self, saved: &[prefs::SavedTab], projects: &[ProjectEntry]) {
        let current = (self.app.base_url.clone(), self.app.project_id.clone());
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut seen_current = false;
        for tab in saved {
            if crate::storage::is_same_project(
                &ProjectEntry {
                    gitlab_url: tab.url.clone(),
                    project_id: tab.id.clone(),
                    ..ProjectEntry::default()
                },
                &current.0,
                &current.1,
            ) {
                seen_current = true;
                continue;
            }
            let Some(entry) = projects
                .iter()
                .find(|p| crate::storage::is_same_project(p, &tab.url, &tab.id))
            else {
                continue;
            };
            if let Some(page) = &tab.page {
                self.left_off.insert(
                    (
                        entry.gitlab_url.trim_end_matches('/').to_string(),
                        entry.project_id.clone(),
                    ),
                    page.clone(),
                );
            }
            let slot = self.load_slot(entry.clone(), false);
            if seen_current {
                after.push(slot);
            } else {
                before.push(slot);
            }
        }
        let active = before.len();
        before.append(&mut self.tabs);
        before.append(&mut after);
        self.tabs = before;
        debug_assert!(matches!(self.tabs[active].state, SlotState::Active));
    }

    /// The tab of `(url, id)`, if open.
    pub(super) fn tab_of(&self, url: &str, id: &str) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.url == url.trim_end_matches('/') && t.id == id)
    }

    /// The index of the tab shown.
    pub(super) fn active(&self) -> usize {
        self.tabs
            .iter()
            .position(|t| matches!(t.state, SlotState::Active))
            .unwrap_or(0)
    }

    /// Shows the tab `index` (a loaded one), parking the one shown.
    pub(super) fn activate(&mut self, index: usize) {
        let current = self.active();
        if index == current || index >= self.tabs.len() {
            return;
        }
        let SlotState::Parked(mut tab) =
            std::mem::replace(&mut self.tabs[index].state, SlotState::Active)
        else {
            // Still loading: shown once ready.
            if let SlotState::Loading { show, .. } = &mut self.tabs[index].state {
                *show = true;
            }
            return;
        };
        self.swap_tab(&mut tab);
        self.tabs[current].state = SlotState::Parked(tab);
        self.after_swap();
    }

    /// Exchanges the tab shown with `tab`.
    pub(super) fn swap_tab(&mut self, tab: &mut Tab) {
        use std::mem::swap;
        swap(&mut self.app, &mut tab.app);
        swap(&mut self.tx, &mut tab.tx);
        swap(&mut self.events, &mut tab.events);
        swap(&mut self.api_semaphore, &mut tab.api_semaphore);
        swap(&mut self.last_known_branches, &mut tab.last_known_branches);
        swap(&mut self.state_writer, &mut tab.state_writer);
        swap(&mut self.save_clock, &mut tab.save_clock);
        swap(&mut self.page, &mut tab.page);
        swap(&mut self.history, &mut tab.history);
        swap(&mut self.text_filter, &mut tab.text_filter);
        swap(&mut self.members, &mut tab.members);
        swap(&mut self.baseline, &mut tab.baseline);
        swap(&mut self.toasts, &mut tab.toasts);
        swap(&mut self.last_sync, &mut tab.last_sync);
        swap(&mut self.was_syncing, &mut tab.was_syncing);
        swap(&mut self.digest_closed, &mut tab.digest_closed);
        swap(&mut self.statuses, &mut tab.statuses);
        swap(&mut self.statuses_rx, &mut tab.statuses_rx);
        swap(&mut self.moves, &mut tab.moves);
        swap(&mut self.marked, &mut tab.marked);
        swap(&mut self.folded, &mut tab.folded);
        swap(&mut self.details, &mut tab.details);
    }

    /// Once another tab is shown: its colors, the picks of the last one dropped,
    /// and `projects.toml` marks it active, so the terminal opens it next.
    pub(super) fn after_swap(&mut self) {
        self.set_theme(self.theme.mode);
        self.bulk = None;
        self.bulk_mode = false;
        self.next_focus = None;
        self.merged_gap = None;
        self.select_id = None;
        self.scroll_to = None;
        if !self.demo {
            let (url, id) = (self.app.base_url.clone(), self.app.project_id.clone());
            tokio::spawn(async move { crate::storage::set_active_project_async(&url, &id).await });
        }
    }

    /// Closes the tab `index`; the last one stays.
    pub(super) fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if matches!(self.tabs[index].state, SlotState::Active) {
            // Show the nearest loaded tab first.
            let next = (index + 1..self.tabs.len())
                .chain((0..index).rev())
                .find(|&i| matches!(self.tabs[i].state, SlotState::Parked(_)));
            let Some(next) = next else {
                return;
            };
            self.activate(next);
        }
        let slot = self.tabs.remove(index);
        if let (SlotState::Parked(tab), false) = (slot.state, self.demo) {
            // Its last state on disk; its fetches stop with its channel.
            save_state(&tab.app, &tab.last_known_branches, &tab.state_writer);
            block_on(tab.state_writer.flush());
        }
    }

    /// Shows the MR of each desktop notification clicked, in its tab, and brings
    /// the window to the front; a URL no open tab knows opens in the browser.
    pub(super) fn poll_clicks(&mut self) {
        while let Ok(url) = self.clicks.try_recv() {
            let found = self.tabs.iter().enumerate().find_map(|(index, slot)| {
                let app = match &slot.state {
                    SlotState::Active => &self.app,
                    SlotState::Parked(tab) => &tab.app,
                    _ => return None,
                };
                app.mrs
                    .iter()
                    .find(|mr| mr.web_url == url)
                    .map(|mr| (index, mr.id.clone()))
            });
            match found {
                Some((index, id)) => {
                    self.open_link(index, &id);
                    self.ctx
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                None => {
                    let _ = open::that(&url);
                }
            }
        }
    }

    /// Takes the tabs loaded in the background, and runs the parked ones: their
    /// events apply (refreshes, notifications, state saves) while hidden.
    pub(super) fn poll_tabs(&mut self, now: f64) {
        let mut show = None;
        let mut tokens = None;
        for index in 0..self.tabs.len() {
            let slot = &mut self.tabs[index];
            let SlotState::Loading { rx, show: wanted } = &slot.state else {
                continue;
            };
            let wanted = *wanted;
            let Ok(result) = rx.try_recv() else {
                continue;
            };
            match result {
                Ok(Loaded::Session(session)) => {
                    let mut tab = Tab::new(*session, &self.ctx);
                    tab.app.theme = self.app.theme;
                    // Its start page (Settings › Startup): where it was left,
                    // or the persona's, or the one always opened.
                    let left_off = self.left_off.remove(&(slot.url.clone(), slot.id.clone()));
                    let opening = self.rail.opening_page(left_off.as_deref());
                    tab.page = match prefs::page_of(opening) {
                        #[cfg(feature = "stats")]
                        Some(Page::Stats) => Page::MergeRequests,
                        Some(page) => page,
                        None => Page::MergeRequests,
                    };
                    slot.state = SlotState::Parked(Box::new(tab));
                    if wanted {
                        show = Some(index);
                    }
                }
                Ok(Loaded::NeedsTokens(mode)) => {
                    slot.state = SlotState::Closed;
                    if wanted || tokens.is_none() {
                        tokens = Some(mode);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Could not load a project");
                    slot.state = SlotState::Closed;
                    self.app.flash(
                        format!(
                            "✘ {}: {e:#}",
                            tf("Could not open {project}", &[("project", &slot.name)])
                        ),
                        true,
                    );
                }
            }
        }
        if let Some(mode) = tokens {
            self.wizard = Some(setup::ProjectForm::new(&self.ctx, mode));
        }
        // Found again by project: a failed tab before it may have gone.
        let show = show.map(|i| (self.tabs[i].url.clone(), self.tabs[i].id.clone()));
        self.tabs.retain(|t| !matches!(t.state, SlotState::Closed));
        if let Some(index) = show.and_then(|(url, id)| self.tab_of(&url, &id)) {
            self.activate(index);
            let name = shell::project_name(self);
            self.app.flash(
                format!("✔ {}", tf("Opened {project}", &[("project", &name)])),
                false,
            );
        }
        let theme = self.theme;
        for slot in &mut self.tabs {
            if let SlotState::Parked(tab) = &mut slot.state {
                tab.run_hidden(&theme, now, self.demo);
            }
        }
    }
}
