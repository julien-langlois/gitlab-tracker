//! Open projects behind the tabs: loading one, its events, its state, the parked tabs.

use super::*;

/// What moving a ticket said: the message to flash, or the error.
pub(super) type MoveResult = Result<String, String>;
pub(super) type MoveSender = std::sync::mpsc::Sender<MoveResult>;

/// The ids of the listed MRs behind each cockpit counter, by key (see
/// [`crate::ui::cockpit::metric_members`]); shared with the memo that keeps it.
pub(super) type Members = std::rc::Rc<HashMap<String, Vec<String>>>;

/// The counters when the list was first complete, and when that was.
pub(super) struct Baseline {
    pub(super) counts: HashMap<String, usize>,
    pub(super) at: String,
}

/// The MRs behind `key`: a counter of the cockpit, `release-open:<title>` (the
/// release's MRs still to merge), or `size:<min>:<max>` (the merged MRs with that
/// many changed lines, a bucket of the stats).
pub(super) fn members_of(
    app: &App,
    members: &HashMap<String, Vec<String>>,
    key: &str,
) -> Vec<String> {
    if let Some(ids) = flow::stage_members(app, key) {
        return ids;
    }
    let buckets = crate::next_step::Bucket::of_key(key);
    if !buckets.is_empty() {
        return crate::next_step::all(app)
            .into_iter()
            .filter(|(_, step)| buckets.contains(&step.bucket()))
            .map(|(id, _)| id)
            .collect();
    }
    if let Some(range) = key.strip_prefix("size:") {
        let mut bounds = range.split(':').map(|b| b.parse::<u32>().ok());
        let min = bounds.next().flatten().unwrap_or(0);
        let max = bounds.next().flatten();
        return app
            .mrs
            .iter()
            .filter(|mr| mr.state == crate::models::GitlabMrState::Merged)
            .filter(|mr| {
                mr.diff_stats.as_ref().is_some_and(|d| {
                    let lines = d.additions + d.deletions;
                    lines >= min && max.is_none_or(|max| lines <= max)
                })
            })
            .map(|mr| mr.id.clone())
            .collect();
    }
    if let Some(release) = key.strip_prefix("release-open:") {
        return ["WIP", "Blocked", "Review"]
            .iter()
            .flat_map(|bucket| {
                members
                    .get(&crate::ui::cockpit::release_metric_key(release, bucket))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
    }
    members.get(key).cloned().unwrap_or_default()
}

/// Routes the clicks on desktop notifications to the window (see
/// [`GuiApp::poll_clicks`]), waking it for each.
pub(super) fn notification_clicks(ctx: &egui::Context) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let ctx = ctx.clone();
    crate::notify::on_click(move |url| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(url.to_string());
        }
        ctx.request_repaint();
    });
    rx
}

/// Forwards the session's events to the window, waking it for each one: the
/// window sleeps between frames otherwise.
pub(super) fn forward_events(
    mut rx: UnboundedReceiver<AppEvent>,
    ctx: &egui::Context,
) -> std::sync::mpsc::Receiver<AppEvent> {
    let (forward, events) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if forward.send(event).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    events
}

/// What loading a project in a tab gave.
pub(super) enum Loaded {
    Session(Box<Session>),
    /// The tokens it lacks, asked in the project form.
    NeedsTokens(setup::Mode),
}

pub(super) async fn load_project(
    global_config: &AppConfig,
    project: ProjectEntry,
    shared: &Shared,
    skip_redmine: bool,
) -> anyhow::Result<Loaded> {
    let probe = project.clone();
    let missing = tokio::task::spawn_blocking(move || setup::missing_tokens(&probe))
        .await
        .unwrap_or(None);
    // A missing Redmine key is asked once; the project opens without its tickets
    // when it is left empty (`skip_redmine`).
    if let Some(mode) = missing.filter(|m| match m {
        setup::Mode::Tokens { gitlab, .. } => *gitlab || !skip_redmine,
        setup::Mode::New => false,
    }) {
        return Ok(Loaded::NeedsTokens(mode));
    }
    let project = with_username(project).await;
    let session = build_session(global_config, project, shared).await?;
    Ok(Loaded::Session(Box::new(session)))
}

/// `project` with the GitLab username of its token's owner, read from GitLab and
/// saved to `projects.toml` when it has none: the "me" views (Today, "To review
/// (me)") need it.
pub(super) async fn with_username(mut project: ProjectEntry) -> ProjectEntry {
    if project.gitlab_username.is_some() {
        return project;
    }
    let url = project.gitlab_url.clone();
    let token = tokio::task::spawn_blocking(move || crate::storage::stored_gitlab_token(&url))
        .await
        .ok()
        .flatten();
    let Some(token) = token else {
        return project;
    };
    if let Some(username) = crate::gitlab::current_username(&project.gitlab_url, &token).await {
        crate::storage::save_gitlab_username_async(
            &username,
            &project.gitlab_url,
            &project.project_id,
        )
        .await;
        project.gitlab_username = Some(username);
    }
    project
}

/// Applies the events received for a tab; whether its state must be saved.
pub(super) fn drain_events(
    app: &mut App,
    events: &std::sync::mpsc::Receiver<AppEvent>,
    api_semaphore: &Arc<Semaphore>,
    tx: &UnboundedSender<AppEvent>,
    last_known_branches: &mut HashMap<String, HashSet<String>>,
) -> bool {
    let mut needs_save = false;
    app.release_kept();
    let selected = app.selected_id();
    while let Ok(event) = events.try_recv() {
        needs_save |= app.apply_event(event, api_semaphore.clone(), tx, last_known_branches);
        memo::changed();
    }
    app.flush_pending_sort();
    app.keep_selection(selected);
    app.ensure_time_entries(tx);
    needs_save
}

/// Whether the tab's MRs are being read (startup or refresh).
pub(super) fn syncing(app: &App) -> bool {
    !app.pending_initial_fetches.is_empty() || !app.pending_refresh_fetches.is_empty()
}

/// How often, at most, a tab's state is copied for a save while its MRs are read.
pub(super) const SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// When a tab's state was last copied for a save, and whether newer data waits.
///
/// The copy is taken on the UI thread (the write is not), and on a large
/// project it costs a frame: during a refresh, whose events land over many
/// frames, it is taken every [`SAVE_EVERY`] at most, then once the refresh is
/// over. Outside a refresh, new data is saved at once.
#[derive(Default)]
pub(super) struct SaveClock {
    at: Option<std::time::Instant>,
    due: bool,
}

impl SaveClock {
    /// Whether to save now, given whether new data came (`changed`) and whether
    /// the MRs are still being read; called on every frame.
    pub(super) fn ready(&mut self, changed: bool, syncing: bool, now: std::time::Instant) -> bool {
        self.due |= changed;
        let ready = self.due
            && (!syncing
                || self
                    .at
                    .is_none_or(|at| now.duration_since(at) >= SAVE_EVERY));
        if ready {
            self.due = false;
            self.at = Some(now);
        }
        ready
    }

    /// Whether data waits to be saved, which is then no longer due (the window
    /// closes, the tab is closed).
    pub(super) fn take_due(&mut self) -> bool {
        std::mem::take(&mut self.due)
    }
}

/// Saves a tab's state in the background: the frame never waits on the disk.
pub(super) fn save_state(
    app: &App,
    last_known_branches: &HashMap<String, HashSet<String>>,
    writer: &StateWriter,
) {
    if let Some(file) = StateFile::new(
        &app.mrs,
        last_known_branches,
        app.discovery_started_at.as_deref(),
        &app.dismissed_mr_ids,
        &app.base_url,
        &app.project_id,
    ) {
        writer.save(file);
    }
}

/// A title-bar tab: an open project.
pub(super) struct Slot {
    /// GitLab URL (no trailing `/`) and project id: what the tab is for.
    pub(super) url: String,
    pub(super) id: String,
    /// Shown while it loads.
    pub(super) name: String,
    pub(super) state: SlotState,
}

pub(super) enum SlotState {
    /// Shown: its state is in [`GuiApp`]'s fields.
    Active,
    /// Open in the background.
    Parked(Box<Tab>),
    /// Loading; shown once ready when `show`.
    Loading {
        rx: std::sync::mpsc::Receiver<anyhow::Result<Loaded>>,
        show: bool,
    },
    /// Failed to load: dropped at the end of the frame.
    Closed,
}

impl Slot {
    /// The tab of the project `app` runs, shown.
    pub(super) fn active(app: &App) -> Self {
        Self {
            url: app.base_url.trim_end_matches('/').to_string(),
            id: app.project_id.clone(),
            name: app
                .project_name
                .clone()
                .unwrap_or_else(|| app.project_id.clone()),
            state: SlotState::Active,
        }
    }
}

/// What a project needs while open in a tab: its session and the window's state
/// for it. The shown tab's lives in [`GuiApp`] (see [`GuiApp::swap_tab`]).
pub(super) struct Tab {
    pub(super) app: App,
    pub(super) tx: UnboundedSender<AppEvent>,
    pub(super) events: std::sync::mpsc::Receiver<AppEvent>,
    pub(super) api_semaphore: Arc<Semaphore>,
    pub(super) last_known_branches: HashMap<String, HashSet<String>>,
    pub(super) state_writer: StateWriter,
    pub(super) save_clock: SaveClock,
    pub(super) page: Page,
    pub(super) history: history::History,
    pub(super) text_filter: (Option<usize>, String),
    pub(super) members: Members,
    pub(super) baseline: Option<Baseline>,
    pub(super) toasts: toasts::Toasts,
    pub(super) last_sync: Option<chrono::DateTime<chrono::Local>>,
    pub(super) was_syncing: bool,
    pub(super) digest_closed: bool,
    pub(super) statuses: Option<Vec<gitlab_tracker_core::TicketTransitionTarget>>,
    pub(super) statuses_rx:
        Option<std::sync::mpsc::Receiver<Vec<gitlab_tracker_core::TicketTransitionTarget>>>,
    pub(super) moves: (MoveSender, std::sync::mpsc::Receiver<MoveResult>),
    pub(super) marked: HashSet<String>,
    pub(super) folded: HashSet<String>,
    pub(super) details: details::Details,
}

impl Tab {
    pub(super) fn new(session: Session, ctx: &egui::Context) -> Self {
        let Session {
            app,
            tx,
            rx,
            api_semaphore,
            last_known_branches,
        } = session;
        Self {
            app,
            tx,
            events: forward_events(rx, ctx),
            api_semaphore,
            last_known_branches,
            state_writer: StateWriter::default(),
            save_clock: SaveClock::default(),
            page: Page::MergeRequests,
            history: history::History::default(),
            text_filter: (None, String::new()),
            members: Members::default(),
            baseline: None,
            toasts: toasts::Toasts::default(),
            last_sync: None,
            was_syncing: false,
            digest_closed: false,
            statuses: None,
            statuses_rx: None,
            moves: std::sync::mpsc::channel(),
            marked: HashSet::new(),
            folded: HashSet::new(),
            details: details::Details::default(),
        }
    }

    /// A hidden tab's frame: its events apply and its changes are noted for the
    /// bell, as if shown.
    pub(super) fn run_hidden(&mut self, theme: &bevel_theme::Theme, now: f64, demo: bool) {
        if demo {
            while let Ok(event) = self.events.try_recv() {
                crate::demo::apply_demo_event(&mut self.app, event);
                memo::changed();
            }
        } else {
            let changed = drain_events(
                &mut self.app,
                &self.events,
                &self.api_semaphore,
                &self.tx,
                &mut self.last_known_branches,
            );
            if self
                .save_clock
                .ready(changed, syncing(&self.app), std::time::Instant::now())
            {
                save_state(&self.app, &self.last_known_branches, &self.state_writer);
            }
        }
        self.toasts.watch(&self.app, theme, now);
        let syncing = syncing(&self.app);
        if !syncing && self.was_syncing {
            self.last_sync = Some(chrono::Local::now());
        }
        self.was_syncing = syncing;
    }

    /// Whether a fetch of this tab failed: its tab shows a warning.
    pub(super) fn has_errors(&self) -> bool {
        self.app
            .mrs
            .iter()
            .any(|mr| matches!(mr.status, crate::models::MrStatus::Error(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_refresh_copies_the_state_every_few_seconds_then_once_it_is_over() {
        let mut clock = SaveClock::default();
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        // The first data of a refresh is saved at once.
        assert!(clock.ready(true, true, at(0)));
        // More data during the refresh: not copied on every frame…
        assert!(!clock.ready(true, true, at(16)));
        assert!(!clock.ready(false, true, at(1000)));
        // …but every SAVE_EVERY, even on a frame with no new data.
        assert!(clock.ready(false, true, at(2000)));
        assert!(!clock.ready(false, true, at(2100)));
        assert!(!clock.ready(true, true, at(2200)));
        // The refresh over: what waits is saved on the next frame, once.
        assert!(clock.ready(false, false, at(2300)));
        assert!(!clock.ready(false, false, at(2400)));
        // Outside a refresh, new data is saved at once.
        assert!(clock.ready(true, false, at(2416)));
    }

    #[test]
    fn data_not_saved_yet_is_handed_over_once_when_the_window_closes() {
        let mut clock = SaveClock::default();
        assert!(clock.ready(true, true, Instant::now()));
        assert!(!clock.ready(true, true, Instant::now()));
        assert!(clock.take_due());
        assert!(!clock.take_due());
    }
}
