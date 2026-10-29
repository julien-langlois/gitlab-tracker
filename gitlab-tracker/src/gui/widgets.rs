//! Small building blocks shared by the window's views.

use bevel_theme::{fonts, Chip, Theme, Tone};
use bevel_widgets::buttons::{IconButton, IconButtonStyle};
use egui::{Color32, CornerRadius, FontId, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use crate::app::App;

pub fn icon_button(
    ui: &mut Ui,
    label: &str,
    icon: Icon,
    style: &IconButtonStyle,
    size: f32,
) -> egui::Response {
    icon_toggle(ui, label, icon, style, size, false)
}

/// An icon button drawn as selected when `selected`.
pub fn icon_toggle(
    ui: &mut Ui,
    label: &str,
    icon: Icon,
    style: &IconButtonStyle,
    size: f32,
    selected: bool,
) -> egui::Response {
    IconButton::new(label, Vec2::splat(size), style)
        .selected(selected)
        .show(ui, |ui, rect, tint| {
            icons::paint(ui.painter(), rect.shrink(size * 0.26), icon, tint)
        })
        .on_hover_text(label)
}

/// Text in Inter SemiBold: titles, headings, badges.
pub fn semibold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(fonts::semibold(size))
}

pub fn mono(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::monospace(size))
}

pub fn muted(theme: &Theme, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .size(theme.text_size - 1.0)
        .color(theme.palette.text_muted)
}

/// A button with an optional leading icon. `primary` fills it with the accent;
/// otherwise it is outlined.
pub fn button(
    ui: &mut Ui,
    theme: &Theme,
    icon: Option<Icon>,
    text: &str,
    primary: bool,
) -> egui::Response {
    let p = &theme.palette;
    let (fill, stroke, color) = if primary {
        (p.accent_fill, Stroke::NONE, Color32::WHITE)
    } else {
        (
            Color32::TRANSPARENT,
            Stroke::new(1.0, p.border),
            p.text_secondary,
        )
    };
    let font = fonts::medium(theme.text_size - 0.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), color);
    let icon_room = if icon.is_some() { 20.0 } else { 0.0 };
    let padding = if primary { 12.0 } else { 10.0 };
    let size = Vec2::new(
        galley.size().x + icon_room + padding * 2.0,
        theme.spacing.control_height + 2.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = match (primary, hovered) {
            (true, true) => p.accent_fill.gamma_multiply(1.15),
            (false, true) => p.raised,
            _ => fill,
        };
        let radius = CornerRadius::same(theme.radii.medium);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter()
            .rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        let mut x = rect.left() + padding;
        if let Some(icon) = icon {
            let icon_rect =
                Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), Vec2::splat(14.0));
            let tint = if hovered && !primary { p.text } else { color };
            icons::paint(ui.painter(), icon_rect, icon, tint);
            x += icon_room;
        }
        let tint = if hovered && !primary { p.text } else { color };
        ui.painter().galley(
            egui::pos2(x, rect.center().y - galley.size().y / 2.0),
            galley,
            tint,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    button(ui, theme, None, text, true)
}

/// A page's heading: its title, the subtitle below it.
pub fn page_title(ui: &mut Ui, theme: &Theme, title: &str, subtitle: impl Into<String>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(semibold(title, theme.text_size + 5.0));
        ui.label(muted(theme, subtitle));
    });
}

/// A panel or inspector section heading.
pub fn section_title(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(semibold(text, theme.text_size - 1.0).color(theme.palette.text_secondary));
}

/// A section of a side panel: a soft rule above, then the title and the body.
pub fn section(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui)) {
    rule(ui, theme);
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            section_title(ui, theme, title);
            body(ui);
        });
}

/// A full-width soft line between sections.
pub fn rule(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.palette.border_soft);
}

/// Caption / value rows, captions in a fixed column, as in the inspector's Review
/// section. Long values wrap in their own column.
pub fn key_values(ui: &mut Ui, theme: &Theme, rows: impl FnOnce(&mut KeyValues<'_>)) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 7.0;
        rows(&mut KeyValues { ui, theme });
    });
}

pub struct KeyValues<'a> {
    ui: &'a mut Ui,
    theme: &'a Theme,
}

/// Width of the caption column of [`key_values`].
const CAPTION_WIDTH: f32 = 84.0;

impl KeyValues<'_> {
    pub fn row(&mut self, caption: &str, value: impl FnOnce(&mut Ui)) {
        let theme = self.theme;
        self.ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                Vec2::new(CAPTION_WIDTH, 18.0),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    ui.set_width(CAPTION_WIDTH);
                    ui.label(RichText::new(caption).color(theme.palette.text_muted));
                },
            );
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(width, 0.0),
                egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                |ui| {
                    ui.set_max_width(width);
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                    value(ui);
                },
            );
        });
    }
}

/// How a [`field`] looks.
pub struct FieldLook<'a> {
    /// Shown, muted, while the field is empty.
    pub hint: &'a str,
    /// Drawn before the text.
    pub icon: Option<Icon>,
    pub width: f32,
    /// Keeps the accent border without focus (a filter in use).
    pub active: bool,
}

/// A text field drawn like the mockup's inputs: input fill, a border that turns
/// to the accent when focused or active, an optional leading icon, a muted hint.
pub fn field(
    ui: &mut Ui,
    theme: &Theme,
    text: &mut String,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    look: FieldLook<'_>,
) -> egui::Response {
    let FieldLook {
        hint,
        icon,
        width,
        active,
    } = look;
    let height = theme.spacing.control_height + 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = CornerRadius::same(theme.radii.medium);
    ui.painter().rect_filled(rect, radius, theme.palette.input);
    let mut inner = rect.shrink2(Vec2::new(10.0, 0.0));
    if let Some(icon) = icon {
        let icon_rect = Rect::from_center_size(
            egui::pos2(inner.left() + 7.0, rect.center().y),
            Vec2::splat(14.0),
        );
        icons::paint(ui.painter(), icon_rect, icon, theme.palette.text_muted);
        inner.min.x += 22.0;
    }
    let edit = egui::TextEdit::singleline(text)
        .id_salt(id_salt)
        .frame(egui::Frame::NONE)
        .margin(Margin::ZERO)
        .hint_text(RichText::new(hint).color(theme.palette.text_muted))
        .align(egui::Align2::LEFT_CENTER);
    let response = ui.put(inner, edit);
    let border = if active || response.has_focus() {
        theme.palette.accent
    } else {
        theme.palette.border
    };
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    response
}

/// A card on the content area, with a title.
pub fn card(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui)) {
    theme.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.label(semibold(title, theme.text_size));
            ui.add_space(8.0);
            body(ui);
        });
    });
}

/// A status dot followed by a caption.
pub fn dot_label(ui: &mut Ui, theme: &Theme, color: Color32, text: impl Into<String>) {
    ui.horizontal(|ui| {
        dot(ui, color, 7.0);
        ui.label(muted(theme, text));
    });
}

pub fn dot(ui: &mut Ui, color: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
}

/// An on/off switch with its label; returns whether it was clicked.
pub fn switch(ui: &mut Ui, theme: &Theme, on: bool, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme.text_size),
        theme.palette.text,
    );
    // No gap after the track without a label (a switch beside its caption).
    let gap = if label.is_empty() { 0.0 } else { 10.0 };
    let size = Vec2::new(30.0 + gap + galley.size().x, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, on);
        let track = Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - 8.5),
            Vec2::new(30.0, 17.0),
        );
        let idle = theme.palette.border;
        let fill = lerp_color(idle, theme.palette.accent_fill, how_on);
        ui.painter().rect_filled(track, CornerRadius::same(9), fill);
        let x = egui::lerp(track.left() + 8.5..=track.right() - 8.5, how_on);
        ui.painter()
            .circle_filled(egui::pos2(x, track.center().y), 6.5, Color32::WHITE);
        ui.painter().galley(
            egui::pos2(
                track.right() + 10.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme.palette.text,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A rounded toggle in a tone: the effort pills of the views panel.
pub fn pill_toggle(ui: &mut Ui, theme: &Theme, text: &str, tone: Tone, on: bool) -> egui::Response {
    let font = FontId::proportional(theme.text_size - 1.0);
    let color = if on {
        tone.fg
    } else {
        theme.palette.text_secondary
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = Vec2::new(galley.size().x + 20.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, stroke) = if on {
            (tone.bg, tone.fg)
        } else if response.hovered() {
            (theme.palette.raised, theme.palette.text_muted)
        } else {
            (Color32::TRANSPARENT, theme.palette.border)
        };
        let radius = CornerRadius::same(13);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A state badge (`OPEN`, `MERGED`…): semibold caps on a tone.
pub fn badge(ui: &mut Ui, theme: &Theme, text: &str, tone: Tone) -> egui::Response {
    ui.add(
        Chip::new(text, tone)
            .strong()
            .font(fonts::semibold(theme.text_size - 2.5)),
    )
}

/// A thin horizontal bar filled to `fraction`.
pub fn bar(ui: &mut Ui, theme: &Theme, fraction: f32, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let radius = CornerRadius::same((height / 2.0).round() as u8);
    ui.painter()
        .rect_filled(rect, radius, theme.palette.border_soft);
    let mut filled = rect;
    filled.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    if filled.width() > 0.5 {
        ui.painter().rect_filled(filled, radius, color);
    }
}

/// The colors of a GitLab label chip: the project's `label_colors`, else the
/// label's GitLab color, else a tone by scope.
pub fn label_tone(app: &App, label: &str, theme: &Theme) -> Tone {
    let gitlab = app
        .config
        .gitlab_label_colors
        .get(&label.to_lowercase())
        .map(String::as_str);
    let configured = app.config.label_colors.keys().any(|pattern| {
        let pattern = pattern.to_lowercase();
        let label = label.to_lowercase();
        match pattern.strip_suffix('*') {
            Some(prefix) => label.starts_with(prefix),
            None => label == pattern,
        }
    });
    if configured || gitlab.is_some() {
        let (bg, fg) = app.config.get_label_style(label, gitlab);
        if let (ratatui::style::Color::Rgb(br, bgc, bb), ratatui::style::Color::Rgb(fr, fgc, fb)) =
            (bg, fg)
        {
            return Tone {
                bg: Color32::from_rgb(br, bgc, bb),
                fg: Color32::from_rgb(fr, fgc, fb),
            };
        }
        if let Some(fg) = super::colors::foreground(bg, theme) {
            return Tone {
                bg: fg.gamma_multiply(0.2),
                fg,
            };
        }
    }
    super::tones::label(label, &theme.tones)
}

/// A label chip in its GitLab or configured colors, in monospace as in GitLab.
pub fn label_chip(ui: &mut Ui, app: &App, label: &str, theme: &Theme) -> egui::Response {
    ui.add(
        Chip::new(label, label_tone(app, label, theme))
            .font(FontId::monospace(theme.text_size - 2.5)),
    )
}

/// The activity badge of an MR (`Active`, `Slowing`, `Stale`), from the project's
/// activity thresholds.
pub fn activity(app: &App, updated_at: Option<&str>, theme: &Theme) -> (&'static str, Tone) {
    let (label, _) = app.config.activity_badge(updated_at);
    let t = &theme.tones;
    if label.contains("Active") {
        ("Active", t.green)
    } else if label.contains("Stale") {
        ("Stale", t.red)
    } else if label.contains("Unknown") {
        ("Unknown", t.gray)
    } else {
        ("Slowing", t.amber)
    }
}

/// "Display Name" and "handle" out of GitLab's "Display Name (@handle)".
pub fn split_person(display: &str) -> (&str, Option<&str>) {
    match display.split_once(" (@") {
        Some((name, handle)) => (name, Some(handle.trim_end_matches(')'))),
        None => (display, None),
    }
}

/// A round avatar with the initials of `name`, in a tone picked from the name so
/// each person keeps theirs.
pub fn avatar(ui: &mut Ui, theme: &Theme, name: &str, diameter: f32) -> egui::Response {
    let t = &theme.tones;
    let palette = [t.blue, t.purple, t.teal, t.orange, t.green, t.amber];
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    let tone = palette[hash as usize % palette.len()];
    let initials: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, tone.bg);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        fonts::semibold(diameter * 0.45),
        tone.fg,
    );
    response
}

/// Frames per second of [`spinner`]: enough to read as motion.
const SPINNER_FPS: f64 = 10.0;

/// A spinner like `egui::Spinner`, animated at [`SPINNER_FPS`]: egui's repaints
/// the whole window at the display rate for as long as it shows (a whole sync).
pub fn spinner(ui: &mut Ui, size: f32, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let time = ui.input(|i| i.time);
    let step = (time * SPINNER_FPS).floor();
    let next = std::time::Duration::from_secs_f64((step + 1.0) / SPINNER_FPS - time);
    ui.ctx().request_repaint_after(next);

    // egui's arc, at the step's time.
    let time = step / SPINNER_FPS;
    let radius = size * 0.5 - 2.0;
    let points = (radius.round() as u32).clamp(8, 128);
    let start = time * std::f64::consts::TAU;
    let end = start + 240f64.to_radians() * time.sin();
    let points: Vec<egui::Pos2> = (0..points)
        .map(|i| {
            let angle = egui::emath::lerp(start..=end, f64::from(i) / f64::from(points));
            let (sin, cos) = angle.sin_cos();
            rect.center() + radius * Vec2::new(cos as f32, sin as f32)
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(3.0, color)));
    response
}
