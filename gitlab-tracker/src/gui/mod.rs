//! Desktop window (`gitlab-tracker gui`, feature `gui`).
//!
//! The window drives the same [`App`] as the TUI: it loads the project with
//! `build_session`, applies the async [`AppEvent`]s each frame, and turns clicks and
//! keys into [`Action`]s handled by [`dispatch`]. Popups follow `App::input_mode`,
//! as in the TUI. Only the drawing lives here.

mod board;
mod changelog;
mod ci;
mod cockpit;
mod colors;
mod delivery;
mod details;
mod dialogs;
mod environments;
mod flow;
mod history;
mod i18n;
mod icons;
mod inspector;
mod journal;
mod keys;
mod markdown;
mod milestones;
mod mr_page;
mod mr_pipelines;
mod next;
mod overlaps;
mod palette;
mod prefs;
mod promote;
mod rail_badges;
mod setup;
mod shell;
mod sidebar;
mod stacks;
#[cfg(feature = "stats")]
mod stats;
mod table;
mod team;
mod toasts;
mod today;
mod tones;
#[cfg(debug_assertions)]
mod tour;
mod tracker;
mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use tokio::sync::{mpsc::UnboundedReceiver, mpsc::UnboundedSender, Semaphore};

use crate::actions::{dispatch, Action, ActionContext};
use crate::app::{App, Focus};
use crate::config::AppConfig;
use crate::i18n::{tf, tl};
use crate::models::AppEvent;
use crate::storage::{ProjectEntry, StateFile, StateWriter};
use crate::{build_session, Session, Shared};

/// Environment prefix of the title-bar override: `GITLAB_TRACKER_TITLEBAR=app|system`.
const TITLEBAR_PREFIX: bevel_window::Prefix = bevel_window::Prefix::new("GITLAB_TRACKER");

/// Loads `project` like the TUI does, then opens the window and runs until it is
/// closed. The window never prompts in the terminal: a missing token is asked in
/// a window first.
pub async fn run(global_config: AppConfig, project: ProjectEntry) -> anyhow::Result<()> {
    gitlab_tracker_core::secrets::set_interactive(false);
    let probe = project.clone();
    let missing = tokio::task::spawn_blocking(move || setup::missing_tokens(&probe))
        .await
        .unwrap_or(None);
    if let Some(mode) = missing {
        let needs_gitlab = matches!(mode, setup::Mode::Tokens { gitlab: true, .. });
        if !setup::run(Some(mode)).await? && needs_gitlab {
            return Ok(());
        }
    }
    let shared = Shared {
        palette: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        #[cfg(feature = "stats")]
        stats_db: crate::open_stats_db().await,
    };
    let project = with_username(project).await;
    let session = build_session(&global_config, project, &shared).await?;
    open_window(Launch {
        session,
        more: Vec::new(),
        global_config,
        shared,
        demo: false,
    })
}

/// The window on the demo project (`gitlab-tracker --demo gui`): mock data,
/// no token, no network and no disk writes.
pub async fn run_demo(global_config: AppConfig) -> anyhow::Result<()> {
    let mut first = crate::demo::demo_app(global_config.clone()).await;
    crate::demo::enrich_for_window(&mut first);
    // "Me", for the Today page and the reviews waiting for me.
    first.config.gitlab_username = Some("thomas_db".into());
    let session = demo_session(first);
    // A second project, in a tab of its own, to show the tabs.
    let mut second = crate::demo::demo_app(global_config.clone()).await;
    second.project_id = "654321".into();
    second.project_name = Some("Mobile app".into());
    crate::demo::enrich_for_window(&mut second);
    second.mrs.truncate(5);
    second.config.gitlab_username = Some("thomas_db".into());
    let shared = Shared {
        palette: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        #[cfg(feature = "stats")]
        stats_db: None,
    };
    open_window(Launch {
        session,
        more: vec![demo_session(second)],
        global_config,
        shared,
        demo: true,
    })
}

/// A demo project's session: a 1 s tick, and statuses for the board's columns
/// (moving a ticket stays refused).
fn demo_session(mut app: App) -> Session {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let timer = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if timer.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });
    app.ticket_transitioner = Some(Arc::new(crate::demo::DemoTracker));
    Session {
        app,
        tx,
        rx,
        api_semaphore: Arc::new(Semaphore::new(1)),
        last_known_branches: HashMap::new(),
    }
}

/// With no project configured: asks for the first one in a window. `false`
/// when it was closed without one.
pub async fn first_run() -> anyhow::Result<bool> {
    setup::run(None).await
}

/// Everything the window starts from.
struct Launch {
    session: Session,
    /// More projects, open in tabs behind the first (the demo's).
    more: Vec<Session>,
    /// Kept to load another project from the project picker.
    global_config: AppConfig,
    shared: Shared,
    demo: bool,
}

/// Opens the window and runs until it is closed.
fn open_window(launch: Launch) -> anyhow::Result<()> {
    let choice = bevel_window::Choice::probe(TITLEBAR_PREFIX);
    // Borderless on Linux, as VS Code: the window draws its own title bar and
    // buttons whatever the desktop offers, so a slow or skipped Wayland probe
    // never falls back to winit's grey frame. `GITLAB_TRACKER_TITLEBAR=system`
    // keeps the desktop's frame.
    let frame = match (choice.overridden(), cfg!(target_os = "linux")) {
        (None, true) => bevel_window::Frame::App(bevel_window::Layout::from_desktop()),
        _ => choice.into_frame(),
    };
    let viewport = bevel_window::decorate(
        egui::ViewportBuilder::default()
            .with_title("GitLab Tracker")
            .with_app_id("gitlab-tracker")
            .with_icon(window_icon())
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        &frame,
    );
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_SIZE=3440x1440` opens the
    // window at that size, to check the layout at other screen widths.
    #[cfg(debug_assertions)]
    let viewport = match std::env::var("GITLAB_TRACKER_GUI_SIZE").ok().and_then(|v| {
        v.split_once('x')
            .map(|(w, h)| (w.parse::<f32>(), h.parse::<f32>()))
    }) {
        Some((Ok(w), Ok(h))) => viewport.with_inner_size([w, h]),
        _ => viewport,
    };
    // The size the window had when it was last closed (not for the demo).
    let viewport = match prefs::load().window.filter(|_| !launch.demo) {
        Some([w, h]) if w >= 960.0 && h >= 600.0 => viewport.with_inner_size([w, h]),
        _ => viewport,
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // eframe owns this thread until the window closes; the runtime's workers keep
    // running the fetches meanwhile.
    tokio::task::block_in_place(|| {
        eframe::run_native(
            "gitlab-tracker",
            options,
            Box::new(move |cc| Ok(Box::new(GuiApp::new(cc, launch, frame)))),
        )
    })
    .map_err(|e| anyhow::anyhow!("{e}"))
    .context("The GUI window failed")
}

/// The window and taskbar icon on X11, Windows and macOS. GNOME on Wayland takes
/// it from the desktop entry named like the app id (`assets/linux/`) instead.
fn window_icon() -> Arc<egui::IconData> {
    let png = include_bytes!("../../../assets/icon/png/gitlab-tracker-256.png");
    Arc::new(eframe::icon_data::from_png_bytes(png).expect("the bundled icon is a valid PNG"))
}

/// The page picked in the icon rail. The stats page also follows
/// `InputMode::Stats`, as the TUI's stats overlay does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    /// What waits for me, across the open tabs.
    Today,
    MergeRequests,
    /// The selected MR on a page of its own (double click, Enter).
    Mr,
    Cockpit,
    /// What is ready to go from one tracked branch to the next.
    Promote,
    Tracker,
    #[cfg(feature = "stats")]
    Stats,
    /// Who waits on whom: reviews, load, who reviews whom.
    Team,
    /// What happened, in order, across the open tabs.
    Journal,
    /// The tracked branches from feature to production, and their deployments.
    Environments,
    /// The pipelines: failing and flaky jobs, durations, blocked MRs.
    Ci,
    /// Progress of each milestone, and whether it will make its date.
    Milestones,
    /// Open MRs that change the same files.
    Overlaps,
    /// How fast changes reach production: DORA, where the time goes.
    Delivery,
    /// MRs opened on another MR's branch.
    Stacks,
}

/// An inspector section a shortcut brings into view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorSection {
    Review,
    Changes,
    Branches,
    Pipelines,
    Ticket,
    Description,
    /// The unresolved threads.
    Threads,
}

/// The window's state: the project session plus what only the window needs.
struct GuiApp {
    app: App,
    tx: UnboundedSender<AppEvent>,
    /// Events forwarded from the session channel; each one also requests a repaint.
    events: std::sync::mpsc::Receiver<AppEvent>,
    api_semaphore: Arc<Semaphore>,
    last_known_branches: HashMap<String, HashSet<String>>,
    global_config: AppConfig,
    shared: Shared,
    /// The open projects, as title-bar tabs, in order: the active one's state
    /// lives in the fields of this struct, the others' in their [`Slot`].
    tabs: Vec<Slot>,
    /// The project form (`+`, or a project without its token), while open.
    wizard: Option<setup::ProjectForm>,
    /// Redmine instances whose key was left empty in the project form: their
    /// projects open without tickets for the rest of the session, unasked.
    skip_redmine: HashSet<String>,
    ctx: egui::Context,
    /// Who draws the window frame; `Frame::App` adds window buttons to the title bar.
    frame: bevel_window::Frame,
    theme: bevel_theme::Theme,
    /// The demo project: events go through `demo::apply_demo_event`.
    demo: bool,
    /// The user's choice to show the side panels; `None` follows the window width.
    show_views: Option<bool>,
    show_inspector: Option<bool>,
    /// Whether the views panel showed on the last frame, once the width is applied:
    /// the `F` key toggles from it.
    views_shown: bool,
    /// The page shown when the stats are not open (see [`GuiApp::page`]).
    page: Page,
    /// The pages visited, for the back and forward buttons.
    history: history::History,
    /// Set by a shortcut: the inspector scrolls to this section on the next frame.
    scroll_to: Option<InspectorSection>,
    /// The MR whose ticket the inspector was last brought to on the tracker page;
    /// `None` away from that page.
    ticket_anchor: Option<String>,
    /// The text filter being edited in the views panel: its index in
    /// `App::filter_defs` and the text typed so far.
    text_filter: (Option<usize>, String),
    /// Set by the `/` shortcut: the search field takes focus on the next frame.
    focus_search: bool,
    /// Actions requested while drawing, dispatched once the frame is drawn.
    pending: Vec<Action>,
    /// Writes the state off the UI thread.
    state_writer: StateWriter,
    /// The selected MR's description, rendered in the inspector.
    description: markdown::Description,
    /// Diffs and job logs read on demand for the inspector.
    details: details::Details,
    /// The `language` setting of `projects.toml` (`None`: the system's), and the
    /// choice being edited in the settings dialog.
    language: Option<String>,
    language_draft: Option<String>,
    /// The ids of the listed MRs behind each cockpit counter (focus aside),
    /// computed once per frame: the cockpit, the rail badges and the focus use it.
    members: HashMap<String, Vec<String>>,
    /// The rail's badges of the pages under its rule, kept for a moment.
    rail_badges: rail_badges::Cache,
    /// The counters when the list was first complete: the cockpit's trends.
    baseline: Option<Baseline>,
    /// The cockpit hides its counters at zero.
    hide_zeros: bool,
    /// How the table groups its rows, and the groups folded away.
    group_by: table::GroupBy,
    folded: HashSet<String>,
    /// The MR page's tab, and whether it lists the resolved threads.
    mr_tab: mr_page::Tab,
    show_resolved: bool,
    /// The bell panel's filter, and whether it was open at the last frame (its
    /// notifications count as read once it closes).
    bell_filter: toasts::Filter,
    bell_open: bool,
    /// The URLs of the desktop notifications clicked: the window shows their MR.
    clicks: std::sync::mpsc::Receiver<String>,
    /// The branches the Promote page compares, once picked: from, to.
    promote: Option<(String, String)>,
    /// The tracker page shows its tickets as a board (columns by status).
    tracker_board: bool,
    /// The table's compact density: one line per MR.
    compact: bool,
    /// The table's Next step column is hidden (the window's column picker).
    hide_next_column: bool,
    /// The table shows the branches as one flow cell instead of a column each.
    flow_cells: bool,
    /// The Promote page's ready MRs left out of the release, by id.
    promote_skipped: HashSet<String>,
    /// The inspector's section last picked in its tabs.
    inspector_tab: InspectorSection,
    /// When the Today feed was last marked as seen.
    feed_seen_at: Option<chrono::DateTime<chrono::Local>>,
    /// The period the cockpit's trends compare with.
    trend_period: flow::Period,
    /// MRs picked with Ctrl / Shift + click, by id, and what the bulk bar asked for.
    marked: HashSet<String>,
    bulk: Option<table::Bulk>,
    /// The window's inner size, saved with the preferences.
    window_size: Option<egui::Vec2>,
    /// Picked while drawing, applied once the frame is drawn: a focus to show
    /// (`Some(None)` clears it) and an MR to select, by id.
    next_focus: Option<Option<Focus>>,
    select_id: Option<String>,
    /// The command palette (Ctrl+K), while open.
    palette: Option<palette::Palette>,
    /// What changed at the last refreshes, in the window's corner.
    toasts: toasts::Toasts,
    /// When the list was last complete (startup load or refresh), and whether
    /// fetches were running at the last frame.
    last_sync: Option<chrono::DateTime<chrono::Local>>,
    was_syncing: bool,
    /// The "since your last session" card was closed.
    digest_closed: bool,
    /// The project pages' choices.
    team: team::State,
    journal: journal::State,
    environments: environments::State,
    ci: ci::State,
    milestones: milestones::State,
    /// The Plan page's changelog: the project's git-cliff config, the dialog.
    changelog: changelog::State,
    overlaps: overlaps::State,
    delivery: delivery::State,
    stacks: stacks::State,
    /// The tracker's statuses (the board's columns), once fetched, and the
    /// fetch in flight.
    statuses: Option<Vec<gitlab_tracker_core::TicketTransitionTarget>>,
    statuses_rx:
        Option<std::sync::mpsc::Receiver<Vec<gitlab_tracker_core::TicketTransitionTarget>>>,
    /// Tickets being moved to another status: what to say once done.
    moves: (MoveSender, std::sync::mpsc::Receiver<MoveResult>),
    #[cfg(debug_assertions)]
    tour: Option<tour::Tour>,
    #[cfg(target_os = "macos")]
    movable: Option<bool>,
}

impl GuiApp {
    fn new(cc: &eframe::CreationContext<'_>, launch: Launch, frame: bevel_window::Frame) -> Self {
        let Launch {
            session,
            more,
            global_config,
            shared,
            demo,
        } = launch;
        bevel_theme::fonts::install(&cc.egui_ctx);
        let projects_config = block_on(crate::storage::load_projects_toml());
        let language = projects_config.language;
        i18n::install();
        i18n::set_language(language.as_deref());
        let first = Slot::active(&session.app);
        let Tab {
            app,
            tx,
            events,
            api_semaphore,
            last_known_branches,
            state_writer,
            page,
            history,
            ticket_anchor,
            text_filter,
            members,
            baseline,
            toasts,
            last_sync,
            was_syncing,
            digest_closed,
            statuses,
            statuses_rx,
            moves,
            marked,
            folded,
            details,
        } = Tab::new(session, &cc.egui_ctx);

        let mut gui = Self {
            app,
            tx,
            events,
            api_semaphore,
            last_known_branches,
            global_config,
            shared,
            tabs: vec![first],
            wizard: None,
            skip_redmine: HashSet::new(),
            ctx: cc.egui_ctx.clone(),
            frame,
            theme: bevel_theme::Theme::dark(),
            demo,
            show_views: None,
            show_inspector: None,
            views_shown: false,
            page,
            history,
            scroll_to: None,
            ticket_anchor,
            text_filter,
            focus_search: false,
            pending: Vec::new(),
            state_writer,
            description: markdown::Description::default(),
            details,
            language_draft: language.clone(),
            language,
            members,
            rail_badges: rail_badges::Cache::default(),
            baseline,
            hide_zeros: false,
            group_by: table::GroupBy::None,
            folded,
            tracker_board: false,
            promote: None,
            mr_tab: mr_page::Tab::Discussions,
            show_resolved: false,
            clicks: notification_clicks(&cc.egui_ctx),
            bell_filter: toasts::Filter::All,
            bell_open: false,
            compact: false,
            hide_next_column: false,
            flow_cells: true,
            promote_skipped: HashSet::new(),
            inspector_tab: InspectorSection::Review,
            feed_seen_at: None,
            trend_period: flow::Period::Month,
            marked,
            bulk: None,
            window_size: None,
            next_focus: None,
            select_id: None,
            palette: None,
            toasts,
            last_sync,
            was_syncing,
            digest_closed,
            team: team::State::default(),
            journal: journal::State::default(),
            environments: environments::State::default(),
            ci: ci::State::default(),
            milestones: milestones::State::default(),
            changelog: changelog::State::default(),
            overlaps: overlaps::State::default(),
            delivery: delivery::State::default(),
            stacks: stacks::State::default(),
            statuses,
            statuses_rx,
            moves,
            #[cfg(debug_assertions)]
            tour: tour::Tour::from_env(),
            #[cfg(target_os = "macos")]
            movable: None,
        };
        for session in more {
            let mut slot = Slot::active(&session.app);
            slot.state = SlotState::Parked(Box::new(Tab::new(session, &cc.egui_ctx)));
            gui.tabs.push(slot);
        }
        gui.set_theme(bevel_theme::Mode::Dark);
        if !gui.demo {
            let prefs = prefs::load();
            prefs::apply(&mut gui, &prefs);
            gui.history.clear();
            gui.restore_tabs(&prefs.tabs, &projects_config.projects);
        }
        gui
    }

    /// Fetches the tracker's statuses once (for the board's columns).
    fn ensure_statuses(&mut self) {
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
    fn move_ticket(&mut self, ticket: String, target: String, label: String) {
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
    fn poll_tracker(&mut self) {
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
    fn track_sync(&mut self) {
        let syncing = !self.app.pending_initial_fetches.is_empty()
            || !self.app.pending_refresh_fetches.is_empty();
        let first = self.last_sync.is_none() && !self.app.mrs.is_empty();
        if !syncing && (self.was_syncing || first) {
            self.last_sync = Some(chrono::Local::now());
        }
        self.was_syncing = syncing;
    }

    /// Applies the events received since the last frame, as the TUI loop does.
    fn apply_events(&mut self) {
        if self.demo {
            while let Ok(event) = self.events.try_recv() {
                crate::demo::apply_demo_event(&mut self.app, event);
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
        if needs_save {
            self.save_state();
        }
    }

    /// Applies the language picked in the settings dialog and saves it (not in
    /// the demo, which never writes to disk).
    fn save_language(&mut self) {
        if self.language_draft == self.language {
            return;
        }
        self.language = self.language_draft.clone();
        i18n::set_language(self.language.as_deref());
        if !self.demo && !self.app.read_only {
            block_on(crate::storage::set_language_async(self.language.clone()));
        }
    }

    /// Saves the state in the background: the frame never waits on the disk.
    fn save_state(&self) {
        save_state(&self.app, &self.last_known_branches, &self.state_writer);
    }

    fn dispatch_pending(&mut self) {
        let mut ctx = ActionContext {
            api_semaphore: &self.api_semaphore,
            tx: &self.tx,
            last_known_branches: &mut self.last_known_branches,
            state_writer: Some(&self.state_writer),
        };
        for action in std::mem::take(&mut self.pending) {
            block_on(dispatch(action, &mut self.app, &mut ctx));
        }
    }

    /// Opens the project the project picker asked for (`App::switch_to`).
    fn take_switch(&mut self) {
        if let Some(next) = self.app.switch_to.take() {
            self.open_project(next);
        }
    }

    /// Shows `project`: its tab when it is open, else a new tab, loaded in the
    /// background (the token lookup waits on the keyring) and shown once ready.
    fn open_project(&mut self, project: ProjectEntry) {
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
    fn load_slot(&self, project: ProjectEntry, show: bool) -> Slot {
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
    fn restore_tabs(&mut self, saved: &[prefs::SavedTab], projects: &[ProjectEntry]) {
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
    fn tab_of(&self, url: &str, id: &str) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.url == url.trim_end_matches('/') && t.id == id)
    }

    /// The index of the tab shown.
    fn active(&self) -> usize {
        self.tabs
            .iter()
            .position(|t| matches!(t.state, SlotState::Active))
            .unwrap_or(0)
    }

    /// Shows the tab `index` (a loaded one), parking the one shown.
    fn activate(&mut self, index: usize) {
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
    fn swap_tab(&mut self, tab: &mut Tab) {
        use std::mem::swap;
        swap(&mut self.app, &mut tab.app);
        swap(&mut self.tx, &mut tab.tx);
        swap(&mut self.events, &mut tab.events);
        swap(&mut self.api_semaphore, &mut tab.api_semaphore);
        swap(&mut self.last_known_branches, &mut tab.last_known_branches);
        swap(&mut self.state_writer, &mut tab.state_writer);
        swap(&mut self.page, &mut tab.page);
        swap(&mut self.history, &mut tab.history);
        swap(&mut self.ticket_anchor, &mut tab.ticket_anchor);
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
    fn after_swap(&mut self) {
        self.set_theme(self.theme.mode);
        self.bulk = None;
        self.next_focus = None;
        self.select_id = None;
        self.scroll_to = None;
        if !self.demo {
            let (url, id) = (self.app.base_url.clone(), self.app.project_id.clone());
            tokio::spawn(async move { crate::storage::set_active_project_async(&url, &id).await });
        }
    }

    /// Closes the tab `index`; the last one stays.
    fn close_tab(&mut self, index: usize) {
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
    fn poll_clicks(&mut self) {
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
    fn poll_tabs(&mut self, now: f64) {
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

    /// Applies `mode` to the window and to the text the terminal views produce.
    fn set_theme(&mut self, mode: bevel_theme::Mode) {
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
    fn on_stats(&self) -> bool {
        matches!(self.app.input_mode, crate::app::InputMode::Stats)
    }

    /// The page shown: the stats while they are open, else the one picked last.
    fn page(&self) -> Page {
        #[cfg(feature = "stats")]
        if self.on_stats() {
            return Page::Stats;
        }
        match self.page {
            Page::Tracker if self.app.tracker.is_none() => Page::MergeRequests,
            Page::Mr if self.app.selected_mr().is_none() => Page::MergeRequests,
            page => page,
        }
    }

    /// Shows `page`, opening or closing the stats as the TUI's `G` does.
    fn open_page(&mut self, page: Page) {
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
    fn focus_on(&mut self, key: &str, label: impl Into<String>) {
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

    /// Lists the MRs carrying `label` alone, on the merge requests page: the
    /// label view on it (one label at a time), no cockpit focus, no search.
    fn filter_by_label(&mut self, label: &str) {
        let Some(index) = self
            .app
            .filter_defs
            .iter()
            .position(|def| def.id == "label")
        else {
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
            index,
            query: label.to_string(),
        });
        self.open_page(Page::MergeRequests);
    }

    /// Deep link: the MR `id`'s page, out of the view or search that hides it
    /// (merged MRs hidden by `M` stay hidden: [`GuiApp::apply_picks`] says so).
    fn open_mr(&mut self, id: &str) {
        let visible = self.app.visible_mrs().any(|mr| mr.id == id);
        if !visible {
            if self.app.focus.is_some() {
                self.next_focus = Some(None);
            }
            self.pending.push(Action::SetFilter {
                index: 0,
                query: String::new(),
            });
            if self.app.status_pick.is_some() {
                self.pending.push(Action::SetStatusPick(None));
            }
            self.app.input.clear();
        }
        self.select_id = Some(id.to_string());
        self.open_page(Page::Mr);
    }

    /// Deep link across tabs: the MR `id` of the tab `tab`, on its page.
    fn open_link(&mut self, tab: usize, id: &str) {
        self.activate(tab);
        self.open_mr(id);
    }

    /// Selects the MR `id` on the merge requests page, every MR shown again.
    fn show_mr(&mut self, id: &str) {
        if self.app.focus.is_some() {
            self.next_focus = Some(None);
        }
        self.select_id = Some(id.to_string());
        self.open_page(Page::MergeRequests);
    }

    /// Counts the MRs behind each counter, before the frame's snapshot: the
    /// focus aside, and on the cockpit without it. Keeps the focus up to date.
    fn count_members(&mut self) {
        self.app.focus_suspended = true;
        self.members = crate::ui::cockpit::metric_members(&self.app);
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
    fn apply_bulk(&mut self) {
        let Some(bulk) = self.bulk.take() else {
            return;
        };
        let ids = self.marked.clone();
        match bulk {
            table::Bulk::Flag(on) => {
                for mr in self.app.mrs.iter_mut().filter(|m| ids.contains(&m.id)) {
                    mr.flagged = on;
                }
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
    fn apply_picks(&mut self) {
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

/// What moving a ticket said: the message to flash, or the error.
type MoveResult = Result<String, String>;
type MoveSender = std::sync::mpsc::Sender<MoveResult>;

/// The counters when the list was first complete, and when that was.
struct Baseline {
    counts: HashMap<String, usize>,
    at: String,
}

/// The MRs behind `key`: a counter of the cockpit, `release-open:<title>` (the
/// release's MRs still to merge), or `size:<min>:<max>` (the merged MRs with that
/// many changed lines, a bucket of the stats).
fn members_of(app: &App, members: &HashMap<String, Vec<String>>, key: &str) -> Vec<String> {
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
fn notification_clicks(ctx: &egui::Context) -> std::sync::mpsc::Receiver<String> {
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
fn forward_events(
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
enum Loaded {
    Session(Box<Session>),
    /// The tokens it lacks, asked in the project form.
    NeedsTokens(setup::Mode),
}

async fn load_project(
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
async fn with_username(mut project: ProjectEntry) -> ProjectEntry {
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
fn drain_events(
    app: &mut App,
    events: &std::sync::mpsc::Receiver<AppEvent>,
    api_semaphore: &Arc<Semaphore>,
    tx: &UnboundedSender<AppEvent>,
    last_known_branches: &mut HashMap<String, HashSet<String>>,
) -> bool {
    let mut needs_save = false;
    while let Ok(event) = events.try_recv() {
        needs_save |= app.apply_event(event, api_semaphore.clone(), tx, last_known_branches);
    }
    app.flush_pending_sort();
    app.ensure_time_entries(tx);
    needs_save
}

/// Saves a tab's state in the background: the frame never waits on the disk.
fn save_state(
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
struct Slot {
    /// GitLab URL (no trailing `/`) and project id: what the tab is for.
    url: String,
    id: String,
    /// Shown while it loads.
    name: String,
    state: SlotState,
}

enum SlotState {
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
    fn active(app: &App) -> Self {
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
struct Tab {
    app: App,
    tx: UnboundedSender<AppEvent>,
    events: std::sync::mpsc::Receiver<AppEvent>,
    api_semaphore: Arc<Semaphore>,
    last_known_branches: HashMap<String, HashSet<String>>,
    state_writer: StateWriter,
    page: Page,
    history: history::History,
    ticket_anchor: Option<String>,
    text_filter: (Option<usize>, String),
    members: HashMap<String, Vec<String>>,
    baseline: Option<Baseline>,
    toasts: toasts::Toasts,
    last_sync: Option<chrono::DateTime<chrono::Local>>,
    was_syncing: bool,
    digest_closed: bool,
    statuses: Option<Vec<gitlab_tracker_core::TicketTransitionTarget>>,
    statuses_rx:
        Option<std::sync::mpsc::Receiver<Vec<gitlab_tracker_core::TicketTransitionTarget>>>,
    moves: (MoveSender, std::sync::mpsc::Receiver<MoveResult>),
    marked: HashSet<String>,
    folded: HashSet<String>,
    details: details::Details,
}

impl Tab {
    fn new(session: Session, ctx: &egui::Context) -> Self {
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
            page: Page::MergeRequests,
            history: history::History::default(),
            ticket_anchor: None,
            text_filter: (None, String::new()),
            members: HashMap::new(),
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
    fn run_hidden(&mut self, theme: &bevel_theme::Theme, now: f64, demo: bool) {
        if demo {
            while let Ok(event) = self.events.try_recv() {
                crate::demo::apply_demo_event(&mut self.app, event);
            }
        } else if drain_events(
            &mut self.app,
            &self.events,
            &self.api_semaphore,
            &self.tx,
            &mut self.last_known_branches,
        ) {
            save_state(&self.app, &self.last_known_branches, &self.state_writer);
        }
        self.toasts.watch(&self.app, theme, now);
        let syncing = !self.app.pending_initial_fetches.is_empty()
            || !self.app.pending_refresh_fetches.is_empty();
        if !syncing && self.was_syncing {
            self.last_sync = Some(chrono::Local::now());
        }
        self.was_syncing = syncing;
    }

    /// Whether a fetch of this tab failed: its tab shows a warning.
    fn has_errors(&self) -> bool {
        self.app
            .mrs
            .iter()
            .any(|mr| matches!(mr.status, crate::models::MrStatus::Error(_)))
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_tabs(ui.input(|i| i.time));
        self.poll_clicks();
        self.apply_events();
        // Keys move the selection and change the list: before the frame's snapshot.
        keys::handle(self, ui.ctx());
        // Development aid (debug builds): `GITLAB_TRACKER_GUI_KEYS=g` types these
        // shortcuts once the window is up, to check a view without clicking;
        // `GITLAB_TRACKER_GUI_PAGE=cockpit` opens that page, and
        // `GITLAB_TRACKER_GUI_THEME=light` the light theme.
        #[cfg(debug_assertions)]
        if ui.ctx().cumulative_pass_nr() == 3 {
            for c in std::env::var("GITLAB_TRACKER_GUI_KEYS")
                .unwrap_or_default()
                .chars()
            {
                keys::character(self, c);
            }
            // `GITLAB_TRACKER_GUI_TABLE=columns,group` shows a column per branch
            // and groups the table by next step.
            let table = std::env::var("GITLAB_TRACKER_GUI_TABLE").unwrap_or_default();
            if table.split(',').any(|t| t == "columns") {
                self.flow_cells = false;
            }
            if table.split(',').any(|t| t == "group") {
                self.group_by = table::GroupBy::NextStep;
            }
            match std::env::var("GITLAB_TRACKER_GUI_PAGE").as_deref() {
                Ok("cockpit") => self.open_page(Page::Cockpit),
                Ok("tracker") => self.open_page(Page::Tracker),
                Ok("today") => self.open_page(Page::Today),
                Ok("promote") => self.open_page(Page::Promote),
                Ok("team") => self.open_page(Page::Team),
                Ok("journal") => self.open_page(Page::Journal),
                Ok("environments") => self.open_page(Page::Environments),
                Ok("ci") => self.open_page(Page::Ci),
                Ok("milestones") => self.open_page(Page::Milestones),
                Ok("overlaps") => self.open_page(Page::Overlaps),
                Ok("delivery") => self.open_page(Page::Delivery),
                Ok("stacks") => self.open_page(Page::Stacks),
                // The bell's panel, with notifications in both tabs.
                Ok("bell") => {
                    use toasts::Kind;
                    let now = self.ctx.input(|i| i.time);
                    let t = self.theme.tones;
                    let items = [
                        (
                            "106",
                            "chore(deps): Bump tokio to 1.37 and update async ecosystem",
                            Kind::PipelineFailed,
                            "!106 · pipeline failed",
                            t.red,
                        ),
                        (
                            "104",
                            "feat(api): Introduce GraphQL endpoint for MR metadata",
                            Kind::Merged,
                            "!104 · now MERGED",
                            t.purple,
                        ),
                        (
                            "102",
                            "fix(db): Resolve connection pool deadlocks under heavy load",
                            Kind::Status,
                            "!102 · now CONFLICT",
                            t.red,
                        ),
                        (
                            "101",
                            "feat(auth): Add OAuth2 PKCE flow for mobile clients",
                            Kind::PipelinePassed,
                            "!101 · pipeline passed",
                            t.green,
                        ),
                    ];
                    for (id, title, kind, text, tone) in items {
                        self.toasts.push(id, title, kind, text.into(), tone, now);
                    }
                    // Seen earlier: read.
                    if let Some(first) = self.toasts.history.first_mut() {
                        first.read = true;
                        first.when -= chrono::Duration::minutes(48);
                    }
                    if let Some(second) = self.toasts.history.get_mut(1) {
                        second.read = true;
                        second.when -= chrono::Duration::minutes(12);
                    }
                    for slot in &mut self.tabs {
                        if let SlotState::Parked(tab) = &mut slot.state {
                            tab.toasts.push(
                                "103",
                                "refactor(ui): Optimize Ratatui render loop",
                                Kind::Status,
                                "!103 · now APPROVED".into(),
                                t.green,
                                now,
                            );
                            if let Some(p) = tab.toasts.history.last_mut() {
                                p.when -= chrono::Duration::days(1);
                            }
                        }
                    }
                }
                Ok("mr") => {
                    // The demo's MR with the most to show: open, a conflict, threads.
                    let id = self
                        .app
                        .mrs
                        .iter()
                        .find(|mr| {
                            mr.mergeability == gitlab_tracker_core::MergeabilityStatus::Conflict
                        })
                        .map(|mr| mr.id.clone());
                    let id = std::env::var("GITLAB_TRACKER_GUI_MR").ok().or(id);
                    if let Some(id) = id {
                        self.select_id = Some(id);
                    }
                    self.open_page(Page::Mr);
                    if let Ok(tab) = std::env::var("GITLAB_TRACKER_GUI_MR_TAB") {
                        self.mr_tab = match tab.as_str() {
                            "changes" => mr_page::Tab::Changes,
                            "commits" => mr_page::Tab::Commits,
                            "pipelines" => mr_page::Tab::Pipelines,
                            _ => mr_page::Tab::Discussions,
                        };
                    }
                }
                // The inspector's diff (`diff`) and failed job's log (`log`).
                Ok(which @ ("diff" | "log")) => {
                    // The demo's failed pipeline is an older one: shown as the
                    // latest, the only one whose jobs are listed.
                    let failing = self.app.mrs.iter_mut().find(|mr| {
                        mr.pipelines
                            .iter()
                            .any(|p| p.jobs.iter().any(|j| j.status == "failed"))
                    });
                    if let Some(mr) = failing {
                        mr.pipelines
                            .sort_by_key(|p| p.status != crate::models::PipelineState::Failed);
                    }
                    let failing = self.app.mrs.iter().find(|mr| {
                        mr.pipelines
                            .first()
                            .is_some_and(|p| p.jobs.iter().any(|j| j.status == "failed"))
                    });
                    if let Some(id) = failing.map(|mr| mr.id.clone()) {
                        self.details.show_demo(&id);
                        if self.app.selected_mr().map(|mr| &mr.id) != Some(&id) {
                            self.select_id = Some(id);
                        }
                        self.show_inspector = Some(true);
                        self.scroll_to = Some(if which == "log" {
                            InspectorSection::Pipelines
                        } else {
                            InspectorSection::Changes
                        });
                    }
                }
                // The project picker and the project form, which the read-only
                // demo cannot open otherwise.
                Ok("projects") => {
                    self.app.project_picker = self
                        .tabs
                        .iter()
                        .map(|t| ProjectEntry {
                            name: Some(t.name.clone()),
                            gitlab_url: t.url.clone(),
                            project_id: t.id.clone(),
                            ..ProjectEntry::default()
                        })
                        .chain(std::iter::once(ProjectEntry {
                            name: Some("Billing API".into()),
                            gitlab_url: "https://gitlab.example.com".into(),
                            project_id: "billing/api".into(),
                            ..ProjectEntry::default()
                        }))
                        .collect();
                    self.app.input_mode = crate::app::InputMode::ProjectPicker;
                }
                Ok("tokens") => {
                    let mut entry = self.app.project_settings.clone();
                    entry.tracker = Some(crate::storage::TrackerConfig {
                        provider: "redmine".into(),
                        url: "https://redmine.example.com".into(),
                        extra: toml::Table::new(),
                    });
                    self.wizard = Some(setup::ProjectForm::new(
                        &self.ctx,
                        setup::Mode::Tokens {
                            entry: Box::new(entry),
                            gitlab: true,
                            redmine: true,
                        },
                    ));
                }
                Ok("wizard") => {
                    self.wizard = Some(setup::ProjectForm::new(&self.ctx, setup::Mode::New));
                }
                _ => {}
            }
            if std::env::var("GITLAB_TRACKER_GUI_THEME").as_deref() == Ok("light") {
                self.set_theme(bevel_theme::Mode::Light);
            }
        }
        #[cfg(debug_assertions)]
        tour::drive(self, ui.ctx());
        self.dispatch_pending();
        self.count_members();
        rail_badges::refresh(self, ui.ctx());
        self.track_sync();
        self.poll_tracker();
        self.window_size = ui
            .ctx()
            .input(|i| i.viewport().inner_rect.map(|r| r.size()));
        self.toasts
            .watch(&self.app, &self.theme, ui.input(|i| i.time));

        // Snapshot the visible list for this frame (see `App::begin_render_cache`):
        // drawing records actions instead of changing the list.
        self.app.begin_render_cache();
        shell::draw(self, ui);
        self.app.end_render_cache();
        self.app.focus_suspended = false;
        // Last, over the window's edges: an undecorated window (GNOME) resizes from them.
        bevel_window::resize::edges(ui, &self.frame);

        self.dispatch_pending();
        // After the actions: a view picked with an MR to select applies first.
        self.apply_picks();
        self.take_switch();

        #[cfg(target_os = "macos")]
        bevel_window::title_bar::sync_movable(ui.ctx(), &mut self.movable);

        #[cfg(debug_assertions)]
        screenshot_for_development(ui.ctx());
    }

    /// Writes the last state queued before the window closed, and the window's
    /// preferences.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        block_on(self.state_writer.flush());
        for slot in &self.tabs {
            if let SlotState::Parked(tab) = &slot.state {
                block_on(tab.state_writer.flush());
            }
        }
        if !self.demo {
            prefs::save(&prefs::current(self));
        }
    }
}

/// Runs a short async step (a file save, an action) to completion from the UI thread.
///
/// The UI thread is the runtime's `block_on` thread (inside `block_in_place`), so the
/// runtime context is current: tokio I/O inside `fut` works, and it never waits on
/// the network — fetches are spawned, not awaited.
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    futures::executor::block_on(fut)
}

/// Development aid (debug builds): with `GITLAB_TRACKER_GUI_SCREENSHOT=<file.ppm>`,
/// saves the window once it has settled, as a binary PPM, and closes it.
#[cfg(debug_assertions)]
fn screenshot_for_development(ctx: &egui::Context) {
    let Ok(path) = std::env::var("GITLAB_TRACKER_GUI_SCREENSHOT") else {
        return;
    };
    if ctx.cumulative_pass_nr() != 10 {
        ctx.request_repaint();
        return;
    }
    let close = ctx.clone();
    ctx.request_screenshot(move |image| {
        save_ppm(&image, std::path::Path::new(&path));
        close.send_viewport_cmd(egui::ViewportCommand::Close);
        close.request_repaint();
    });
    ctx.request_repaint();
}

/// Saves a screenshot as a binary PPM.
#[cfg(debug_assertions)]
fn save_ppm(image: &egui::ColorImage, path: &std::path::Path) {
    let [w, h] = image.size;
    let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
    for pixel in &image.pixels {
        ppm.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
    }
    if let Err(e) = std::fs::write(path, ppm) {
        tracing::warn!(error = %e, path = %path.display(), "Could not save the screenshot");
    }
}
