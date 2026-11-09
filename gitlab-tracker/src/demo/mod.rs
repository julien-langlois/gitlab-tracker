//! The demo (`--demo`): two projects of a parcel-delivery company, read-only,
//! with no token, no network and no disk writes.
//!
//! Each project is written as what happened to it ([`orbit_api`],
//! [`orbit_mobile`]): its MRs as GitLab records them (opened, pushed, reviewed,
//! approved, merged…), its promotions, copies and deployments. [`engine`] plays
//! that history back once, and everything the app shows is read from it: the
//! list, the branches, the pipelines and their logs, the MR pages, the
//! deployments, the stats, the tickets. So the pages agree with each other,
//! and the dates follow the day the demo runs.

#![cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "the window reads the rest of the history")
)]

mod clock;
mod engine;
#[cfg(feature = "gui")]
mod gitlab;
#[cfg(feature = "gui")]
mod logs;
mod model;
pub mod orbit_api;
pub mod orbit_mobile;
#[cfg(feature = "stats")]
mod stats;
mod tracker;

pub(crate) use tracker::DemoTracker;

use crate::app::{App, AppInit};
use crate::config::{AppConfig, VisibleColumns};
use crate::events::{handle_key_event, handle_mouse_event};
use crate::models::{AppEvent, GitLabMilestone};
use crate::ui;
use crossterm::event::{self, Event, KeyEventKind};
use engine::World;
use std::sync::OnceLock;
use std::time::Duration;

const GITLAB_URL: &str = "https://gitlab.orbit-logistics.dev";

/// The two projects, played once.
fn worlds() -> &'static [World] {
    static WORLDS: OnceLock<Vec<World>> = OnceLock::new();
    WORLDS.get_or_init(|| {
        vec![
            World::build(orbit_api::spec()),
            World::build(orbit_mobile::spec()),
        ]
    })
}

fn world(project_id: &str) -> Option<&'static World> {
    worlds().iter().find(|w| w.spec.id == project_id)
}

/// The world and index of an MR, by its URL.
fn find(web_url: &str) -> Option<(&'static World, usize)> {
    worlds().iter().find_map(|w| {
        w.mrs
            .iter()
            .position(|mr| w.web_url(&mr.iid) == web_url)
            .map(|i| (w, i))
    })
}

/// The iid the demo gave to the MR named `key` (tests).
#[cfg(test)]
pub fn iid(key: &str) -> String {
    worlds()
        .iter()
        .find_map(|w| w.mr(key).map(|i| w.mrs[i].iid.clone()))
        .unwrap_or_else(|| panic!("no demo MR {key}"))
}

/// The main project: the back end, with the stats.
pub async fn demo_app(config: AppConfig) -> App {
    app_for(&worlds()[0], config, 1_904_000).await
}

/// The second project, in a tab of its own: the Android app.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub async fn mobile_app(config: AppConfig) -> App {
    app_for(&worlds()[1], config, 1_917_500).await
}

async fn app_for(world: &'static World, config: AppConfig, first_time_entry: u64) -> App {
    let spec = &world.spec;
    let config = AppConfig {
        visible_columns: {
            let mut columns = VisibleColumns::default();
            columns.set_visible("activity", true);
            columns.set_visible("milestone", true);
            columns.set_visible("notes", true);
            columns
        },
        gitlab_username: Some(orbit_api::ME.into()),
        complexity_profile: spec.profile.clone(),
        ..config
    };
    let project = crate::storage::ProjectEntry {
        name: Some(spec.name.into()),
        gitlab_url: GITLAB_URL.into(),
        project_id: spec.id.into(),
        active: true,
        tracked_branches: Some(spec.branches.iter().map(|b| b.to_string()).collect()),
        branch_flow: spec.flow.clone(),
        complexity_profile: Some(spec.profile.clone()),
        refresh_interval_secs: Some(900),
        activity_stale_days: Some(7),
        activity_recent_days: Some(2),
        show_cockpit: Some(true),
        gitlab_username: Some(orbit_api::ME.into()),
        ..Default::default()
    };
    let mut app = App::new(AppInit {
        token: std::sync::Arc::new("demo-token".to_string().into()),
        project_id: spec.id.into(),
        base_url: GITLAB_URL.into(),
        project_name: Some(spec.name.into()),
        refresh_interval_secs: 900,
        config,
        // The demo always uses the dark palette: no terminal query needed.
        theme: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        project_settings: project,
    });

    let tickets = tracker::Tickets::new(spec.tickets.clone());
    app.branches = spec.branches.iter().map(|b| b.to_string()).collect();
    app.mrs = (0..world.mrs.len())
        .map(|i| world.tracked_mr(i, &tickets))
        .collect();
    app.branch_arrivals = world.branch_arrivals();
    app.branch_commits = world.branch_commits();
    app.milestones = spec
        .milestones
        .iter()
        .map(|m| GitLabMilestone {
            title: m.title.into(),
            due_date: m.due.map(|due| clock::date(clock::clock().at(due))),
            description: Some(m.description.into()),
        })
        .collect();

    // What changed since the app was last closed.
    let last = clock::clock().at(spec.last_session);
    let before = (0..world.mrs.len())
        .filter_map(|i| {
            let (state, branches, pipeline) = world.as_of(i, last)?;
            Some((world.mrs[i].iid.clone(), state, branches, pipeline))
        })
        .collect();
    app.changes_since(before);

    #[cfg(feature = "stats")]
    {
        app.stats_db = stats::seed(world, &app.mrs, spec.id, &spec.profile).await;
    }

    // The tracker's data is loaded up front: the demo drops fetch results.
    app.tracker = Some(std::sync::Arc::new(DemoTracker));
    app.activities = tracker::activities();
    app.time_entries = tickets.time_entries(first_time_entry);
    app.sort_mrs();
    app.table_state.select(Some(0));
    // The demo runs the real key handler; this flag turns off its disk writes
    // and network requests.
    app.read_only = true;
    app
}

/// The stats database of the main project, recorded under `project_id`
/// (the stats tests).
#[cfg(all(feature = "stats", test))]
pub(crate) async fn seed_demo_stats_db(
    project_id: &str,
) -> Option<std::sync::Arc<gitlab_tracker_stats::SqliteStatsDb>> {
    let world = &worlds()[0];
    let tickets = tracker::Tickets::new(world.spec.tickets.clone());
    let mrs: Vec<_> = (0..world.mrs.len())
        .map(|i| world.tracked_mr(i, &tickets))
        .collect();
    stats::seed(world, &mrs, project_id, &world.spec.profile).await
}

/// Applies `event` to the demo app: only the clock and the stats results, since
/// every other event (add / remove MR…) would mutate or fetch.
pub fn apply_demo_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::Tick => {
            app.tick_flash();
            if app.time_left > 0 {
                app.time_left -= 1;
            } else {
                app.time_left = app.refresh_interval_secs;
            }
        }
        // Route stats results into app state so the overlay re-renders.
        #[cfg(feature = "stats")]
        AppEvent::StatsReportLoaded { generation, result } => {
            app.stats_view.apply_result(generation, result);
        }
        #[cfg(feature = "stats")]
        AppEvent::StatsCockpitLoaded { generation, result } => {
            app.stats_view.apply_cockpit_result(generation, result);
        }
        _ => {}
    }
}

/// The terminal demo.
pub async fn run_demo_mode(config: AppConfig) -> anyhow::Result<()> {
    use anyhow::Context;

    let mut app = demo_app(config).await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    // Required by the key handler's signature; unused in read-only mode.
    let api_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let mut last_known_branches = std::collections::HashMap::new();
    let tx_timer = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let _ = tx_timer.send(AppEvent::Tick);
        }
    });

    // Enable mouse capture so VHS scroll simulation works in demo mode.
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
        .context("Failed to enable mouse capture")?;

    let mut terminal = ratatui::init();
    let _terminal_guard = crate::utils::TerminalGuard;

    loop {
        while let Ok(event) = rx.try_recv() {
            apply_demo_event(&mut app, event);
        }

        terminal
            .draw(|f| ui::render_ui(f, &mut app))
            .context("Failed to draw the terminal")?;

        if event::poll(Duration::from_millis(50)).context("Failed to poll terminal input")? {
            match event::read().context("Failed to read terminal input")? {
                Event::Mouse(mouse) => {
                    handle_mouse_event(mouse, &mut app);
                }
                // Repeat too, so held keys scroll smoothly in recordings.
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    let quit = handle_key_event(
                        key,
                        &mut app,
                        &api_semaphore,
                        &tx,
                        &mut last_known_branches,
                    )
                    .await;
                    if quit {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    // `_terminal_guard` disables mouse capture and restores the terminal on drop.
    Ok(())
}

// ── What the window reads from "GitLab" ──────────────────────────────────────

/// What an MR's page reads when it opens.
#[cfg(feature = "gui")]
pub fn demo_page_data(mr: &crate::models::TrackedMr) -> crate::gitlab_detail::PageData {
    find(&mr.web_url).map_or_else(
        || crate::gitlab_detail::PageData {
            overview: None,
            conflicts: None,
            rules: None,
            threads: Ok(Vec::new()),
            deployments: Vec::new(),
            behind: None,
        },
        |(world, i)| world.page_data(i),
    )
}

/// The project's deployments and branch pipelines.
#[cfg(feature = "gui")]
pub fn demo_project_overview(app: &App) -> crate::gitlab_detail::ProjectOverview {
    world(&app.project_id)
        .map(World::overview)
        .unwrap_or_default()
}

/// The files an MR changes, with their patches.
#[cfg(feature = "gui")]
pub fn demo_file_diffs(app: &App, mr_id: &str) -> Vec<crate::gitlab::FileDiff> {
    world(&app.project_id)
        .and_then(|w| w.mrs.iter().position(|m| m.iid == mr_id).map(|i| (w, i)))
        .map(|(w, i)| w.file_diffs(i))
        .unwrap_or_default()
}

/// The end of a job's log.
#[cfg(feature = "gui")]
pub fn demo_job_log(app: &App, job_id: u64) -> String {
    world(&app.project_id)
        .and_then(|w| w.job_log(job_id))
        .unwrap_or_else(|| "The job's log is no longer kept.".into())
}

/// What a notification in the bell is about.
#[cfg(feature = "gui")]
pub enum Notice {
    PipelineFailed,
    PipelinePassed,
    Merged,
    Closed,
}

/// The notifications the window showed this morning: the MRs' pipelines that
/// ended, the merges and the closes of the last hours, newest last. Those older
/// than an hour and a half were seen already.
#[cfg(feature = "gui")]
pub fn demo_notifications(
    app: &App,
) -> Vec<(String, String, Notice, chrono::DateTime<chrono::Utc>, bool)> {
    use crate::models::PipelineState;
    let Some(world) = world(&app.project_id) else {
        return Vec::new();
    };
    let since = world.now - chrono::Duration::hours(8);
    let seen = world.now - chrono::Duration::minutes(90);
    let mut list = Vec::new();
    for (i, mr) in world.mrs.iter().enumerate() {
        if mr.spec.fetch == model::Fetch::Loading {
            continue;
        }
        let title = mr.spec.title.to_string();
        for p in world.pipelines_of(i) {
            let Some(done) = p.finished.filter(|at| *at >= since) else {
                continue;
            };
            let kind = match p.status {
                PipelineState::Failed => Notice::PipelineFailed,
                PipelineState::Success => Notice::PipelinePassed,
                _ => continue,
            };
            list.push((mr.iid.clone(), title.clone(), kind, done, done < seen));
        }
        if let Some((at, _)) = mr.merged.filter(|(at, _)| *at >= since) {
            list.push((mr.iid.clone(), title.clone(), Notice::Merged, at, at < seen));
        }
        if let Some((at, _)) = mr.closed.filter(|(at, _)| *at >= since) {
            list.push((mr.iid.clone(), title.clone(), Notice::Closed, at, at < seen));
        }
    }
    list.sort_by_key(|n| n.3);
    list
}

/// The release notes' input: the tag of the last release, and the MRs merged
/// into the default branch since.
#[cfg(feature = "gui")]
pub fn demo_changelog(app: &App) -> (String, Vec<String>) {
    let Some(world) = world(&app.project_id) else {
        return (String::new(), Vec::new());
    };
    // The last milestone delivered: its due date is the release.
    let released = world
        .spec
        .milestones
        .iter()
        .filter_map(|m| Some((m.title, clock::clock().at(m.due?))))
        .filter(|(title, due)| {
            *due < world.now && title.starts_with(|c: char| c == 'v' || c.is_ascii_digit())
        })
        .max_by_key(|(_, due)| *due);
    let (tag, since) = released.unwrap_or(("", world.now - chrono::Duration::days(90)));
    let titles = world
        .mrs
        .iter()
        .filter(|mr| mr.merged.is_some_and(|(at, _)| at > since))
        .filter(|mr| world.spec.branches.first() == Some(&mr.spec.target))
        .map(|mr| mr.spec.title.to_string())
        .collect();
    (tag.to_string(), titles)
}

#[cfg(test)]
mod tests;
