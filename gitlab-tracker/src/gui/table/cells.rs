//! One row and its cells: next step, CI, flow, branches, title, context menu.

use super::*;

/// A row; with the "Behind" count drawn as a link when the MR lacks commits.
pub(super) fn row(
    ui: &mut Ui,
    theme: &Theme,
    grid: &Grid,
    r: &RowView,
) -> (Option<RowAction>, Option<egui::Response>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(grid.width, grid.row_height), Sense::hover());
    let response = ui.interact(rect, ui.scope_id().with(("row", &r.id)), Sense::click());
    let radius = CornerRadius::same(theme.radii.medium);
    let fill = if r.selected || r.marked {
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
    // The status in the margin, to scan the list by color.
    let stripe = Rect::from_min_size(
        egui::pos2(rect.left() + 2.0, rect.top() + 9.0),
        Vec2::new(3.0, rect.height() - 18.0),
    );
    ui.painter()
        .rect_filled(stripe, CornerRadius::same(2), r.status.1.fg);

    let mut action = None;
    let mut behind = None;
    for &(cell, x, w) in &grid.cells {
        let cell_rect = grid.rect(rect, x, w);
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(cell_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        ui.set_clip_rect(cell_rect.intersect(ui.clip_rect()));
        // The row takes the click: selectable texts (the id, the title, the
        // author…) would take it over the row, and a click on them selected
        // nothing.
        ui.style_mut().interaction.selectable_labels = false;
        ui.spacing_mut().item_spacing.x = 6.0;
        match cell {
            Cell::Flag if r.bulk_mode => {
                if tick(&mut ui, theme, r.marked).clicked() {
                    action = Some(RowAction::Toggle(r.index));
                }
            }
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
            Cell::Title => {
                if let Some(label) = title_cell(&mut ui, theme, r, grid) {
                    action = Some(RowAction::Label(label));
                }
            }
            Cell::Status => {
                badge(&mut ui, theme, r.status.0, r.status.1);
            }
            Cell::Optional(Col::Next) => next_cell(&mut ui, theme, r, grid.row_height < ROW_HEIGHT),
            Cell::Flow => flow_cell(&mut ui, theme, &r.branches, &grid.names, grid.flow_step),
            Cell::Optional(Col::Behind) if r.opened && r.behind.is_some_and(|n| n > 0) => {
                behind = Some(behind_link(&mut ui, theme, r.behind.unwrap_or_default()));
            }
            Cell::Optional(col) => {
                if let Some(label) = cell_content(&mut ui, theme, r, col) {
                    action = Some(RowAction::Label(label));
                }
            }
            Cell::Branch(i) => {
                if let Some(cell) = r.branches.get(i) {
                    branch_cell(&mut ui, theme, *cell);
                }
            }
        }
    }

    let modifiers = ui.input(|i| i.modifiers);
    if super::super::widgets::double_clicked(ui, &response, ("mr", &r.id)) {
        action = Some(RowAction::Open(r.index));
    } else if response.clicked() && (modifiers.command || (r.bulk_mode && !modifiers.shift)) {
        action.get_or_insert(RowAction::Toggle(r.index));
    } else if response.clicked() && modifiers.shift {
        action.get_or_insert(RowAction::Range(r.index));
    } else if response.clicked() || response.secondary_clicked() {
        action.get_or_insert(RowAction::Select(r.index));
    }
    super::super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
    if r.marked && !r.selected {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme.palette.accent.gamma_multiply(0.5)),
            egui::StrokeKind::Inside,
        );
    }
    response.context_menu(|ui| {
        if let Some(picked) = context_menu(ui, r) {
            action = Some(RowAction::Then(r.index, picked));
            ui.close();
        }
    });
    (action, behind)
}

/// The count of commits an MR lacks, underlined on hover: a click lists them.
pub(super) fn behind_link(ui: &mut Ui, theme: &Theme, n: u32) -> egui::Response {
    let tone = if n >= 10 {
        theme.tones.red
    } else {
        theme.tones.amber
    };
    let id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let mut text = RichText::new(n.to_string()).color(tone.fg);
    if hovered {
        text = text.underline();
    }
    ui.add(
        egui::Label::new(text)
            .sense(Sense::click())
            .selectable(false),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .tip(tl("See what it lacks"))
}

/// The Next step cell: who (in their tone, stronger when it is me) and what,
/// over since when; one line in the compact density. The todos on hover.
pub(super) fn next_cell(ui: &mut Ui, theme: &Theme, r: &RowView, compact: bool) {
    let Some(next) = &r.next else {
        ui.label(muted(theme, "—"))
            .tip(tl("Known once the MR is loaded"));
        return;
    };
    let who_color = if next.mine || next.tone.fg != theme.tones.gray.fg {
        next.tone.fg
    } else {
        theme.palette.text
    };
    let head = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            RichText::new(&next.who)
                .font(fonts::semibold(theme.text_size - 0.5))
                .color(who_color),
        );
        if !next.what.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("· {}", next.what))
                        .font(fonts::medium(theme.text_size - 1.0))
                        .color(theme.palette.text_secondary),
                )
                .truncate(),
            );
        }
    };
    let response = if compact {
        ui.horizontal(|ui| head(ui)).response
    } else {
        super::super::widgets::centered_block(ui, ("next", &r.id), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| head(ui));
            if !next.since.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(&next.since)
                            .size(theme.text_size - 1.5)
                            .color(theme.palette.text_muted),
                    )
                    .truncate(),
                );
            }
        })
        .response
    };
    if !next.hint.is_empty() {
        let mut hint = Tip::new();
        if compact && !next.since.is_empty() {
            hint = hint.title(&next.since);
        }
        response.rich(hint.text(&next.hint));
    }
}

/// The latest pipeline as a round icon: a check, a cross, a ring while it runs.
pub(super) fn ci_cell(ui: &mut Ui, theme: &Theme, state: Option<crate::models::PipelineState>) {
    use crate::models::PipelineState as P;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
    let c = rect.center();
    let painter = ui.painter();
    let Some(state) = state else {
        painter.hline(
            c.x - 4.0..=c.x + 4.0,
            c.y,
            Stroke::new(1.5, theme.palette.border),
        );
        response.tip(tl("No pipeline"));
        return;
    };
    let (label, tone) = tones::pipeline(state, &theme.tones);
    let stroke = Stroke::new(1.8, tone.fg);
    match state {
        P::Success => {
            painter.circle_filled(c, 8.0, tone.bg);
            painter.line_segment([c + Vec2::new(-3.5, 0.2), c + Vec2::new(-1.0, 2.8)], stroke);
            painter.line_segment([c + Vec2::new(-1.0, 2.8), c + Vec2::new(3.8, -2.6)], stroke);
        }
        P::Failed => {
            painter.circle_filled(c, 8.0, tone.bg);
            painter.line_segment([c + Vec2::new(-3.0, -3.0), c + Vec2::new(3.0, 3.0)], stroke);
            painter.line_segment([c + Vec2::new(3.0, -3.0), c + Vec2::new(-3.0, 3.0)], stroke);
        }
        P::Running | P::Pending | P::Created => {
            painter.circle_stroke(c, 6.5, Stroke::new(1.8, tone.fg.gamma_multiply(0.35)));
            let arc: Vec<egui::Pos2> = (0..=12)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
                    c + Vec2::angled(a) * 6.5
                })
                .collect();
            painter.add(egui::Shape::line(arc, Stroke::new(1.8, tone.fg)));
        }
        _ => {
            painter.circle_stroke(c, 6.5, Stroke::new(1.5, tone.fg));
        }
    }
    response.rich(Tip::new().dot(tone.fg, label));
}

/// Every branch of the row as a line of dots, joined: the flow at a glance.
pub(super) fn flow_cell(
    ui: &mut Ui,
    theme: &Theme,
    cells: &[BranchCell],
    names: &[String],
    step: f32,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(cells.len() as f32 * step, 22.0), Sense::hover());
    let painter = ui.painter();
    let center = |i: usize| egui::pos2(rect.left() + 11.0 + i as f32 * step, rect.center().y);
    for i in 1..cells.len() {
        painter.line_segment(
            [
                center(i - 1) + Vec2::new(8.0, 0.0),
                center(i) - Vec2::new(8.0, 0.0),
            ],
            Stroke::new(1.5, theme.palette.border),
        );
    }
    let mut hints = Tip::new();
    for (i, cell) in cells.iter().enumerate() {
        let name = names.get(i).map_or("", String::as_str);
        match cell {
            BranchCell::Known(glyph, label, tone) => {
                painter.circle_filled(center(i), 8.0, tone.bg);
                presence_glyph(painter, center(i), glyph, tone.fg);
                hints = hints.dot(tone.fg, format!("{name} · {label}"));
            }
            BranchCell::Loading => {
                painter.circle_stroke(center(i), 6.0, Stroke::new(1.5, theme.palette.text_muted));
                hints = hints.dot(
                    theme.palette.text_muted,
                    format!("{name} · {}", tl("Loading")),
                );
            }
            BranchCell::Unknown => {
                painter.circle_stroke(center(i), 6.0, Stroke::new(1.0, theme.palette.border));
                hints = hints.dot(theme.palette.border, format!("{name} · —"));
            }
        }
    }
    response.rich(hints);
}

/// A branch cell: a dot in the presence's tone with its glyph, named on hover.
pub(super) fn branch_cell(ui: &mut Ui, theme: &Theme, cell: BranchCell) {
    match cell {
        BranchCell::Known(glyph, label, tone) => {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 11.0, tone.bg);
            presence_glyph(ui.painter(), rect.center(), glyph, tone.fg);
            response.rich(Tip::new().dot(tone.fg, label));
        }
        BranchCell::Loading => {
            shimmer(ui, theme, 22.0, 22.0).tip(tl("Loading"));
        }
        BranchCell::Unknown => {
            ui.add_space(7.0);
            ui.label(muted(theme, "—"))
                .tip(tl("Not checked yet: the next refresh will tell"));
        }
    }
}

/// Paints a presence glyph (see `tones::presence`) as shapes centered on
/// `center`: the font's glyphs sit off center, each by its own offset.
pub(super) fn presence_glyph(
    painter: &egui::Painter,
    center: egui::Pos2,
    glyph: &str,
    color: egui::Color32,
) {
    let r = 4.5;
    let stroke = Stroke::new(1.5, color);
    match glyph {
        // Present.
        "●" => {
            painter.circle_filled(center, r, color);
        }
        // Absent.
        "○" => {
            painter.circle_stroke(center, r - 0.5, stroke);
        }
        // Partial: the left half filled.
        "◐" => {
            let half: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let angle =
                        std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    center + Vec2::angled(angle) * r
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(half, color, Stroke::NONE));
            painter.circle_stroke(center, r - 0.5, stroke);
        }
        // Cherry-picked.
        "◆" => {
            let d = r + 0.5;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(0.0, -d),
                    center + Vec2::new(d, 0.0),
                    center + Vec2::new(0.0, d),
                    center + Vec2::new(-d, 0.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        // Copied: two waves.
        "≈" => {
            for dy in [-2.0, 2.0] {
                let wave: Vec<egui::Pos2> = (0..=12)
                    .map(|i| {
                        let t = i as f32 / 12.0;
                        let x = (t - 0.5) * 2.0 * r;
                        let y = dy - (t * std::f32::consts::TAU).sin() * 1.2;
                        center + Vec2::new(x, y)
                    })
                    .collect();
                painter.add(egui::Shape::line(wave, stroke));
            }
        }
        // Failed.
        "✕" => {
            let d = r - 1.0;
            painter.line_segment(
                [center + Vec2::new(-d, -d), center + Vec2::new(d, d)],
                stroke,
            );
            painter.line_segment(
                [center + Vec2::new(-d, d), center + Vec2::new(d, -d)],
                stroke,
            );
        }
        other => {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                other,
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }
}

pub(super) fn flag(ui: &mut Ui, theme: &Theme, flagged: bool) -> egui::Response {
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
        .tip(label)
}

/// Bulk mode's box, ticked when the row is picked.
pub(super) fn tick(ui: &mut Ui, theme: &Theme, ticked: bool) -> egui::Response {
    let style = theme.icon_button();
    let label = tl(if ticked { "Unselect" } else { "Select" });
    IconButton::new(label, Vec2::splat(18.0), &style)
        .show(ui, |ui, rect, tint| {
            let square = Rect::from_center_size(rect.center(), Vec2::splat(13.0));
            let radius = CornerRadius::same(3);
            if ticked {
                ui.painter()
                    .rect_filled(square, radius, theme.palette.accent_fill);
                let s = square.shrink(3.0);
                ui.painter().line(
                    vec![
                        egui::pos2(s.left(), s.center().y),
                        egui::pos2(s.left() + s.width() * 0.38, s.bottom()),
                        egui::pos2(s.right(), s.top()),
                    ],
                    Stroke::new(1.6, Color32::WHITE),
                );
            } else {
                ui.painter().rect_stroke(
                    square,
                    radius,
                    Stroke::new(1.2, tint.gamma_multiply(0.8)),
                    egui::StrokeKind::Inside,
                );
            }
        })
        .tip(label)
}

/// The title over "@author · when" and the scoped labels (unless they have
/// their own column); returns the label clicked, if any.
pub(super) fn title_cell(ui: &mut Ui, theme: &Theme, r: &RowView, grid: &Grid) -> Option<String> {
    if grid.row_height < ROW_HEIGHT {
        // Compact: the title alone, who and when on hover.
        let hint = Tip::new()
            .mr(&r.id, &r.title)
            .person(&r.author)
            .text(&r.updated);
        ui.add(
            egui::Label::new(RichText::new(&r.title).font(fonts::medium(theme.text_size)))
                .truncate()
                .show_tooltip_when_elided(false),
        )
        .rich(hint);
        return None;
    }
    let mut clicked = None;
    let labels_column = grid
        .cells
        .iter()
        .any(|(c, _, _)| *c == Cell::Optional(Col::Labels));
    super::super::widgets::centered_block(ui, ("title", &r.id), |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        if r.loading {
            ui.add_space(3.0);
            shimmer(ui, theme, (ui.available_width() * 0.6).min(260.0), 12.0);
            ui.add_space(3.0);
        } else {
            ui.add(
                egui::Label::new(RichText::new(&r.title).font(fonts::medium(theme.text_size)))
                    .truncate(),
            );
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            meta_line(ui, theme, r);
            if let Some(badge) = &r.signatures {
                ui.add_space(2.0);
                signature_chip(ui, theme, badge);
            }
            if !labels_column {
                ui.add_space(3.0);
                for (label, tone) in r.labels.iter().filter(|(l, _)| l.contains("::")) {
                    if toned_label_chip(ui, theme, label, *tone).clicked() {
                        clicked = Some(label.clone());
                    }
                }
            }
        });
    });
    clicked
}

/// Who last touched the MR and when: an initials avatar, the author's name (the
/// handle on hover), a clock and the relative date.
pub(super) fn meta_line(ui: &mut Ui, theme: &Theme, r: &RowView) {
    let size = theme.text_size - 1.5;
    let (name, handle) = split_person(&r.author);
    let response = avatar(ui, theme, name, 18.0);
    if handle.is_some() {
        response.rich(Tip::new().person(&r.author));
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
    let color = r.age.unwrap_or(theme.palette.text_muted);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
    icons::paint(ui.painter(), rect, Icon::Clock, color);
    let date = ui.label(RichText::new(&r.updated).size(size).color(color));
    if r.age.is_some() {
        date.tip(r.activity.0);
    }
}

pub(super) fn select(g: &mut GuiApp, index: usize) {
    if g.app.table_state.selected() != Some(index) {
        g.app.table_state.select(Some(index));
        g.app.layout.inspector.reset();
    }
}

/// One optional cell; returns the label clicked in the Labels one, if any.
pub(super) fn cell_content(ui: &mut Ui, theme: &Theme, r: &RowView, col: Col) -> Option<String> {
    let mut clicked = None;
    match col {
        Col::Next => next_cell(ui, theme, r, false),
        Col::Ci => ci_cell(ui, theme, r.pipeline),
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
                if toned_label_chip(ui, theme, label, *tone).clicked() {
                    clicked = Some(label.clone());
                }
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
        Col::Queue => {
            if let Some((text, tone, hint)) = &r.queue {
                ui.add(Chip::new(text, *tone))
                    .rich(Tip::new().chip(text.as_str(), *tone).text(hint.as_str()));
            }
        }
    }
    clicked
}

/// The right-click menu of a row; returns the action picked.
pub(super) fn context_menu(ui: &mut Ui, r: &RowView) -> Option<Action> {
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
