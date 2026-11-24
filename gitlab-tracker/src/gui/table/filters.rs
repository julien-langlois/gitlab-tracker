//! Active filters as chips (with undo), the empty state, search and milestone suggestions.

use super::*;

/// What narrows the list beyond every tracked MR, as chips that each undo
/// theirs: the cockpit focus, the view, the search, hidden merged MRs.
pub(super) fn active_filters(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let tones = &theme.tones;
    let mut chips: Vec<(String, Tone, Undo)> = Vec::new();
    if let Some(focus) = &g.app.focus {
        let template = if !crate::next_step::Bucket::of_key(&focus.key).is_empty() {
            "Waiting on: {name}"
        } else {
            "Cockpit: {name}"
        };
        chips.push((
            tf(template, &[("name", &focus.label)]),
            tones.blue,
            Undo::Focus,
        ));
    }
    if g.app.active_filter.index != 0 {
        let label = g.app.active_filter.label(&g.app.filter_defs);
        chips.push((
            tf("View: {name}", &[("name", &label)]),
            tones.purple,
            Undo::View,
        ));
    }
    for (i, filter) in g.app.filters.iter().enumerate() {
        chips.push((
            tf(
                "Filter: {name}",
                &[("name", &filter.label(&g.app.filter_defs))],
            ),
            tones.amber,
            Undo::Filter(i),
        ));
    }
    if let Some(kind) = g.app.status_pick {
        chips.push((
            tf("Status: {name}", &[("name", tl(kind.name()))]),
            tones::status_tone(kind, tones),
            Undo::Status,
        ));
    }
    let search = g.app.input.trim();
    if !search.is_empty() && !search.starts_with('@') {
        chips.push((
            tf("Search: {text}", &[("text", search)]),
            tones.teal,
            Undo::Search,
        ));
    }
    if g.app.config.hide_merged {
        chips.push((tl("Merged hidden").into(), tones.gray, Undo::HideMerged));
    }
    if chips.is_empty() {
        return;
    }
    let mut undo = Vec::new();
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                ui.label(muted(&theme, tl("Showing")));
                for (text, tone, what) in &chips {
                    if removable_chip(ui, &theme, text, *tone) {
                        undo.push(*what);
                    }
                }
                if chips.len() > 1 {
                    let reset = ui
                        .add(
                            egui::Label::new(
                                RichText::new(tl("Clear all"))
                                    .size(theme.text_size - 1.0)
                                    .color(theme.palette.accent),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if reset.clicked() {
                        undo = chips.iter().map(|c| c.2).collect();
                    }
                }
            });
        });
    // The filters taken off, in one go: each chip's index is in the list as it is.
    let gone: Vec<usize> = undo
        .iter()
        .filter_map(|u| match u {
            Undo::Filter(i) => Some(*i),
            _ => None,
        })
        .collect();
    if !gone.is_empty() {
        let filters = g
            .app
            .filters
            .iter()
            .enumerate()
            .filter(|(i, _)| !gone.contains(i))
            .map(|(_, f)| f.clone())
            .collect();
        g.pending.push(Action::SetFilters(filters));
    }
    for what in undo {
        apply_undo(g, what);
    }
}

/// What a chip of [`active_filters`] undoes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Undo {
    Focus,
    View,
    /// The sidebar filter at this index of `App::filters`.
    Filter(usize),
    Status,
    Search,
    HideMerged,
}

pub(super) fn apply_undo(g: &mut GuiApp, what: Undo) {
    match what {
        Undo::Focus => g.next_focus = Some(None),
        // Taken off together by `active_filters`.
        Undo::Filter(_) => {}
        Undo::View => g.pending.push(Action::SetFilter {
            index: 0,
            query: String::new(),
        }),
        Undo::Status => g.pending.push(Action::SetStatusPick(None)),
        Undo::Search => {
            g.app.input.clear();
            g.app.milestone_suggestions.clear();
        }
        Undo::HideMerged => g.pending.push(Action::ToggleHideMerged),
    }
}

/// A chip in `tone` with a ✕; returns whether it was clicked.
pub(super) fn removable_chip(ui: &mut Ui, theme: &Theme, text: &str, tone: Tone) -> bool {
    let font = fonts::medium(theme.text_size - 1.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, tone.fg);
    let size = Vec2::new(galley.size().x + 32.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        tone.bg.gamma_multiply(1.4)
    } else {
        tone.bg
    };
    let radius = CornerRadius::same(12);
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().galley(
        egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        tone.fg,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 12.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        "✕",
        egui::FontId::proportional(theme.text_size - 3.0),
        tone.fg,
    );
    super::super::widgets::focus_ring(ui, theme, &response, 12);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl("Remove"))
        .clicked()
}

/// No row to show: why, and the way back.
pub(super) fn empty_state(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let nothing_tracked = g.app.mrs.is_empty();
    let narrowed = g.app.focus.is_some()
        || g.app.status_pick.is_some()
        || g.app.active_filter.index != 0
        || !g.app.filters.is_empty()
        || !g.app.input.trim().is_empty()
        || g.app.config.hide_merged;
    ui.add_space(72.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 32.0, theme.palette.raised);
        let icon = if nothing_tracked {
            Icon::MergeRequests
        } else {
            Icon::Search
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(28.0)),
            icon,
            theme.palette.text_muted,
        );
        ui.add_space(14.0);
        let (title, text) = if nothing_tracked {
            (
                tl("No merge request tracked yet"),
                tl("Type an MR number in the search field and press Enter, or @ and a milestone to track all of its MRs."),
            )
        } else {
            (
                tl("No merge request matches"),
                tl("The view, the search or the cockpit selection hides every tracked MR."),
            )
        };
        ui.label(semibold(title, theme.text_size + 2.0));
        ui.add_space(4.0);
        ui.label(muted(&theme, text));
        ui.add_space(16.0);
        if nothing_tracked {
            if button(ui, &theme, Some(Icon::Search), tl("Track an MR"), true).clicked() {
                g.focus_search = true;
            }
        } else if narrowed
            && button(ui, &theme, Some(Icon::Close), tl("Show every MR"), true).clicked()
        {
            for what in [Undo::Focus, Undo::View, Undo::Search] {
                apply_undo(g, what);
            }
            if g.app.config.hide_merged {
                apply_undo(g, Undo::HideMerged);
            }
        }
    });
}

/// The search field. It filters the table as you type; Enter on an MR number
/// tracks it (`-123` stops tracking), and `@` + a milestone tracks its MRs.
pub(super) fn search(g: &mut GuiApp, ui: &mut Ui) {
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
pub(super) fn milestone_suggestions(g: &mut GuiApp, ui: &mut Ui, bar: Rect) {
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
