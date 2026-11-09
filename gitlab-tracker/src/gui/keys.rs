//! Keyboard shortcuts: the TUI's keys, wherever they make sense in a window.
//! They only act when no text field has the keyboard.

use egui::{Event, Key, Modifiers};

use super::{GuiApp, InspectorSection, Page};
use crate::actions::Action;
use crate::app::InputMode;

pub fn handle(g: &mut GuiApp, ctx: &egui::Context) {
    // The palette's shortcut works everywhere, even from a text field.
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K)) {
        super::palette::toggle(g);
        return;
    }
    if tab_keys(g, ctx) {
        return;
    }
    let typing = ctx.memory(|m| m.focused().is_some());
    if g.palette.is_none() && g.wizard.is_none() {
        super::history::handle(g, ctx, typing);
    }
    if g.palette.is_some() || typing {
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

/// The tabs' keys, as in a browser: Ctrl+Tab and Ctrl+Shift+Tab (or
/// Ctrl+PageDown / PageUp) cycle, Ctrl+1…9 pick, Ctrl+W closes. `true` when one
/// was used.
fn tab_keys(g: &mut GuiApp, ctx: &egui::Context) -> bool {
    let count = g.tabs.len();
    if count == 0 || g.wizard.is_some() {
        return false;
    }
    let active = g.active();
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let target = ctx.input_mut(|i| {
        if i.consume_key(ctrl_shift, Key::Tab) || i.consume_key(Modifiers::COMMAND, Key::PageUp) {
            return Some((active + count - 1) % count);
        }
        if i.consume_key(Modifiers::COMMAND, Key::Tab)
            || i.consume_key(Modifiers::COMMAND, Key::PageDown)
        {
            return Some((active + 1) % count);
        }
        const DIGITS: [Key; 9] = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (n, key) in DIGITS.iter().enumerate() {
            if i.consume_key(Modifiers::COMMAND, *key) {
                // Ctrl+9 is the last tab, as in browsers.
                return Some(if n == 8 { count - 1 } else { n });
            }
        }
        None
    });
    if let Some(index) = target.filter(|&i| i < count) {
        g.activate(index);
        return true;
    }
    if !g.demo && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::W)) {
        g.close_tab(active);
        return true;
    }
    false
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
    let on_mr = g.page() == Page::Mr;
    let on_person = g.page() == Page::Person;
    match key {
        // Enter opens the page of the person shown, Esc goes back to the team,
        // the arrows move to the next or previous person.
        Key::Enter if g.page() == Page::Team => g.team.open = true,
        Key::Escape if on_person => g.open_page(Page::Team),
        Key::ArrowDown if on_person => g.team.step += 1,
        Key::ArrowUp if on_person => g.team.step -= 1,
        // Enter opens the selected MR's page, Esc goes back to the list.
        Key::Enter
            if !on_mr && g.page() == Page::MergeRequests && g.app.selected_mr().is_some() =>
        {
            g.open_page(Page::Mr)
        }
        Key::Escape if on_mr => g.open_page(Page::MergeRequests),
        Key::ArrowDown => g.app.next_row(),
        Key::ArrowUp => g.app.prev_row(),
        Key::Delete => g.pending.push(Action::RemoveSelected),
        Key::Escape => {
            g.app.input.clear();
            g.app.milestone_suggestions.clear();
            g.marked.clear();
            g.bulk_mode = false;
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
    if g.page() == Page::Person && matches!(c, 'j' | 'k') {
        g.team.step += if c == 'j' { 1 } else { -1 };
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
            toggle_views(g);
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

/// Shows or hides the views panel, like its toolbar button. The panel lives on the
/// MR page only, so elsewhere the key opens that page with the panel shown.
fn toggle_views(g: &mut GuiApp) {
    if g.page() != Page::MergeRequests {
        g.open_page(Page::MergeRequests);
        g.show_views = Some(true);
    } else {
        g.show_views = Some(!g.views_shown);
    }
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
