//! Which columns show and how wide: the grid the header and the rows share.

use super::*;

/// The optional columns, by their `ColumnDef` id (shared with the TUI's column picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    /// Who the MR waits on, and since when (the window's own: not in the TUI).
    Next,
    /// The latest pipeline, as an icon (the window's own).
    Ci,
    /// Its place in the merge train, or in the queue of the MRs set to
    /// auto-merge (the window's own; shown while some MR is in one).
    Queue,
    Effort,
    Notes,
    Activity,
    Labels,
    Milestone,
    Target,
    Behind,
    Ticket,
}

impl Col {
    /// Display order.
    pub(super) const ALL: [Col; 11] = [
        Col::Next,
        Col::Ci,
        Col::Queue,
        Col::Effort,
        Col::Notes,
        Col::Activity,
        Col::Labels,
        Col::Milestone,
        Col::Target,
        Col::Behind,
        Col::Ticket,
    ];
    /// The order columns are dropped in when the window is too narrow for them.
    pub(super) const DROP_ORDER: [Col; 11] = [
        Col::Activity,
        Col::Labels,
        Col::Ticket,
        Col::Behind,
        Col::Target,
        Col::Milestone,
        Col::Notes,
        Col::Effort,
        Col::Ci,
        // Only there while some MR is in a queue: kept as long as can be.
        Col::Queue,
        Col::Next,
    ];

    /// Drawn by the window alone: shown unless hidden in its column picker.
    pub fn window_only(self) -> bool {
        matches!(self, Col::Next | Col::Ci | Col::Queue)
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Col::Next => "next_step",
            Col::Ci => "ci",
            Col::Queue => "merge_queue",
            Col::Effort => "diff_stats",
            Col::Notes => "notes",
            Col::Activity => "activity",
            Col::Labels => "labels",
            Col::Milestone => "milestone",
            Col::Target => "target_branch",
            Col::Behind => "commits_behind",
            Col::Ticket => "tracker_ticket",
        }
    }

    pub(super) fn width(self) -> f32 {
        match self {
            Col::Next => 196.0,
            Col::Ci => 36.0,
            Col::Queue => 60.0,
            Col::Effort => 92.0,
            Col::Notes => 44.0,
            Col::Activity => 96.0,
            Col::Labels => 200.0,
            Col::Milestone => 140.0,
            Col::Target => 120.0,
            Col::Behind => 68.0,
            Col::Ticket => 150.0,
        }
    }

    pub(crate) fn title(self) -> &'static str {
        tl(match self {
            Col::Next => "Next step",
            Col::Ci => "CI",
            Col::Queue => "Queue",
            Col::Effort => "Effort",
            Col::Notes => "Threads",
            Col::Activity => "Activity",
            Col::Labels => "Labels",
            Col::Milestone => "Milestone",
            Col::Target => "Target",
            Col::Behind => "Behind",
            Col::Ticket => "Ticket",
        })
    }
}

/// The columns the user chose. Until they pick some (here or in the TUI), the
/// window shows a set suited to its width rather than the TUI's empty default.
pub(crate) fn wanted_columns(app: &App, hide_next: bool, queue: bool) -> Vec<Col> {
    let offered: Vec<&str> = app.visible_column_defs().iter().map(|c| c.id).collect();
    Col::ALL
        .into_iter()
        .filter(|c| c.window_only() || offered.contains(&c.id()))
        .filter(|c| {
            if *c == Col::Queue {
                queue
            } else if c.window_only() {
                !(hide_next && *c == Col::Next)
            } else {
                shows_column(app, c.id())
            }
        })
        .collect()
}

/// Whether the table shows the shared column `id`: the user's choice once made
/// (here or in the TUI), else the window's own set.
pub(crate) fn shows_column(app: &App, id: &str) -> bool {
    if app.project_settings.visible_columns.is_some() {
        app.config.visible_columns.is_visible(id)
    } else {
        !matches!(id, "activity" | "labels")
    }
}

pub(super) const FLAG_WIDTH: f32 = 18.0;
pub(super) const ID_WIDTH: f32 = 52.0;
pub(super) const STATUS_WIDTH: f32 = 116.0;
/// Fixed columns: flag, MR, status (the title takes the rest).
pub(super) const FIXED_COLUMNS_WIDTH: f32 = FLAG_WIDTH + ID_WIDTH + STATUS_WIDTH;
/// The title never gets narrower than this; below, the table scrolls sideways.
pub(super) const MIN_TITLE_WIDTH: f32 = 200.0;
/// Room the title keeps before an optional column is dropped.
pub(super) const TITLE_COMFORT_WIDTH: f32 = 220.0;
pub(super) const COLUMN_GAP: f32 = 8.0;
/// A dot per branch: narrow, the branch's name on top (in full on hover).
pub(super) const BRANCH_WIDTH: f32 = 50.0;
/// Branch cells stop growing there: the room left goes to the title.
pub(super) const BRANCH_MAX_WIDTH: f32 = 72.0;
/// Inside a row, left and right.
pub(super) const ROW_PADDING: f32 = 8.0;
pub(super) const ROW_HEIGHT: f32 = 50.0;
/// A row in the compact density: the title only.
pub(super) const COMPACT_ROW_HEIGHT: f32 = 34.0;
pub(super) const ROW_GAP: f32 = 2.0;

/// Fits the wanted columns into `width`: drops the least useful ones until the
/// title keeps its room, and widens the branch cells with what is left.
/// Returns the columns kept (in display order), how many were dropped, and the
/// width of each branch column.
pub fn fit_columns(wanted: &[Col], width: f32, branch_count: usize) -> (Vec<Col>, usize, f32) {
    fit_columns_with(wanted, width, branch_count, 0.0)
}

/// [`fit_columns`], with `extra` points taken by another fixed cell (the flow).
pub fn fit_columns_with(
    wanted: &[Col],
    width: f32,
    branch_count: usize,
    extra: f32,
) -> (Vec<Col>, usize, f32) {
    let base = ROW_PADDING * 2.0
        + FIXED_COLUMNS_WIDTH
        + extra
        + branch_count as f32 * BRANCH_WIDTH
        + (3 + branch_count) as f32 * COLUMN_GAP;
    let mut kept: Vec<Col> = wanted.to_vec();
    let used = |cols: &[Col]| cols.iter().map(|c| c.width() + COLUMN_GAP).sum::<f32>();
    let mut drop = Col::DROP_ORDER.iter();
    while base + used(&kept) + TITLE_COMFORT_WIDTH > width {
        let Some(next) = drop.next() else { break };
        kept.retain(|c| c != next);
    }
    let left = (width - base - used(&kept) - TITLE_COMFORT_WIDTH).max(0.0);
    let branch_width = if branch_count == 0 {
        BRANCH_WIDTH
    } else {
        (BRANCH_WIDTH + left / branch_count as f32 / 4.0).min(BRANCH_MAX_WIDTH)
    };
    (kept.clone(), wanted.len() - kept.len(), branch_width)
}

/// One column of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Cell {
    Flag,
    Id,
    Title,
    Status,
    Optional(Col),
    Branch(usize),
    /// Every branch in one cell, as a line of dots (see `GuiApp::flow_cells`).
    Flow,
}

/// Where each column sits inside a row, from its left edge.
pub(super) struct Grid {
    pub(super) cells: Vec<(Cell, f32, f32)>,
    pub(super) width: f32,
    pub(super) row_height: f32,
    /// The tracked branches, named in the flow cell's hover.
    pub(super) names: Vec<String>,
    /// Their short names, over the flow cell's dots and the branch columns.
    pub(super) aliases: Vec<String>,
    /// The space between two dots of the flow cell, for the widest alias.
    pub(super) flow_step: f32,
}

/// The tracked branches from feature to production (the timeline's order),
/// each with its short name: `(name, alias)`.
pub(super) fn branch_order(app: &crate::app::App) -> Vec<(String, String)> {
    app.branch_flow()
        .in_order()
        .into_iter()
        .map(|b| (b.name.clone(), b.alias.clone()))
        .collect()
}

/// The flow cell: a dot per branch, joined.
pub(super) fn flow_width(branches: usize, step: f32) -> f32 {
    (branches as f32 * step + 12.0).max(64.0)
}
/// The narrowest space between two dots of the flow cell.
pub(super) const FLOW_STEP: f32 = 30.0;

/// The space between two dots for the widest of `aliases` to sit over its dot.
pub(super) fn flow_step(ui: &Ui, theme: &Theme, aliases: &[String]) -> f32 {
    let font = egui::FontId::monospace(theme.text_size - 3.0);
    let widest = aliases
        .iter()
        .map(|a| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(a.clone(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
        })
        .fold(0.0, f32::max);
    (widest + 8.0).max(FLOW_STEP).ceil()
}

impl Grid {
    /// The branches in one flow cell instead of a column each.
    pub(super) fn with_flow(columns: &[Col], branches: usize, step: f32, width: f32) -> Self {
        let flow = flow_width(branches, step);
        let mut grid = Self::new(columns, 0, BRANCH_WIDTH, width - flow - COLUMN_GAP);
        let x = grid.width - ROW_PADDING + COLUMN_GAP;
        grid.cells.push((Cell::Flow, x, flow));
        grid.width += flow + COLUMN_GAP;
        grid.flow_step = step;
        grid
    }

    pub(super) fn new(columns: &[Col], branches: usize, branch_width: f32, width: f32) -> Self {
        let mut cells: Vec<(Cell, f32)> = vec![
            (Cell::Flag, FLAG_WIDTH),
            (Cell::Id, ID_WIDTH),
            (Cell::Title, 0.0),
            (Cell::Status, STATUS_WIDTH),
        ];
        cells.extend(columns.iter().map(|c| (Cell::Optional(*c), c.width())));
        cells.extend((0..branches).map(|i| (Cell::Branch(i), branch_width)));
        let fixed: f32 = cells.iter().map(|(_, w)| w).sum::<f32>()
            + COLUMN_GAP * (cells.len() - 1) as f32
            + ROW_PADDING * 2.0;
        let title = (width - fixed).max(MIN_TITLE_WIDTH);
        let mut x = ROW_PADDING;
        let cells = cells
            .into_iter()
            .map(|(cell, w)| {
                let w = if cell == Cell::Title { title } else { w };
                let placed = (cell, x, w);
                x += w + COLUMN_GAP;
                placed
            })
            .collect();
        Self {
            cells,
            width: fixed + title,
            row_height: ROW_HEIGHT,
            names: Vec::new(),
            aliases: Vec::new(),
            flow_step: FLOW_STEP,
        }
    }

    pub(super) fn rect(&self, row: Rect, x: f32, w: f32) -> Rect {
        Rect::from_min_size(
            egui::pos2(row.left() + x, row.top()),
            Vec2::new(w, row.height()),
        )
    }
}

// ── Rows ─────────────────────────────────────────────────────────────────────
