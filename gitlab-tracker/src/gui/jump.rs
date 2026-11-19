//! Shortcuts to the sections of a long page: a bar above the page's scrolling
//! area lists the sections it shows, a click scrolls to one (its frame lights
//! up a moment), and the bar lights the section in view. `[` / `]` move to the
//! previous or next section, Home goes back to the top.
//!
//! A page keeps a [`Jumps`], draws [`bar`] above its `ScrollArea`, calls
//! [`Jumps::begin`] first thing in it, wraps each section in
//! [`Jumps::section`], and gives the area's output to [`Jumps::end`]. The
//! positions come from the frame before: nothing is measured twice.

use bevel_theme::{fonts, Theme};
use egui::{Align, Color32, CornerRadius, Margin, Rect, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use super::tip::TipExt;
use crate::i18n::tl;

/// The anchor at the top of the page.
pub const TOP: &str = "top";
/// How long a section's frame stays lit after a jump to it, in seconds.
const FLASH: f64 = 0.7;
/// The bar shows once the content is this many times the visible height…
const SHOW_AT: f32 = 1.5;
/// …and hides below this one: no blinking around the threshold.
const HIDE_AT: f32 = 1.35;

/// An entry of the bar.
pub struct Entry {
    /// What a click returns.
    pub id: &'static str,
    /// The section it scrolls to: several entries may share one (the tabs
    /// of a card).
    pub anchor: &'static str,
    pub label: String,
    pub count: Option<usize>,
    /// A dot after the label: something waits there.
    pub dot: Option<Color32>,
    pub tip: Option<String>,
    /// Lit when its section is in view: of the entries sharing a section,
    /// only the one showing (the tab open).
    pub lit: bool,
    /// Starts a group: a separator before it.
    pub group: bool,
}

impl Entry {
    pub fn new(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            anchor: id,
            label: label.into(),
            count: None,
            dot: None,
            tip: None,
            lit: true,
            group: false,
        }
    }

    pub fn anchor(mut self, anchor: &'static str) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn count(mut self, n: usize) -> Self {
        self.count = Some(n);
        self
    }

    pub fn dot(mut self, color: Option<Color32>, tip: Option<String>) -> Self {
        self.dot = color;
        self.tip = color.and(tip);
        self
    }

    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    pub fn group(mut self) -> Self {
        self.group = true;
        self
    }
}

/// A page's sections: where they are, the one asked for, the one reached.
#[derive(Debug, Default, Clone)]
pub struct Jumps {
    /// The section to scroll to while it is drawn.
    asked: Option<&'static str>,
    /// What was asked when this frame's drawing began.
    stale: Option<&'static str>,
    /// The content's top on screen, this frame.
    origin: f32,
    /// Each section's top in the content, this frame and the one before.
    tops: Vec<(&'static str, f32)>,
    last: Vec<(&'static str, f32)>,
    content: f32,
    viewport: f32,
    offset: f32,
    /// The sections the bar lists, in its order.
    order: Vec<&'static str>,
    /// The section scrolled to, and when.
    reached: Option<(&'static str, f64)>,
    /// The time this frame.
    now: f64,
    /// The section in view, from the frame before.
    current: Option<&'static str>,
    shown: bool,
    /// Whether the last section drawn showed anything.
    drew: bool,
    accent: Color32,
    radius: u8,
}

impl Jumps {
    /// Whether the bar showed this frame.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Whether the last section drawn showed anything: some have nothing to
    /// show at times, and no gap should follow them then.
    pub fn drew(&self) -> bool {
        self.drew
    }

    /// Scrolls to `anchor` when it is next drawn.
    pub fn ask(&mut self, anchor: &'static str) {
        self.asked = Some(anchor);
    }

    /// Scrolls to the next section (`step` 1) or the previous one (-1).
    pub fn step(&mut self, step: i32) {
        // A little room: the section just jumped to is the one we are on.
        let here = self.offset + 4.0;
        let tops: Vec<(&'static str, f32)> = self
            .order
            .iter()
            .filter_map(|a| self.top(a).map(|t| (*a, t)))
            .collect();
        let target = if step > 0 {
            tops.iter()
                .filter(|(_, t)| *t > here)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(a, _)| *a)
        } else {
            tops.iter()
                .filter(|(_, t)| *t < here - 8.0)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(a, _)| *a)
                .or(Some(TOP))
        };
        if let Some(anchor) = target {
            self.ask(anchor);
        }
    }

    /// Seconds since `at`, on the clock of the last frame.
    fn age(&self, at: f64) -> f64 {
        self.now - at
    }

    fn top(&self, anchor: &str) -> Option<f32> {
        self.last
            .iter()
            .find(|(a, _)| *a == anchor)
            .map(|(_, t)| *t)
    }

    /// First thing in the scrolling area: where the content starts.
    pub fn begin(&mut self, ui: &mut Ui) {
        self.origin = ui.cursor().top();
        self.now = ui.input(|i| i.time);
        self.tops.clear();
        self.stale = self.asked;
        if self.asked == Some(TOP) {
            let at = ui.cursor().min;
            ui.scroll_to_rect(Rect::from_min_size(at, Vec2::splat(1.0)), Some(Align::TOP));
            self.asked = None;
        }
    }

    /// Draws a section with `body`: notes where it is, scrolls to it when asked,
    /// and lights its frame a moment after a jump to it.
    pub fn section<R>(
        &mut self,
        ui: &mut Ui,
        anchor: &'static str,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let top = ui.cursor().top();
        self.tops.push((anchor, top - self.origin));
        if self.asked == Some(anchor) {
            // A few points above, so the section's edge shows.
            let at = egui::pos2(ui.cursor().left(), top - 6.0);
            ui.scroll_to_rect(Rect::from_min_size(at, Vec2::splat(1.0)), Some(Align::TOP));
            self.asked = None;
            self.reached = Some((anchor, ui.input(|i| i.time)));
        }
        let response = ui.scope(body);
        self.drew = response.response.rect.height() > 0.5;
        if let Some((_, at)) = self.reached.filter(|(a, _)| *a == anchor) {
            let age = ui.input(|i| i.time) - at;
            if age < FLASH {
                let fade = (1.0 - age / FLASH) as f32;
                ui.painter().rect_stroke(
                    response.response.rect,
                    CornerRadius::same(self.radius),
                    Stroke::new(2.0, self.accent.gamma_multiply(fade)),
                    egui::StrokeKind::Inside,
                );
                ui.ctx().request_repaint();
            }
        }
        response.inner
    }

    /// After the scrolling area: what it measured, and the section in view.
    pub fn end<R>(&mut self, output: &egui::scroll_area::ScrollAreaOutput<R>) {
        self.content = output.content_size.y;
        self.viewport = output.inner_rect.height();
        self.offset = output.state.clamped_offset().y;
        // An anchor asked for before this frame and still not drawn: a
        // section absent this time.
        if self.asked.is_some() && self.asked == self.stale {
            self.asked = None;
        }
        self.last = std::mem::take(&mut self.tops);
        let line = self.offset + self.viewport / 3.0;
        let mut listed: Vec<(&'static str, f32)> = self
            .last
            .iter()
            .filter(|(a, _)| self.order.contains(a))
            .copied()
            .collect();
        listed.sort_by(|a, b| a.1.total_cmp(&b.1));
        // Nothing lit before the page has scrolled: the reader is at its start.
        self.current = listed
            .iter()
            .rev()
            .find(|(_, t)| *t <= line)
            .map(|(a, _)| *a)
            .filter(|_| self.offset > 1.0);
        // The section jumped to stays lit while it is in view: the last ones
        // of a page never reach the line. Forgotten once scrolled away (after
        // the scroll that brought it, which takes a moment).
        if let Some((reached, at)) = self.reached {
            let seen = self
                .top(reached)
                .is_some_and(|t| t >= self.offset - 12.0 && t < self.offset + self.viewport - 24.0);
            if seen {
                self.current = Some(reached);
            } else if output.inner_rect.height() > 0.0 && self.age(at) > 1.0 {
                self.reached = None;
            }
        }
    }
}

/// The bar of `entries`, when the page is long enough to need it. Returns the
/// id of the entry clicked; it has already asked for its section.
pub fn bar(
    ui: &mut Ui,
    theme: &Theme,
    jumps: &mut Jumps,
    entries: &[Entry],
) -> Option<&'static str> {
    jumps.accent = theme.palette.accent;
    jumps.radius = theme.radii.large;
    jumps.order.clear();
    for e in entries {
        if !jumps.order.contains(&e.anchor) {
            jumps.order.push(e.anchor);
        }
    }
    let ratio = if jumps.viewport > 0.0 {
        jumps.content / jumps.viewport
    } else {
        0.0
    };
    jumps.shown = if jumps.shown {
        ratio >= HIDE_AT
    } else {
        ratio >= SHOW_AT
    };
    if !jumps.shown || entries.is_empty() {
        return None;
    }
    let mut clicked = None;
    let size = theme.text_size - 1.5;
    let height = size + 13.0;
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_width(width);
            let (row, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
            // Back to the top, on the right, once the page has scrolled.
            let mut room = width;
            if jumps.offset > 24.0 {
                let galley = ui.painter().layout_no_wrap(
                    tl("Top").to_string(),
                    fonts::medium(size),
                    theme.palette.text_secondary,
                );
                let w = galley.size().x + 36.0;
                let rect = Rect::from_min_size(
                    egui::pos2(row.right() - w, row.top()),
                    Vec2::new(w, height),
                );
                let response = ui
                    .interact(rect, ui.scope_id().with("jump_top"), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .tip(tl("Back to the top (Home)"));
                let color = if response.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(theme.radii.medium),
                        theme.palette.raised,
                    );
                    theme.palette.text
                } else {
                    theme.palette.text_secondary
                };
                let icon = Rect::from_center_size(
                    egui::pos2(rect.left() + 15.0, rect.center().y),
                    Vec2::splat(size),
                );
                icons::paint(ui.painter(), icon, Icon::ArrowUp, color);
                ui.painter().galley(
                    egui::pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    color,
                );
                if response.clicked() {
                    jumps.ask(TOP);
                }
                room -= w + 6.0;
            }
            let strip = Rect::from_min_size(row.min, Vec2::new(room, height));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(strip)
                    .layout(egui::Layout::left_to_right(Align::Center)),
            );
            // Too many for the width: they scroll sideways, on one line.
            egui::ScrollArea::horizontal()
                .id_salt("jump_bar")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .max_width(room)
                .show(&mut child, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                    ui.horizontal(|ui| {
                        for (i, e) in entries.iter().enumerate() {
                            if e.group && i > 0 {
                                ui.add_space(4.0);
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::new(1.0, height), Sense::hover());
                                ui.painter().vline(
                                    rect.center().x,
                                    rect.y_range().shrink(5.0),
                                    Stroke::new(1.0, theme.palette.border),
                                );
                                ui.add_space(4.0);
                            }
                            if entry(
                                ui,
                                theme,
                                e,
                                jumps.current == Some(e.anchor) && e.lit,
                                height,
                            ) {
                                clicked = Some(e.id);
                                jumps.ask(e.anchor);
                            }
                        }
                    });
                });
        });
    clicked
}

/// One entry: its label, its count muted, its dot. True when clicked.
fn entry(ui: &mut Ui, theme: &Theme, e: &Entry, on: bool, height: f32) -> bool {
    let size = theme.text_size - 1.5;
    let label =
        ui.painter()
            .layout_no_wrap(e.label.clone(), fonts::medium(size), theme.palette.text);
    let count = e.count.map(|n| {
        ui.painter()
            .layout_no_wrap(n.to_string(), fonts::medium(size), theme.palette.text_muted)
    });
    let mut w = label.size().x + 20.0;
    if let Some(c) = &count {
        w += c.size().x + 5.0;
    }
    if e.dot.is_some() {
        w += 11.0;
    }
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, height), Sense::click());
    let mut response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(tip) = &e.tip {
        response = response.tip(tip);
    }
    let radius = CornerRadius::same(theme.radii.medium);
    let color = if on {
        ui.painter()
            .rect_filled(rect, radius, theme.palette.selection);
        theme.palette.text
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, theme.palette.raised);
        theme.palette.text
    } else {
        theme.palette.text_secondary
    };
    super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
    let mut x = rect.left() + 10.0;
    let y = rect.center().y;
    let label_w = label.size().x;
    ui.painter()
        .galley(egui::pos2(x, y - label.size().y / 2.0), label, color);
    x += label_w;
    if let Some(c) = count {
        x += 5.0;
        let cw = c.size().x;
        ui.painter().galley(
            egui::pos2(x, y - c.size().y / 2.0),
            c,
            theme.palette.text_muted,
        );
        x += cw;
    }
    if let Some(dot) = e.dot {
        ui.painter().circle_filled(egui::pos2(x + 7.0, y), 3.5, dot);
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three sections at 100, 600 and 1200, the page scrolled to `offset`.
    fn page(offset: f32) -> Jumps {
        Jumps {
            last: vec![("a", 100.0), ("b", 600.0), ("c", 1200.0)],
            order: vec!["a", "b", "c"],
            offset,
            ..Jumps::default()
        }
    }

    #[test]
    fn next_section_is_the_first_below_the_top() {
        let mut j = page(0.0);
        j.step(1);
        assert_eq!(j.asked, Some("a"));
        // On a section just jumped to (a few points over it), the next one.
        let mut j = page(598.0);
        j.step(1);
        assert_eq!(j.asked, Some("c"));
    }

    #[test]
    fn previous_section_is_the_last_above_and_then_the_top() {
        let mut j = page(700.0);
        j.step(-1);
        assert_eq!(j.asked, Some("b"));
        let mut j = page(100.0);
        j.step(-1);
        assert_eq!(j.asked, Some(TOP));
    }

    #[test]
    fn past_the_last_section_next_does_nothing() {
        let mut j = page(1300.0);
        j.step(1);
        assert_eq!(j.asked, None);
    }

    #[test]
    fn sections_not_in_the_bar_are_skipped() {
        let mut j = page(0.0);
        j.order = vec!["b", "c"];
        j.step(1);
        assert_eq!(j.asked, Some("b"));
    }
}
