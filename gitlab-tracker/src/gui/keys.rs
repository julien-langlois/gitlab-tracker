//! Keyboard shortcuts: the TUI's keys, wherever they make sense in a window.
//! They only act when no text field has the keyboard.

use egui::{Event, Key, Modifiers};

use super::{GuiApp, InspectorSection, Page};
use crate::actions::Action;
use crate::app::InputMode;

pub fn handle(g: &mut GuiApp, ctx: &egui::Context) {
    if ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for event in events {
        match event {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => named_key(g, key, modifiers),
            // Characters, so shortcuts follow the keyboard layout (`?` on AZERTY).
            Event::Text(text) => {
                for c in text.chars() {
                    character(g, c);
                }
            }
            _ => {}
        }
    }
}

fn named_key(g: &mut GuiApp, key: Key, modifiers: Modifiers) {
    #[cfg(feature = "stats")]
    if g.on_stats() {
        stats_key(g, key, modifiers);
        return;
    }
    if g.app.input_mode != InputMode::Normal {
        return;
    }
    match key {
        Key::ArrowDown => g.app.next_row(),
        Key::ArrowUp => g.app.prev_row(),
        Key::Delete => g.pending.push(Action::RemoveSelected),
        Key::Escape => {
            g.app.input.clear();
            g.app.milestone_suggestions.clear();
        }
        // Space arrives as a key, not as text, when nothing has focus.
        Key::Space if modifiers.is_none() => g.pending.push(Action::ToggleFlagSelected),
        _ => {}
    }
}

pub(super) fn character(g: &mut GuiApp, c: char) {
    #[cfg(feature = "stats")]
    if g.on_stats() {
        stats_character(g, c);
        return;
    }
    if g.app.input_mode != InputMode::Normal {
        return;
    }
    let action = match c {
        'j' => {
            g.app.next_row();
            return;
        }
        'k' => {
            g.app.prev_row();
            return;
        }
        'o' | 'O' => Action::OpenSelectedMr,
        'y' | 'Y' => Action::YankCloneCommand,
        'e' | 'E' => Action::CopyChecklist,
        'r' | 'R' => Action::Refresh,
        'l' | 'L' => Action::OpenLogTime,
        #[cfg(feature = "stats")]
        'g' | 'G' => Action::OpenStats,
        's' => Action::CycleSortColumn,
        'S' => Action::ToggleSortOrder,
        'm' | 'M' => Action::ToggleHideMerged,
        'w' | 'W' => Action::OpenProjectPicker,
        ',' => Action::OpenSettings,
        '/' | 'i' | 'I' => {
            g.focus_search = true;
            return;
        }
        'f' | 'F' => {
            g.show_views = Some(true);
            return;
        }
        'c' | 'C' => {
            g.app.input_mode = InputMode::ColumnPicker;
            return;
        }
        '?' => {
            g.app.input_mode = InputMode::Help;
            return;
        }
        'p' | 'P' => {
            show_section(g, InspectorSection::Pipelines);
            return;
        }
        't' => {
            show_section(g, InspectorSection::Ticket);
            return;
        }
        'T' => Action::OpenSelectedTicket,
        _ => return,
    };
    g.pending.push(action);
}

/// Shows the inspector and scrolls it to `section`.
fn show_section(g: &mut GuiApp, section: InspectorSection) {
    if g.page() == Page::Cockpit {
        g.open_page(Page::MergeRequests);
    }
    g.show_inspector = Some(true);
    g.scroll_to = Some(section);
}

#[cfg(feature = "stats")]
fn stats_key(g: &mut GuiApp, key: Key, modifiers: Modifiers) {
    let view = &mut g.app.stats_view;
    match key {
        Key::Escape => g.app.input_mode = InputMode::Normal,
        Key::Tab if modifiers.shift => view.tab = view.tab.previous(),
        Key::Tab => view.tab = view.tab.next(),
        _ => {}
    }
}

#[cfg(feature = "stats")]
fn stats_character(g: &mut GuiApp, c: char) {
    match c {
        'g' | 'G' => g.app.input_mode = InputMode::Normal,
        'w' | 'W' => g.pending.push(Action::CycleStatsWindow),
        'r' | 'R' => g.pending.push(Action::RefreshStats),
        c => {
            if let Some(tab) = crate::app::StatsTab::from_digit(c) {
                g.app.stats_view.tab = tab;
            }
        }
    }
}
