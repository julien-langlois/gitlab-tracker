use super::ticks::FLASH_TICKS;
use super::*;
use crate::models::Pipeline;
use crossterm::event::KeyCode;

mod branches;
mod list;
mod refresh;
mod session;

/// An empty app on project `1`, shared with the other modules' tests.
pub(crate) fn test_app() -> App {
    App::new(AppInit {
        token: std::sync::Arc::new("t".to_string().into()),
        project_id: "1".into(),
        base_url: "https://gitlab.example".into(),
        project_name: None,
        refresh_interval_secs: 900,
        sentinel_interval_secs: 120,
        config: AppConfig::default(),
        theme: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        project_settings: ProjectEntry {
            name: None,
            gitlab_url: "https://gitlab.example".into(),
            project_id: "1".into(),
            active: true,
            default_branches: None,
            table_label_prefixes: None,
            complexity_profile: None,
            tracked_branches: None,
            branch_flow: None,
            refresh_interval_secs: None,
            sentinel_interval_secs: None,
            activity_stale_days: None,
            activity_recent_days: None,
            review_reminder_hours: None,
            hotfix_branches: None,
            release_note_tickets: None,
            freeze_periods: None,
            show_cockpit: None,
            hide_merged: None,
            cockpit_thresholds: None,
            stats: None,
            visible_columns: None,
            views: Vec::new(),
            label_colors: None,
            signed_commits: None,
            tracker: None,
            gitlab_username: None,
            discover_new_mrs: None,
        },
    })
}
/// Tracks MRs "1".."n" with the given `(title, milestone)` pairs.
pub(crate) async fn app_with_mrs(mrs: &[(&str, &str)]) -> App {
    let mut app = test_app();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    for (i, _) in mrs.iter().enumerate() {
        app.apply_event(
            AppEvent::MrAdded((i + 1).to_string()),
            Arc::new(Semaphore::new(1)),
            &tx,
            &mut HashMap::new(),
        );
    }
    // Built outside an event drain: no selection move left pending.
    app.selection_moved = false;
    for (mr, (title, milestone)) in app.mrs.iter_mut().zip(mrs) {
        mr.title = (*title).into();
        // Test shorthand: "None" means no milestone.
        mr.milestone = Some(*milestone).filter(|m| *m != "None").map(Into::into);
    }
    app
}
fn ids(app: &App) -> Vec<&str> {
    app.mrs.iter().map(|mr| mr.id.as_str()).collect()
}
/// Tracker double: time-entry responses are queued per test; calls are counted.
#[derive(Default)]
struct FakeTracker {
    time_entries: std::sync::Mutex<
        std::collections::VecDeque<
            Result<Vec<gitlab_tracker_core::TimeEntry>, gitlab_tracker_core::TrackerError>,
        >,
    >,
    time_entry_calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl gitlab_tracker_core::TrackerProvider for FakeTracker {
    fn name(&self) -> &'static str {
        "Fake"
    }
    fn detect_ticket_id(&self, _title: &str, _description: &str) -> Option<String> {
        None
    }
    async fn fetch_ticket(
        &self,
        ticket_id: &str,
    ) -> Result<gitlab_tracker_core::LinkedTicket, gitlab_tracker_core::TrackerError> {
        Err(gitlab_tracker_core::TrackerError::NotFound(
            ticket_id.into(),
        ))
    }
    fn ticket_url(&self, ticket_id: &str) -> String {
        format!("https://tracker.example/{ticket_id}")
    }
    async fn fetch_time_entries(
        &self,
        _ticket_id: &str,
    ) -> Result<Vec<gitlab_tracker_core::TimeEntry>, gitlab_tracker_core::TrackerError> {
        self.time_entry_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.time_entries
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(vec![]))
    }
}
async fn press(app: &mut App, tx: &UnboundedSender<AppEvent>, code: KeyCode) -> bool {
    crate::events::handle_key_event(
        crossterm::event::KeyEvent::from(code),
        app,
        &Arc::new(Semaphore::new(1)),
        tx,
        &mut HashMap::new(),
    )
    .await
}
fn scanned(
    app: &mut App,
    commits: Option<BranchCommits>,
    scan: crate::gitlab::BranchScan,
    head: &str,
) {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    app.pending_branch_scans = 1;
    app.apply_event(
        AppEvent::BranchCommitsLoaded {
            branch: "prod".into(),
            commits,
            scan,
            head: Some(head.into()),
        },
        Arc::new(Semaphore::new(1)),
        &tx,
        &mut HashMap::new(),
    );
}
fn shas(list: &[&str]) -> BranchCommits {
    BranchCommits {
        shas: list.iter().map(|s| s.to_string()).collect(),
        ..BranchCommits::default()
    }
}
