//! Terminal colors as theme colors, for what the window shares with the terminal
//! UI (cockpit counters, configured label colors).
//!
//! Named colors become the theme's tones, and RGB colors are kept but nudged
//! toward the text color when they would be unreadable on the theme's surfaces.

use bevel_theme::{Theme, Tone};
use egui::Color32;
use ratatui::style::Color;

/// The tone a named terminal color stands for.
fn tone(color: Color, theme: &Theme) -> Option<Tone> {
    let t = &theme.tones;
    Some(match color {
        Color::Red | Color::LightRed => t.red,
        Color::Green | Color::LightGreen => t.green,
        Color::Yellow | Color::LightYellow => t.amber,
        Color::Blue | Color::LightBlue => t.blue,
        Color::Magenta | Color::LightMagenta => t.purple,
        Color::Cyan | Color::LightCyan => t.teal,
        Color::Gray | Color::DarkGray => t.gray,
        _ => return None,
    })
}

/// A terminal foreground color as a theme color (`None`: the default text color).
pub fn foreground(color: Color, theme: &Theme) -> Option<Color32> {
    let p = &theme.palette;
    match color {
        Color::Reset | Color::White | Color::Black | Color::Indexed(_) => None,
        // Cyan is the terminal UI's accent.
        Color::Cyan | Color::LightCyan => Some(p.accent),
        Color::Gray => Some(p.text_secondary),
        Color::DarkGray => Some(p.text_muted),
        Color::Rgb(r, g, b) => Some(legible(Color32::from_rgb(r, g, b), theme)),
        named => tone(named, theme).map(|t| t.fg),
    }
}

/// `color`, moved toward the text color until it reads on the theme's surfaces.
fn legible(color: Color32, theme: &Theme) -> Color32 {
    let surface = theme.palette.content;
    let mut c = color;
    for step in 1..=4 {
        if contrast(c, surface) >= 3.0 {
            return c;
        }
        c = lerp(color, theme.palette.text, step as f32 * 0.25);
    }
    c
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}
