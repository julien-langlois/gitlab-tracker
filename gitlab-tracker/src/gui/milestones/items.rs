//! One row per MR of the milestone.

use super::*;

/// A line of the MR list: id, title, next step, a mark per branch, author.
pub(super) fn item_row(ui: &mut Ui, theme: &Theme, item: &Item, ordered: &[String]) -> bool {
    let (rect, response) = board::row(ui, theme, 36.0, false);
    board::row_rule(ui, theme, rect);
    let cy = rect.center().y;
    let left = rect.left() + 8.0;
    let w = board::paint_tag(
        ui,
        theme,
        egui::pos2(left, cy),
        &format!("!{}", item.id),
        board::id_tone(theme),
        true,
    );
    // From the right: the author, the branch marks, the next step.
    let author_w = 130.0;
    let (name, _) = split_person(&item.author);
    let author_rect = Rect::from_min_size(
        egui::pos2(rect.right() - 8.0 - author_w, rect.top()),
        Vec2::new(author_w, rect.height()),
    );
    {
        let mut cell = board::inside(ui, author_rect);
        cell.spacing_mut().item_spacing.x = 6.0;
        avatar(&mut cell, theme, name, 20.0);
        cell.add(
            egui::Label::new(
                egui::RichText::new(board::first_name(&item.author))
                    .size(theme.text_size - 1.5)
                    .color(theme.palette.text_secondary),
            )
            .truncate(),
        );
    }
    let marks_w = ordered.len() as f32 * 22.0;
    let marks_left = author_rect.left() - 12.0 - marks_w;
    for (k, presence) in item.branches.iter().enumerate() {
        let (glyph, label, tone) = super::super::tones::presence(*presence, false, &theme.tones);
        let cell = Rect::from_min_size(
            egui::pos2(marks_left + k as f32 * 22.0, cy - 9.0),
            Vec2::splat(18.0),
        );
        ui.painter()
            .rect_filled(cell, CornerRadius::same(4), tone.bg);
        board::paint_text(
            ui,
            cell.center(),
            Align2::CENTER_CENTER,
            glyph,
            fonts::semibold(theme.text_size - 3.0),
            tone.fg,
            18.0,
        );
        ui.interact(
            cell,
            ui.scope_id().with(("mark", item.id.as_str(), k)),
            Sense::hover(),
        )
        .rich(
            Tip::new()
                .mono(ordered.get(k).map_or("", String::as_str))
                .chip(format!("{glyph} {label}"), tone),
        );
    }
    let step = match item.stand {
        Stand::Delivered => tl("delivered").to_string(),
        Stand::OnTheWay => item
            .step
            .todos
            .first()
            .map(|t| t.short())
            .unwrap_or_else(|| tl("to promote").to_string()),
        _ => item
            .step
            .todos
            .first()
            .map(|t| t.short())
            .unwrap_or_else(|| item.step.who()),
    };
    let tone = if item.stand == Stand::Delivered {
        theme.tones.green
    } else {
        super::super::next::tone(&item.step, theme)
    };
    let step_w = {
        let galley = ui.painter().layout_no_wrap(
            step.clone(),
            fonts::medium(theme.text_size - 2.5),
            tone.fg,
        );
        (galley.size().x + 12.0).min(170.0)
    };
    let step_left = marks_left - 14.0 - step_w;
    board::paint_tag(ui, theme, egui::pos2(step_left, cy), &step, tone, false);
    board::paint_text(
        ui,
        egui::pos2(left + w + 10.0, cy),
        Align2::LEFT_CENTER,
        &item.title,
        FontId::proportional(theme.text_size - 0.5),
        theme.palette.text,
        (step_left - 12.0 - (left + w + 10.0)).max(0.0),
    );
    response
        .rich(
            Tip::new()
                .mr(&item.id, &item.title)
                .hint(tl("Open its page")),
        )
        .clicked()
}

// ── Inspector ────────────────────────────────────────────────────────────────
