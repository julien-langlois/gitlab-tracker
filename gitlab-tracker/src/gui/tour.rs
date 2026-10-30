//! Development aid (debug builds): `GITLAB_TRACKER_GUI_TOUR=<dir>` walks through the
//! pages and the dialogs, saves one binary PPM per step (`01.ppm`, `02.ppm`…) in
//! `dir`, and closes the window. It drives the app from the inside, so it needs no
//! input tool and works the same on Wayland and X11.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{keys, GuiApp, Page};
use crate::app::InputMode;

/// How long a step gets to settle before its picture, in seconds: the stats load
/// in the background.
const SETTLE: f64 = 1.0;

/// One picture each. The selection stays on the first demo MR: it has pipelines
/// and a ticket to show in the inspector.
const STEPS: &[fn(&mut GuiApp)] = &[
    |_| {},
    |g| keys::character(g, 'p'),
    |g| keys::character(g, 't'),
    |g| g.open_page(Page::Cockpit),
    |g| g.open_page(Page::Tracker),
    #[cfg(feature = "stats")]
    |g| g.open_page(Page::Stats),
    #[cfg(feature = "stats")]
    |g| keys::character(g, '2'),
    #[cfg(feature = "stats")]
    |g| keys::character(g, '3'),
    #[cfg(feature = "stats")]
    |g| keys::character(g, '4'),
    #[cfg(feature = "stats")]
    |g| keys::character(g, '5'),
    |g| {
        g.open_page(Page::MergeRequests);
        keys::character(g, 'c');
    },
    |g| dialog(g, ','),
    |g| dialog(g, '?'),
];

/// Replaces the open dialog with the one `shortcut` opens.
fn dialog(g: &mut GuiApp, shortcut: char) {
    g.app.input_mode = InputMode::Normal;
    keys::character(g, shortcut);
}

/// How far the tour has gone.
pub struct Tour {
    dir: PathBuf,
    /// The next step to run.
    step: usize,
    /// When the current step's picture is due.
    shot_at: f64,
    /// Set from the screenshot callback, once the picture is saved.
    saved: Arc<AtomicBool>,
}

impl Tour {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("GITLAB_TRACKER_GUI_TOUR")?);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(error = %e, path = %dir.display(), "Could not create the tour directory");
            return None;
        }
        Some(Self {
            dir,
            step: 0,
            shot_at: f64::INFINITY,
            // Nothing to wait for before the first step.
            saved: Arc::new(AtomicBool::new(true)),
        })
    }
}

/// Runs the next step or takes the current picture, when due. Called before the
/// frame's pending actions are dispatched, so a step's action shows in that frame.
pub fn drive(g: &mut GuiApp, ctx: &egui::Context) {
    let Some(tour) = &mut g.tour else {
        return;
    };
    ctx.request_repaint();
    let now = ctx.input(|i| i.time);
    if tour.saved.load(Ordering::Acquire) {
        let Some(step) = STEPS.get(tour.step) else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        let step = *step;
        tour.step += 1;
        tour.shot_at = now + SETTLE;
        tour.saved.store(false, Ordering::Release);
        step(g);
    } else if now >= tour.shot_at {
        tour.shot_at = f64::INFINITY;
        let path = tour.dir.join(format!("{:02}.ppm", tour.step));
        let saved = tour.saved.clone();
        ctx.request_screenshot(move |image| {
            super::save_ppm(&image, &path);
            saved.store(true, Ordering::Release);
        });
    }
}
