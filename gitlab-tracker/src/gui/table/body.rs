//! The table itself: header, groups, rows in view, the behind popups.

use super::*;

/// What a click in the table asked for, applied once the table is drawn.
pub(super) enum RowAction {
    Select(usize),
    /// Ctrl + click: add it to the picked MRs, or take it out.
    Toggle(usize),
    /// Shift + click: pick every MR from the selected one to this one.
    Range(usize),
    Then(usize, Action),
    /// Double click: the MR's page.
    Open(usize),
    /// A click on a label chip: the MRs carrying that label alone.
    Label(String),
}

pub(super) fn table(g: &mut GuiApp, ui: &mut Ui, shown: usize, inspector_shown: bool) {
    let theme = g.theme;
    // The "Behind" counts drawn, by MR id: a click lists what the MR lacks.
    let mut behind_cells: Vec<(String, egui::Response)> = Vec::new();
    let lines = lines(g);
    let (branches, aliases): (Vec<String>, Vec<String>) = branch_order(&g.app).into_iter().unzip();
    let wanted = wanted_columns(&g.app, g.hide_next_column, !g.queues.is_empty());
    let flow = g.flow_cells && !branches.is_empty();
    let step = flow_step(ui, &theme, &aliases);
    let (columns, dropped, branch_width) = if flow {
        fit_columns_with(
            &wanted,
            ui.available_width(),
            0,
            flow_width(branches.len(), step) + COLUMN_GAP,
        )
    } else {
        fit_columns(&wanted, ui.available_width(), branches.len())
    };
    g.shown_columns.clone_from(&columns);
    let mut grid = if flow {
        Grid::with_flow(&columns, branches.len(), step, ui.available_width())
    } else {
        Grid::new(&columns, branches.len(), branch_width, ui.available_width())
    };
    if g.compact {
        grid.row_height = COMPACT_ROW_HEIGHT;
    }
    grid.names = branches.clone();
    grid.aliases = aliases;

    // Keyboard moves bring the selected row into view.
    let selected = g.app.table_state.selected().filter(|&i| i < shown);
    let memory = ui.make_persistent_id("table_scroll");
    let (last_selected, offset, viewport): (Option<usize>, f32, f32) =
        ui.data(|d| d.get_temp(memory)).unwrap_or((None, 0.0, 0.0));
    let stride = grid.row_height + ROW_GAP;
    let mut scroll_to = None;
    let selected_line = selected.and_then(|s| {
        lines
            .iter()
            .position(|l| matches!(l, Line::Mr(i) if *i == s))
    });
    if let Some(line) = selected_line.filter(|_| selected != last_selected) {
        let top = line as f32 * stride;
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
    let mut fold = None;
    // From the hidden-columns note: the columns (false), or more room (true).
    let mut footer_pick: Option<bool> = None;
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
            let footer = if dropped > 0 { 32.0 } else { 0.0 };
            let output = area
                .max_height(ui.available_height() - footer)
                .show_rows(ui, stride, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let in_view = &lines[range];
                    let indexes: Vec<usize> = in_view
                        .iter()
                        .filter_map(|l| match l {
                            Line::Mr(i) => Some(*i),
                            Line::Heading { .. } => None,
                        })
                        .collect();
                    let mut views = row_views(g, &indexes).into_iter();
                    for line in in_view {
                        ui.add_space(ROW_GAP);
                        match line {
                            Line::Heading {
                                key,
                                label,
                                count,
                                tone,
                                folded,
                            } => {
                                if group_heading(
                                    ui,
                                    &theme,
                                    (grid.width, grid.row_height),
                                    label,
                                    *count,
                                    *tone,
                                    *folded,
                                ) {
                                    fold = Some(key.clone());
                                }
                            }
                            Line::Mr(_) => {
                                let Some(r) = views.next() else { continue };
                                let (picked, behind) = row(ui, &theme, &grid, &r);
                                if let Some(picked) = picked {
                                    action = Some(picked);
                                }
                                if let Some(response) = behind {
                                    behind_cells.push((r.id.clone(), response));
                                }
                            }
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
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.label(muted(
                        &theme,
                        tn(
                            dropped as u64,
                            "{n} column hidden to fit the window — widen it, or pick fewer in Columns.",
                            "{n} columns hidden to fit the window — widen it, or pick fewer in Columns.",
                        ),
                    ));
                    if link(ui, &theme, tl("Columns…")).clicked() {
                        footer_pick = Some(false);
                    }
                    if inspector_shown && link(ui, &theme, tl("Hide the inspector")).clicked() {
                        footer_pick = Some(true);
                    }
                });
            }
        });

    if let Some(column) = sort_by {
        g.pending.push(Action::SortBy(column));
    }
    behind_popups(g, ui, &behind_cells);
    if let Some(key) = fold {
        if !g.folded.remove(&key) {
            g.folded.insert(key);
        }
    }
    let _ = shown;
    match footer_pick {
        Some(true) => g.show_inspector = Some(false),
        Some(false) => g.app.input_mode = InputMode::ColumnPicker,
        None => {}
    }
    match action {
        Some(RowAction::Select(index)) => {
            g.marked.clear();
            select(g, index);
        }
        Some(RowAction::Toggle(index)) => {
            let ids: Vec<String> = g.app.visible_mrs().map(|m| m.id.clone()).collect();
            if g.marked.is_empty() && !g.bulk_mode {
                if let Some(current) = g.app.table_state.selected().and_then(|i| ids.get(i)) {
                    g.marked.insert(current.clone());
                }
            }
            if let Some(id) = ids.get(index) {
                if !g.marked.remove(id) {
                    g.marked.insert(id.clone());
                }
            }
            select(g, index);
        }
        Some(RowAction::Range(index)) => {
            let ids: Vec<String> = g.app.visible_mrs().map(|m| m.id.clone()).collect();
            let from = g.app.table_state.selected().unwrap_or(index);
            // In the order shown: the groups reorder the list.
            let shown_order: Vec<usize> = lines
                .iter()
                .filter_map(|l| match l {
                    Line::Mr(i) => Some(*i),
                    Line::Heading { .. } => None,
                })
                .collect();
            let a = shown_order.iter().position(|i| *i == from).unwrap_or(0);
            let b = shown_order.iter().position(|i| *i == index).unwrap_or(a);
            g.marked.clear();
            for i in &shown_order[a.min(b)..=a.max(b)] {
                g.marked.extend(ids.get(*i).cloned());
            }
            select(g, index);
        }
        Some(RowAction::Then(index, action)) => {
            select(g, index);
            g.pending.push(action);
        }
        Some(RowAction::Open(index)) => {
            select(g, index);
            g.open_page(Page::Mr);
        }
        Some(RowAction::Label(label)) => g.filter_by_label(&label),
        None => {}
    }
}

pub(super) fn header(
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
                    .tip(tl("Sort by this column"))
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
                response.tip(tl("Unresolved threads"));
            }
            (Cell::Optional(col), _) => {
                ui.label(caption(col.title()));
            }
            (Cell::Flow, _) => {
                // Each branch's short name over its dot (see `flow_cell`).
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(branches.len() as f32 * grid.flow_step, 16.0),
                    Sense::hover(),
                );
                let font = egui::FontId::monospace(theme.text_size - 3.0);
                for (i, alias) in grid.aliases.iter().enumerate() {
                    let x = rect.left() + 11.0 + i as f32 * grid.flow_step;
                    ui.painter().text(
                        egui::pos2(x, rect.center().y),
                        egui::Align2::CENTER_CENTER,
                        alias,
                        font.clone(),
                        theme.palette.text_muted,
                    );
                }
                response.rich(Tip::new().mono(branches.join(" → ")));
            }
            (Cell::Branch(i), _) => {
                let name = branches.get(i).map_or("", String::as_str);
                let alias = grid.aliases.get(i).map_or(name, String::as_str);
                ui.add(
                    egui::Label::new(
                        mono(alias, theme.text_size - 1.5).color(theme.palette.text_muted),
                    )
                    .truncate()
                    .show_tooltip_when_elided(false),
                )
                .rich(Tip::new().mono(name));
            }
            _ => {}
        }
    }
}

/// A group's heading, as tall as a row: a chevron, its name and its count.
/// Returns whether it was clicked (to fold or unfold it).
pub(super) fn group_heading(
    ui: &mut Ui,
    theme: &Theme,
    (width, height): (f32, f32),
    label: &str,
    count: usize,
    tone: Option<Tone>,
    folded: bool,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let band = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 30.0), rect.max);
    if response.hovered() {
        ui.painter().rect_filled(
            band,
            CornerRadius::same(theme.radii.medium),
            theme.palette.border_soft.gamma_multiply(0.6),
        );
    }
    ui.painter().hline(
        band.x_range(),
        band.bottom() - 0.5,
        Stroke::new(1.0, theme.palette.border_soft),
    );
    let y = band.center().y;
    let chevron = if folded { "▸" } else { "▾" };
    ui.painter().text(
        egui::pos2(band.left() + 14.0, y),
        egui::Align2::CENTER_CENTER,
        chevron,
        egui::FontId::proportional(theme.text_size),
        theme.palette.text_muted,
    );
    let mut x = band.left() + 28.0;
    if let Some(tone) = tone {
        ui.painter()
            .circle_filled(egui::pos2(x + 4.0, y), 4.0, tone.fg);
        x += 14.0;
    }
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        fonts::semibold(theme.text_size),
        theme.palette.text,
    );
    let text_width = galley.size().x;
    ui.painter().galley(
        egui::pos2(x, y - galley.size().y / 2.0),
        galley,
        theme.palette.text,
    );
    let count = ui.painter().layout_no_wrap(
        count.to_string(),
        fonts::medium(theme.text_size - 2.0),
        theme.palette.text_secondary,
    );
    let pill = Rect::from_center_size(
        egui::pos2(x + text_width + 10.0 + count.size().x / 2.0 + 7.0, y),
        Vec2::new(count.size().x + 14.0, 18.0),
    );
    ui.painter()
        .rect_filled(pill, CornerRadius::same(9), theme.tones.gray.bg);
    ui.painter().galley(
        pill.center() - count.size() / 2.0,
        count,
        theme.palette.text_secondary,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl(if folded { "Unfold" } else { "Fold" }))
        .clicked()
}

/// Text in the accent that reads as a link; returns its response.
pub(super) fn link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(theme.text_size - 1.0)
                .color(theme.palette.accent),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A rounded bar that pulses: something is loading there.
pub(super) fn shimmer(ui: &mut Ui, theme: &Theme, width: f32, height: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let time = ui.input(|i| i.time);
    let pulse = 0.35 + 0.25 * ((time * 3.0).sin() as f32 + 1.0) / 2.0;
    ui.painter().rect_filled(
        rect,
        CornerRadius::same((height / 2.0) as u8),
        theme.palette.text_muted.gamma_multiply(pulse * 0.5),
    );
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
    response
}

/// Draws one row; returns what a click on it asked for.
/// The popup of each "Behind" count, the MR's page data read once one opens
/// (see [`super::super::mr_page::behind_popup`]); the MR picked there opens.
pub(super) fn behind_popups(g: &mut GuiApp, ui: &Ui, cells: &[(String, egui::Response)]) {
    let mut open = None;
    for (id, response) in cells {
        let popup = egui::Popup::default_response_id(response);
        if !response.clicked() && !egui::Popup::is_id_open(ui.ctx(), popup) {
            continue;
        }
        let Some(mr) = g.app.mrs.iter().find(|m| &m.id == id).cloned() else {
            continue;
        };
        let data = g.details.page_data(&g.app, &mr, ui.ctx()).cloned();
        if let Some(picked) = super::super::mr_page::behind_popup(g, response, &mr, data.as_ref()) {
            open = Some(picked);
        }
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}
