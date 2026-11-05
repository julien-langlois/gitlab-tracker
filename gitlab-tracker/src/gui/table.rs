//! The merge requests table and its toolbar.
//!
//! Rows are drawn by hand rather than with a table widget: each one is a rounded
//! card (selection ring, highlight of the MRs updated by the last refresh), and
//! only the rows in view are laid out.

use bevel_theme::{fonts, Chip, Theme, Tone};
use bevel_widgets::buttons::IconButton;
use egui::{
    Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2,
};

use super::icons::{self, Icon};
use super::shell::Sizes;
use super::widgets::{
    activity, avatar, badge, button, field, icon_button, icon_toggle, label_tone, mono, muted,
    semibold, split_person, toned_label_chip, FieldLook,
};
use super::{tones, GuiApp, Page};
use crate::actions::Action;
use crate::app::{App, InputMode, SortColumn, SortOrder};
use crate::i18n::{tf, tl, tn};
use crate::models::{Effort, GitlabMrState, MrStatus};

pub fn view(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let shown = g.app.visible_mrs().count();
    toolbar(g, ui, shown, sizes);
    super::shell::fetch_errors(g, ui);
    status_bar(g, ui);
    next_chips(g, ui);
    active_filters(g, ui);
    bulk_bar(g, ui);

    if shown == 0 {
        empty_state(g, ui);
        return;
    }

    let inspector_shown = sizes.inspector.is_some();
    egui::Frame::new()
        .inner_margin(Margin::symmetric(8, 0))
        .show(ui, |ui| table(g, ui, shown, inspector_shown));
}

/// What the bulk bar asked for, applied once the frame is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bulk {
    Flag(bool),
    CopyChecklist,
    StopTracking,
}

/// The MRs picked with Ctrl / Shift + click, and what to do with them all.
fn bulk_bar(g: &mut GuiApp, ui: &mut Ui) {
    // Gone from the list: no longer picked.
    let ids: std::collections::HashSet<String> = g.app.mrs.iter().map(|m| m.id.clone()).collect();
    g.marked.retain(|id| ids.contains(id));
    if g.marked.is_empty() {
        return;
    }
    let theme = g.theme;
    let n = g.marked.len();
    let mut picked = None;
    let mut clear = false;
    egui::Frame::new()
        .fill(theme.palette.selection)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 6))
        .outer_margin(Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(semibold(
                    tn(n as u64, "{n} MR selected", "{n} MRs selected"),
                    theme.text_size,
                ))
                .on_hover_text(tl("Ctrl+click adds one · Shift+click a range"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, &theme, Some(Icon::Close), tl("Clear"), false).clicked() {
                        clear = true;
                    }
                    let stop = button(ui, &theme, None, tl("Stop tracking…"), false);
                    egui::Popup::menu(&stop).show(|ui| {
                        ui.label(tn(
                            n as u64,
                            "Stop tracking {n} MR?",
                            "Stop tracking these {n} MRs?",
                        ));
                        if ui.button(tl("Stop tracking")).clicked() {
                            picked = Some(Bulk::StopTracking);
                        }
                    });
                    if button(
                        ui,
                        &theme,
                        Some(Icon::Clipboard),
                        tl("Copy checklist"),
                        false,
                    )
                    .clicked()
                    {
                        picked = Some(Bulk::CopyChecklist);
                    }
                    if button(ui, &theme, Some(Icon::StarOutline), tl("Unflag"), false).clicked() {
                        picked = Some(Bulk::Flag(false));
                    }
                    if button(ui, &theme, Some(Icon::Star), tl("Flag"), false).clicked() {
                        picked = Some(Bulk::Flag(true));
                    }
                });
            });
        });
    if clear {
        g.marked.clear();
    }
    g.bulk = picked.or(g.bulk);
}

// ── Toolbar ──────────────────────────────────────────────────────────────────

/// How the table groups its rows, under foldable headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    None,
    Status,
    Milestone,
    Author,
    /// Who each MR waits on (see `crate::next_step`).
    NextStep,
}

impl GroupBy {
    const ALL: [GroupBy; 5] = [
        GroupBy::None,
        GroupBy::Status,
        GroupBy::Milestone,
        GroupBy::Author,
        GroupBy::NextStep,
    ];

    fn label(self) -> &'static str {
        tl(match self {
            GroupBy::None => "No grouping",
            GroupBy::Status => "By status",
            GroupBy::Milestone => "By milestone",
            GroupBy::Author => "By author",
            GroupBy::NextStep => "By next step",
        })
    }
}

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
        display_button(g, ui);
        search(g, ui);
    });
    milestone_suggestions(g, ui, bar);
}

/// "↓ Updated": the sort; a click opens the display menu: sort, grouping,
/// density and columns, in one place to leave the search its room.
fn display_button(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let icon = match g.app.sort_order {
        SortOrder::Ascending => Icon::ArrowUp,
        SortOrder::Descending => Icon::ArrowDown,
    };
    let text = match g.group_by {
        GroupBy::None => sort_label(g.app.sort_column).to_string(),
        group => format!("{} · {}", sort_label(g.app.sort_column), group.label()),
    };
    let response = button(ui, &theme, Some(icon), &text, false).on_hover_text(tl(
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
            .on_hover_text(tl("One line per MR: twice as many in view"))
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

/// The MRs of the list by status at a glance: a bar, a segment per status as
/// wide as its share, over a legend with the counts. A click on either shows
/// that status alone (again: all of them).
fn status_bar(g: &mut GuiApp, ui: &mut Ui) {
    let counts = g.app.status_counts();
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    if total == 0 {
        return;
    }
    let theme = g.theme;
    let picked = g.app.status_pick;
    let mut clicked = None;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The bar.
            const HEIGHT: f32 = 8.0;
            const GAP: f32 = 3.0;
            let (bar, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), HEIGHT + 8.0),
                Sense::hover(),
            );
            let usable = bar.width() - GAP * (counts.len() - 1) as f32;
            let mut x = bar.left();
            for &(kind, n) in &counts {
                let width = usable * n as f32 / total as f32;
                let segment = Rect::from_min_size(
                    egui::pos2(x, bar.center().y - HEIGHT / 2.0),
                    Vec2::new(width, HEIGHT),
                );
                x += width + GAP;
                let swatch = tones::status_swatch(kind, &theme.tones);
                let response = ui.interact(
                    segment.expand2(Vec2::new(GAP / 2.0, 4.0)),
                    ui.scope_id().with(("status segment", kind)),
                    Sense::click(),
                );
                let on = picked == Some(kind);
                let color = if picked.is_some() && !on {
                    swatch.gamma_multiply(0.25)
                } else if response.hovered() {
                    swatch.gamma_multiply(1.15)
                } else {
                    swatch
                };
                let radius = CornerRadius::same((HEIGHT / 2.0) as u8);
                ui.painter().rect_filled(segment, radius, color);
                if on {
                    ui.painter().rect_stroke(
                        segment.expand(2.0),
                        CornerRadius::same((HEIGHT / 2.0 + 2.0) as u8),
                        Stroke::new(1.5, theme.palette.text),
                        egui::StrokeKind::Middle,
                    );
                }
                if status_hint(response, kind, n, total, on).clicked() {
                    clicked = Some(kind);
                }
            }
            ui.add_space(4.0);
            // The legend, the total on the right.
            let total_text = if picked.is_some() {
                tl("Click it again to show them all").to_string()
            } else {
                tn(total as u64, "{n} MR in the list", "{n} MRs in the list")
            };
            let total_galley = ui.painter().layout_no_wrap(
                total_text,
                egui::FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
            );
            ui.horizontal_top(|ui| {
                let legend_width = ui.available_width() - total_galley.size().x - 16.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(legend_width, 0.0),
                    Layout::left_to_right(Align::Center).with_main_wrap(true),
                    |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for &(kind, n) in &counts {
                            let on = picked == Some(kind);
                            let dim = picked.is_some() && !on;
                            let response = legend_item(ui, &theme, kind, n, on, dim);
                            if status_hint(response, kind, n, total, on).clicked() {
                                clicked = Some(kind);
                            }
                        }
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(total_galley.size().x, 24.0),
                        Sense::hover(),
                    );
                    ui.painter().galley(
                        egui::pos2(rect.left(), rect.center().y - total_galley.size().y / 2.0),
                        total_galley,
                        theme.palette.text_muted,
                    );
                });
            });
        });
    if let Some(kind) = clicked {
        let pick = (picked != Some(kind)).then_some(kind);
        g.pending.push(Action::SetStatusPick(pick));
    }
}

/// A status of the legend: its dot, its name, its count; filled when `on`,
/// faded when `dim` (another status is picked).
fn legend_item(
    ui: &mut Ui,
    theme: &Theme,
    kind: crate::status::StatusKind,
    count: usize,
    on: bool,
    dim: bool,
) -> egui::Response {
    let tone = tones::status_tone(kind, &theme.tones);
    let name_font = fonts::medium(theme.text_size - 1.5);
    let count_font = fonts::semibold(theme.text_size - 1.5);
    let text_color = if dim {
        theme.palette.text_muted
    } else {
        theme.palette.text_secondary
    };
    let name = ui
        .painter()
        .layout_no_wrap(tl(kind.name()).to_string(), name_font, text_color);
    let number = ui.painter().layout_no_wrap(
        count.to_string(),
        count_font,
        if dim {
            theme.palette.text_muted
        } else {
            theme.palette.text
        },
    );
    let size = Vec2::new(
        10.0 + 7.0 + 6.0 + name.size().x + 6.0 + number.size().x + 10.0,
        24.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = CornerRadius::same(12);
    if on {
        ui.painter().rect_filled(rect, radius, tone.bg);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tone.fg),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, theme.palette.raised);
    }
    let swatch = tones::status_swatch(kind, &theme.tones);
    let dot = if dim {
        swatch.gamma_multiply(0.35)
    } else {
        swatch
    };
    let mut x = rect.left() + 10.0;
    ui.painter()
        .circle_filled(egui::pos2(x + 3.5, rect.center().y), 3.5, dot);
    x += 13.0;
    let name_width = name.size().x;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - name.size().y / 2.0),
        name,
        text_color,
    );
    x += name_width + 6.0;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - number.size().y / 2.0),
        number,
        theme.palette.text,
    );
    super::widgets::focus_ring(ui, theme, &response, 12);
    response
}

/// The tone of a next-step bucket: mine in orange, the rest by stage.
pub fn bucket_tone(bucket: crate::next_step::Bucket, tones: &bevel_theme::Tones) -> Tone {
    use crate::next_step::Bucket;
    match bucket {
        Bucket::OnMe => tones.orange,
        Bucket::OnAuthor => tones.amber,
        Bucket::OnReviewers => tones.purple,
        Bucket::ToMerge => tones.teal,
        Bucket::ToDeploy => tones.blue,
        Bucket::Done => tones.green,
        Bucket::Unknown => tones.gray,
    }
}

/// Who the tracked MRs wait on, a chip per bucket with its count: a click lists
/// that bucket alone (again: all of them).
fn next_chips(g: &mut GuiApp, ui: &mut Ui) {
    use crate::next_step::Bucket;
    if g.app.mrs.is_empty() {
        return;
    }
    let theme = g.theme;
    let steps = crate::next_step::all(&g.app);
    let current = g.app.focus.as_ref().map(|f| f.key.clone());
    let mut clicked = None;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 0,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                for bucket in Bucket::ALL {
                    let n = steps.values().filter(|s| s.bucket() == bucket).count();
                    if bucket == Bucket::Unknown || bucket == Bucket::ToMerge && n == 0 {
                        continue;
                    }
                    let on = current.as_deref() == Some(bucket.key());
                    let tone = bucket_tone(bucket, &theme.tones);
                    let response = pill(
                        ui,
                        &theme,
                        bucket.label(),
                        n,
                        tone,
                        on,
                        current.is_some() && !on,
                    );
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tl(if on {
                            "Click to show them all"
                        } else {
                            "Click to show only these"
                        }))
                        .clicked()
                    {
                        clicked = Some((bucket, on));
                    }
                }
            });
        });
    match clicked {
        Some((_, true)) => g.next_focus = Some(None),
        Some((bucket, false)) => g.focus_on(bucket.key(), bucket.label()),
        None => {}
    }
}

/// A rounded chip with a dot, a name and a count; filled when `on`.
fn pill(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    count: usize,
    tone: Tone,
    on: bool,
    dim: bool,
) -> egui::Response {
    let font = fonts::medium(theme.text_size - 1.5);
    let text_color = if dim {
        theme.palette.text_muted
    } else {
        theme.palette.text_secondary
    };
    let name_g = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), text_color);
    let count_g = ui.painter().layout_no_wrap(
        count.to_string(),
        fonts::semibold(theme.text_size - 1.5),
        if count > 0 && !dim {
            tone.fg
        } else {
            theme.palette.text_muted
        },
    );
    let size = Vec2::new(
        10.0 + 13.0 + name_g.size().x + 6.0 + count_g.size().x + 10.0,
        24.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = CornerRadius::same(12);
    if on {
        ui.painter().rect_filled(rect, radius, tone.bg);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tone.fg),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, theme.palette.raised);
    } else {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme.palette.border_soft),
            egui::StrokeKind::Inside,
        );
    }
    let mut x = rect.left() + 10.0;
    ui.painter().circle_filled(
        egui::pos2(x + 3.5, rect.center().y),
        3.5,
        if dim {
            tone.fg.gamma_multiply(0.35)
        } else {
            tone.fg
        },
    );
    x += 13.0;
    let w = name_g.size().x;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - name_g.size().y / 2.0),
        name_g,
        text_color,
    );
    x += w + 6.0;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - count_g.size().y / 2.0),
        count_g,
        theme.palette.text,
    );
    super::widgets::focus_ring(ui, theme, &response, 12);
    response
}

/// "Conflict · 1 MR (14 %)" and what a click does, on a segment or a legend item.
fn status_hint(
    response: egui::Response,
    kind: crate::status::StatusKind,
    count: usize,
    total: usize,
    on: bool,
) -> egui::Response {
    let share = (count as f32 * 100.0 / total as f32).round();
    let hint = format!(
        "{} · {} ({share} %)\n{}",
        tl(kind.name()),
        tn(count as u64, "{n} MR", "{n} MRs"),
        tl(if on {
            "Click to show them all"
        } else {
            "Click to show only these"
        }),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// What narrows the list beyond every tracked MR, as chips that each undo
/// theirs: the cockpit focus, the view, the search, hidden merged MRs.
fn active_filters(g: &mut GuiApp, ui: &mut Ui) {
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
    for what in undo {
        apply_undo(g, what);
    }
}

/// What a chip of [`active_filters`] undoes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Undo {
    Focus,
    View,
    Status,
    Search,
    HideMerged,
}

fn apply_undo(g: &mut GuiApp, what: Undo) {
    match what {
        Undo::Focus => g.next_focus = Some(None),
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
fn removable_chip(ui: &mut Ui, theme: &Theme, text: &str, tone: Tone) -> bool {
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
    super::widgets::focus_ring(ui, theme, &response, 12);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Remove"))
        .clicked()
}

/// No row to show: why, and the way back.
fn empty_state(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let nothing_tracked = g.app.mrs.is_empty();
    let narrowed = g.app.focus.is_some()
        || g.app.status_pick.is_some()
        || g.app.active_filter.index != 0
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
    /// Who the MR waits on, and since when (the window's own: not in the TUI).
    Next,
    /// The latest pipeline, as an icon (the window's own).
    Ci,
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
    const ALL: [Col; 10] = [
        Col::Next,
        Col::Ci,
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
    const DROP_ORDER: [Col; 10] = [
        Col::Activity,
        Col::Labels,
        Col::Ticket,
        Col::Behind,
        Col::Target,
        Col::Milestone,
        Col::Notes,
        Col::Effort,
        Col::Ci,
        Col::Next,
    ];

    /// Drawn by the window alone: shown unless hidden in its column picker.
    pub fn window_only(self) -> bool {
        matches!(self, Col::Next | Col::Ci)
    }

    fn id(self) -> &'static str {
        match self {
            Col::Next => "next_step",
            Col::Ci => "ci",
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
            Col::Next => 196.0,
            Col::Ci => 36.0,
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

    fn title(self) -> &'static str {
        tl(match self {
            Col::Next => "Next step",
            Col::Ci => "CI",
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
fn wanted_columns(app: &App, hide_next: bool) -> Vec<Col> {
    let offered: Vec<&str> = app.visible_column_defs().iter().map(|c| c.id).collect();
    let chosen = app.project_settings.visible_columns.is_some();
    Col::ALL
        .into_iter()
        .filter(|c| c.window_only() || offered.contains(&c.id()))
        .filter(|c| {
            if c.window_only() {
                !(hide_next && *c == Col::Next)
            } else if chosen {
                app.config.visible_columns.is_visible(c.id())
            } else {
                !matches!(c, Col::Activity | Col::Labels)
            }
        })
        .collect()
}

const FLAG_WIDTH: f32 = 18.0;
const ID_WIDTH: f32 = 52.0;
const STATUS_WIDTH: f32 = 116.0;
/// Fixed columns: flag, MR, status (the title takes the rest).
const FIXED_COLUMNS_WIDTH: f32 = FLAG_WIDTH + ID_WIDTH + STATUS_WIDTH;
/// The title never gets narrower than this; below, the table scrolls sideways.
const MIN_TITLE_WIDTH: f32 = 200.0;
/// Room the title keeps before an optional column is dropped.
const TITLE_COMFORT_WIDTH: f32 = 220.0;
const COLUMN_GAP: f32 = 8.0;
/// A dot per branch: narrow, the branch's name on top (in full on hover).
const BRANCH_WIDTH: f32 = 50.0;
/// Branch cells stop growing there: the room left goes to the title.
const BRANCH_MAX_WIDTH: f32 = 72.0;
/// Inside a row, left and right.
const ROW_PADDING: f32 = 8.0;
const ROW_HEIGHT: f32 = 50.0;
/// A row in the compact density: the title only.
const COMPACT_ROW_HEIGHT: f32 = 34.0;
const ROW_GAP: f32 = 2.0;

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
enum Cell {
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
struct Grid {
    cells: Vec<(Cell, f32, f32)>,
    width: f32,
    row_height: f32,
    /// The tracked branches, named in the flow cell's hover.
    names: Vec<String>,
    /// Their short names, over the flow cell's dots and the branch columns.
    aliases: Vec<String>,
    /// The space between two dots of the flow cell, for the widest alias.
    flow_step: f32,
}

/// The tracked branches from feature to production (the timeline's order),
/// each with its short name: `(name, alias)`.
fn branch_order(app: &crate::app::App) -> Vec<(String, String)> {
    app.branch_flow()
        .in_order()
        .into_iter()
        .map(|b| (b.name.clone(), b.alias.clone()))
        .collect()
}

/// The flow cell: a dot per branch, joined.
fn flow_width(branches: usize, step: f32) -> f32 {
    (branches as f32 * step + 12.0).max(64.0)
}
/// The narrowest space between two dots of the flow cell.
const FLOW_STEP: f32 = 30.0;

/// The space between two dots for the widest of `aliases` to sit over its dot.
fn flow_step(ui: &Ui, theme: &Theme, aliases: &[String]) -> f32 {
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
    fn with_flow(columns: &[Col], branches: usize, step: f32, width: f32) -> Self {
        let flow = flow_width(branches, step);
        let mut grid = Self::new(columns, 0, BRANCH_WIDTH, width - flow - COLUMN_GAP);
        let x = grid.width - ROW_PADDING + COLUMN_GAP;
        grid.cells.push((Cell::Flow, x, flow));
        grid.width += flow + COLUMN_GAP;
        grid.flow_step = step;
        grid
    }

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
            row_height: ROW_HEIGHT,
            names: Vec::new(),
            aliases: Vec::new(),
            flow_step: FLOW_STEP,
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
    /// Picked with Ctrl / Shift + click, for the bulk bar.
    marked: bool,
    status: (&'static str, Tone),
    effort: Option<(&'static str, Tone)>,
    threads: u32,
    activity: (&'static str, Tone),
    /// The color of the "updated" date: amber once slowing, red once stale.
    age: Option<egui::Color32>,
    /// The title is not loaded yet, and a fetch is on its way.
    loading: bool,
    branches: Vec<BranchCell>,
    milestone: Option<String>,
    target: String,
    behind: Option<u32>,
    /// Still open: what it lacks can be listed.
    opened: bool,
    ticket: Option<(String, String)>,
    has_ticket: bool,
    /// Who it waits on, for the Next step column.
    next: Option<NextView>,
    /// The latest pipeline: its state, `None` without one.
    pipeline: Option<crate::models::PipelineState>,
}

/// The Next step cell: who, what, since when.
#[derive(Debug, Clone)]
pub struct NextView {
    /// "You", "Alex", "Deploy"…
    pub who: String,
    /// "rebase, 2 threads"
    pub what: String,
    /// "waiting 12 d", "on staging for 4 d"
    pub since: String,
    pub tone: Tone,
    /// I have to act: drawn stronger.
    pub mine: bool,
    /// The todos in full, for the hover.
    pub hint: String,
}

impl NextView {
    pub fn of(step: &crate::next_step::NextStep, theme: &Theme) -> Option<Self> {
        use crate::next_step::{days_text, Actor};
        let tones = &theme.tones;
        let now = chrono::Utc::now();
        let waited = step.waited_days(now).map(days_text);
        let (tone, since) = match step.actor {
            Actor::Unknown => return None,
            Actor::Nobody if step.closed => (tones.gray, String::new()),
            Actor::Nobody => (
                tones.green,
                step.reached
                    .as_ref()
                    .map(|b| tf("on {branch}", &[("branch", b)]))
                    .unwrap_or_default(),
            ),
            Actor::Deploy => (
                tones.blue,
                match (&step.reached, &waited) {
                    (Some(b), Some(w)) => {
                        tf("on {branch} for {time}", &[("branch", b), ("time", w)])
                    }
                    (Some(b), None) => tf("on {branch}", &[("branch", b)]),
                    _ => String::new(),
                },
            ),
            _ => (
                if step.mine { tones.orange } else { tones.gray },
                waited
                    .map(|w| tf("waiting {time}", &[("time", &w)]))
                    .unwrap_or_default(),
            ),
        };
        let mut hint: Vec<String> = step
            .todos
            .iter()
            .map(|t| format!("• {}", t.text()))
            .collect();
        if !step.people.is_empty() && !step.mine {
            hint.insert(
                0,
                step.people
                    .iter()
                    .map(|p| split_person(p).0.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !step.then.is_empty() {
            hint.push(tf("Then: {steps}", &[("steps", &step.then.join(" → "))]));
        }
        Some(Self {
            who: step.who(),
            what: step.summary(),
            since,
            tone,
            mine: step.mine,
            hint: hint.join("\n"),
        })
    }
}

/// A branch cell: where the MR stands on it, or why that is not known yet.
#[derive(Debug, Clone, Copy)]
enum BranchCell {
    Known(&'static str, &'static str, Tone),
    /// A fetch is on its way.
    Loading,
    /// Not fetched, and nothing on its way: the next refresh tells.
    Unknown,
}

/// The rows of `indexes` (into the visible list): only those in view are copied
/// out, so a frame costs the same whatever the number of MRs.
fn row_views(g: &GuiApp, indexes: &[usize]) -> Vec<RowView> {
    let app = &g.app;
    let theme = &g.theme;
    let tones = &theme.tones;
    let selected = app.table_state.selected();
    let fresh_window = app.update_highlight_ticks > 0;
    let me = app.config.gitlab_username.as_deref();
    let order = branch_order(app);
    let mut views: Vec<RowView> = app
        .visible_mrs()
        .enumerate()
        .filter(|(index, _)| indexes.contains(index))
        .map(|(index, mr)| {
            let failed = matches!(mr.status, MrStatus::Error(_));
            let fetching = app.pending_initial_fetches.contains(&mr.id)
                || app.pending_refresh_fetches.contains(&mr.id);
            let activity = activity(app, mr.updated_at.as_deref(), theme);
            RowView {
                index,
                id: mr.id.clone(),
                title: if mr.title.is_empty() {
                    tl("Loading…").into()
                } else {
                    mr.title.clone()
                },
                loading: mr.title.is_empty() && fetching,
                age: [tones.amber, tones.red]
                    .iter()
                    .find(|t| t.fg == activity.1.fg)
                    .map(|t| t.fg),
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
                marked: g.marked.contains(&mr.id),
                status: tones::status(mr.state, &mr.mergeability, tones),
                effort: mr.diff_stats.as_ref().map(|s| {
                    let effort = Effort::from_score(s.difficulty(&app.config.complexity_profile));
                    tones::effort(effort, tones)
                }),
                threads: mr.user_notes_count,
                activity,
                branches: order
                    .iter()
                    .map(|(b, _)| match app.branch_presence(mr, b) {
                        None if !failed && fetching => BranchCell::Loading,
                        None if !failed => BranchCell::Unknown,
                        presence => {
                            let (glyph, label, tone) = tones::presence(presence, failed, tones);
                            BranchCell::Known(glyph, label, tone)
                        }
                    })
                    .collect(),
                milestone: mr.milestone.clone(),
                target: mr.target_branch.clone(),
                behind: mr.diff_stats.as_ref().and_then(|s| s.commits_behind),
                opened: mr.state == GitlabMrState::Opened,
                ticket: mr
                    .linked_ticket
                    .as_ref()
                    .map(|t| (t.id.clone(), t.status.clone())),
                has_ticket: mr.linked_ticket.is_some() && app.tracker.is_some(),
                next: NextView::of(&crate::next_step::of(app, mr, me), theme),
                pipeline: mr.pipelines.first().map(|p| p.status),
            }
        })
        .collect();
    // In the order asked for (the groups reorder the list).
    views.sort_by_key(|v| indexes.iter().position(|i| *i == v.index));
    views
}

/// A line of the table: a group's heading, or an MR (its index in the visible list).
enum Line {
    Heading {
        key: String,
        label: String,
        count: usize,
        tone: Option<Tone>,
        folded: bool,
    },
    Mr(usize),
}

/// The table's lines: the visible MRs, under their group's heading when grouped,
/// folded groups without their MRs.
fn lines(g: &GuiApp) -> Vec<Line> {
    let app = &g.app;
    if g.group_by == GroupBy::None {
        return (0..app.visible_mrs().count()).map(Line::Mr).collect();
    }
    let tones = &g.theme.tones;
    let me = app.config.gitlab_username.as_deref();
    // (key, label, tone, members), groups in the order they first show up.
    let mut groups: Vec<(String, String, Option<Tone>, Vec<usize>)> = Vec::new();
    for (index, mr) in app.visible_mrs().enumerate() {
        let (label, tone) = match g.group_by {
            GroupBy::NextStep => {
                let bucket = crate::next_step::of(app, mr, me).bucket();
                (bucket.label().to_string(), Some(bucket_tone(bucket, tones)))
            }
            GroupBy::Status => {
                let (label, tone) = tones::status(mr.state, &mr.mergeability, tones);
                (label.to_string(), Some(tone))
            }
            GroupBy::Milestone => (
                mr.milestone
                    .clone()
                    .unwrap_or_else(|| tl("No milestone").into()),
                None,
            ),
            GroupBy::Author => (split_person(&mr.author).0.to_string(), None),
            GroupBy::None => unreachable!(),
        };
        match groups.iter_mut().find(|grp| grp.1 == label) {
            Some(group) => group.3.push(index),
            None => groups.push((
                format!("{:?}:{label}", g.group_by),
                label,
                tone,
                vec![index],
            )),
        }
    }
    if g.group_by == GroupBy::NextStep {
        // In the order of the flow: me first, done last.
        groups.sort_by_key(|grp| {
            crate::next_step::Bucket::ALL
                .iter()
                .position(|b| b.label() == grp.1)
                .unwrap_or(usize::MAX)
        });
    }
    let mut lines = Vec::new();
    for (key, label, tone, members) in groups {
        let folded = g.folded.contains(&key);
        lines.push(Line::Heading {
            key,
            label,
            count: members.len(),
            tone,
            folded,
        });
        if !folded {
            lines.extend(members.into_iter().map(Line::Mr));
        }
    }
    lines
}

/// What a click in the table asked for, applied once the table is drawn.
enum RowAction {
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

fn table(g: &mut GuiApp, ui: &mut Ui, shown: usize, inspector_shown: bool) {
    let theme = g.theme;
    // The "Behind" counts drawn, by MR id: a click lists what the MR lacks.
    let mut behind_cells: Vec<(String, egui::Response)> = Vec::new();
    let lines = lines(g);
    let (branches, aliases): (Vec<String>, Vec<String>) = branch_order(&g.app).into_iter().unzip();
    let wanted = wanted_columns(&g.app, g.hide_next_column);
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
            if g.marked.is_empty() {
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
                response.on_hover_text(branches.join(" → "));
            }
            (Cell::Branch(i), _) => {
                let name = branches.get(i).map_or("", String::as_str);
                let alias = grid.aliases.get(i).map_or(name, String::as_str);
                ui.add(
                    egui::Label::new(
                        mono(alias, theme.text_size - 1.5).color(theme.palette.text_muted),
                    )
                    .truncate(),
                )
                .on_hover_text(name);
            }
            _ => {}
        }
    }
}

/// A group's heading, as tall as a row: a chevron, its name and its count.
/// Returns whether it was clicked (to fold or unfold it).
fn group_heading(
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
        .on_hover_text(tl(if folded { "Unfold" } else { "Fold" }))
        .clicked()
}

/// Text in the accent that reads as a link; returns its response.
fn link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
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
fn shimmer(ui: &mut Ui, theme: &Theme, width: f32, height: f32) -> egui::Response {
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
/// (see [`super::mr_page::behind_popup`]); the MR picked there opens.
fn behind_popups(g: &mut GuiApp, ui: &Ui, cells: &[(String, egui::Response)]) {
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
        if let Some(picked) = super::mr_page::behind_popup(g, response, &mr, data.as_ref()) {
            open = Some(picked);
        }
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

/// A row; with the "Behind" count drawn as a link when the MR lacks commits.
fn row(
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
    if response.double_clicked() {
        action = Some(RowAction::Open(r.index));
    } else if response.clicked() && modifiers.command {
        action.get_or_insert(RowAction::Toggle(r.index));
    } else if response.clicked() && modifiers.shift {
        action.get_or_insert(RowAction::Range(r.index));
    } else if response.clicked() || response.secondary_clicked() {
        action.get_or_insert(RowAction::Select(r.index));
    }
    super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
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
fn behind_link(ui: &mut Ui, theme: &Theme, n: u32) -> egui::Response {
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
    .on_hover_text(tl("See what it lacks"))
}

/// The Next step cell: who (in their tone, stronger when it is me) and what,
/// over since when; one line in the compact density. The todos on hover.
fn next_cell(ui: &mut Ui, theme: &Theme, r: &RowView, compact: bool) {
    let Some(next) = &r.next else {
        ui.label(muted(theme, "—"))
            .on_hover_text(tl("Known once the MR is loaded"));
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
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add_space(7.0);
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
        let hint = if compact && !next.since.is_empty() {
            format!("{}\n{}", next.since, next.hint)
        } else {
            next.hint.clone()
        };
        response.on_hover_text(hint);
    }
}

/// The latest pipeline as a round icon: a check, a cross, a ring while it runs.
fn ci_cell(ui: &mut Ui, theme: &Theme, state: Option<crate::models::PipelineState>) {
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
        response.on_hover_text(tl("No pipeline"));
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
    response.on_hover_text(label);
}

/// Every branch of the row as a line of dots, joined: the flow at a glance.
fn flow_cell(ui: &mut Ui, theme: &Theme, cells: &[BranchCell], names: &[String], step: f32) {
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
    let mut hints = Vec::new();
    for (i, cell) in cells.iter().enumerate() {
        let name = names.get(i).map_or("", String::as_str);
        match cell {
            BranchCell::Known(glyph, label, tone) => {
                painter.circle_filled(center(i), 8.0, tone.bg);
                presence_glyph(painter, center(i), glyph, tone.fg);
                hints.push(format!("{name}: {label}"));
            }
            BranchCell::Loading => {
                painter.circle_stroke(center(i), 6.0, Stroke::new(1.5, theme.palette.text_muted));
                hints.push(format!("{name}: {}", tl("Loading")));
            }
            BranchCell::Unknown => {
                painter.circle_stroke(center(i), 6.0, Stroke::new(1.0, theme.palette.border));
                hints.push(format!("{name}: —"));
            }
        }
    }
    response.on_hover_text(hints.join("\n"));
}

/// A branch cell: a dot in the presence's tone with its glyph, named on hover.
fn branch_cell(ui: &mut Ui, theme: &Theme, cell: BranchCell) {
    match cell {
        BranchCell::Known(glyph, label, tone) => {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 11.0, tone.bg);
            presence_glyph(ui.painter(), rect.center(), glyph, tone.fg);
            response.on_hover_text(label);
        }
        BranchCell::Loading => {
            shimmer(ui, theme, 22.0, 22.0).on_hover_text(tl("Loading"));
        }
        BranchCell::Unknown => {
            ui.add_space(7.0);
            ui.label(muted(theme, "—"))
                .on_hover_text(tl("Not checked yet: the next refresh will tell"));
        }
    }
}

/// Paints a presence glyph (see `tones::presence`) as shapes centered on
/// `center`: the font's glyphs sit off center, each by its own offset.
fn presence_glyph(painter: &egui::Painter, center: egui::Pos2, glyph: &str, color: egui::Color32) {
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
/// their own column); returns the label clicked, if any.
fn title_cell(ui: &mut Ui, theme: &Theme, r: &RowView, grid: &Grid) -> Option<String> {
    if grid.row_height < ROW_HEIGHT {
        // Compact: the title alone, who and when on hover.
        let (name, _) = split_person(&r.author);
        let hint = if r.updated.is_empty() {
            name.to_string()
        } else {
            format!("{name} · {}", r.updated)
        };
        ui.add(
            egui::Label::new(RichText::new(&r.title).font(fonts::medium(theme.text_size)))
                .truncate(),
        )
        .on_hover_text(hint);
        return None;
    }
    let mut clicked = None;
    let labels_column = grid
        .cells
        .iter()
        .any(|(c, _, _)| *c == Cell::Optional(Col::Labels));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add_space(6.0);
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
    let color = r.age.unwrap_or(theme.palette.text_muted);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
    icons::paint(ui.painter(), rect, Icon::Clock, color);
    let date = ui.label(RichText::new(&r.updated).size(size).color(color));
    if r.age.is_some() {
        date.on_hover_text(r.activity.0);
    }
}

fn select(g: &mut GuiApp, index: usize) {
    if g.app.table_state.selected() != Some(index) {
        g.app.table_state.select(Some(index));
        g.app.layout.inspector.reset();
    }
}

/// One optional cell; returns the label clicked in the Labels one, if any.
fn cell_content(ui: &mut Ui, theme: &Theme, r: &RowView, col: Col) -> Option<String> {
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
    }
    clicked
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
}
