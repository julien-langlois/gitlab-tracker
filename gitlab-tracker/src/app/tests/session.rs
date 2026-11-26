//! The session: digest, flashes, stats, theme, panes, project picker.

use super::*;

#[tokio::test]
async fn session_digest_counts_what_changed_while_closed() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None")]).await;
    let opened = |branches: &[&str]| MrBaseline {
        state: GitlabMrState::Opened,
        branches: branches.iter().map(|b| b.to_string()).collect(),
        pipeline: Some(PipelineState::Success),
    };
    let baseline = HashMap::from([
        ("1".to_string(), opened(&[])),
        ("2".to_string(), opened(&["main"])),
        ("3".to_string(), opened(&[])),
    ]);
    // !1 got merged and reached main + staging; !2 now fails CI; !3 is unchanged.
    app.mrs[0].data.state = GitlabMrState::Merged;
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".into(), "staging".into()]));
    app.mrs[1].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[1].data.pipelines = vec![Pipeline {
        id: 1,
        status: PipelineState::Failed,
        created_at: None,
        jobs: vec![],
        ..Pipeline::default()
    }];
    app.mrs[2].status = MrStatus::MergedIn(HashSet::new());

    assert_eq!(
        session_digest(&baseline, &app.mrs).as_deref(),
        Some("Since last session: 1 merged · 2 new on a branch · 1 CI failing")
    );
    assert_eq!(session_digest(&baseline, &app.mrs[2..]), None);
    // The same changes one by one: as many as the digest counts.
    let changes = session_changes(&baseline, &app.mrs);
    assert_eq!(changes.len(), 4);
    assert_eq!(changes[0].kind, ChangeKind::Merged);
    assert!(changes
        .iter()
        .any(|c| c.id == "2" && c.kind == ChangeKind::CiFailing));
    assert!(session_changes(&baseline, &app.mrs[2..]).is_empty());
}
#[test]
fn flash_message_expires_after_a_few_ticks() {
    let mut app = test_app();
    app.flash("done", false);
    for _ in 1..FLASH_TICKS {
        app.tick_flash();
    }
    assert!(app.flash.is_some());
    app.tick_flash();
    assert!(app.flash.is_none());
}
#[cfg(feature = "stats")]
#[tokio::test]
async fn stats_snapshot_stores_sentinels_as_null() {
    use gitlab_tracker_stats::snapshot::SnapshotTrigger;
    let mut app = app_with_mrs(&[("t", "None")]).await;
    let profile = app.config.complexity_profile.clone();
    let mr = &mut app.mrs[0];
    // Placeholder: no assignee, no milestone.
    let snap = mr.stats_snapshot(SnapshotTrigger::OnRefresh, "42", &profile);
    assert_eq!((snap.assignee, snap.milestone), (None, None));

    mr.assignee = Some("Jane (@jane)".into());
    mr.milestone = Some("v1.0".into());
    let snap = mr.stats_snapshot(SnapshotTrigger::OnRefresh, "42", &profile);
    assert_eq!(snap.assignee.as_deref(), Some("Jane (@jane)"));
    assert_eq!(snap.milestone.as_deref(), Some("v1.0"));
    assert_eq!(snap.project_id, "42");
}
#[test]
fn mouse_focus_follows_rendered_pane_areas() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut app = test_app();
    app.layout.inspector_area = Rect::new(65, 0, 35, 20);
    app.layout.tracker_area = Some(Rect::new(65, 20, 35, 10));
    let mut hover = |column, row| {
        let kind = MouseEventKind::Moved;
        let modifiers = KeyModifiers::NONE;
        crate::events::handle_mouse_event(
            MouseEvent {
                kind,
                column,
                row,
                modifiers,
            },
            &mut app,
        );
        app.active_pane
    };
    assert_eq!(hover(10, 5), ActivePane::Dashboard);
    assert_eq!(hover(70, 19), ActivePane::Inspector);
    // First Tracker row: the old hand-made layout put this in the Inspector.
    assert_eq!(hover(70, 20), ActivePane::Tracker);
}
#[tokio::test]
async fn light_theme_never_uses_dark_only_foregrounds() {
    use crate::ui::theme::{self, Palette, ThemeMode};
    use ratatui::{backend::TestBackend, style::Color, Terminal};

    let mut app = app_with_mrs(&[("fix login", "v1.0"), ("add stats", "None")]).await;
    app.theme = Palette::for_mode(ThemeMode::Light);
    app.table_state.select(Some(0));
    theme::set_light(true);

    let dark = Palette::for_mode(ThemeMode::Dark);
    let dark_only = [
        Color::White,
        dark.fg,
        dark.muted,
        dark.muted_dim,
        dark.muted_hint,
    ];
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    for mode in [InputMode::Normal, InputMode::Help, InputMode::ColumnPicker] {
        app.input_mode = mode;
        terminal
            .draw(|f| crate::ui::render_ui(f, &mut app))
            .unwrap();
        for cell in terminal.backend().buffer().content() {
            // Text on the terminal's own (light) background must use light colours.
            if cell.bg == Color::Reset && cell.symbol().trim() != "" {
                assert!(
                    !dark_only.contains(&cell.fg),
                    "{:?}: {:?} drawn in dark-theme colour {:?}",
                    app.input_mode,
                    cell.symbol(),
                    cell.fg
                );
            }
        }
    }
    theme::set_light(false);
}
#[test]
fn legacy_state_sentinels_restore_as_none() {
    use crate::models::without_sentinel;
    for legacy in ["None", "none", "Loading", "", "  "] {
        assert_eq!(without_sentinel(Some(legacy.into())), None, "{legacy:?}");
    }
    assert_eq!(
        without_sentinel(Some("v1.0".into())).as_deref(),
        Some("v1.0")
    );
    assert_eq!(without_sentinel(None), None);
}
#[cfg(feature = "stats")]
#[test]
fn stale_stats_report_is_ignored() {
    let mut view = StatsViewState::default();
    let old = view.next_generation(); // e.g. 30-day window
    view.loading = true;
    let new = view.next_generation(); // [W] pressed: 90-day window
    let invalid = |msg: &str| gitlab_tracker_stats::StatsError::InvalidData(msg.into());
    view.apply_result(old, Err(invalid("old window")));
    assert!(
        view.loading && view.error.is_none(),
        "late response ignored"
    );
    view.apply_result(new, Err(invalid("new window")));
    assert!(!view.loading);
    assert_eq!(view.error.as_deref(), Some("Invalid data: new window"));
}
#[tokio::test]
async fn project_picker_only_switches_to_another_project() {
    let mut app = test_app();
    let entry = |url: &str, id: &str| ProjectEntry {
        gitlab_url: url.into(),
        project_id: id.into(),
        ..ProjectEntry::default()
    };
    // The current project (trailing slash ignored), then another one.
    app.project_picker = vec![
        entry("https://gitlab.example/", "1"),
        entry("https://gitlab.example", "2"),
    ];
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    app.input_mode = InputMode::ProjectPicker;
    press(&mut app, &tx, KeyCode::Enter).await;
    assert!(
        app.switch_to.is_none(),
        "re-selecting the current project is a no-op"
    );

    app.input_mode = InputMode::ProjectPicker;
    press(&mut app, &tx, KeyCode::Down).await;
    press(&mut app, &tx, KeyCode::Enter).await;
    assert_eq!(app.switch_to.map(|p| p.project_id), Some("2".to_string()));
    assert_eq!(app.input_mode, InputMode::Normal);
}
#[cfg(feature = "stats")]
#[tokio::test]
async fn demo_stats_overlay_uses_the_real_handler() {
    let mut app = app_with_mrs(&[("t", "None")]).await;
    app.read_only = true;
    app.stats_db = crate::demo::seed_demo_stats_db("1").await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    press(&mut app, &tx, KeyCode::Char('g')).await;
    assert_eq!(app.input_mode, InputMode::Stats);
    let first = app.stats_view.generation;
    assert!(first > 0, "opening the overlay starts a report");

    // [W] used to change the title only: it now recomputes the report.
    let window = app.stats_view.window;
    press(&mut app, &tx, KeyCode::Char('w')).await;
    assert_ne!(app.stats_view.window, window);
    assert!(app.stats_view.generation > first);

    // Esc used to quit the demo from the overlay: it closes the overlay.
    assert!(!press(&mut app, &tx, KeyCode::Esc).await);
    assert_eq!(app.input_mode, InputMode::Normal);
    // Quitting needs a confirmation, as in the app.
    assert!(!press(&mut app, &tx, KeyCode::Esc).await);
    assert!(press(&mut app, &tx, KeyCode::Char('y')).await);
}
#[test]
fn pane_scroll_stops_at_the_last_rendered_line() {
    let mut pane = PaneScroll {
        offset: 0,
        content_lines: 30,
        height: 10,
    };
    pane.down(15);
    assert_eq!(pane.offset, 15);
    pane.down(15);
    assert_eq!(pane.offset, 20, "30 lines in a 10-row pane: max offset 20");
    pane.up(25);
    assert_eq!(pane.offset, 0);
    pane.down(3);
    pane.reset();
    assert_eq!(pane.offset, 0);
}
