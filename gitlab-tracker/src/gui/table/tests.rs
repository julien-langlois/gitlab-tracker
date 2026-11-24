use super::*;

const WANTED: [Col; 6] = [
    Col::Effort,
    Col::Notes,
    Col::Milestone,
    Col::Target,
    Col::Behind,
    Col::Ticket,
];

#[tokio::test]
async fn quick_picks_take_the_shown_mrs_in_that_state() {
    let mut app = crate::app::tests::app_with_mrs(&[
        ("a", "None"),
        ("b", "None"),
        ("c", "None"),
        ("d", "None"),
    ])
    .await;
    app.config.hide_merged = false;
    app.mrs[0].state = GitlabMrState::Closed;
    app.mrs[1].state = GitlabMrState::Merged;
    app.mrs[3].state = GitlabMrState::Closed;
    let mut closed = quick_pick(&app, QuickPick::State(GitlabMrState::Closed));
    closed.sort();
    assert_eq!(closed, ["1", "4"]);
    assert_eq!(
        quick_pick(&app, QuickPick::State(GitlabMrState::Merged)),
        ["2"]
    );
    assert_eq!(quick_pick(&app, QuickPick::All).len(), 4);
    assert!(quick_pick(&app, QuickPick::None).is_empty());
    // Hidden by "Hide merged": not picked.
    app.config.hide_merged = true;
    assert!(quick_pick(&app, QuickPick::State(GitlabMrState::Merged)).is_empty());
    assert_eq!(quick_pick(&app, QuickPick::All).len(), 3);
}

#[test]
fn narrow_tables_drop_the_least_useful_columns_first() {
    // 1440 points with both side panels, as in the mockup: about 840 of table.
    let (kept, dropped, branch) = fit_columns(&WANTED, 840.0, 3);
    assert_eq!(kept, [Col::Effort, Col::Notes]);
    assert_eq!(dropped, 4);
    // The dots leave a little room, shared with the branch cells.
    assert!(
        (BRANCH_WIDTH..BRANCH_WIDTH + 8.0).contains(&branch),
        "{branch}"
    );
}

#[test]
fn wide_tables_keep_every_column_and_widen_the_branch_cells() {
    // Ultrawide at 100 %: everything fits, branch cells grow up to their cap.
    let (kept, dropped, branch) = fit_columns(&WANTED, 4300.0, 3);
    assert_eq!(kept, WANTED);
    assert_eq!(dropped, 0);
    assert_eq!(branch, BRANCH_MAX_WIDTH);
}

#[test]
fn columns_come_back_in_reverse_drop_order() {
    // 1080p with both panels: about 1100 points of table.
    let (kept, _, _) = fit_columns(&WANTED, 1100.0, 3);
    assert_eq!(kept, [Col::Effort, Col::Notes, Col::Milestone, Col::Target]);
    // Narrower still: effort and threads, then only the effort.
    let (kept, _, _) = fit_columns(&WANTED, 780.0, 3);
    assert_eq!(kept, [Col::Effort, Col::Notes]);
    let (kept, _, _) = fit_columns(&WANTED, 740.0, 3);
    assert_eq!(kept, [Col::Effort]);
}

#[test]
fn the_title_takes_the_width_left_by_the_other_columns() {
    let grid = Grid::new(&[Col::Effort], 3, BRANCH_WIDTH, 1200.0);
    assert_eq!(grid.width, 1200.0);
    let (_, x, w) = grid.cells[2];
    assert_eq!(grid.cells[2].0, Cell::Title);
    assert_eq!(x, ROW_PADDING + FLAG_WIDTH + ID_WIDTH + 2.0 * COLUMN_GAP);
    // Narrower than the columns need: the title keeps its minimum, the grid overflows.
    let narrow = Grid::new(&[Col::Effort], 3, BRANCH_WIDTH, 500.0);
    assert_eq!(narrow.cells[2].2, MIN_TITLE_WIDTH);
    assert!(narrow.width > 500.0);
    assert!(w > MIN_TITLE_WIDTH);
}
