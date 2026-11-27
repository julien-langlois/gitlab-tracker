//! Opening the window: language, fonts, icon, the demo's start and the first run.

use super::*;

/// Environment prefix of the title-bar override: `GITLAB_TRACKER_TITLEBAR=app|system`.
pub(super) const TITLEBAR_PREFIX: bevel_window::Prefix =
    bevel_window::Prefix::new("GITLAB_TRACKER");

/// Loads `project` like the TUI does, then opens the window and runs until it is
/// closed. The window never prompts in the terminal: a missing token is asked in
/// a window first.
pub async fn run(global_config: AppConfig, project: ProjectEntry) -> anyhow::Result<()> {
    gitlab_tracker_core::secrets::set_interactive(false);
    install_language().await;
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
    let probe = project.clone();
    let session = build_session(&global_config, project, &shared).await?;
    read_username(&probe, session.tx.clone());
    open_window(Launch {
        session,
        more: Vec::new(),
        global_config,
        shared,
        demo: false,
    })
}

/// Speaks the window's language before the session is built: what the session
/// writes while loading (the "Since last session" digest, flashes) is then
/// translated too, not left in English.
pub(super) async fn install_language() {
    i18n::install();
    i18n::set_language(
        crate::storage::load_projects_toml()
            .await
            .language
            .as_deref(),
    );
}

/// The demo's notifications of the last hours, kept for the bell (no toast:
/// they came before the window opened).
pub(super) fn seed_demo_bell(toasts: &mut toasts::Toasts, app: &App, tones: &bevel_theme::Tones) {
    use crate::demo::Notice;
    for (id, title, notice, at, read) in crate::demo::demo_notifications(app) {
        let state = app
            .mrs
            .iter()
            .find(|mr| mr.id == id)
            .map(|mr| (mr.state, mr.mergeability));
        let (kind, text, tone) = match notice {
            Notice::PipelineFailed => (
                toasts::Kind::PipelineFailed,
                tf("!{id} · pipeline failed", &[("id", &id)]),
                tones.red,
            ),
            Notice::PipelinePassed => (
                toasts::Kind::PipelinePassed,
                tf("!{id} · pipeline passed", &[("id", &id)]),
                tones.green,
            ),
            Notice::Merged | Notice::Closed => {
                let (state, mergeability) = state.unwrap_or_default();
                let (status, tone) = tones::status(state, &mergeability, tones);
                let kind = if matches!(notice, Notice::Merged) {
                    toasts::Kind::Merged
                } else {
                    toasts::Kind::Status
                };
                (
                    kind,
                    tf("!{id} · now {status}", &[("id", &id), ("status", status)]),
                    tone,
                )
            }
        };
        toasts.history.push(toasts::Past {
            id,
            title,
            kind,
            text,
            tone,
            when: at.with_timezone(&chrono::Local),
            read,
        });
    }
    // The tracked branches red this morning, unread.
    let overview = crate::demo::demo_project_overview(app);
    for red in super::ci::red_branches(app, &overview.pipelines) {
        let Some(at) = super::board::parse_time(&red.first_red.created_at) else {
            continue;
        };
        let (text, title) = super::ci::bell_text(&red);
        toasts.history.push(toasts::Past {
            id: super::ci::branch_id(&red.branch),
            title,
            kind: toasts::Kind::BranchRed,
            text,
            tone: tones.red,
            when: at.with_timezone(&chrono::Local),
            read: false,
        });
    }
    toasts.history.sort_by_key(|p| p.when);
}

/// The window on the demo project (`gitlab-tracker --demo gui`): mock data,
/// no token, no network and no disk writes.
pub async fn run_demo(global_config: AppConfig) -> anyhow::Result<()> {
    install_language().await;
    let first = crate::demo::demo_app(global_config.clone()).await;
    let session = demo_session(first);
    // A second project, in a tab of its own: the mobile app of the same team.
    let second = crate::demo::mobile_app(global_config.clone()).await;
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
pub(super) fn demo_session(mut app: App) -> Session {
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
pub(super) struct Launch {
    pub(super) session: Session,
    /// More projects, open in tabs behind the first (the demo's).
    pub(super) more: Vec<Session>,
    /// Kept to load another project from the project picker.
    pub(super) global_config: AppConfig,
    pub(super) shared: Shared,
    pub(super) demo: bool,
}

/// Opens the window and runs until it is closed.
pub(super) fn open_window(launch: Launch) -> anyhow::Result<()> {
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

/// The symbols the window draws that Inter lacks (▸ ▾ ✕ ✎ ≡ ◉ ◌ ℹ…), from
/// DejaVu Sans (`assets/fonts/`, Bitstream Vera licence), after every other font
/// of each family: whatever the machine's fonts, none is drawn as a box. The
/// GUI tests list any character no bundled font has.
pub(super) fn install_symbol_font(ctx: &egui::Context) {
    let families = [
        egui::FontFamily::Proportional,
        egui::FontFamily::Monospace,
        bevel_theme::fonts::medium(1.0).family,
        bevel_theme::fonts::semibold(1.0).family,
    ];
    ctx.add_font(egui::FontInsert::new(
        "DejaVuSans",
        egui::FontData::from_static(include_bytes!("../../assets/fonts/DejaVuSans.ttf")),
        families
            .into_iter()
            .map(|family| egui::InsertFontFamily {
                family,
                priority: egui::FontPriority::Lowest,
            })
            .collect(),
    ));
}

/// The window and taskbar icon on X11, Windows and macOS. GNOME on Wayland takes
/// it from the desktop entry named like the app id (`assets/linux/`) instead.
pub(super) fn window_icon() -> Arc<egui::IconData> {
    let png = include_bytes!("../../assets/window-icon.png");
    Arc::new(eframe::icon_data::from_png_bytes(png).expect("the bundled icon is a valid PNG"))
}
