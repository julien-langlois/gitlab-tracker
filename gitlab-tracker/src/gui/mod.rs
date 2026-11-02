//! Desktop window (`gitlab-tracker gui`, feature `gui`).
//!
//! The window drives the same [`App`] as the TUI: it loads the project with
//! `build_session`, applies the async [`AppEvent`]s each frame, and turns clicks and
//! keys into [`Action`]s handled by [`dispatch`]. Popups follow `App::input_mode`,
//! as in the TUI. Only the drawing lives here.

mod cockpit;
mod colors;
mod dialogs;
mod i18n;
mod icons;
mod inspector;
mod keys;
mod markdown;
mod palette;
mod shell;
mod sidebar;
#[cfg(feature = "stats")]
mod stats;
mod table;
mod toasts;
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

/// Loads `project` like the TUI does (the token lookup may still prompt in the
/// terminal), then opens the window and runs until it is closed.
pub async fn run(global_config: AppConfig, project: ProjectEntry) -> anyhow::Result<()> {
    let shared = Shared {
        palette: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        #[cfg(feature = "stats")]
        stats_db: crate::open_stats_db().await,
    };
    let session = build_session(&global_config, project, &shared).await?;
    open_window(Launch {
        session,
        global_config,
        shared,
        demo: false,
    })
}

/// The window on the demo project (`gitlab-tracker --demo gui`): mock data,
/// no token, no network and no disk writes.
pub async fn run_demo(global_config: AppConfig) -> anyhow::Result<()> {
    let app = crate::demo::demo_app(global_config.clone()).await;
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
    let session = Session {
        app,
        tx,
        rx,
        api_semaphore: Arc::new(Semaphore::new(1)),
        last_known_branches: HashMap::new(),
    };
    let shared = Shared {
        palette: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        #[cfg(feature = "stats")]
        stats_db: None,
    };
    open_window(Launch {
        session,
        global_config,
        shared,
        demo: true,
    })
}

/// Everything the window starts from.
struct Launch {
    session: Session,
    /// Kept to load another project from the project picker.
    global_config: AppConfig,
    shared: Shared,
    demo: bool,
}

/// Opens the window and runs until it is closed.
fn open_window(launch: Launch) -> anyhow::Result<()> {
    let frame = bevel_window::Choice::probe(TITLEBAR_PREFIX).into_frame();
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
    MergeRequests,
    Cockpit,
    Tracker,
    #[cfg(feature = "stats")]
    Stats,
}

/// An inspector section a shortcut brings into view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorSection {
    Pipelines,
    Ticket,
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
    /// The projects of `projects.toml`, shown as title-bar tabs (none in the demo).
    projects: Vec<ProjectEntry>,
    ctx: egui::Context,
    /// Who draws the window frame; `Frame::App` adds window buttons to the title bar.
    frame: bevel_window::Frame,
    theme: bevel_theme::Theme,
    /// The demo project: events go through `demo::apply_demo_event`.
    demo: bool,
    /// The user's choice to show the side panels; `None` follows the window width.
    show_views: Option<bool>,
    show_inspector: Option<bool>,
    /// The page shown when the stats are not open (see [`GuiApp::page`]).
    page: Page,
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
    /// The `language` setting of `projects.toml` (`None`: the system's), and the
    /// choice being edited in the settings dialog.
    language: Option<String>,
    language_draft: Option<String>,
    /// The ids of the listed MRs behind each cockpit counter (focus aside),
    /// computed once per frame: the cockpit, the rail badges and the focus use it.
    members: HashMap<String, Vec<String>>,
    /// The counters when the list was first complete: the cockpit's trends.
    baseline: Option<Baseline>,
    /// The cockpit hides its counters at zero.
    hide_zeros: bool,
    /// How the table groups its rows, and the groups folded away.
    group_by: table::GroupBy,
    folded: HashSet<String>,
    /// The tracker page shows its tickets as a board (columns by status).
    tracker_board: bool,
    /// Picked while drawing, applied once the frame is drawn: a focus to show
    /// (`Some(None)` clears it) and an MR to select, by id.
    next_focus: Option<Option<Focus>>,
    select_id: Option<String>,
    /// The command palette (Ctrl+K), while open.
    palette: Option<palette::Palette>,
    /// What changed at the last refreshes, in the window's corner.
    toasts: toasts::Toasts,
    /// The project being loaded in the background (see [`GuiApp::switch_project`]).
    switching: Option<std::sync::mpsc::Receiver<anyhow::Result<Loaded>>>,
    #[cfg(debug_assertions)]
    tour: Option<tour::Tour>,
    #[cfg(target_os = "macos")]
    movable: Option<bool>,
}

impl GuiApp {
    fn new(cc: &eframe::CreationContext<'_>, launch: Launch, frame: bevel_window::Frame) -> Self {
        let Launch {
            session,
            global_config,
            shared,
            demo,
        } = launch;
        let Session {
            app,
            tx,
            rx,
            api_semaphore,
            last_known_branches,
        } = session;
        let events = forward_events(rx, &cc.egui_ctx);
        bevel_theme::fonts::install(&cc.egui_ctx);
        let projects_config = block_on(crate::storage::load_projects_toml());
        let language = projects_config.language;
        i18n::install();
        i18n::set_language(language.as_deref());
        let projects = if demo {
            Vec::new()
        } else {
            projects_config.projects
        };

        let mut gui = Self {
            app,
            tx,
            events,
            api_semaphore,
            last_known_branches,
            global_config,
            shared,
            projects,
            ctx: cc.egui_ctx.clone(),
            frame,
            theme: bevel_theme::Theme::dark(),
            demo,
            show_views: None,
            show_inspector: None,
            page: Page::MergeRequests,
            scroll_to: None,
            ticket_anchor: None,
            text_filter: (None, String::new()),
            focus_search: false,
            pending: Vec::new(),
            state_writer: StateWriter::default(),
            description: markdown::Description::default(),
            language_draft: language.clone(),
            language,
            switching: None,
            members: HashMap::new(),
            baseline: None,
            hide_zeros: false,
            group_by: table::GroupBy::None,
            folded: HashSet::new(),
            tracker_board: false,
            next_focus: None,
            select_id: None,
            palette: None,
            toasts: toasts::Toasts::default(),
            #[cfg(debug_assertions)]
            tour: tour::Tour::from_env(),
            #[cfg(target_os = "macos")]
            movable: None,
        };
        gui.set_theme(bevel_theme::Mode::Dark);
        gui
    }

    /// Applies the events received since the last frame, as the TUI loop does.
    fn apply_events(&mut self) {
        if self.demo {
            while let Ok(event) = self.events.try_recv() {
                crate::demo::apply_demo_event(&mut self.app, event);
            }
            return;
        }
        let mut needs_save = false;
        while let Ok(event) = self.events.try_recv() {
            needs_save |= self.app.apply_event(
                event,
                self.api_semaphore.clone(),
                &self.tx,
                &mut self.last_known_branches,
            );
        }
        self.app.flush_pending_sort();
        self.app.ensure_time_entries(&self.tx);
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
        let app = &self.app;
        if let Some(file) = StateFile::new(
            &app.mrs,
            &self.last_known_branches,
            app.discovery_started_at.as_deref(),
            &app.dismissed_mr_ids,
            &app.base_url,
            &app.project_id,
        ) {
            self.state_writer.save(file);
        }
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

    /// Starts loading the project picked in the project picker, as the TUI does,
    /// in the background: the token lookup waits on the keyring, or even on a
    /// prompt in the terminal the window was started from. The window keeps
    /// showing the current project until [`GuiApp::finish_switch`] swaps it.
    fn switch_project(&mut self) {
        let Some(next) = self.app.switch_to.take() else {
            return;
        };
        if self.switching.is_some() {
            self.app.flash(tl("Another project is still loading"), true);
            return;
        }
        self.save_state();
        let name = next.name.clone().unwrap_or_else(|| next.project_id.clone());
        self.app
            .flash(tf("Loading {project}…", &[("project", &name)]), false);

        let (done, loaded) = std::sync::mpsc::channel();
        self.switching = Some(loaded);
        let global_config = self.global_config.clone();
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            let _ = done.send(load_project(&global_config, next, &shared).await);
            ctx.request_repaint();
        });
    }

    /// Swaps in the project [`GuiApp::switch_project`] loaded, once it is ready.
    fn finish_switch(&mut self) {
        let Some(loaded) = self.switching.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.switching = None;
        match loaded {
            Ok(Loaded { session, projects }) => {
                // The old project's events would apply to the new one: save what
                // they changed first (`events` goes with the old session).
                self.apply_events();
                let Session {
                    app,
                    tx,
                    rx,
                    api_semaphore,
                    last_known_branches,
                } = session;
                self.app = app;
                self.tx = tx;
                self.events = forward_events(rx, &self.ctx);
                self.api_semaphore = api_semaphore;
                self.last_known_branches = last_known_branches;
                self.text_filter = (None, String::new());
                self.baseline = None;
                self.toasts = toasts::Toasts::default();
                self.set_theme(self.theme.mode);
                self.projects = projects;
                let name = self.app.project_name.clone();
                let switched = tf(
                    "Switched to {project}",
                    &[("project", name.as_deref().unwrap_or(&self.app.project_id))],
                );
                self.app.flash(format!("✔ {switched}"), false);
            }
            Err(e) => {
                tracing::warn!(error = %e, "Project switch failed — staying on the current project");
                self.app
                    .flash(format!("✘ {}: {e:#}", tl("Project switch failed")), true);
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
            page => page,
        }
    }

    /// Shows `page`, opening or closing the stats as the TUI's `G` does.
    fn open_page(&mut self, page: Page) {
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
        let ids = members_of(&self.members, key).into_iter().collect();
        self.next_focus = Some(Some(Focus {
            key: key.to_string(),
            label: label.into(),
            ids,
        }));
        self.open_page(Page::MergeRequests);
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
        if let Some(focus) = &mut self.app.focus {
            focus.ids = members_of(&self.members, &focus.key).into_iter().collect();
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

    /// Applies what the frame picked: the focus, then the MR to select.
    fn apply_picks(&mut self) {
        if let Some(focus) = self.next_focus.take() {
            self.app.focus_suspended = false;
            self.app.set_focus(focus);
        }
        if let Some(id) = self.select_id.take() {
            let found = self.app.visible_mrs().position(|mr| mr.id == id);
            if let Some(index) = found {
                self.app.table_state.select(Some(index));
                self.app.layout.inspector.reset();
            }
        }
    }
}

/// The counters when the list was first complete, and when that was.
struct Baseline {
    counts: HashMap<String, usize>,
    at: String,
}

/// The MRs behind `key`: a counter of the cockpit, or `release-open:<title>`,
/// the release's MRs still to merge.
fn members_of(members: &HashMap<String, Vec<String>>, key: &str) -> Vec<String> {
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

/// A project loaded by [`GuiApp::switch_project`], with `projects.toml` reread
/// once it is marked active.
struct Loaded {
    session: Session,
    projects: Vec<ProjectEntry>,
}

async fn load_project(
    global_config: &AppConfig,
    project: ProjectEntry,
    shared: &Shared,
) -> anyhow::Result<Loaded> {
    let session = build_session(global_config, project, shared).await?;
    crate::storage::set_active_project_async(&session.app.base_url, &session.app.project_id).await;
    let projects = crate::storage::load_projects_toml().await.projects;
    Ok(Loaded { session, projects })
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.finish_switch();
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
            match std::env::var("GITLAB_TRACKER_GUI_PAGE").as_deref() {
                Ok("cockpit") => self.open_page(Page::Cockpit),
                Ok("tracker") => self.open_page(Page::Tracker),
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
        self.switch_project();

        #[cfg(target_os = "macos")]
        bevel_window::title_bar::sync_movable(ui.ctx(), &mut self.movable);

        #[cfg(debug_assertions)]
        screenshot_for_development(ui.ctx());
    }

    /// Writes the last state queued before the window closed.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        block_on(self.state_writer.flush());
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
