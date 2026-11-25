//! The column picker: a live preview of the table's head, then the columns as cards.

use super::*;

/// What a shared column shows, by its id.
pub(super) fn column_detail(id: &str) -> &'static str {
    tl(match id {
        "activity" => "Recent, slowing or stale, from its last update",
        "target_branch" => "The branch it merges into",
        "labels" => "Its GitLab labels",
        "milestone" => "The milestone it belongs to",
        "notes" => "How many comments it holds",
        "diff_stats" => "How hard it is to review, from its size",
        "commits_behind" => "The commits its target branch gained since",
        "tracker_ticket" => "Its linked ticket",
        _ => "",
    })
}

/// The column picker: a live preview of the table's head, then the columns as
/// cards to switch on and off, those shared with the terminal UI first.
pub(super) fn columns(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    body(ui, |ui| {
        caption(ui, &theme, tl("Preview"));
        ui.add_space(2.0);
        preview(g, ui);
        ui.add_space(20.0);

        caption(ui, &theme, tl("Shared with the terminal UI"));
        ui.add_space(4.0);
        let defs = g.app.visible_column_defs();
        let cards: Vec<(&str, &str, bool)> = defs
            .iter()
            .map(|def| {
                (
                    tl(def.label),
                    column_detail(def.id),
                    table::shows_column(&g.app, def.id),
                )
            })
            .collect();
        if let Some(i) = toggle_cards(ui, &theme, "shared_columns", &cards) {
            toggle_column(g, defs[i].id);
        }
        ui.add_space(12.0);

        caption(ui, &theme, tl("This window only"));
        ui.add_space(4.0);
        let cards = [
            (
                tl("Next step"),
                tl("Who each MR waits on, and since when"),
                !g.hide_next_column,
            ),
            (
                tl("Branches in one cell"),
                tl("A dot per branch, joined, instead of a column each: room for the title"),
                g.flow_cells,
            ),
        ];
        match toggle_cards(ui, &theme, "window_columns", &cards) {
            Some(0) => g.hide_next_column = !g.hide_next_column,
            Some(_) => g.flow_cells = !g.flow_cells,
            None => {}
        }
    });
    footer(ui, &theme, |ui| {
        ui.label(muted(&theme, tl("Applied at once, kept for this project.")));
        buttons(ui, |ui| {
            if primary_button(ui, &theme, tl("Done")).clicked() {
                close(g);
            }
        });
    });
}

/// Shows or hides the shared column `id`. The first choice starts from the
/// columns the table shows, not from the terminal UI's defaults.
pub(super) fn toggle_column(g: &mut GuiApp, id: &str) {
    if g.app.project_settings.visible_columns.is_none() {
        for def in g.app.visible_column_defs() {
            let shown = table::shows_column(&g.app, def.id);
            g.app.config.visible_columns.set_visible(def.id, shown);
        }
    }
    g.app.config.visible_columns.toggle(id);
    // From now on the table follows this choice (see `table::wanted_columns`).
    g.app.project_settings.visible_columns = Some(g.app.config.visible_columns.clone());
}

pub(super) fn save_columns(g: &mut GuiApp) {
    if g.app.project_settings.visible_columns.is_some() {
        g.pending.push(Action::SaveVisibleColumns);
    }
}

/// How a column shows in the preview.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Head {
    /// Always there: the MR, its title, the branches.
    Fixed,
    /// Picked, and shown.
    Shown,
    /// Picked, but the window is too narrow for it.
    Hidden,
}

/// The table's head as the table draws it now, the picked columns in the
/// accent; those the window has no room for, dashed.
pub(super) fn preview(g: &GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let wanted = table::wanted_columns(&g.app, g.hide_next_column, !g.queues.is_empty());
    // The table fits its columns while drawn: elsewhere, nothing is dropped.
    let on_table = matches!(g.page(), super::super::Page::MergeRequests);
    let shown = |col: &table::Col| !on_table || g.shown_columns.contains(col);
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(12, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                head(ui, &theme, tl("MR"), Head::Fixed, 0.0);
                head(ui, &theme, tl("Title"), Head::Fixed, 130.0);
                for col in &wanted {
                    let kind = if shown(col) {
                        Head::Shown
                    } else {
                        Head::Hidden
                    };
                    head(ui, &theme, col.title(), kind, 0.0);
                }
                head(ui, &theme, tl("Branches"), Head::Fixed, 0.0);
            });
        });
    let hidden = wanted.iter().filter(|c| !shown(c)).count();
    if hidden > 0 {
        ui.add_space(6.0);
        ui.label(
            RichText::new(tn(
                hidden as u64,
                "{n} column hidden: the window is too narrow for it. Widen it to see it.",
                "{n} columns hidden: the window is too narrow for them. Widen it to see them.",
            ))
            .size(theme.text_size - 1.5)
            .color(theme.tones.amber.fg),
        );
    }
}

pub(super) fn head(ui: &mut Ui, theme: &Theme, text: &str, kind: Head, min_width: f32) {
    let p = &theme.palette;
    let color = match kind {
        Head::Fixed => p.text_secondary,
        Head::Shown => p.accent,
        Head::Hidden => p.text_muted,
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        bevel_theme::fonts::semibold(theme.text_size - 2.0),
        color,
    );
    let size = Vec2::new((galley.size().x + 18.0).max(min_width), 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let radius = CornerRadius::same(theme.radii.medium);
    match kind {
        Head::Fixed => {
            ui.painter().rect_filled(rect, radius, p.input);
        }
        Head::Shown => {
            ui.painter()
                .rect_filled(rect, radius, p.accent.gamma_multiply(0.14));
            ui.painter().rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, p.accent.gamma_multiply(0.35)),
                StrokeKind::Inside,
            );
        }
        Head::Hidden => {
            let r = rect.shrink(0.5);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            ui.painter().extend(egui::Shape::dashed_line(
                &corners,
                Stroke::new(1.0, p.border),
                4.0,
                3.0,
            ));
        }
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if kind == Head::Hidden {
        response.tip(tl("Hidden: the window is too narrow"));
    }
}

/// `cards` (title, detail, on) two by two; the one clicked, if any.
pub(super) fn toggle_cards(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    cards: &[(&str, &str, bool)],
) -> Option<usize> {
    const GAP: f32 = 10.0;
    let width = ((ui.available_width() - GAP) / 2.0).floor();
    let mut clicked = None;
    for (r, pair) in cards.chunks(2).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (c, (title, detail, on)) in pair.iter().enumerate() {
                let index = r * 2 + c;
                if toggle_card(ui, theme, (id, index), title, detail, *on, width).clicked() {
                    clicked = Some(index);
                }
            }
        });
        ui.add_space(GAP - ui.spacing().item_spacing.y);
    }
    clicked
}

/// A card to switch on and off: its title over what it shows, and a switch.
pub(super) fn toggle_card(
    ui: &mut Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    detail: &str,
    on: bool,
    width: f32,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 62.0), Sense::hover());
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), on, title)
    });
    let p = &theme.palette;
    let mut cut = false;
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, on);
        let radius = CornerRadius::same(theme.radii.large);
        let hovered = response.hovered();
        ui.painter()
            .rect_filled(rect, radius, if hovered { p.content } else { p.panel });
        if how_on > 0.0 {
            ui.painter()
                .rect_filled(rect, radius, p.accent.gamma_multiply(0.08 * how_on));
        }
        let stroke = if on {
            p.accent.gamma_multiply(0.6)
        } else if hovered {
            p.text_muted.gamma_multiply(0.7)
        } else {
            p.border
        };
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0, stroke), StrokeKind::Inside);
        let text_width = rect.width() - 14.0 - 30.0 - 28.0;
        let title_color = if on { p.text } else { p.text_secondary };
        let galley = one_line(
            ui,
            title,
            bevel_theme::fonts::medium(theme.text_size),
            title_color,
            text_width,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 14.0, rect.top() + 12.0),
            galley,
            title_color,
        );
        let font = egui::FontId::proportional(theme.text_size - 1.5);
        cut = ui
            .painter()
            .layout_no_wrap(detail.to_owned(), font.clone(), p.text_muted)
            .size()
            .x
            > text_width;
        let galley = one_line(ui, detail, font, p.text_muted, text_width);
        ui.painter().galley(
            egui::pos2(rect.left() + 14.0, rect.top() + 34.0),
            galley,
            p.text_muted,
        );
        super::super::widgets::switch_track(
            ui,
            theme,
            egui::pos2(rect.right() - 14.0 - 30.0, rect.center().y),
            how_on,
        );
    }
    focus_ring(ui, theme, &response, theme.radii.large);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if cut {
        response.tip(detail)
    } else {
        response
    }
}
