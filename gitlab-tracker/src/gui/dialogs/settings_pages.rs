//! Settings › Pages and › Startup: the persona and the rail's pages, the page
//! the window opens on. Both are the window's, for every project, edited as a
//! draft that Save applies (see [`GuiApp::save_rail`]).

use super::*;
use crate::gui::persona::{
    choice_chip, emphasised, page_label, page_switch, pick_pill, radio_card, Persona, Start, PAGES,
    TRACKER,
};

/// The rail's pages this build has (no statistics page without them).
fn rail_pages() -> impl Iterator<Item = &'static str> {
    PAGES
        .iter()
        .copied()
        .filter(|k| cfg!(feature = "stats") || *k != "stats")
}

/// Settings › Pages: the persona, what was changed on top of it, a switch per page.
pub(super) fn pages_page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let p = theme.palette;
    ui.label(semibold(tl("Persona"), theme.text_size - 1.0).color(p.text_secondary));
    ui.add_space(8.0);
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for persona in Persona::ALL {
            if choice_chip(ui, &theme, persona.label(), g.rail_draft.persona == persona).clicked() {
                picked = Some(persona);
            }
        }
    });
    if let Some(persona) = picked {
        g.rail_draft.set_persona(persona);
    }
    if g.rail_draft.customised() {
        ui.add_space(12.0);
        customised(g, ui);
    }
    ui.add_space(16.0);
    let keys: Vec<&str> = rail_pages().collect();
    let gap = 18.0;
    let column = (ui.available_width() - gap) / 2.0;
    let mut toggled = None;
    for pair in keys.chunks(2) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for key in pair {
                ui.allocate_ui_with_layout(
                    Vec2::new(column, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        if page_switch(ui, &theme, &g.rail_draft, key).clicked() {
                            toggled = Some(*key);
                        }
                    },
                );
            }
        });
        ui.add_space(4.0);
    }
    if let Some(key) = toggled {
        g.rail_draft.toggle(key);
    }
}

/// The banner over the switches once pages were shown or hidden on top of
/// the persona: what changed, and going back to the persona (confirmed).
fn customised(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let tone = theme.tones.amber;
    let name = g.rail_draft.persona.label();
    let mut changes = Vec::new();
    if !g.rail_draft.added.is_empty() {
        let added: Vec<&str> = g.rail_draft.added.iter().map(|k| page_label(k)).collect();
        changes.push(format!("+ {}", added.join(", ")));
    }
    if !g.rail_draft.removed.is_empty() {
        let removed: Vec<&str> = g.rail_draft.removed.iter().map(|k| page_label(k)).collect();
        changes.push(format!("− {}", removed.join(", ")));
    }
    let mut reset = false;
    egui::Frame::new()
        .fill(tone.bg)
        .stroke(Stroke::new(1.0, tone.fg.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(
                    semibold(tf("{persona} (customised)", &[("persona", name)]), theme.text_size - 0.5)
                        .color(tone.fg),
                );
                ui.label(
                    RichText::new(changes.join(" · "))
                        .size(theme.text_size - 1.0)
                        .color(theme.palette.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = tf("Reset to {persona}…", &[("persona", name)]);
                    let response = button(ui, &theme, None, &label, false);
                    egui::Popup::menu(&response).show(|ui| {
                        ui.set_max_width(300.0);
                        ui.label(tf(
                            "Show the pages of {persona} again? The pages you showed or hid are dropped.",
                            &[("persona", name)],
                        ));
                        ui.add_space(6.0);
                        if button(ui, &theme, None, tl("Reset"), true).clicked() {
                            reset = true;
                        }
                    });
                });
            });
        });
    if reset {
        let persona = g.rail_draft.persona;
        g.rail_draft.set_persona(persona);
    }
}

/// Settings › Startup: the persona's page, always one page, or where each tab was.
pub(super) fn startup_page(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let p = theme.palette;
    let size = theme.text_size - 1.0;
    let persona = g.rail_draft.persona;
    let opens = page_label(persona.start());
    let mut start = None;
    ui.spacing_mut().item_spacing.y = 8.0;

    let text = tf(
        "{persona} opens on {page}. Follows the persona if it changes.",
        &[("persona", persona.label()), ("page", opens)],
    );
    let on = g.rail_draft.start == Start::Persona;
    if radio_card(
        ui,
        &theme,
        tl("From my persona"),
        emphasised(&theme, &text, opens, size, p.text_secondary),
        on,
    )
    .clicked()
    {
        start = Some(Start::Persona);
    }

    let fixed = match &g.rail_draft.start {
        Start::Page(page) => Some(page.clone()),
        _ => None,
    };
    let text = tl("One page, whatever the persona: the pages of your rail.");
    if radio_card(
        ui,
        &theme,
        tl("Always this page"),
        emphasised(&theme, text, "", size, p.text_secondary),
        fixed.is_some(),
    )
    .clicked()
        && fixed.is_none()
    {
        // The page it opens on now, to start from.
        let now = g
            .rail_draft
            .opening_page(Some(super::super::prefs::page_name(g.page())));
        start = Some(Start::Page(now.to_string()));
    }
    if let Some(current) = &fixed {
        // Under the option's title, every line of pills alike.
        egui::Frame::new()
            .inner_margin(Margin {
                left: 46,
                right: 0,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    let linked = g.app.tracker.is_some();
                    for key in
                        rail_pages().filter(|k| g.rail_draft.shows(k) && (*k != TRACKER || linked))
                    {
                        if pick_pill(ui, &theme, page_label(key), key == current).clicked() {
                            start = Some(Start::Page(key.to_string()));
                        }
                    }
                });
            });
        ui.add_space(4.0);
    }

    let text = tl("Each project's tab opens on the page it showed when the window closed.");
    if radio_card(
        ui,
        &theme,
        tl("Where I left off"),
        emphasised(&theme, text, "", size, p.text_secondary),
        g.rail_draft.start == Start::LeftOff,
    )
    .clicked()
    {
        start = Some(Start::LeftOff);
    }
    ui.add_space(6.0);
    ui.label(
        muted(
            &theme,
            tl("The start page cannot be hidden from the rail: its switch in Pages is greyed while it is the start page. With “Where I left off”, a page hidden since opens on the persona's start page."),
        )
        .size(theme.text_size - 1.5),
    );
    if let Some(start) = start {
        g.rail_draft.start = start;
    }
}
