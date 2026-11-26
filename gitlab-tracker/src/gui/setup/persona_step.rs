//! The first launch's second step, once the project is saved: who are you?
//! The persona picked sets the rail's pages and the start page; Settings ›
//! Pages and › Startup change them later.

use super::*;
use crate::gui::persona::{persona_card, persona_card_height, rail_preview, Persona, Rail, Start};

/// The window's size for this step: five cards side by side.
pub const PERSONA_STEP_SIZE: [f32; 2] = [980.0, 580.0];

/// What the step's buttons asked for.
pub enum StepOutcome {
    Stay,
    /// Back to the project, saved already.
    Back,
    /// The rail of this persona, and the window.
    Open,
}

/// The rail and start page `persona` gives: its pages, opening on its page.
pub fn rail_for(persona: Persona) -> Rail {
    Rail {
        persona,
        start: Start::Persona,
        ..Rail::default()
    }
}

/// The step on the whole window: its title, the cards, the rail's preview,
/// then its footer.
pub fn persona_step(ui: &mut Ui, theme: &Theme, picked: &mut Persona) -> StepOutcome {
    let mut outcome = StepOutcome::Stay;
    egui::Panel::bottom("persona_footer")
        .frame(egui::Frame::new().fill(theme.palette.raised))
        .show(ui, |ui| {
            super::super::dialogs::footer(ui, theme, |ui| {
                ui.label(muted(theme, tl("For every project of this machine.")));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if button(ui, theme, None, tl("Open the window"), true).clicked() {
                        outcome = StepOutcome::Open;
                    }
                    if button(ui, theme, None, tl("Back"), false).clicked() {
                        outcome = StepOutcome::Back;
                    }
                });
            });
        });
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme.palette.raised)
                .inner_margin(Margin {
                    left: 28,
                    right: 28,
                    top: 24,
                    bottom: 16,
                }),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                semibold(tl("Step 2 of 2 · Your window").to_uppercase(), theme.text_size - 3.0)
                    .color(theme.palette.text_muted),
            );
            ui.label(semibold(tl("Who are you?"), theme.text_size + 7.0));
            ui.label(
                RichText::new(tl(
                    "The left rail shows the pages for your role. Every page stays one click away, and you can change this any time in Settings › Pages.",
                ))
                .size(theme.text_size - 0.5)
                .color(theme.palette.text_secondary),
            );
            ui.add_space(14.0);
            let gap = 10.0;
            let count = Persona::ALL.len() as f32;
            let width = ((ui.available_width() - gap * (count - 1.0)) / count).floor();
            // Every card as tall as the longest one.
            let height = Persona::ALL
                .iter()
                .map(|p| persona_card_height(ui, theme, *p, width))
                .fold(150.0, f32::max);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for persona in Persona::ALL {
                    if persona_card(ui, theme, persona, *picked == persona, Vec2::new(width, height))
                        .clicked()
                    {
                        *picked = persona;
                    }
                }
            });
            ui.add_space(12.0);
            rail_preview(ui, theme, &rail_for(*picked));
        });
    outcome
}
