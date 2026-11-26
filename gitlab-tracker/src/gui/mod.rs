//! Desktop window (`gitlab-tracker gui`, feature `gui`).
//!
//! The window drives the same [`App`] as the TUI: it loads the project with
//! `build_session`, applies the async [`AppEvent`]s each frame, and turns clicks and
//! keys into [`Action`]s handled by [`dispatch`]. Popups follow `App::input_mode`,
//! as in the TUI. Only the drawing lives here.

mod board;
mod calendar;
mod changelog;
mod ci;
mod cockpit;
mod colors;
mod delivery;
mod details;
mod dialogs;
mod environments;
mod export;
mod flow;
mod freeze;
mod history;
mod hotspots;
mod i18n;
mod icons;
mod inspector;
mod journal;
mod jump;
mod keys;
mod labels;
mod markdown;
mod memo;
mod merge_queue;
mod merged_list;
mod milestones;
mod mr_page;
mod mr_pipelines;
mod next;
mod overlaps;
mod palette;
mod person;
mod persona;
mod pipeline_timeline;
mod prefs;
mod promote;
mod rail_badges;
mod releases;
#[cfg(test)]
mod render_tests;
mod reviewers;
mod schedules;
mod setup;
mod shell;
mod sidebar;
mod stacks;
mod standup;
#[cfg(feature = "stats")]
mod stats;
mod table;
mod team;
mod tip;
mod toasts;
mod today;
mod tones;
#[cfg(debug_assertions)]
mod tour;
mod tracker;
mod views;
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

mod background;
mod launch;
#[cfg(test)]
mod load_tests;
mod navigation;
mod sessions;
mod tabs;

pub use launch::*;
use sessions::*;

/// The page picked in the icon rail. The stats page also follows
/// `InputMode::Stats`, as the TUI's stats overlay does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// The person picked on the team page, on a page of their own (double
    /// click, Enter).
    Person,
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
    /// What shipped in each release, and what the next one will hold.
    Releases,
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
    /// The text filter being edited in the views panel: its index in
    /// `App::filter_defs` and the text typed so far.
    text_filter: (Option<usize>, String),
    /// Set by the `/` shortcut: the search field takes focus on the next frame.
    focus_search: bool,
    /// Actions requested while drawing, dispatched once the frame is drawn.
    pending: Vec<Action>,
    /// Writes the state off the UI thread.
    state_writer: StateWriter,
    /// When the state was last copied for a save (see [`sessions::SaveClock`]).
    save_clock: sessions::SaveClock,
    /// The selected MR's description, rendered in the inspector and the MR page.
    description: markdown::Description,
    /// Diffs and job logs read on demand for the inspector.
    details: details::Details,
    /// The `language` setting of `projects.toml` (`None`: the system's), and the
    /// choice being edited in the settings dialog.
    language: Option<String>,
    language_draft: Option<String>,
    /// The rail's pages and the start page (Settings › Pages, › Startup), and
    /// the choice being edited in the settings dialog.
    rail: persona::Rail,
    rail_draft: persona::Rail,
    /// The page each restored tab showed when the window closed, by project
    /// (GitLab URL, id), until the tab is loaded.
    left_off: HashMap<(String, String), String>,
    /// The ids of the listed MRs behind each cockpit counter (focus aside),
    /// computed once per frame: the cockpit, the rail badges and the focus use it.
    members: Members,
    /// The rail's badges of the pages under its rule, kept for a moment.
    rail_badges: rail_badges::Cache,
    /// What each rail entry flagged when its page was last open, by project:
    /// its dot shows again only once that changes.
    rail_seen: HashMap<(String, Page), String>,
    /// The pages' analyses, kept between frames.
    memo: memo::Memos,
    /// The table's saved views: the one applied, the dialog open.
    views: views::State,
    /// The counters when the list was first complete: the cockpit's trends.
    baseline: Option<Baseline>,
    /// The cockpit hides its counters at zero.
    hide_zeros: bool,
    /// How the table groups its rows, and the groups folded away.
    group_by: table::GroupBy,
    folded: HashSet<String>,
    /// The Flow count the merge requests page was opened on (its banner).
    merged_gap: Option<merged_list::Gap>,
    /// The MR page's tab, and whether it lists the resolved threads.
    mr_tab: mr_page::Tab,
    /// The shortcuts to the MR page's sections.
    mr_jumps: jump::Jumps,
    show_resolved: bool,
    /// The MR whose description the MR page shows in full (folded otherwise).
    mr_description_open: Option<String>,
    /// The bell panel's filter, and whether it was open at the last frame (its
    /// notifications count as read once it closes).
    bell_filter: toasts::Filter,
    /// The latest run of each pipeline schedule the bell has seen.
    schedule_seen: schedules::Seen,
    /// The merge queues of the open project, worked out each frame.
    queues: Vec<merge_queue::Queue>,
    /// The export's choices, and a save under way.
    export: export::State,
    /// How the stand-up is written.
    standup: standup::Settings,
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
    /// The settings dialog's tab (a section's name) and its search.
    settings_tab: String,
    settings_search: String,
    /// The table's optional columns drawn last frame, once fitted to the window:
    /// the column picker's preview.
    shown_columns: Vec<table::Col>,
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
    /// The table's bulk mode: a box on each row, a click picks it.
    bulk_mode: bool,
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
    /// The Releases page: the release picked, the ranges of commits read.
    releases: releases::State,
    stacks: stacks::State,
    /// The tracker's statuses (the board's columns), once fetched, and the
    /// fetch in flight.
    statuses: Option<Vec<gitlab_tracker_core::TicketTransitionTarget>>,
    statuses_rx:
        Option<std::sync::mpsc::Receiver<Vec<gitlab_tracker_core::TicketTransitionTarget>>>,
    /// Tickets being moved to another status: what to say once done.
    moves: (MoveSender, std::sync::mpsc::Receiver<MoveResult>),
    /// A newer version of the app, shown in the title bar; the check in flight.
    update: Option<crate::update_check::Release>,
    update_rx: Option<std::sync::mpsc::Receiver<crate::update_check::Release>>,
    /// Whether to look for newer versions (Settings › Interface).
    update_check: bool,
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
        install_symbol_font(&cc.egui_ctx);
        // egui's 0.3 s for the second click of a double click is shorter than
        // the desktops' own: double clicks made at an ordinary pace counted as
        // two single clicks (the rows use `widgets::double_clicked`).
        cc.egui_ctx.options_mut(|o| {
            o.input_options.max_double_click_delay = widgets::DOUBLE_CLICK;
        });
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
            save_clock,
            page,
            history,
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
            text_filter,
            focus_search: false,
            pending: Vec::new(),
            state_writer,
            save_clock,
            description: markdown::Description::default(),
            details,
            language_draft: language.clone(),
            language,
            rail: persona::Rail::default(),
            rail_draft: persona::Rail::default(),
            left_off: HashMap::new(),
            members,
            rail_badges: rail_badges::Cache::default(),
            rail_seen: HashMap::new(),
            memo: memo::Memos::default(),
            views: views::State::default(),
            baseline,
            hide_zeros: false,
            group_by: table::GroupBy::None,
            merged_gap: None,
            folded,
            tracker_board: false,
            promote: None,
            mr_tab: mr_page::Tab::Discussions,
            mr_jumps: jump::Jumps::default(),
            show_resolved: false,
            mr_description_open: None,
            clicks: notification_clicks(&cc.egui_ctx),
            bell_filter: toasts::Filter::All,
            schedule_seen: schedules::Seen::new(),
            queues: Vec::new(),
            export: export::State::default(),
            standup: standup::Settings::default(),
            bell_open: false,
            compact: false,
            hide_next_column: false,
            flow_cells: true,
            settings_tab: String::new(),
            settings_search: String::new(),
            shown_columns: Vec::new(),
            promote_skipped: HashSet::new(),
            inspector_tab: InspectorSection::Review,
            feed_seen_at: None,
            trend_period: flow::Period::Month,
            marked,
            bulk: None,
            bulk_mode: false,
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
            releases: releases::State::default(),
            stacks: stacks::State::default(),
            statuses,
            statuses_rx,
            moves,
            update: None,
            update_rx: None,
            update_check: true,
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
        if gui.demo {
            let tones = gui.theme.tones;
            seed_demo_bell(&mut gui.toasts, &gui.app, &tones);
            for slot in &mut gui.tabs {
                if let SlotState::Parked(tab) = &mut slot.state {
                    seed_demo_bell(&mut tab.toasts, &tab.app, &tones);
                }
            }
        }
        if !gui.demo {
            let prefs = prefs::load();
            prefs::apply(&mut gui, &prefs);
            gui.history.clear();
            gui.restore_tabs(&prefs.tabs, &projects_config.projects);
        }
        if gui.demo {
            // The demo shows the case: a newer version out, not seen yet.
            let release = crate::update_check::demo();
            gui.toasts.push_update(&release, gui.theme.tones.blue, 0.0);
            gui.update = Some(release);
        } else if gui.update_check {
            gui.check_for_update();
        }
        gui
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_tabs(ui.input(|i| i.time));
        self.poll_clicks();
        self.poll_update(ui.input(|i| i.time));
        self.apply_events();
        self.wake_snoozed();
        export::poll(self);
        // Keys move the selection and change the list: before the frame's snapshot.
        keys::handle(self, ui.ctx());
        // Development aid (debug builds): `GITLAB_TRACKER_GUI_KEYS=g` types these
        // shortcuts once the window is up, to check a view without clicking;
        // `GITLAB_TRACKER_GUI_PAGE=cockpit` opens that page, and
        // `GITLAB_TRACKER_GUI_THEME=light` the light theme.
        // `GITLAB_TRACKER_GUI_JUMP=commits` jumps to that section of the page's
        // bar, once the page has measured its sections.
        #[cfg(debug_assertions)]
        if ui.ctx().cumulative_pass_nr() == 6 {
            if let Ok(id) = std::env::var("GITLAB_TRACKER_GUI_JUMP") {
                let id: &'static str = Box::leak(id.into_boxed_str());
                if self.page() == Page::Mr {
                    let mut jumps = std::mem::take(&mut self.mr_jumps);
                    mr_page::jump_to(self, &mut jumps, id);
                    self.mr_jumps = jumps;
                } else if let Some(jumps) = self.page_jumps() {
                    jumps.ask(id);
                }
            }
        }
        #[cfg(debug_assertions)]
        if ui.ctx().cumulative_pass_nr() == 3 {
            // `GITLAB_TRACKER_GUI_TAB=1` brings the second open project forward.
            if let Some(tab) = std::env::var("GITLAB_TRACKER_GUI_TAB")
                .ok()
                .and_then(|t| t.parse::<usize>().ok())
                .filter(|t| *t < self.tabs.len())
            {
                self.activate(tab);
            }
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
                // Today with the stand-up's panel open.
                Ok("standup") => self.open_page(Page::Today),
                // What a click on Today's "Merged today" opens.
                Ok("merged") => today::merged_by_me(self, self.active()),
                Ok("promote") => self.open_page(Page::Promote),
                // `GITLAB_TRACKER_GUI_PERSON=<login>` picks them in the inspector.
                Ok("team") => {
                    self.open_page(Page::Team);
                    if let Ok(login) = std::env::var("GITLAB_TRACKER_GUI_PERSON") {
                        self.team.selected = Some(login);
                    }
                }
                // A person's page: `GITLAB_TRACKER_GUI_PERSON=<login>`, else
                // the one the team page shows first (the most loaded).
                Ok("person") => {
                    self.open_page(Page::Team);
                    self.team.selected = std::env::var("GITLAB_TRACKER_GUI_PERSON").ok();
                    self.team.open = true;
                }
                Ok("journal") => self.open_page(Page::Journal),
                Ok("environments") => self.open_page(Page::Environments),
                Ok("ci") => self.open_page(Page::Ci),
                Ok("milestones") => self.open_page(Page::Milestones),
                Ok("overlaps") => self.open_page(Page::Overlaps),
                Ok("delivery") => self.open_page(Page::Delivery),
                Ok("releases") => self.open_page(Page::Releases),
                Ok("stacks") => self.open_page(Page::Stacks),
                // The bell's panel stays open (see `toasts`): the demo fills it.
                Ok("bell") => {}
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
                        self.details.show_demo(&self.app, &id);
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
            // `GITLAB_TRACKER_GUI_SETTINGS=Core` opens that tab of the settings
            // (with `GITLAB_TRACKER_GUI_KEYS=,`); `=?text` searches them.
            if let Ok(tab) = std::env::var("GITLAB_TRACKER_GUI_SETTINGS") {
                match tab.strip_prefix('?') {
                    Some(query) => self.settings_search = query.to_string(),
                    None => self.settings_tab = tab,
                }
            }
            if std::env::var("GITLAB_TRACKER_GUI_THEME").as_deref() == Ok("light") {
                self.set_theme(bevel_theme::Mode::Light);
            }
            // `GITLAB_TRACKER_GUI_RAIL=tech_lead,+releases,-stats,start=cockpit`:
            // a persona, pages shown (+) or hidden (-) on top, the start page
            // (`persona`, `left_off` or a page), as Settings › Pages would.
            if let Ok(spec) = std::env::var("GITLAB_TRACKER_GUI_RAIL") {
                let mut rail = persona::Rail::default();
                for part in spec.split(',') {
                    if let Some(page) = part.strip_prefix('+') {
                        rail.added.push(page.to_string());
                    } else if let Some(page) = part.strip_prefix('-') {
                        rail.removed.push(page.to_string());
                    } else if let Some(start) = part.strip_prefix("start=") {
                        rail.start = persona::Start::from_key(start);
                    } else {
                        rail.persona = persona::Persona::from_key(part);
                    }
                }
                self.rail = rail.clone();
                self.rail_draft = rail;
            }
            // `GITLAB_TRACKER_GUI_PALETTE=over`: Ctrl+K open, that typed.
            if let Ok(query) = std::env::var("GITLAB_TRACKER_GUI_PALETTE") {
                self.palette = Some(palette::Palette::with_query(&query));
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
        // The project's overview (trains, schedules), read once per refresh
        // interval whatever the page: the MR page and the table need it too.
        let ctx = ui.ctx().clone();
        self.details.project(&self.app, &ctx);
        self.queues =
            merge_queue::queues(&self.app, self.details.project_ready(), chrono::Utc::now());
        // The next step of an MR on a train is GitLab's: it needs the cars.
        self.app.merge_train = merge_queue::train_places(&self.queues);
        // A scheduled pipeline that failed since the last read, in the bell.
        if let Some(overview) = self.details.project_ready() {
            schedules::watch(
                &mut self.toasts,
                &mut self.schedule_seen,
                &self.app.project_id,
                &overview.schedules,
                &self.theme,
                ui.input(|i| i.time),
            );
        }

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

    /// Development aid (debug builds): `GITLAB_TRACKER_GUI_HOVER=x,y` puts the
    /// pointer there once the page is up, tooltips at once, so a screenshot
    /// shows what hovering says.
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let Ok(at) = std::env::var("GITLAB_TRACKER_GUI_HOVER") else {
            return;
        };
        let Some((x, y)) = at
            .split_once(',')
            .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
        else {
            return;
        };
        if ctx.cumulative_pass_nr() == 5 {
            // No fade either: the capture would catch the tooltip half drawn.
            ctx.global_style_mut(|style| {
                style.interaction.tooltip_delay = 0.0;
                style.animation_time = 0.0;
            });
            raw_input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(x, y)));
        }
    }

    /// Writes the last state queued before the window closed, and the window's
    /// preferences.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // What a refresh read since the last copy is saved before the window goes.
        if self.save_clock.take_due() {
            save_state(&self.app, &self.last_known_branches, &self.state_writer);
        }
        block_on(self.state_writer.flush());
        for slot in &mut self.tabs {
            if let SlotState::Parked(tab) = &mut slot.state {
                if tab.save_clock.take_due() {
                    save_state(&tab.app, &tab.last_known_branches, &tab.state_writer);
                }
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
    // Later with `GITLAB_TRACKER_GUI_SCREENSHOT_PASS`: a jump's scroll to end.
    let pass = std::env::var("GITLAB_TRACKER_GUI_SCREENSHOT_PASS")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(10);
    if ctx.cumulative_pass_nr() != pass {
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
