//! The merge request table's saved views, in the window: the "My views"
//! section of the sidebar, the dialogs that save one and import another
//! project's, and the keys 1 to 9. The views themselves, and what they keep,
//! are in [`crate::saved_views`]; they are saved in `projects.toml` with the
//! project (in memory only in the demo).

use bevel_theme::{fonts, Theme};
use egui::{Color32, CornerRadius, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::Icon;
use super::table::{self, GroupBy};
use super::tip::{Tip, TipExt};
use super::widgets::{button, field, muted, primary_button, section_title, FieldLook};
use super::GuiApp;
use crate::actions::Action;
use crate::app::{SortColumn, SortOrder};
use crate::i18n::{tc, tf, tl, tn};
use crate::saved_views::{Filters, Keep, Order, SavedView, Snapshot};
use crate::status::StatusKind;

/// Views reachable by a key, 1 to 9.
const KEYS: usize = 9;

/// The dialog open, if any.
#[derive(Debug, Clone)]
pub enum Dialog {
    /// Saving the table as a view.
    Save { name: String, keep: Keep },
    /// Picking views of the other projects: (project, view, picked).
    Import {
        views: Vec<(String, SavedView, bool)>,
    },
}

/// What the window remembers of the views.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The view last applied, by name, while the table still follows it.
    pub active: Option<String>,
    pub dialog: Option<Dialog>,
}

// ── The table, in a view's terms ─────────────────────────────────────────────

fn sort_id(column: SortColumn) -> &'static str {
    match column {
        SortColumn::UpdatedAt => "updated",
        SortColumn::Id => "id",
        SortColumn::Milestone => "milestone",
        SortColumn::Title => "title",
    }
}

fn sort_of(id: &str) -> Option<SortColumn> {
    Some(match id {
        "updated" => SortColumn::UpdatedAt,
        "id" => SortColumn::Id,
        "milestone" => SortColumn::Milestone,
        "title" => SortColumn::Title,
        _ => return None,
    })
}

fn group_id(group: GroupBy) -> &'static str {
    match group {
        GroupBy::None => "none",
        GroupBy::Status => "status",
        GroupBy::Milestone => "milestone",
        GroupBy::Author => "author",
        GroupBy::NextStep => "next_step",
    }
}

fn group_of(id: &str) -> GroupBy {
    match id {
        "status" => GroupBy::Status,
        "milestone" => GroupBy::Milestone,
        "author" => GroupBy::Author,
        "next_step" => GroupBy::NextStep,
        _ => GroupBy::None,
    }
}

fn status_id(kind: StatusKind) -> String {
    format!("{kind:?}")
}

fn status_of(id: &str) -> Option<StatusKind> {
    StatusKind::ALL.into_iter().find(|k| status_id(*k) == id)
}

/// The table as it stands.
pub fn snapshot(g: &GuiApp) -> Snapshot {
    let app = &g.app;
    let filter = app
        .filter_defs
        .get(app.active_filter.index)
        .map_or("all", |d| d.id);
    Snapshot {
        filters: Filters {
            filter: filter.to_string(),
            value: app.active_filter.query.clone(),
            focus: app
                .focus
                .as_ref()
                .filter(|f| !f.key.is_empty())
                .map(|f| f.key.clone()),
            status: app.status_pick.map(status_id),
            hide_merged: app.config.hide_merged,
        },
        search: app.input.clone(),
        order: Order {
            sort: sort_id(app.sort_column).to_string(),
            descending: app.sort_order == SortOrder::Descending,
            group: group_id(g.group_by).to_string(),
        },
        columns: app
            .visible_column_defs()
            .into_iter()
            .filter(|d| table::shows_column(app, d.id))
            .map(|d| d.id.to_string())
            .collect(),
    }
}

/// The label of a next-step focus, by its key.
fn focus_label(key: &str) -> Option<&'static str> {
    use crate::next_step::{Bucket, OTHERS_KEY};
    if key == Bucket::OnMe.key() {
        Some(tl("Waiting on me"))
    } else if key == OTHERS_KEY {
        Some(tl("Waiting on others"))
    } else if key == Bucket::ToDeploy.key() {
        Some(tl("To promote"))
    } else {
        None
    }
}

/// Sets the table as `view` keeps it, on the merge request page.
pub fn apply(g: &mut GuiApp, view: &SavedView) {
    if let Some(f) = &view.filters {
        let index = g
            .app
            .filter_defs
            .iter()
            .position(|d| d.id == f.filter)
            .unwrap_or(0);
        g.pending.push(Action::SetFilter {
            index,
            query: f.value.clone(),
        });
        match &f.focus {
            Some(key) => {
                let label = focus_label(key).map_or_else(|| view.name.clone(), str::to_string);
                g.focus_on(key, label);
            }
            None if g.app.focus.is_some() => g.next_focus = Some(None),
            None => {}
        }
        g.app
            .set_status_pick(f.status.as_deref().and_then(status_of));
        if f.hide_merged != g.app.config.hide_merged {
            g.pending.push(Action::ToggleHideMerged);
        }
    }
    if let Some(search) = &view.search {
        g.app.input = search.clone();
    }
    if let Some(order) = &view.order {
        if let Some(column) = sort_of(&order.sort) {
            g.app.sort_column = column;
        }
        g.app.sort_order = if order.descending {
            SortOrder::Descending
        } else {
            SortOrder::Ascending
        };
        g.app.sort_mrs();
        g.group_by = group_of(&order.group);
    }
    if let Some(columns) = &view.columns {
        for def in g.app.visible_column_defs() {
            let shown = columns.iter().any(|c| c == def.id);
            g.app.config.visible_columns.set_visible(def.id, shown);
        }
        g.app.project_settings.visible_columns = Some(g.app.config.visible_columns.clone());
        g.pending.push(Action::SaveVisibleColumns);
    }
    g.views.active = Some(view.name.clone());
    g.open_page(super::Page::MergeRequests);
}

/// The project's views.
fn views(g: &GuiApp) -> &[SavedView] {
    &g.app.project_settings.views
}

/// Replaces the project's views, and saves them (not in the demo).
fn save(g: &mut GuiApp, views: Vec<SavedView>) {
    g.app.project_settings.views = views.clone();
    if g.app.read_only {
        return;
    }
    let (url, id) = (g.app.base_url.clone(), g.app.project_id.clone());
    tokio::spawn(async move {
        if !crate::storage::save_views_async(&views, &url, &id).await {
            tracing::warn!("Could not save the views to projects.toml");
        }
    });
}

/// Applies the view under key `n` (1 to 9); whether there is one.
pub fn key(g: &mut GuiApp, n: usize) -> bool {
    let Some(view) = views(g).get(n.wrapping_sub(1)).cloned() else {
        return false;
    };
    apply(g, &view);
    true
}

/// The views, for the palette: (index, name).
pub fn listed(g: &GuiApp) -> Vec<(usize, String)> {
    views(g)
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v.name.clone()))
        .collect()
}

/// Applies the view at `index` (from the palette).
pub fn apply_at(g: &mut GuiApp, index: usize) {
    if let Some(view) = views(g).get(index).cloned() {
        apply(g, &view);
    }
}

// ── The sidebar ──────────────────────────────────────────────────────────────

/// The "My views" section: the project's views, the one applied lit (with
/// Save and Revert once the table left it), and the buttons to save one and
/// import others.
pub fn section(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let now = snapshot(g);
    let list = views(g).to_vec();
    // The view applied, while the table still is one of its kind.
    let active = g
        .views
        .active
        .as_ref()
        .and_then(|name| list.iter().position(|v| &v.name == name));
    ui.horizontal(|ui| {
        egui::Frame::new()
            .inner_margin(Margin {
                left: 6,
                right: 0,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| section_title(ui, &theme, tl("My views")));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let style = theme.icon_button();
            if super::widgets::icon_button_tip(
                ui,
                tl("Import views"),
                Icon::Download,
                &style,
                22.0,
                Tip::new().text(tl("Import views from another project")),
            )
            .clicked()
            {
                g.views.dialog = Some(import_dialog(g));
            }
            if super::widgets::icon_button_tip(
                ui,
                tl("Save as a view"),
                Icon::Plus,
                &style,
                22.0,
                Tip::new().text(tl("Save the table as a view")),
            )
            .clicked()
            {
                g.views.dialog = Some(Dialog::Save {
                    name: String::new(),
                    keep: Keep::default(),
                });
            }
        });
    });
    ui.add_space(4.0);
    if list.is_empty() {
        egui::Frame::new()
            .inner_margin(Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.label(
                    muted(
                        &theme,
                        tl("Set the filters, the search and the order, then + to keep them under a name."),
                    )
                    .size(theme.text_size - 1.5),
                );
            });
        return;
    }
    let mut picked = None;
    let mut delete = None;
    let mut update = None;
    for (i, view) in list.iter().enumerate() {
        let selected = active == Some(i);
        let modified = selected && view.differs(&now);
        let response = row(g, ui, i, view, selected, modified);
        if response.clicked() {
            picked = Some(i);
        }
        response.context_menu(|ui| {
            if ui.button(tl("Update with the table")).clicked() {
                update = Some(i);
                ui.close();
            }
            if ui.button(tl("Delete")).clicked() {
                delete = Some(i);
                ui.close();
            }
        });
        if modified {
            ui.horizontal(|ui| {
                ui.add_space(30.0);
                ui.spacing_mut().item_spacing.x = 6.0;
                if button(ui, &theme, None, tl("Save"), true)
                    .tip(tl("Keep the table as it is now in this view"))
                    .clicked()
                {
                    update = Some(i);
                }
                if button(ui, &theme, None, tc("saved view", "Restore"), false)
                    .tip(tl("Back to the view as it was saved"))
                    .clicked()
                {
                    picked = Some(i);
                }
            });
            ui.add_space(4.0);
        }
    }
    if let Some(i) = picked {
        apply(g, &list[i]);
    }
    if let Some(i) = update {
        let mut all = list.clone();
        all[i] = all[i].updated(&now);
        g.app
            .flash(tf("View {name} updated", &[("name", &all[i].name)]), false);
        save(g, all);
        g.views.active = Some(list[i].name.clone());
    }
    if let Some(i) = delete {
        let mut all = list.clone();
        let gone = all.remove(i);
        if g.views.active.as_ref() == Some(&gone.name) {
            g.views.active = None;
        }
        g.app
            .flash(tf("View {name} deleted", &[("name", &gone.name)]), false);
        save(g, all);
    }
}

/// A view in the list: its key, its name, "modified", and the MRs it lists
/// when applied.
fn row(
    g: &GuiApp,
    ui: &mut Ui,
    index: usize,
    view: &SavedView,
    selected: bool,
    modified: bool,
) -> egui::Response {
    let theme = g.theme;
    let p = &theme.palette;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let fill = if selected {
        p.selection
    } else if response.hovered() {
        p.raised
    } else {
        Color32::TRANSPARENT
    };
    let radius = CornerRadius::same(theme.radii.medium);
    ui.painter().rect_filled(rect, radius, fill);
    let y = rect.center().y;
    // The key, in a little box.
    if index < KEYS {
        let key =
            egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, y), Vec2::new(16.0, 16.0));
        ui.painter().rect_stroke(
            key,
            CornerRadius::same(3),
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            key.center(),
            egui::Align2::CENTER_CENTER,
            (index + 1).to_string(),
            egui::FontId::monospace(theme.text_size - 3.0),
            if selected { p.accent } else { p.text_muted },
        );
    }
    if selected {
        ui.painter().text(
            egui::pos2(rect.right() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            g.app.visible_mrs().count().to_string(),
            egui::FontId::proportional(theme.text_size - 1.5),
            p.text,
        );
    }
    let mut job = egui::text::LayoutJob::single_section(
        view.name.clone(),
        egui::TextFormat::simple(
            if selected {
                fonts::medium(theme.text_size)
            } else {
                egui::FontId::proportional(theme.text_size)
            },
            if selected { p.text } else { p.text_secondary },
        ),
    );
    if modified {
        job.append(
            &format!("  • {}", tl("modified")),
            0.0,
            egui::TextFormat::simple(
                egui::FontId::proportional(theme.text_size - 2.0),
                theme.tones.amber.fg,
            ),
        );
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 72.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left() + 32.0, y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    super::widgets::focus_ring(ui, &theme, &response, theme.radii.medium);
    let mut tip = Tip::new().title(&view.name).text(describe(g, view));
    if index < KEYS {
        tip = tip.hint(tf("Key {n}", &[("n", &(index + 1).to_string())]));
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(tip)
}

// ── Words for what a view keeps ──────────────────────────────────────────────

fn filters_text(g: &GuiApp, f: &Filters) -> String {
    let mut parts = Vec::new();
    if let Some(def) = g.app.filter_defs.iter().find(|d| d.id == f.filter) {
        if def.id != "all" {
            let label = def.active_label.unwrap_or(def.label);
            parts.push(if f.value.is_empty() {
                tl(label).to_string()
            } else {
                format!("{} {}", tl(label), f.value)
            });
        }
    }
    if let Some(label) = f.focus.as_deref().and_then(focus_label) {
        parts.push(label.to_string());
    }
    if let Some(kind) = f.status.as_deref().and_then(status_of) {
        parts.push(tl(kind.name()).to_string());
    }
    if f.hide_merged {
        parts.push(tl("merged MRs hidden").to_string());
    }
    if parts.is_empty() {
        tl("no filter").to_string()
    } else {
        parts.join(", ")
    }
}

fn order_text(o: &Order) -> String {
    let sort = match o.sort.as_str() {
        "id" => tl("MR number"),
        "milestone" => tl("Milestone"),
        "title" => tl("Title"),
        _ => tl("Updated"),
    };
    let arrow = if o.descending { "↓" } else { "↑" };
    match group_of(&o.group) {
        GroupBy::None => format!("{sort} {arrow}"),
        group => format!("{sort} {arrow}, {}", group.label().to_lowercase()),
    }
}

/// What a view keeps, in one line per part.
fn describe(g: &GuiApp, view: &SavedView) -> String {
    let mut lines = Vec::new();
    if let Some(f) = &view.filters {
        lines.push(format!("{}: {}", tl("Filters"), filters_text(g, f)));
    }
    if let Some(s) = &view.search {
        lines.push(format!(
            "{}: {}",
            tl("Search"),
            if s.is_empty() { tl("empty") } else { s }
        ));
    }
    if let Some(o) = &view.order {
        lines.push(format!("{}: {}", tl("Sort and grouping"), order_text(o)));
    }
    if let Some(c) = &view.columns {
        lines.push(tn(c.len() as u64, "{n} column", "{n} columns"));
    }
    lines.join("\n")
}

// ── The dialogs ──────────────────────────────────────────────────────────────

/// The other projects' views, read from `projects.toml` (the demo, which
/// writes nothing: from its other tabs).
fn import_dialog(g: &GuiApp) -> Dialog {
    let projects: Vec<(String, Vec<SavedView>)> = if g.app.read_only {
        g.tabs
            .iter()
            .filter_map(|slot| match &slot.state {
                super::SlotState::Parked(tab) => Some((
                    tab.app
                        .project_name
                        .clone()
                        .unwrap_or_else(|| tab.app.project_id.clone()),
                    tab.app.project_settings.views.clone(),
                )),
                _ => None,
            })
            .collect()
    } else {
        super::block_on(crate::storage::load_projects_toml())
            .projects
            .into_iter()
            .filter(|p| {
                !(p.gitlab_url.trim_end_matches('/') == g.app.base_url.trim_end_matches('/')
                    && p.project_id == g.app.project_id)
            })
            .map(|p| (p.name.unwrap_or(p.project_id), p.views))
            .collect()
    };
    let views = projects
        .into_iter()
        .flat_map(|(name, views)| views.into_iter().map(move |v| (name.clone(), v, false)))
        .collect();
    Dialog::Import { views }
}

pub fn dialog(g: &mut GuiApp, ctx: &egui::Context) {
    let Some(mut dialog) = g.views.dialog.take() else {
        return;
    };
    let theme = g.theme;
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(18));
    let mut close = false;
    let response = egui::Modal::new(egui::Id::unique("saved_view_dialog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(420.0);
            close = match &mut dialog {
                Dialog::Save { name, keep } => save_dialog(g, ui, &theme, name, keep),
                Dialog::Import { views } => import_body(g, ui, &theme, views),
            };
        });
    if !close && !response.should_close() {
        g.views.dialog = Some(dialog);
    }
}

/// A part a view may keep: a check with what it holds now.
fn part(ui: &mut Ui, theme: &Theme, on: &mut bool, label: &str, now: &str) {
    let color = if *on {
        theme.palette.accent
    } else {
        theme.palette.text_muted
    };
    if super::board::nav_check(ui, theme, label, "", color, *on).clicked() {
        *on = !*on;
    }
    egui::Frame::new()
        .inner_margin(Margin {
            left: 28,
            right: 0,
            top: 0,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(muted(theme, now).size(theme.text_size - 1.5));
        });
}

/// Whether the dialog is done.
fn save_dialog(
    g: &mut GuiApp,
    ui: &mut Ui,
    theme: &Theme,
    name: &mut String,
    keep: &mut Keep,
) -> bool {
    let now = snapshot(g);
    ui.label(RichText::new(tl("Save as a view")).font(fonts::semibold(theme.text_size + 1.0)));
    ui.label(
        muted(theme, tl("Kept with the project in projects.toml.")).size(theme.text_size - 1.5),
    );
    ui.add_space(12.0);
    ui.label(muted(theme, tl("Name")).size(theme.text_size - 1.0));
    let response = field(
        ui,
        theme,
        name,
        "saved_view_name",
        FieldLook {
            hint: tl("e.g. Payments, not in production"),
            icon: None,
            width: ui.available_width(),
            active: true,
        },
    );
    if !response.has_focus() && name.is_empty() {
        response.request_focus();
    }
    let replaces = views(g)
        .iter()
        .any(|v| v.name.eq_ignore_ascii_case(name.trim()));
    if replaces {
        ui.label(
            RichText::new(tl("A view of that name exists: saving replaces it."))
                .size(theme.text_size - 1.5)
                .color(theme.tones.amber.fg),
        );
    }
    ui.add_space(12.0);
    ui.label(muted(theme, tl("What it keeps")).size(theme.text_size - 1.0));
    ui.add_space(4.0);
    part(
        ui,
        theme,
        &mut keep.filters,
        tl("Filters"),
        &filters_text(g, &now.filters),
    );
    let search = if now.search.is_empty() {
        tl("empty: it clears the search").to_string()
    } else {
        now.search.clone()
    };
    part(ui, theme, &mut keep.search, tl("Search"), &search);
    part(
        ui,
        theme,
        &mut keep.order,
        tl("Sort and grouping"),
        &order_text(&now.order),
    );
    part(
        ui,
        theme,
        &mut keep.columns,
        tl("Columns"),
        &tn(
            now.columns.len() as u64,
            "{n} column shown",
            "{n} columns shown",
        ),
    );
    ui.add_space(12.0);
    let index = views(g)
        .iter()
        .position(|v| v.name.eq_ignore_ascii_case(name.trim()))
        .unwrap_or(views(g).len());
    let mut done = false;
    ui.horizontal(|ui| {
        if index < KEYS {
            ui.label(
                muted(
                    theme,
                    tf(
                        "Key {n}, and in the palette (Ctrl K)",
                        &[("n", &(index + 1).to_string())],
                    ),
                )
                .size(theme.text_size - 1.5),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !name.trim().is_empty()
                && *keep
                    != Keep {
                        filters: false,
                        search: false,
                        order: false,
                        columns: false,
                    };
            let enter = ready && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (primary_button(ui, theme, tl("Save the view")).clicked() && ready) || enter {
                let view = SavedView::of(name, &now, *keep);
                let mut all = views(g).to_vec();
                match all
                    .iter()
                    .position(|v| v.name.eq_ignore_ascii_case(&view.name))
                {
                    Some(i) => all[i] = view.clone(),
                    None => all.push(view.clone()),
                }
                g.views.active = Some(view.name.clone());
                g.app
                    .flash(tf("View {name} saved", &[("name", &view.name)]), false);
                save(g, all);
                done = true;
            }
            if button(ui, theme, None, tl("Cancel"), false).clicked() {
                done = true;
            }
        });
    });
    done
}

/// Whether the dialog is done.
fn import_body(
    g: &mut GuiApp,
    ui: &mut Ui,
    theme: &Theme,
    views_found: &mut [(String, SavedView, bool)],
) -> bool {
    ui.label(RichText::new(tl("Import views")).font(fonts::semibold(theme.text_size + 1.0)));
    ui.label(
        muted(
            theme,
            tl("From the other projects of projects.toml: copied into this one."),
        )
        .size(theme.text_size - 1.5),
    );
    ui.add_space(12.0);
    if views_found.is_empty() {
        ui.label(muted(theme, tl("No other project has saved views yet.")));
    }
    let mut last_project = None;
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for (project, view, on) in views_found.iter_mut() {
                if last_project.as_deref() != Some(project.as_str()) {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(project.as_str()).font(fonts::medium(theme.text_size - 0.5)),
                    );
                    last_project = Some(project.clone());
                }
                let color = if *on {
                    theme.palette.accent
                } else {
                    theme.palette.text_muted
                };
                if super::board::nav_check(ui, theme, &view.name, "", color, *on)
                    .tip(describe(g, view))
                    .clicked()
                {
                    *on = !*on;
                }
            }
        });
    ui.add_space(12.0);
    let picked: Vec<SavedView> = views_found
        .iter()
        .filter(|(_, _, on)| *on)
        .map(|(_, v, _)| v.clone())
        .collect();
    let mut done = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = tn(picked.len() as u64, "Import {n} view", "Import {n} views");
            if primary_button(ui, theme, &label).clicked() && !picked.is_empty() {
                let mut all = views(g).to_vec();
                let (added, missing) =
                    crate::saved_views::import(&mut all, &picked, &g.app.branches);
                let mut text = tn(added as u64, "{n} view imported", "{n} views imported");
                if !missing.is_empty() {
                    text.push_str(" · ");
                    text.push_str(&tf(
                        "not tracked here: {list}",
                        &[("list", &missing.join(", "))],
                    ));
                }
                g.app.flash(text, !missing.is_empty());
                save(g, all);
                done = true;
            }
            if button(ui, theme, None, tl("Cancel"), false).clicked() {
                done = true;
            }
        });
    });
    done
}
