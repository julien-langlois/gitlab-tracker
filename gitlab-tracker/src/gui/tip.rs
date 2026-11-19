//! Tooltips in the app's look: a raised card with room to breathe, a header
//! (an MR id pill and its title, a person with their avatar), a body in the
//! secondary text color, facts, tone chips, a key's cap for the shortcut, and
//! a muted footer for what a click does.
//!
//! ```ignore
//! Tip::new().mr(id, title).chip(tag, tone).hint(tl("Open its page")).show(response)
//! response.tip(tl("Settings (,)")) // "Settings" and a "," key cap
//! ```

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Color32, CornerRadius, FontId, Margin, Response, RichText, Stroke, Ui, Vec2};

use super::icons::Icon;
use super::widgets::{avatar, split_person};

/// Widest a tooltip grows before its text wraps.
const MAX_WIDTH: f32 = 320.0;

enum Part {
    /// An id pill (`!12`, `#34`) and a title.
    Item {
        id: String,
        title: String,
    },
    Person(String),
    Pair(String, String),
    Title(String),
    Text(String),
    /// A line led by a colored dot.
    Dot(Color32, String),
    /// Code-like text: a path, a branch, a URL, a command.
    Mono(String),
    /// Label and value rows (the stats page's, for now).
    #[cfg_attr(not(feature = "stats"), allow(dead_code))]
    Facts(Vec<(String, String)>),
    Chips(Vec<(String, Tone)>),
    /// A moment to the minute: the day in words, then the time and its zone.
    Moment {
        date: String,
        time: String,
        zone: String,
    },
}

/// A tooltip's content, laid out top-down; shown with [`Tip::show`].
#[derive(Default)]
#[must_use]
pub struct Tip {
    parts: Vec<Part>,
    key: Option<String>,
    hint: Option<String>,
}

impl Tip {
    pub fn new() -> Self {
        Self::default()
    }

    /// An MR: its `!id` pill and its title, whole.
    pub fn mr(self, id: impl std::fmt::Display, title: &str) -> Self {
        self.item(format!("!{id}"), title)
    }

    /// A ticket: its `#id` pill and its subject, whole.
    pub fn ticket(self, id: impl std::fmt::Display, subject: &str) -> Self {
        self.item(format!("#{id}"), subject)
    }

    /// Any id pill (a commit's short sha…) and a title.
    pub fn item(mut self, id: String, title: &str) -> Self {
        self.parts.push(Part::Item {
            id,
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

    /// A line of explanation, wrapped. Alone, it reads in the text color; under
    /// a title, in the secondary one.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        if !text.is_empty() {
            self.parts.push(Part::Text(text));
        }
        self
    }

    /// A title over its explanation, from "title\nexplanation" text; a single
    /// line stays plain text.
    pub fn lines(self, text: &str) -> Self {
        match text.split_once('\n') {
            Some((title, rest)) => self.title(title).text(rest.trim()),
            None => self.text(text),
        }
    }

    /// A line led by a dot in `color`.
    pub fn dot(mut self, color: Color32, text: impl Into<String>) -> Self {
        self.parts.push(Part::Dot(color, text.into()));
        self
    }

    /// Code-like text in the monospace face: a path, a branch, a URL, a command.
    pub fn mono(mut self, text: impl Into<String>) -> Self {
        self.parts.push(Part::Mono(text.into()));
        self
    }

    /// A label and its value; facts added one after another share a grid.
    #[cfg_attr(not(feature = "stats"), allow(dead_code))]
    pub fn fact(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        let row = (label.into(), value.into());
        match self.parts.last_mut() {
            Some(Part::Facts(rows)) => rows.push(row),
            _ => self.parts.push(Part::Facts(vec![row])),
        }
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

    /// When `at` was, in the machine's time zone: the day in words, then the
    /// time and the zone ("14:32 · UTC+1, local time").
    pub fn moment(mut self, at: chrono::DateTime<chrono::Utc>) -> Self {
        use chrono::Offset;
        let local = at.with_timezone(&chrono::Local);
        let offset = local.offset().fix().local_minus_utc();
        self.parts.push(Part::Moment {
            date: super::widgets::long_date(local.date_naive(), true),
            time: local.format("%H:%M").to_string(),
            zone: format!(
                "{}, {}",
                super::widgets::utc_offset(offset),
                crate::i18n::tl("local time")
            ),
        });
        self
    }

    /// The keyboard shortcut, as a key's cap at the end of the first line.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// What a click does, under a rule at the bottom.
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.hint = Some(text.into());
        self
    }

    /// Plain text, its trailing "(K)" shortcut as a key cap: "Settings (,)".
    pub fn plain(text: &str) -> Self {
        match split_key(text) {
            Some((text, key)) => Self::new().text(text).key(key),
            None => Self::new().text(text),
        }
    }

    /// Attaches the tooltip to `response`.
    pub fn show(self, response: Response) -> Response {
        response.on_hover_ui(move |ui| self.ui(ui))
    }

    /// Attaches the tooltip to `response`, shown while it is disabled.
    pub fn show_disabled(self, response: Response) -> Response {
        response.on_disabled_hover_ui(move |ui| self.ui(ui))
    }

    fn ui(self, ui: &mut Ui) {
        let theme = Theme::get(ui.ctx());
        // The popup frame brings 6 points; this makes it 12 × 10.
        egui::Frame::new()
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.set_max_width(MAX_WIDTH);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 7.0);
                let mut key = self.key.as_deref();
                for (i, part) in self.parts.iter().enumerate() {
                    let lead = i == 0;
                    if let Some(k) = key.take_if(|_| lead) {
                        ui.horizontal(|ui| {
                            part_ui(ui, &theme, part, lead);
                            ui.add_space(4.0);
                            key_cap(ui, &theme, k);
                        });
                    } else {
                        part_ui(ui, &theme, part, lead);
                    }
                }
                if let Some(k) = key {
                    key_cap(ui, &theme, k);
                }
                if let Some(hint) = &self.hint {
                    if !self.parts.is_empty() {
                        ui.add_space(1.0);
                        let width = ui.min_rect().width().max(120.0);
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
                        ui.painter().hline(
                            rect.x_range(),
                            rect.center().y,
                            Stroke::new(1.0, theme.palette.border_soft),
                        );
                    }
                    ui.label(
                        RichText::new(hint)
                            .size(theme.text_size - 2.0)
                            .color(theme.palette.text_muted),
                    );
                }
            });
    }
}

/// `text` without its trailing shortcut, and the shortcut: "Settings (,)",
/// "Flag (Space)", "取消标记（空格）". Only one character or a named key counts,
/// so "To review (me)" stays whole.
fn split_key(text: &str) -> Option<(&str, &str)> {
    const NAMED: [&str; 16] = [
        "Space",
        "Espace",
        "Espacio",
        "Espaço",
        "Leertaste",
        "空格",
        "Del",
        "Suppr",
        "Supr",
        "Entf",
        "Enter",
        "Entrée",
        "Esc",
        "Échap",
        "Tab",
        "Ctrl",
    ];
    let (open, close) = if text.ends_with('）') {
        ('（', '）')
    } else {
        ('(', ')')
    };
    let body = text.strip_suffix(close)?;
    let at = body.rfind(open)?;
    let key = &body[at + open.len_utf8()..];
    let rest = body[..at].trim_end();
    let named = NAMED.iter().any(|n| key == *n || key.starts_with("Ctrl+"));
    (!rest.is_empty() && (key.chars().count() == 1 || named)).then_some((rest, key))
}

/// The tooltip of `self`, in the app's look.
pub trait TipExt {
    /// Plain text; a trailing "(K)" shortcut shows as a key cap.
    fn tip(self, text: impl AsRef<str>) -> Response;
    /// [`TipExt::tip`], shown while the widget is disabled.
    fn tip_disabled(self, text: impl AsRef<str>) -> Response;
    /// "title\nexplanation" text as a title over its explanation.
    fn tip_lines(self, text: impl AsRef<str>) -> Response;
    /// A tooltip built with [`Tip`].
    fn rich(self, tip: Tip) -> Response;
}

impl TipExt for Response {
    fn tip(self, text: impl AsRef<str>) -> Response {
        Tip::plain(text.as_ref()).show(self)
    }

    fn tip_disabled(self, text: impl AsRef<str>) -> Response {
        Tip::plain(text.as_ref()).show_disabled(self)
    }

    fn tip_lines(self, text: impl AsRef<str>) -> Response {
        Tip::new().lines(text.as_ref()).show(self)
    }

    fn rich(self, tip: Tip) -> Response {
        tip.show(self)
    }
}

fn semibold_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(fonts::semibold(theme.text_size - 0.5))
                .color(theme.palette.text),
        )
        .wrap(),
    );
}

fn part_ui(ui: &mut Ui, theme: &Theme, part: &Part, lead: bool) {
    match part {
        Part::Item { id, title } => {
            ui.horizontal_top(|ui| {
                // The id pill on the content ground: the tooltip is itself raised.
                let tone = Tone {
                    bg: theme.palette.content,
                    fg: theme.palette.text,
                };
                ui.add(Chip::new(id, tone).font(FontId::monospace(theme.text_size - 2.0)));
                if !title.is_empty() {
                    semibold_label(ui, theme, title);
                }
            });
        }
        Part::Person(display) => {
            let (name, handle) = split_person(display);
            let name = name.trim_start_matches('@');
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
        Part::Title(text) => semibold_label(ui, theme, text),
        Part::Text(text) => {
            let color = if lead {
                theme.palette.text
            } else {
                theme.palette.text_secondary
            };
            ui.add(
                egui::Label::new(RichText::new(text).size(theme.text_size - 1.0).color(color))
                    .wrap(),
            );
        }
        Part::Dot(color, text) => {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.vertical(|ui| {
                    ui.add_space(5.0);
                    super::widgets::dot(ui, *color, 7.0);
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .size(theme.text_size - 1.0)
                            .color(theme.palette.text_secondary),
                    )
                    .wrap(),
                );
            });
        }
        Part::Mono(text) => {
            egui::Frame::new()
                .fill(theme.palette.content)
                .corner_radius(CornerRadius::same(theme.radii.small))
                .inner_margin(Margin::symmetric(7, 4))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(text)
                                .font(FontId::monospace(theme.text_size - 2.0))
                                .color(theme.palette.text),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                });
        }
        Part::Facts(rows) => {
            egui::Grid::new(ui.next_auto_id())
                .num_columns(2)
                .spacing(Vec2::new(14.0, 4.0))
                .show(ui, |ui| {
                    for (label, value) in rows {
                        ui.label(
                            RichText::new(label)
                                .size(theme.text_size - 1.5)
                                .color(theme.palette.text_muted),
                        );
                        ui.label(
                            RichText::new(value)
                                .font(fonts::medium(theme.text_size - 1.0))
                                .color(theme.palette.text),
                        );
                        ui.end_row();
                    }
                });
        }
        Part::Moment { date, time, zone } => {
            let icon_line = |ui: &mut Ui, icon: Icon, add: &mut dyn FnMut(&mut Ui)| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    let size = theme.text_size;
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
                    super::icons::paint(ui.painter(), rect, icon, theme.palette.text_muted);
                    add(ui);
                });
            };
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                icon_line(ui, Icon::Calendar, &mut |ui| {
                    ui.label(
                        RichText::new(date)
                            .font(fonts::medium(theme.text_size - 0.5))
                            .color(if lead {
                                theme.palette.text
                            } else {
                                theme.palette.text_secondary
                            }),
                    );
                });
                icon_line(ui, Icon::Clock, &mut |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        RichText::new(time)
                            .font(fonts::semibold(theme.text_size - 0.5))
                            .color(theme.palette.text),
                    );
                    ui.label(
                        RichText::new(zone)
                            .size(theme.text_size - 2.0)
                            .color(theme.palette.text_muted),
                    );
                });
            });
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

/// A key's cap, as the shortcuts dialog draws it.
fn key_cap(ui: &mut Ui, theme: &Theme, key: &str) {
    let galley = ui.painter().layout_no_wrap(
        key.to_string(),
        FontId::monospace(theme.text_size - 2.0),
        theme.palette.text,
    );
    let size = Vec2::new((galley.size().x + 10.0).max(20.0), 20.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let radius = CornerRadius::same(theme.radii.small);
    ui.painter().rect_filled(rect, radius, theme.palette.panel);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme.palette.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().hline(
        rect.x_range().shrink(2.0),
        rect.bottom() - 1.0,
        Stroke::new(1.5, theme.palette.border),
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme.palette.text,
    );
}

#[cfg(test)]
mod tests {
    use super::split_key;

    #[test]
    fn shortcuts_come_off_the_end() {
        assert_eq!(split_key("Settings (,)"), Some(("Settings", ",")));
        assert_eq!(split_key("Flag (Space)"), Some(("Flag", "Space")));
        assert_eq!(
            split_key("Démarquer (Espace)"),
            Some(("Démarquer", "Espace"))
        );
        assert_eq!(split_key("取消标记（空格）"), Some(("取消标记", "空格")));
        assert_eq!(split_key("To review (me)"), None);
        assert_eq!(split_key("Remove"), None);
        assert_eq!(split_key("(R)"), None);
    }
}
