//! The merge requests table and its toolbar.
//!
//! Rows are drawn by hand rather than with a table widget: each one is a rounded
//! card (selection ring, highlight of the MRs updated by the last refresh), and
//! only the rows in view are laid out.

use bevel_theme::{fonts, Chip, Theme, Tone};
use bevel_widgets::buttons::IconButton;
use egui::{
    Align, Color32, CornerRadius, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Ui,
    UiBuilder, Vec2,
};

use super::icons::{self, Icon};
use super::shell::Sizes;
use super::widgets::{
    activity, avatar, badge, button, field, icon_button, icon_toggle, label_tone, mono, muted,
    semibold, split_person, FieldLook,
};
use super::{tones, GuiApp};
use crate::actions::Action;
use crate::app::{App, InputMode, SortColumn, SortOrder};
use crate::i18n::{tf, tl, tn};
use crate::models::{Effort, MrStatus};

pub fn view(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let shown = g.app.visible_mrs().count();
    toolbar(g, ui, shown, sizes);

    if shown == 0 {
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            let text = tl(if g.app.mrs.is_empty() {
                "No tracked merge request yet: type its number in the search field and press Enter."
            } else {
                "No merge request matches this view."
            });
            ui.label(muted(&g.theme, text));
        });
        return;
    }

    egui::Frame::new()
        .inner_margin(Margin::symmetric(8, 0))
        .show(ui, |ui| table(g, ui, shown));
}

// ── Toolbar ──────────────────────────────────────────────────────────────────

fn sort_label(column: SortColumn) -> &'static str {
    tl(match column {
        SortColumn::UpdatedAt => "Updated",
        SortColumn::Id => "MR",
        SortColumn::Milestone => "Milestone",
        SortColumn::Title => "Title",
    })
}

const TOOLBAR_HEIGHT: f32 = 52.0;

fn toolbar(g: &mut GuiApp, ui: &mut Ui, shown: usize, sizes: &Sizes) {
    let theme = g.theme;
    let (bar, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), TOOLBAR_HEIGHT),
        Sense::hover(),
    );
    ui.painter().hline(
        bar.x_range(),
        bar.bottom() - 0.5,
        Stroke::new(1.0, theme.palette.border_soft),
    );
    let inner = bar.shrink2(Vec2::new(16.0, 0.0));
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut ui;
    ui.spacing_mut().item_spacing.x = 8.0;

    let style = theme.icon_button();
    let views_shown = sizes.views.is_some();
    let label = tl(if views_shown {
        "Hide the views panel (F)"
    } else {
        "Show the views panel (F)"
    });
    if icon_toggle(ui, label, Icon::PanelLeft, &style, 28.0, views_shown).clicked() {
        g.show_views = Some(!views_shown);
    }
    ui.label(semibold(tl("Merge requests"), theme.text_size + 2.0));
    ui.label(muted(
        &theme,
        tf(
            "{n} of {total}",
            &[
                ("n", &shown.to_string()),
                ("total", &g.app.mrs.len().to_string()),
            ],
        ),
    ));
    ui.add_space(6.0);

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if button(
            ui,
            &theme,
            Some(Icon::Clipboard),
            tl("Copy checklist"),
            true,
        )
        .on_hover_text(tl(
            "Copy the visible MRs as a Markdown release checklist (E)",
        ))
        .clicked()
        {
            g.pending.push(Action::CopyChecklist);
        }
        let outlined = theme.outlined_icon_button();
        let height = theme.spacing.control_height + 2.0;
        if icon_button(ui, tl("Refresh now (R)"), Icon::Refresh, &outlined, height).clicked() {
            g.pending.push(Action::Refresh);
        }
        if icon_button(ui, tl("Columns (C)"), Icon::Columns, &outlined, height).clicked() {
            g.app.input_mode = InputMode::ColumnPicker;
        }
        sort_button(g, ui);
        search(g, ui);
    });
    milestone_suggestions(g, ui, bar);
}

/// "↓ Updated": the sort column and order; a click opens the choices.
fn sort_button(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let icon = match g.app.sort_order {
        SortOrder::Ascending => Icon::ArrowUp,
        SortOrder::Descending => Icon::ArrowDown,
    };
    let response = button(ui, &theme, Some(icon), sort_label(g.app.sort_column), false)
        .on_hover_text(tl("Sort (S: next column, Shift+S: reverse)"));
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(170.0);
        for column in [
            SortColumn::UpdatedAt,
            SortColumn::Id,
            SortColumn::Milestone,
            SortColumn::Title,
        ] {
            let selected = g.app.sort_column == column;
            if ui.selectable_label(selected, sort_label(column)).clicked() && !selected {
                g.pending.push(Action::SortBy(column));
            }
        }
        ui.separator();
        let reverse = tl(match g.app.sort_order {
            SortOrder::Ascending => "Descending order",
            SortOrder::Descending => "Ascending order",
        });
        if ui.button(reverse).clicked() {
            g.pending.push(Action::ToggleSortOrder);
        }
    });
}

/// The search field. It filters the table as you type; Enter on an MR number
/// tracks it (`-123` stops tracking), and `@` + a milestone tracks its MRs.
fn search(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let width = ui.available_width();
    // The keyboard shortcuts stand aside while a field has focus: ↑ and ↓ move
    // through the milestone suggestions here, taken before the field would move
    // its text cursor with them. The field's id is the one of the last frame.
    let field_id = ui.make_persistent_id("search_field_id");
    let focused = ui
        .data(|d| d.get_temp::<egui::Id>(field_id))
        .is_some_and(|id| ui.memory(|m| m.has_focus(id)));
    if focused && !g.app.milestone_suggestions.is_empty() {
        let (down, up) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            )
        });
        if down {
            g.app.milestone_suggestion_next();
        }
        if up {
            g.app.milestone_suggestion_prev();
        }
    }
    let response = ui
        .with_layout(Layout::left_to_right(Align::Center), |ui| {
            field(
                ui,
                &theme,
                &mut g.app.input,
                "search",
                FieldLook {
                    hint: tl("Search title, author, !iid · 123 + Enter tracks an MR · @milestone"),
                    icon: Some(Icon::Search),
                    width,
                    active: false,
                },
            )
        })
        .inner;
    ui.data_mut(|d| d.insert_temp(field_id, response.id));
    if std::mem::take(&mut g.focus_search) {
        response.request_focus();
    }
    if response.changed() {
        g.app.update_milestone_suggestions();
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        g.app.input.clear();
        g.app.milestone_suggestions.clear();
    }
    if !(response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
        return;
    }
    if !g.app.milestone_suggestions.is_empty() {
        if let Some(title) = g.app.confirm_milestone_suggestion() {
            g.pending.push(Action::TrackMilestone(title));
            g.app.input.clear();
        }
        return;
    }
    // Only MR numbers: a word would add a tracked branch (see the views panel).
    let value = g.app.input.trim();
    let number = value.strip_prefix('-').unwrap_or(value);
    if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
        let value = std::mem::take(&mut g.app.input);
        g.pending.push(Action::SubmitInput(value));
    }
}

/// The milestones matching `@…` in the search field, over the table.
fn milestone_suggestions(g: &mut GuiApp, ui: &mut Ui, bar: Rect) {
    if g.app.milestone_suggestions.is_empty() {
        return;
    }
    let theme = g.theme;
    let mut picked = None;
    egui::Area::new(ui.make_persistent_id("milestone_suggestions"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(bar.left() + 220.0, bar.bottom() - 6.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(320.0);
                ui.label(muted(&theme, tl("Track every MR of a milestone:")));
                for (i, title) in g.app.milestone_suggestions.iter().enumerate() {
                    let selected = i == g.app.milestone_suggestion_cursor;
                    if ui.selectable_label(selected, title).clicked() {
                        picked = Some(title.clone());
                    }
                }
            });
        });
    if let Some(title) = picked {
        g.pending.push(Action::TrackMilestone(title));
        g.app.input.clear();
        g.app.milestone_suggestions.clear();
    }
}

// ── Columns ──────────────────────────────────────────────────────────────────

/// The optional columns, by their `ColumnDef` id (shared with the TUI's column picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
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
    const ALL: [Col; 8] = [
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
    const DROP_ORDER: [Col; 8] = [
        Col::Activity,
        Col::Labels,
        Col::Ticket,
        Col::Behind,
        Col::Target,
        Col::Milestone,
        Col::Notes,
        Col::Effort,
    ];

    fn id(self) -> &'static str {
        match self {
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

    fn width(self) -> f32 {
        match self {
            Col::Effort => 80.0,
            Col::Notes => 44.0,
            Col::Activity => 80.0,
            Col::Labels => 200.0,
            Col::Milestone => 140.0,
            Col::Target => 120.0,
            Col::Behind => 56.0,
            Col::Ticket => 150.0,
        }
    }

    fn title(self) -> &'static str {
        tl(match self {
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
fn wanted_columns(app: &App) -> Vec<Col> {
    let offered: Vec<&str> = app.visible_column_defs().iter().map(|c| c.id).collect();
    let chosen = app.project_settings.visible_columns.is_some();
    Col::ALL
        .into_iter()
        .filter(|c| offered.contains(&c.id()))
        .filter(|c| {
            if chosen {
                app.config.visible_columns.is_visible(c.id())
            } else {
                !matches!(c, Col::Activity | Col::Labels)
            }
        })
        .collect()
}

const FLAG_WIDTH: f32 = 18.0;
const ID_WIDTH: f32 = 52.0;
const STATUS_WIDTH: f32 = 96.0;
/// Fixed columns: flag, MR, status (the title takes the rest).
const FIXED_COLUMNS_WIDTH: f32 = FLAG_WIDTH + ID_WIDTH + STATUS_WIDTH;
/// The title never gets narrower than this; below, the table scrolls sideways.
const MIN_TITLE_WIDTH: f32 = 200.0;
/// Room the title keeps before an optional column is dropped.
const TITLE_COMFORT_WIDTH: f32 = 220.0;
const COLUMN_GAP: f32 = 8.0;
const BRANCH_WIDTH: f32 = 78.0;
/// Inside a row, left and right.
const ROW_PADDING: f32 = 8.0;
const ROW_HEIGHT: f32 = 50.0;
const ROW_GAP: f32 = 2.0;

/// Fits the wanted columns into `width`: drops the least useful ones until the
/// title keeps its room, and widens the branch cells with what is left.
/// Returns the columns kept (in display order), how many were dropped, and the
/// width of each branch column.
pub fn fit_columns(wanted: &[Col], width: f32, branch_count: usize) -> (Vec<Col>, usize, f32) {
    let base = ROW_PADDING * 2.0
        + FIXED_COLUMNS_WIDTH
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
        (BRANCH_WIDTH + left / branch_count as f32 / 4.0).min(120.0)
    };
    (kept.clone(), wanted.len() - kept.len(), branch_width)
}

/// One column of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cell {
    Flag,
    Id,
    Title,
    Status,
    Optional(Col),
    Branch(usize),
}

/// Where each column sits inside a row, from its left edge.
struct Grid {
    cells: Vec<(Cell, f32, f32)>,
    width: f32,
}

impl Grid {
    fn new(columns: &[Col], branches: usize, branch_width: f32, width: f32) -> Self {
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
        }
    }

    fn rect(&self, row: Rect, x: f32, w: f32) -> Rect {
        Rect::from_min_size(
            egui::pos2(row.left() + x, row.top()),
            Vec2::new(w, row.height()),
        )
    }
}

// ── Rows ─────────────────────────────────────────────────────────────────────

/// What one table row shows, copied out of the app so the table can record actions.
struct RowView {
    index: usize,
    id: String,
    title: String,
    author: String,
    updated: String,
    labels: Vec<(String, Tone)>,
    flagged: bool,
    fresh: bool,
    selected: bool,
    status: (&'static str, Tone),
    effort: Option<(&'static str, Tone)>,
    threads: u32,
    activity: (&'static str, Tone),
    branches: Vec<(&'static str, &'static str, Tone)>,
    milestone: Option<String>,
    target: String,
    behind: Option<u32>,
    ticket: Option<(String, String)>,
    has_ticket: bool,
}

/// The rows of `range` (indexes into the visible list): only those in view are
/// copied out, so a frame costs the same whatever the number of MRs.
fn row_views(g: &GuiApp, range: std::ops::Range<usize>) -> Vec<RowView> {
    let app = &g.app;
    let theme = &g.theme;
    let tones = &theme.tones;
    let selected = app.table_state.selected();
    let fresh_window = app.update_highlight_ticks > 0;
    app.visible_mrs()
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .map(|(index, mr)| {
            let failed = matches!(mr.status, MrStatus::Error(_));
            RowView {
                index,
                id: mr.id.clone(),
                title: if mr.title.is_empty() {
                    tl("Loading…").into()
                } else {
                    mr.title.clone()
                },
                author: mr.author.clone(),
                updated: mr
                    .updated_at
                    .as_deref()
                    .map(crate::utils::format_relative_date)
                    .unwrap_or_default(),
                labels: mr
                    .labels
                    .iter()
                    .filter(|l| app.config.is_table_label(l))
                    .map(|l| (l.clone(), label_tone(app, l, theme)))
                    .collect(),
                flagged: mr.flagged,
                fresh: mr.recently_updated && fresh_window,
                selected: selected == Some(index),
                status: tones::status(mr.state, &mr.mergeability, tones),
                effort: mr.diff_stats.as_ref().map(|s| {
                    let effort = Effort::from_score(s.difficulty(&app.config.complexity_profile));
                    tones::effort(effort, tones)
                }),
                threads: mr.user_notes_count,
                activity: activity(app, mr.updated_at.as_deref(), theme),
                branches: app
                    .branches
                    .iter()
                    .map(|b| tones::presence(app.branch_presence(mr, b), failed, tones))
                    .collect(),
                milestone: mr.milestone.clone(),
                target: mr.target_branch.clone(),
                behind: mr.diff_stats.as_ref().and_then(|s| s.commits_behind),
                ticket: mr
                    .linked_ticket
                    .as_ref()
                    .map(|t| (t.id.clone(), t.status.clone())),
                has_ticket: mr.linked_ticket.is_some() && app.tracker.is_some(),
            }
        })
        .collect()
}

/// What a click in the table asked for, applied once the table is drawn.
enum RowAction {
    Select(usize),
    Then(usize, Action),
}

fn table(g: &mut GuiApp, ui: &mut Ui, shown: usize) {
    let theme = g.theme;
    let branches = g.app.branches.clone();
    let wanted = wanted_columns(&g.app);
    let (columns, dropped, branch_width) =
        fit_columns(&wanted, ui.available_width(), branches.len());
    let grid = Grid::new(&columns, branches.len(), branch_width, ui.available_width());

    // Keyboard moves bring the selected row into view.
    let selected = g.app.table_state.selected().filter(|&i| i < shown);
    let memory = ui.make_persistent_id("table_scroll");
    let (last_selected, offset, viewport): (Option<usize>, f32, f32) =
        ui.data(|d| d.get_temp(memory)).unwrap_or((None, 0.0, 0.0));
    let stride = ROW_HEIGHT + ROW_GAP;
    let mut scroll_to = None;
    if let Some(index) = selected.filter(|_| selected != last_selected) {
        let top = index as f32 * stride;
        let bottom = top + stride;
        if top < offset {
            scroll_to = Some(top);
        } else if viewport > 0.0 && bottom > offset + viewport {
            scroll_to = Some(bottom - viewport);
        }
    }

    let sort = (g.app.sort_column, g.app.sort_order);
    let mut sort_by = None;
    let mut action = None;
    egui::ScrollArea::horizontal()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_width(grid.width);
            header(ui, &theme, &grid, &branches, sort, &mut sort_by);
            let mut area = egui::ScrollArea::vertical()
                .id_salt("table_rows")
                .auto_shrink(false);
            if let Some(y) = scroll_to {
                area = area.vertical_scroll_offset(y);
            }
            let footer = if dropped > 0 { 26.0 } else { 0.0 };
            let output = area
                .max_height(ui.available_height() - footer)
                .show_rows(ui, stride, shown, |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for r in &row_views(g, range) {
                        ui.add_space(ROW_GAP);
                        if let Some(picked) = row(ui, &theme, &grid, r) {
                            action = Some(picked);
                        }
                    }
                });
            ui.data_mut(|d| {
                d.insert_temp(
                    memory,
                    (selected, output.state.clamped_offset().y, output.inner_rect.height()),
                )
            });
            if dropped > 0 {
                ui.add_space(6.0);
                ui.label(muted(
                    &theme,
                    tn(
                        dropped as u64,
                        "{n} column hidden to fit the window — widen it, or pick fewer in Columns.",
                        "{n} columns hidden to fit the window — widen it, or pick fewer in Columns.",
                    ),
                ));
            }
        });

    if let Some(column) = sort_by {
        g.pending.push(Action::SortBy(column));
    }
    match action {
        Some(RowAction::Select(index)) => select(g, index),
        Some(RowAction::Then(index, action)) => {
            select(g, index);
            g.pending.push(action);
        }
        None => {}
    }
}

fn header(
    ui: &mut Ui,
    theme: &Theme,
    grid: &Grid,
    branches: &[String],
    sort: (SortColumn, SortOrder),
    sort_by: &mut Option<SortColumn>,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(grid.width, 34.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, theme.palette.border_soft),
    );
    for &(cell, x, w) in &grid.cells {
        let cell_rect = grid.rect(rect, x, w);
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(cell_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let caption = |text: &str| {
            RichText::new(text)
                .font(fonts::medium(theme.text_size - 1.5))
                .color(theme.palette.text_muted)
        };
        let sortable = match cell {
            Cell::Id => Some((tl("MR"), SortColumn::Id)),
            Cell::Title => Some((tl("Title"), SortColumn::Title)),
            Cell::Optional(Col::Milestone) => Some((tl("Milestone"), SortColumn::Milestone)),
            _ => None,
        };
        match (cell, sortable) {
            (_, Some((title, column))) => {
                let text = if sort.0 == column {
                    let arrow = match sort.1 {
                        SortOrder::Ascending => "↑",
                        SortOrder::Descending => "↓",
                    };
                    caption(&format!("{title} {arrow}")).color(theme.palette.text)
                } else {
                    caption(title)
                };
                if ui
                    .add(egui::Label::new(text).sense(Sense::click()))
                    .on_hover_text(tl("Sort by this column"))
                    .clicked()
                {
                    *sort_by = Some(column);
                }
            }
            (Cell::Status, _) => {
                ui.label(caption(tl("Status")));
            }
            (Cell::Optional(Col::Notes), _) => {
                // The column is narrower than its name: an icon, named on hover.
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                icons::paint(ui.painter(), rect, Icon::Comment, theme.palette.text_muted);
                response.on_hover_text(tl("Unresolved threads"));
            }
            (Cell::Optional(col), _) => {
                ui.label(caption(col.title()));
            }
            (Cell::Branch(i), _) => {
                let name = branches.get(i).map_or("", String::as_str);
                ui.add(
                    egui::Label::new(
                        mono(name, theme.text_size - 1.5).color(theme.palette.text_muted),
                    )
                    .truncate(),
                );
            }
            _ => {}
        }
    }
}

/// Draws one row; returns what a click on it asked for.
fn row(ui: &mut Ui, theme: &Theme, grid: &Grid, r: &RowView) -> Option<RowAction> {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(grid.width, ROW_HEIGHT), Sense::hover());
    let response = ui.interact(rect, ui.scope_id().with(("row", &r.id)), Sense::click());
    let radius = CornerRadius::same(theme.radii.medium);
    let fill = if r.selected {
        theme.palette.selection
    } else if r.fresh {
        theme.tones.green.bg.gamma_multiply(0.55)
    } else if response.hovered() {
        theme.palette.border_soft.gamma_multiply(0.6)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, radius, fill);
    if r.selected {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme.palette.accent),
            egui::StrokeKind::Inside,
        );
    }

    let mut action = None;
    for &(cell, x, w) in &grid.cells {
        let cell_rect = grid.rect(rect, x, w);
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(cell_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        ui.set_clip_rect(cell_rect.intersect(ui.clip_rect()));
        ui.spacing_mut().item_spacing.x = 6.0;
        match cell {
            Cell::Flag => {
                if flag(&mut ui, theme, r.flagged).clicked() {
                    action = Some(RowAction::Then(r.index, Action::ToggleFlagSelected));
                }
            }
            Cell::Id => {
                let color = if r.fresh {
                    theme.tones.green.fg
                } else {
                    theme.palette.text_secondary
                };
                ui.label(mono(format!("!{}", r.id), theme.text_size - 1.0).color(color));
            }
            Cell::Title => title_cell(&mut ui, theme, r, grid),
            Cell::Status => {
                badge(&mut ui, theme, r.status.0, r.status.1);
            }
            Cell::Optional(col) => cell_content(&mut ui, theme, r, col),
            Cell::Branch(i) => {
                if let Some((glyph, label, tone)) = r.branches.get(i) {
                    ui.add(Chip::new(label, *tone).glyph(glyph));
                }
            }
        }
    }

    if response.double_clicked() {
        action = Some(RowAction::Then(r.index, Action::OpenSelectedMr));
    } else if response.clicked() || response.secondary_clicked() {
        action.get_or_insert(RowAction::Select(r.index));
    }
    response.context_menu(|ui| {
        if let Some(picked) = context_menu(ui, r) {
            action = Some(RowAction::Then(r.index, picked));
            ui.close();
        }
    });
    action
}

fn flag(ui: &mut Ui, theme: &Theme, flagged: bool) -> egui::Response {
    let style = theme.icon_button();
    let (icon, label) = if flagged {
        (Icon::Star, tl("Unflag (Space)"))
    } else {
        (Icon::StarOutline, tl("Flag (Space)"))
    };
    IconButton::new(label, Vec2::splat(18.0), &style)
        .show(ui, |ui, rect, tint| {
            let color = if flagged {
                theme.tones.amber.fg
            } else {
                tint.gamma_multiply(0.7)
            };
            icons::paint(ui.painter(), rect.shrink(2.5), icon, color);
        })
        .on_hover_text(label)
}

/// The title over "@author · when" and the scoped labels (unless they have
/// their own column).
fn title_cell(ui: &mut Ui, theme: &Theme, r: &RowView, grid: &Grid) {
    let labels_column = grid
        .cells
        .iter()
        .any(|(c, _, _)| *c == Cell::Optional(Col::Labels));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(RichText::new(&r.title).font(fonts::medium(theme.text_size)))
                .truncate(),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            meta_line(ui, theme, r);
            if !labels_column {
                ui.add_space(3.0);
                for (label, tone) in r.labels.iter().filter(|(l, _)| l.contains("::")) {
                    ui.add(Chip::new(label, *tone).font(FontId::monospace(theme.text_size - 2.5)));
                }
            }
        });
    });
}

/// Who last touched the MR and when: an initials avatar, the author's name (the
/// handle on hover), a clock and the relative date.
fn meta_line(ui: &mut Ui, theme: &Theme, r: &RowView) {
    let size = theme.text_size - 1.5;
    let (name, handle) = split_person(&r.author);
    let response = avatar(ui, theme, name, 18.0);
    if let Some(handle) = handle {
        response.on_hover_text(format!("@{handle}"));
    }
    ui.label(
        RichText::new(name)
            .font(fonts::medium(size))
            .color(theme.palette.text_secondary),
    );
    if r.updated.is_empty() {
        return;
    }
    ui.label(
        RichText::new("·")
            .size(size)
            .color(theme.palette.text_muted),
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
    icons::paint(ui.painter(), rect, Icon::Clock, theme.palette.text_muted);
    ui.label(
        RichText::new(&r.updated)
            .size(size)
            .color(theme.palette.text_muted),
    );
}

fn select(g: &mut GuiApp, index: usize) {
    if g.app.table_state.selected() != Some(index) {
        g.app.table_state.select(Some(index));
        g.app.layout.inspector.reset();
    }
}

fn cell_content(ui: &mut Ui, theme: &Theme, r: &RowView, col: Col) {
    match col {
        Col::Effort => match r.effort {
            Some((label, tone)) => {
                ui.add(Chip::new(label, tone).glyph("●").pill());
            }
            None => {
                ui.label(muted(theme, "—"));
            }
        },
        Col::Notes => {
            let color = if r.threads > 0 {
                theme.tones.amber.fg
            } else {
                theme.palette.text_muted
            };
            ui.spacing_mut().item_spacing.x = 4.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Comment, color);
            ui.label(RichText::new(r.threads.to_string()).color(color));
        }
        Col::Activity => {
            ui.add(Chip::new(r.activity.0, r.activity.1).glyph("●"));
        }
        Col::Labels => {
            if r.labels.is_empty() {
                ui.label(muted(theme, "—"));
            }
            for (label, tone) in &r.labels {
                ui.add(Chip::new(label, *tone).font(FontId::monospace(theme.text_size - 2.5)));
            }
        }
        Col::Milestone => {
            let text = r.milestone.as_deref().unwrap_or("—");
            ui.add(egui::Label::new(muted(theme, text)).truncate());
        }
        Col::Target => {
            ui.add(egui::Label::new(mono(&r.target, theme.text_size - 1.0)).truncate());
        }
        Col::Behind => match r.behind {
            Some(0) => {
                ui.label(muted(theme, "0"));
            }
            Some(n) => {
                let tone = if n >= 10 {
                    theme.tones.red
                } else {
                    theme.tones.amber
                };
                ui.label(RichText::new(n.to_string()).color(tone.fg));
            }
            None => {
                ui.label(muted(theme, "—"));
            }
        },
        Col::Ticket => match &r.ticket {
            Some((id, status)) => {
                ui.label(mono(format!("#{id}"), theme.text_size - 1.0).color(theme.palette.accent));
                ui.add(Chip::new(status, theme.tones.blue));
            }
            None => {
                ui.label(muted(theme, "—"));
            }
        },
    }
}

/// The right-click menu of a row; returns the action picked.
fn context_menu(ui: &mut Ui, r: &RowView) -> Option<Action> {
    let mut picked = None;
    if ui.button(tl("Open in GitLab (O)")).clicked() {
        picked = Some(Action::OpenSelectedMr);
    }
    if ui.button(tl("Copy the git clone command (Y)")).clicked() {
        picked = Some(Action::YankCloneCommand);
    }
    let flag = tl(if r.flagged {
        "Unflag (Space)"
    } else {
        "Flag (Space)"
    });
    if ui.button(flag).clicked() {
        picked = Some(Action::ToggleFlagSelected);
    }
    if r.has_ticket {
        ui.separator();
        if ui.button(tl("Open the ticket")).clicked() {
            picked = Some(Action::OpenSelectedTicket);
        }
        if ui.button(tl("Log time… (L)")).clicked() {
            picked = Some(Action::OpenLogTime);
        }
    }
    ui.separator();
    if ui.button(tl("Stop tracking (Del)")).clicked() {
        picked = Some(Action::RemoveSelected);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANTED: [Col; 6] = [
        Col::Effort,
        Col::Notes,
        Col::Milestone,
        Col::Target,
        Col::Behind,
        Col::Ticket,
    ];

    #[test]
    fn narrow_tables_drop_the_least_useful_columns_first() {
        // 1440 points with both side panels, as in the mockup: about 840 of table.
        let (kept, dropped, branch) = fit_columns(&WANTED, 840.0, 3);
        assert_eq!(kept, [Col::Effort, Col::Notes]);
        assert_eq!(dropped, 4);
        assert!(
            (BRANCH_WIDTH..BRANCH_WIDTH + 4.0).contains(&branch),
            "{branch}"
        );
    }

    #[test]
    fn wide_tables_keep_every_column_and_widen_the_branch_cells() {
        // Ultrawide at 100 %: everything fits, branch cells grow up to their cap.
        let (kept, dropped, branch) = fit_columns(&WANTED, 4300.0, 3);
        assert_eq!(kept, WANTED);
        assert_eq!(dropped, 0);
        assert_eq!(branch, 120.0);
    }

    #[test]
    fn columns_come_back_in_reverse_drop_order() {
        // 1080p with both panels: about 1100 points of table.
        let (kept, _, _) = fit_columns(&WANTED, 1100.0, 3);
        assert_eq!(kept, [Col::Effort, Col::Notes, Col::Milestone, Col::Target]);
        // Narrower still: only the effort is left.
        let (kept, _, _) = fit_columns(&WANTED, 780.0, 3);
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
}
