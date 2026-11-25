//! The views of the list: built-in and saved ones, next ones, focus items.

use super::*;

/// One view: glyph, name and how many MRs it shows.
pub(super) fn view_item(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef, glyph: &str) {
    let theme = g.theme;
    let p = &theme.palette;
    let selected = index == g.app.active_filter.index;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let fill = if selected {
        p.selection
    } else if response.hovered() {
        p.raised
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    let y = rect.center().y;
    let glyph_color = if selected { p.accent } else { p.text_muted };
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, y),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(theme.text_size - 1.0),
        glyph_color,
    );
    let count = g.app.filter_count(index).to_string();
    let count_rect = ui.painter().text(
        egui::pos2(rect.right() - 8.0, y),
        egui::Align2::RIGHT_CENTER,
        &count,
        egui::FontId::proportional(theme.text_size - 1.5),
        p.text_muted,
    );
    let label_color = if selected { p.text } else { p.text_secondary };
    let font = if selected {
        bevel_theme::fonts::medium(theme.text_size)
    } else {
        egui::FontId::proportional(theme.text_size)
    };
    let max_width = count_rect.left() - rect.left() - 40.0;
    let galley = ui
        .painter()
        .layout(clean_label(def), font, label_color, max_width.max(20.0));
    let pos = egui::pos2(rect.left() + 32.0, y - galley.size().y / 2.0);
    ui.painter()
        .with_clip_rect(rect.shrink(2.0))
        .galley(pos, galley, label_color);
    super::super::widgets::focus_ring(ui, &theme, &response, theme.radii.medium);
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
        && !selected
    {
        g.pending.push(Action::SetFilter {
            index,
            query: String::new(),
        });
    }
}

/// The views by next step: what waits on me, on the others, to deploy. Each
/// lists its MRs as the cockpit's counters do; a second click shows them all.
pub(super) fn next_views(g: &mut GuiApp, ui: &mut Ui) {
    use crate::next_step::{Bucket, OTHERS_KEY};
    let steps = crate::next_step::all(&g.app);
    let count = |key: &str| {
        let buckets = Bucket::of_key(key);
        steps
            .values()
            .filter(|s| buckets.contains(&s.bucket()))
            .count()
    };
    let current = g.app.focus.as_ref().map(|f| f.key.clone());
    let mut picked = None;
    for (key, label, glyph) in [
        (Bucket::OnMe.key(), tl("Waiting on me"), "◎"),
        (OTHERS_KEY, tl("Waiting on others"), "◌"),
        (Bucket::ToDeploy.key(), tl("To promote"), "↑"),
    ] {
        let n = count(key);
        let selected = current.as_deref() == Some(key);
        let highlight = (key == Bucket::OnMe.key() && n > 0).then_some(g.theme.tones.orange.fg);
        if focus_item(g, ui, label, glyph, n, selected, highlight) {
            picked = Some((key, label, selected));
        }
    }
    match picked {
        Some((_, _, true)) => g.next_focus = Some(None),
        Some((key, label, false)) => g.focus_on(key, label),
        None => {}
    }
}

/// A view row like [`view_item`], for a focus; `true` when clicked.
pub(super) fn focus_item(
    g: &GuiApp,
    ui: &mut Ui,
    label: &str,
    glyph: &str,
    count: usize,
    selected: bool,
    count_color: Option<Color32>,
) -> bool {
    let theme = g.theme;
    let p = &theme.palette;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let fill = if selected {
        p.selection
    } else if response.hovered() {
        p.raised
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    let y = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, y),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(theme.text_size - 1.0),
        if selected { p.accent } else { p.text_muted },
    );
    ui.painter().text(
        egui::pos2(rect.right() - 8.0, y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        if count_color.is_some() {
            bevel_theme::fonts::semibold(theme.text_size - 1.5)
        } else {
            egui::FontId::proportional(theme.text_size - 1.5)
        },
        count_color.unwrap_or(p.text_muted),
    );
    let label_color = if selected { p.text } else { p.text_secondary };
    let font = if selected {
        bevel_theme::fonts::medium(theme.text_size)
    } else {
        egui::FontId::proportional(theme.text_size)
    };
    ui.painter().text(
        egui::pos2(rect.left() + 32.0, y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        label_color,
    );
    super::super::widgets::focus_ring(ui, &theme, &response, theme.radii.medium);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl(if selected {
            "Click to show them all"
        } else {
            "Click to show only these"
        }))
        .clicked()
}
