//! The toolbar above the table: sort, search width, buttons that fold into a menu.

use super::*;

pub(super) const TOOLBAR_HEIGHT: f32 = 52.0;

pub(super) fn toolbar(g: &mut GuiApp, ui: &mut Ui, shown: usize, sizes: &Sizes) {
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
    let count = muted(
        &theme,
        tf(
            "{n} of {total}",
            &[
                ("n", &shown.to_string()),
                ("total", &g.app.mrs.len().to_string()),
            ],
        ),
    );
    // What the right side needs, its buttons with their text or their icon
    // alone, and the search at its narrowest: the buttons lose their text
    // before anything overlaps, then the count goes.
    let fit = ToolbarFit::plan(g, ui, &count);
    if fit.count {
        ui.label(count);
    }
    ui.add_space(6.0);
    let icon_only = fit.icon_only;

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if button(
            ui,
            &theme,
            Some(Icon::Clipboard),
            if icon_only { "" } else { tl("Copy checklist") },
            true,
        )
        .tip(tl(
            "Copy the visible MRs as a Markdown release checklist (E)",
        ))
        .clicked()
        {
            g.pending.push(Action::CopyChecklist);
        }
        super::super::export::button_and_panel(g, ui, icon_only);
        let select = if icon_only { "" } else { tl("Select") };
        if button(ui, &theme, Some(Icon::List), select, g.bulk_mode)
            .tip(tl(
                "Bulk mode: tick several MRs to flag them, copy them or stop tracking them",
            ))
            .clicked()
        {
            g.bulk_mode = !g.bulk_mode;
            if !g.bulk_mode {
                g.marked.clear();
            }
        }
        let outlined = theme.outlined_icon_button();
        let height = theme.spacing.control_height + 2.0;
        if icon_button(ui, tl("Refresh now (R)"), Icon::Refresh, &outlined, height).clicked() {
            g.pending.push(Action::Refresh);
        }
        display_button(g, ui, icon_only);
        search(g, ui);
    });
    milestone_suggestions(g, ui, bar);
}

/// The narrowest the search gets before the toolbar's buttons give up their text.
pub(super) const SEARCH_MIN_WIDTH: f32 = 200.0;

/// How the toolbar fits the width it has.
pub(super) struct ToolbarFit {
    /// The buttons show their icon alone (their tooltip still says what they do).
    pub(super) icon_only: bool,
    /// The "n of total" count is shown.
    pub(super) count: bool,
}

impl ToolbarFit {
    /// Plans the toolbar from the room left after the title (`ui` is placed
    /// right after it) for the count, the buttons and the search.
    pub(super) fn plan(g: &GuiApp, ui: &Ui, count: &RichText) -> Self {
        let theme = &g.theme;
        let spacing = ui.spacing().item_spacing.x;
        let buttons = |icon_only: bool| {
            let text = |s: &str| {
                if icon_only {
                    String::new()
                } else {
                    s.to_string()
                }
            };
            let refresh = theme.spacing.control_height + 2.0;
            [
                (text(tl("Copy checklist")), true),
                (text(tl("Export")), false),
                (text(tl("Select")), false),
                (text(&display_text(g)), false),
            ]
            .iter()
            .map(|(t, primary)| button_width_for(ui, theme, true, t, *primary) + spacing)
            .sum::<f32>()
                + refresh
                + spacing
                + SEARCH_MIN_WIDTH
        };
        let count_width = egui::WidgetText::from(count.clone())
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            )
            .size()
            .x
            + spacing;
        // The 6 px set aside before the right side.
        let room = ui.available_width() - 6.0;
        if room >= count_width + buttons(false) {
            Self {
                icon_only: false,
                count: true,
            }
        } else {
            Self {
                icon_only: true,
                count: room >= count_width + buttons(true),
            }
        }
    }
}

/// The display button's text: the sort, and the grouping when there is one.
pub(super) fn display_text(g: &GuiApp) -> String {
    match g.group_by {
        GroupBy::None => sort_label(g.app.sort_column).to_string(),
        group => format!("{} · {}", sort_label(g.app.sort_column), group.label()),
    }
}

/// "↓ Updated": the sort; a click opens the display menu: sort, grouping,
/// density and columns, in one place to leave the search its room. `icon_only`:
/// the arrow alone, for a narrow toolbar.
pub(super) fn display_button(g: &mut GuiApp, ui: &mut Ui, icon_only: bool) {
    let theme = g.theme;
    let icon = match g.app.sort_order {
        SortOrder::Ascending => Icon::ArrowUp,
        SortOrder::Descending => Icon::ArrowDown,
    };
    let text = if icon_only {
        String::new()
    } else {
        display_text(g)
    };
    let response = button(ui, &theme, Some(icon), &text, false).tip(tl(
        "Sort, grouping, density and columns (S: next sort column, Shift+S: reverse, C: columns)",
    ));
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(200.0);
        ui.label(muted(&theme, tl("Sort by")));
        for column in [
            SortColumn::UpdatedAt,
            SortColumn::Id,
            SortColumn::Milestone,
            SortColumn::Title,
            SortColumn::MergedAt,
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
        ui.separator();
        ui.label(muted(&theme, tl("Group")));
        for group in GroupBy::ALL {
            if ui
                .selectable_label(g.group_by == group, group.label())
                .clicked()
            {
                g.group_by = group;
                g.folded.clear();
            }
        }
        ui.separator();
        ui.label(muted(&theme, tl("Density")));
        if ui.selectable_label(!g.compact, tl("Comfortable")).clicked() {
            g.compact = false;
        }
        if ui
            .selectable_label(g.compact, tl("Compact"))
            .tip(tl("One line per MR: twice as many in view"))
            .clicked()
        {
            g.compact = true;
        }
        ui.separator();
        if ui.button(tl("Columns… (C)")).clicked() {
            g.app.input_mode = InputMode::ColumnPicker;
        }
    });
}
