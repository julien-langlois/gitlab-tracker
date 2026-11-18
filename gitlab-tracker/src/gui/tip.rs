//! Tooltips in the app's look: a raised card with room to breathe, a header
//! (an MR id pill and its title, a person with their avatar), a body in the
//! secondary text color, tone chips, and a muted footer for what a click does.
//!
//! ```ignore
//! Tip::new().mr(id, title).chip(tag, tone).hint(tl("Open its page")).show(response)
//! ```

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{FontId, Margin, Response, RichText, Stroke, Ui, Vec2};

use super::widgets::{avatar, split_person};

/// Widest a tooltip grows before its text wraps.
const MAX_WIDTH: f32 = 300.0;

enum Part {
    Mr { id: String, title: String },
    Person(String),
    Pair(String, String),
    Title(String),
    Text(String),
    Chips(Vec<(String, Tone)>),
}

/// A tooltip's content, laid out top-down; shown with [`Tip::show`].
#[derive(Default)]
#[must_use]
pub struct Tip {
    parts: Vec<Part>,
    hint: Option<String>,
}

impl Tip {
    pub fn new() -> Self {
        Self::default()
    }

    /// An MR: its `!id` pill and its title, whole.
    pub fn mr(mut self, id: &str, title: &str) -> Self {
        self.parts.push(Part::Mr {
            id: id.to_string(),
            title: title.to_string(),
        });
        self
    }

    /// A person, from a "Full Name (@login)" string: avatar, name, handle.
    pub fn person(mut self, display: &str) -> Self {
        self.parts.push(Part::Person(display.to_string()));
        self
    }

    /// Two people, "from → to", from "Full Name (@login)" strings.
    pub fn pair(mut self, from: &str, to: &str) -> Self {
        self.parts
            .push(Part::Pair(from.to_string(), to.to_string()));
        self
    }

    /// A semibold line: what the tooltip is about.
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.parts.push(Part::Title(text.into()));
        self
    }

    /// A line of explanation, wrapped.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.parts.push(Part::Text(text.into()));
        self
    }

    /// A tone chip; chips added one after another share a row.
    pub fn chip(mut self, text: impl Into<String>, tone: Tone) -> Self {
        let chip = (text.into(), tone);
        match self.parts.last_mut() {
            Some(Part::Chips(chips)) => chips.push(chip),
            _ => self.parts.push(Part::Chips(vec![chip])),
        }
        self
    }

    /// What a click does, under a rule at the bottom.
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.hint = Some(text.into());
        self
    }

    /// Attaches the tooltip to `response`.
    pub fn show(self, response: Response) -> Response {
        response.on_hover_ui(move |ui| self.ui(ui))
    }

    fn ui(self, ui: &mut Ui) {
        let theme = Theme::get(ui.ctx());
        // The popup frame brings 6 points; this makes it 12 × 10.
        egui::Frame::new()
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.set_max_width(MAX_WIDTH);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 7.0);
                for part in &self.parts {
                    part_ui(ui, &theme, part);
                }
                if let Some(hint) = &self.hint {
                    ui.add_space(1.0);
                    let width = ui.min_rect().width().max(120.0);
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
                    ui.painter().hline(
                        rect.x_range(),
                        rect.center().y,
                        Stroke::new(1.0, theme.palette.border_soft),
                    );
                    ui.label(
                        RichText::new(hint)
                            .size(theme.text_size - 2.0)
                            .color(theme.palette.text_muted),
                    );
                }
            });
    }
}

fn part_ui(ui: &mut Ui, theme: &Theme, part: &Part) {
    match part {
        Part::Mr { id, title } => {
            ui.horizontal_top(|ui| {
                // The id pill on the content ground: the tooltip is itself raised.
                let tone = Tone {
                    bg: theme.palette.content,
                    fg: theme.palette.text,
                };
                ui.add(
                    Chip::new(&format!("!{id}"), tone)
                        .font(FontId::monospace(theme.text_size - 2.0)),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(fonts::semibold(theme.text_size - 0.5))
                            .color(theme.palette.text),
                    )
                    .wrap(),
                );
            });
        }
        Part::Person(display) => {
            let (name, handle) = split_person(display);
            ui.horizontal(|ui| {
                avatar(ui, theme, name, 26.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(
                        RichText::new(name)
                            .font(fonts::semibold(theme.text_size - 0.5))
                            .color(theme.palette.text),
                    );
                    if let Some(handle) = handle {
                        ui.label(
                            RichText::new(format!("@{handle}"))
                                .size(theme.text_size - 2.0)
                                .color(theme.palette.text_muted),
                        );
                    }
                });
            });
        }
        Part::Pair(from, to) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, display) in [from, to].into_iter().enumerate() {
                    if i == 1 {
                        ui.label(
                            RichText::new("→")
                                .size(theme.text_size)
                                .color(theme.palette.text_muted),
                        );
                    }
                    let name = split_person(display).0;
                    avatar(ui, theme, name, 20.0);
                    ui.label(
                        RichText::new(name)
                            .font(fonts::semibold(theme.text_size - 0.5))
                            .color(theme.palette.text),
                    );
                }
            });
        }
        Part::Title(text) => {
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .font(fonts::semibold(theme.text_size - 0.5))
                        .color(theme.palette.text),
                )
                .wrap(),
            );
        }
        Part::Text(text) => {
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .size(theme.text_size - 1.0)
                        .color(theme.palette.text_secondary),
                )
                .wrap(),
            );
        }
        Part::Chips(chips) => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (text, tone) in chips {
                    ui.add(Chip::new(text, *tone).font(fonts::medium(theme.text_size - 2.5)));
                }
            });
        }
    }
}
