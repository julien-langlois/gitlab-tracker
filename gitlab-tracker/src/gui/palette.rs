//! The command palette (Ctrl+K): one field to reach a page, a view, a command or
//! any tracked MR by typing part of its name.

use bevel_theme::{fonts, Theme};
use egui::{CornerRadius, Key, Margin, Modifiers, Sense, Stroke, Vec2};

use super::icons::{self, Icon};
use super::widgets::{field, muted, FieldLook};
use super::{GuiApp, Page};
use crate::actions::Action;
use crate::app::InputMode;
use crate::i18n::tl;

/// Results shown at once.
const SHOWN: usize = 9;

/// The palette while open: what is typed, and the highlighted result.
#[derive(Default)]
pub struct Palette {
    query: String,
    cursor: usize,
    /// The field took the keyboard (on the first frame).
    focused: bool,
}

impl Palette {
    /// The palette with `query` typed (the development tour).
    #[cfg(debug_assertions)]
    pub fn with_query(query: &str) -> Self {
        Self {
            query: query.into(),
            ..Self::default()
        }
    }
}

/// What a result does.
#[derive(Clone)]
enum Command {
    Page(Page),
    View(usize),
    Mr(String),
    Refresh,
    CopyChecklist,
    Settings,
    Shortcuts,
    Columns,
    Theme,
    HideMerged,
    ClearFocus,
    Tab(usize),
    NewProject,
}

/// A result: its kind, its text, a detail on the right, and what it does.
struct Entry {
    kind: &'static str,
    icon: Icon,
    text: String,
    detail: String,
    command: Command,
}

fn entries(g: &GuiApp) -> Vec<Entry> {
    let mut list = Vec::new();
    let command = |text: &str, detail: &str, icon: Icon, command: Command| Entry {
        kind: tl("Command"),
        icon,
        text: text.to_string(),
        detail: detail.to_string(),
        command,
    };
    list.push(command(
        tl("Refresh now"),
        "R",
        Icon::Refresh,
        Command::Refresh,
    ));
    list.push(command(
        tl("Copy the release checklist"),
        "E",
        Icon::Clipboard,
        Command::CopyChecklist,
    ));
    list.push(command(
        tl("Columns…"),
        "C",
        Icon::Columns,
        Command::Columns,
    ));
    list.push(command(
        tl(if g.app.config.hide_merged {
            "Show merged MRs"
        } else {
            "Hide merged MRs"
        }),
        "M",
        Icon::MergeRequests,
        Command::HideMerged,
    ));
    list.push(command(
        tl("Switch light / dark theme"),
        "",
        Icon::Sun,
        Command::Theme,
    ));
    list.push(command(
        tl("Settings…"),
        ",",
        Icon::Settings,
        Command::Settings,
    ));
    list.push(command(
        tl("Keyboard shortcuts"),
        "?",
        Icon::Help,
        Command::Shortcuts,
    ));
    if g.app.selected_mr().is_some() && g.page() != Page::Mr {
        list.push(command(
            tl("Open the selected MR's page"),
            "Enter",
            Icon::Maximize,
            Command::Page(Page::Mr),
        ));
    }
    if !g.app.read_only {
        list.push(command(
            tl("Add a project…"),
            "W",
            Icon::Plus,
            Command::NewProject,
        ));
    }
    for (index, tab) in g.tabs.iter().enumerate() {
        if matches!(tab.state, super::SlotState::Active) {
            continue;
        }
        let name = match &tab.state {
            super::SlotState::Parked(t) => t
                .app
                .project_name
                .clone()
                .unwrap_or_else(|| t.app.project_id.clone()),
            _ => tab.name.clone(),
        };
        list.push(Entry {
            kind: tl("Project"),
            icon: Icon::MergeRequests,
            text: name,
            detail: if index < 8 {
                format!("Ctrl+{}", index + 1)
            } else {
                String::new()
            },
            command: Command::Tab(index),
        });
    }
    if let Some(focus) = &g.app.focus {
        list.push(command(
            &format!("{} · {}", tl("Show every MR again"), focus.label),
            "",
            Icon::Close,
            Command::ClearFocus,
        ));
    }

    let mut pages = vec![
        (Page::Today, tl("Today"), Icon::Clock),
        (Page::Promote, tl("Promote"), Icon::ArrowUp),
        (
            Page::MergeRequests,
            tl("Merge requests"),
            Icon::MergeRequests,
        ),
        (Page::Cockpit, tl("Cockpit"), Icon::Cockpit),
    ];
    if g.app.tracker.is_some() {
        pages.push((Page::Tracker, tl("Linked tickets"), Icon::Ticket));
    }
    #[cfg(feature = "stats")]
    pages.push((Page::Stats, tl("Statistics"), Icon::Stats));
    for (page, name, icon) in pages {
        list.push(Entry {
            kind: tl("Page"),
            icon,
            text: name.to_string(),
            detail: String::new(),
            command: Command::Page(page),
        });
    }

    for (index, def) in g.app.filter_defs.iter().enumerate() {
        if def.needs_text_input {
            continue;
        }
        list.push(Entry {
            kind: tl("View"),
            icon: Icon::Columns,
            text: tl(def.active_label()).to_string(),
            detail: g.app.filter_count(index).to_string(),
            command: Command::View(index),
        });
    }

    for mr in &g.app.mrs {
        list.push(Entry {
            kind: "MR",
            icon: Icon::MergeRequests,
            text: format!("!{} {}", mr.id, mr.title),
            detail: super::widgets::split_person(&mr.author).0.to_string(),
            command: Command::Mr(mr.id.clone()),
        });
    }
    list
}

/// The entries matching `query`, best first (all of them, in order, without one).
fn ranked(list: Vec<Entry>, query: &str) -> Vec<Entry> {
    let query = query.trim();
    if query.is_empty() {
        return list;
    }
    let mut scored: Vec<(f64, Entry)> = list
        .into_iter()
        .filter_map(|e| {
            let score = crate::utils::fuzzy_score(query, &e.text)
                .or_else(|| crate::utils::fuzzy_score(query, &e.detail).map(|s| s - 1.0))?;
            Some((score, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, e)| e).collect()
}

pub fn show(g: &mut GuiApp, ctx: &egui::Context) {
    let Some(mut palette) = g.palette.take() else {
        return;
    };
    let theme = g.theme;
    let results = ranked(entries(g), &palette.query);
    let (down, up, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::Enter),
        )
    });
    let shown = results.len().min(SHOWN);
    if down && shown > 0 {
        palette.cursor = (palette.cursor + 1) % shown;
    }
    if up && shown > 0 {
        palette.cursor = (palette.cursor + shown - 1) % shown;
    }
    palette.cursor = palette.cursor.min(shown.saturating_sub(1));
    let mut run = enter
        .then(|| results.get(palette.cursor).map(|e| e.command.clone()))
        .flatten();

    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(12));
    let modal = egui::Modal::new(egui::Id::unique("command_palette"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            let response = field(
                ui,
                &theme,
                &mut palette.query,
                "palette",
                FieldLook {
                    hint: tl("Go to a page, a view, an MR… or run a command"),
                    icon: Some(Icon::Search),
                    width: 560.0,
                    active: true,
                },
            );
            if !palette.focused {
                response.request_focus();
                palette.focused = true;
            }
            if response.changed() {
                palette.cursor = 0;
            }
            ui.add_space(8.0);
            if results.is_empty() {
                ui.add_space(8.0);
                ui.label(muted(&theme, tl("Nothing matches.")));
                ui.add_space(8.0);
            }
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, entry) in results.iter().take(SHOWN).enumerate() {
                if row(ui, &theme, entry, i == palette.cursor).clicked() {
                    run = Some(entry.command.clone());
                }
            }
            ui.add_space(6.0);
            ui.label(
                muted(&theme, tl("↑ ↓ to move · Enter to run · Esc to close"))
                    .size(theme.text_size - 2.0),
            );
        });
    if modal.should_close() {
        return;
    }
    match run {
        Some(command) => execute(g, command),
        None => g.palette = Some(palette),
    }
}

fn row(ui: &mut egui::Ui, theme: &Theme, entry: &Entry, highlighted: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
    let fill = if highlighted {
        theme.palette.selection
    } else if response.hovered() {
        theme.palette.border_soft
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    let painter = ui.painter().with_clip_rect(rect);
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 18.0, rect.center().y),
        Vec2::splat(14.0),
    );
    let tint = if highlighted {
        theme.palette.accent
    } else {
        theme.palette.text_muted
    };
    icons::paint(&painter, icon, entry.icon, tint);
    let kind = painter.layout_no_wrap(
        entry.kind.to_string(),
        fonts::medium(theme.text_size - 2.5),
        theme.palette.text_muted,
    );
    let detail = painter.layout_no_wrap(
        entry.detail.clone(),
        egui::FontId::monospace(theme.text_size - 2.0),
        theme.palette.text_muted,
    );
    let right = rect.right() - 12.0;
    painter.galley(
        egui::pos2(
            right - detail.size().x,
            rect.center().y - detail.size().y / 2.0,
        ),
        detail.clone(),
        theme.palette.text_muted,
    );
    let text_left = rect.left() + 36.0;
    let text_room = right - detail.size().x - 16.0 - text_left - kind.size().x - 12.0;
    let mut job = egui::text::LayoutJob::single_section(
        entry.text.clone(),
        egui::TextFormat::simple(fonts::medium(theme.text_size), theme.palette.text),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(text_room.max(40.0));
    let text = painter.layout_job(job);
    painter.galley(
        egui::pos2(text_left, rect.center().y - text.size().y / 2.0),
        text.clone(),
        theme.palette.text,
    );
    painter.galley(
        egui::pos2(
            text_left + text.size().x + 10.0,
            rect.center().y - kind.size().y / 2.0,
        ),
        kind,
        theme.palette.text_muted,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn execute(g: &mut GuiApp, command: Command) {
    match command {
        Command::Page(page) => g.open_page(page),
        Command::View(index) => {
            g.next_focus = g.app.focus.is_some().then_some(None);
            g.pending.push(Action::SetFilter {
                index,
                query: String::new(),
            });
            g.open_page(Page::MergeRequests);
        }
        Command::Mr(id) => {
            // Shown whatever the view: back to every MR when this one is hidden.
            let hidden = !g.app.visible_mrs().any(|mr| mr.id == id);
            if hidden {
                g.pending.push(Action::SetFilter {
                    index: 0,
                    query: String::new(),
                });
                g.app.input.clear();
            }
            g.show_mr(&id);
        }
        Command::Refresh => g.pending.push(Action::Refresh),
        Command::CopyChecklist => g.pending.push(Action::CopyChecklist),
        Command::Settings => g.pending.push(Action::OpenSettings),
        Command::Shortcuts => g.app.input_mode = InputMode::Help,
        Command::Columns => g.app.input_mode = InputMode::ColumnPicker,
        Command::HideMerged => g.pending.push(Action::ToggleHideMerged),
        Command::ClearFocus => g.next_focus = Some(None),
        Command::Tab(index) => g.activate(index),
        Command::NewProject => {
            g.wizard = Some(super::setup::ProjectForm::new(
                &g.ctx,
                super::setup::Mode::New,
            ));
        }
        Command::Theme => {
            let next = match g.theme.mode {
                bevel_theme::Mode::Dark => bevel_theme::Mode::Light,
                bevel_theme::Mode::Light => bevel_theme::Mode::Dark,
            };
            g.set_theme(next);
        }
    }
}

/// Opens the palette (or closes it when open).
pub fn toggle(g: &mut GuiApp) {
    g.palette = match g.palette {
        Some(_) => None,
        None => Some(Palette::default()),
    };
}
