//! The people table: columns, rows, who reviews whom per row.

use super::*;

/// The table's columns: their left edges, from the row's left, and the widths.
pub(super) struct Cols {
    pub(super) person: (f32, f32),
    pub(super) reviews: (f32, f32),
    pub(super) mrs: (f32, f32),
    pub(super) threads: Option<(f32, f32)>,
    pub(super) response: Option<(f32, f32)>,
    pub(super) load: (f32, f32),
}

pub(super) fn cols(width: f32) -> Cols {
    let pad = 12.0;
    let gap = 14.0;
    let person = 190.0;
    let mrs = 200.0;
    let load = 150.0;
    let threads = 64.0;
    let response = 84.0;
    let fixed = pad * 2.0 + person + mrs + load + gap * 3.0;
    // The narrow columns go first when the room runs out.
    let with_both = fixed + threads + response + gap * 2.0;
    let (show_threads, show_response) = if width - with_both >= 200.0 {
        (true, true)
    } else if width - (fixed + response + gap) >= 180.0 {
        (false, true)
    } else {
        (false, false)
    };
    let reviews = width
        - fixed
        - if show_threads { threads + gap } else { 0.0 }
        - if show_response { response + gap } else { 0.0 };
    let mut x = pad;
    let mut take = |w: f32| {
        let at = x;
        x += w + gap;
        (at, w)
    };
    let person = take(person);
    let reviews = take(reviews.max(120.0));
    let mrs = take(mrs);
    let threads = show_threads.then(|| take(threads));
    let response = show_response.then(|| take(response));
    let load = take(load);
    Cols {
        person,
        reviews,
        mrs,
        threads,
        response,
        load,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn people_table(
    ui: &mut Ui,
    theme: &Theme,
    list: &[&Person],
    shown: Option<&str>,
    now: DateTime<Utc>,
    reminder: f64,
    picked: &mut Option<String>,
    opened: &mut Option<String>,
    open_mr: &mut Option<String>,
) {
    let c = cols(ui.available_width());
    // Header.
    let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let head_font = FontId::proportional(theme.text_size - 1.5);
    let muted_color = theme.palette.text_muted;
    let head = |col: (f32, f32), text: &str| {
        board::paint_text(
            ui,
            egui::pos2(header.left() + col.0, header.center().y),
            Align2::LEFT_CENTER,
            text,
            head_font.clone(),
            muted_color,
            col.1,
        );
    };
    head(c.person, tl("Person"));
    head(c.reviews, tl("Reviews waiting on them"));
    head(c.mrs, tl("Their open MRs"));
    if let Some(col) = c.threads {
        head(col, tl("Threads"));
    }
    if let Some(col) = c.response {
        head(col, tl("Response"));
    }
    head(c.load, tl("Load"));

    for person in list {
        let selected = shown == Some(person.login.as_str());
        let (rect, response) = board::row(ui, theme, 50.0, selected);
        if !selected {
            board::row_rule(ui, theme, rect);
        }
        let x = |col: (f32, f32)| rect.left() + col.0;
        let cy = rect.center().y;
        // Person: avatar, name over login.
        {
            let mut cell = board::inside(
                ui,
                Rect::from_min_size(
                    egui::pos2(x(c.person), rect.top()),
                    Vec2::new(26.0, rect.height()),
                ),
            );
            avatar(&mut cell, theme, &person.name, 24.0);
            let text_left = x(c.person) + 34.0;
            let width = c.person.1 - 34.0;
            board::paint_text(
                ui,
                egui::pos2(text_left, cy - 1.0),
                Align2::LEFT_BOTTOM,
                &person.name,
                fonts::medium(theme.text_size),
                theme.palette.text,
                width,
            );
            board::paint_text(
                ui,
                egui::pos2(text_left, cy + 2.0),
                Align2::LEFT_TOP,
                &format!("@{}", person.login),
                FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
                width,
            );
        }
        // Reviews waiting: an id and how long, as many as fit, then "+n".
        let pairs: Vec<(String, String, Tone, String)> = person
            .reviews
            .iter()
            .map(|r| {
                let age = r.since.map(|s| board::days_between(s, now));
                let tone = match age {
                    Some(a) if a >= reminder => theme.tones.red,
                    Some(a) if a >= reminder / 2.0 => theme.tones.amber,
                    _ => theme.tones.gray,
                };
                let text = age.map_or_else(|| board::dash().to_string(), board::duration_text);
                (r.id.clone(), text, tone, r.title.clone())
            })
            .collect();
        if pairs.is_empty() {
            let text = match person.last_request {
                Some(at) if person.level != Level::Bot => {
                    tf("Last asked {when}", &[("when", &board::ago(at, now))])
                }
                _ => board::dash().to_string(),
            };
            board::paint_text(
                ui,
                egui::pos2(x(c.reviews), cy),
                Align2::LEFT_CENTER,
                &text,
                FontId::proportional(theme.text_size - 1.5),
                theme.palette.text_muted,
                c.reviews.1,
            );
        } else if let Some(id) = pair_cell(ui, theme, rect, c.reviews, &pairs) {
            *open_mr = Some(id);
        }
        // Own MRs: an id and its next step.
        let own: Vec<(String, String, Tone, String)> = person
            .mrs
            .iter()
            .map(|m| {
                let tone = super::super::next::tone(&m.step, theme);
                let what = m
                    .step
                    .todos
                    .first()
                    .map(|t| t.short())
                    .unwrap_or_else(|| m.step.who());
                (m.id.clone(), what, tone, m.title.clone())
            })
            .collect();
        if own.is_empty() {
            board::paint_text(
                ui,
                egui::pos2(x(c.mrs), cy),
                Align2::LEFT_CENTER,
                board::dash(),
                FontId::proportional(theme.text_size - 1.5),
                theme.palette.text_muted,
                c.mrs.1,
            );
        } else if let Some(id) = pair_cell(ui, theme, rect, c.mrs, &own) {
            *open_mr = Some(id);
        }
        if let Some(col) = c.threads {
            let color = if person.threads > 0 {
                theme.tones.amber.fg
            } else {
                theme.palette.text_muted
            };
            let icon = Rect::from_center_size(egui::pos2(x(col) + 7.0, cy), Vec2::splat(14.0));
            super::super::icons::paint(
                ui.painter(),
                icon,
                super::super::icons::Icon::Comment,
                color,
            );
            board::paint_text(
                ui,
                egui::pos2(x(col) + 20.0, cy),
                Align2::LEFT_CENTER,
                &person.threads.to_string(),
                fonts::medium(theme.text_size - 1.0),
                color,
                col.1 - 20.0,
            );
        }
        if let Some(col) = c.response {
            board::paint_text(
                ui,
                egui::pos2(x(col), cy),
                Align2::LEFT_CENTER,
                &person
                    .response
                    .map_or_else(|| board::dash().to_string(), board::duration_text),
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_secondary,
                col.1,
            );
        }
        // Load: the level and the share above a bar.
        {
            let tone = person.level.tone(theme);
            let left = x(c.load);
            board::paint_text(
                ui,
                egui::pos2(left, cy - 3.0),
                Align2::LEFT_BOTTOM,
                person.level.label(),
                fonts::medium(theme.text_size - 2.0),
                tone.fg,
                c.load.1 - 44.0,
            );
            board::paint_text(
                ui,
                egui::pos2(left + c.load.1, cy - 3.0),
                Align2::RIGHT_BOTTOM,
                &board::percent(f64::from(person.load)),
                FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
                44.0,
            );
            let track = Rect::from_min_size(egui::pos2(left, cy + 3.0), Vec2::new(c.load.1, 6.0));
            ui.painter()
                .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
            let mut fill = track;
            fill.set_width((track.width() * person.load).max(if person.load > 0.0 {
                3.0
            } else {
                0.0
            }));
            if fill.width() > 0.0 {
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(3), tone.fg);
            }
        }
        if super::super::widgets::double_clicked(ui, &response, ("team", &person.login)) {
            *opened = Some(person.login.clone());
        } else if response.clicked() {
            *picked = Some(person.login.clone());
        }
    }
}

/// Pairs of (MR id, tag) painted in a cell, as many as fit, then "+n"; the MR
/// whose id was clicked.
pub(super) fn pair_cell(
    ui: &mut Ui,
    theme: &Theme,
    row: Rect,
    col: (f32, f32),
    pairs: &[(String, String, Tone, String)],
) -> Option<String> {
    let cy = row.center().y;
    let right = row.left() + col.0 + col.1;
    let mut x = row.left() + col.0;
    let mut clicked = None;
    let id_font = FontId::monospace(theme.text_size - 2.0);
    let tag_font = fonts::medium(theme.text_size - 2.5);
    let more_room = 30.0;
    for (index, (id, tag, tone, title)) in pairs.iter().enumerate() {
        let id_text = format!("!{id}");
        let id_galley = ui
            .painter()
            .layout_no_wrap(id_text, id_font.clone(), theme.palette.text);
        let tag_galley = ui
            .painter()
            .layout_no_wrap(tag.clone(), tag_font.clone(), tone.fg);
        let width = id_galley.size().x + 12.0 + tag_galley.size().x + 12.0;
        let left_after = pairs.len() - index - 1;
        let limit = if left_after > 0 {
            right - more_room
        } else {
            right
        };
        if x + width > limit {
            let rest = pairs.len() - index;
            board::paint_text(
                ui,
                egui::pos2(x, cy),
                Align2::LEFT_CENTER,
                &format!("+{rest}"),
                fonts::medium(theme.text_size - 2.0),
                theme.palette.text_muted,
                right - x,
            );
            break;
        }
        let id_rect = Rect::from_min_size(
            egui::pos2(x, cy - 10.0),
            Vec2::new(id_galley.size().x + 12.0, 20.0),
        );
        let tag_rect = Rect::from_min_size(
            egui::pos2(id_rect.right(), cy - 10.0),
            Vec2::new(tag_galley.size().x + 12.0, 20.0),
        );
        let r = theme.radii.small;
        ui.painter().rect_filled(
            id_rect,
            CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            theme.palette.raised,
        );
        ui.painter().rect_filled(
            tag_rect,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: r,
                se: r,
            },
            tone.bg,
        );
        ui.painter().galley(
            id_rect.center() - id_galley.size() / 2.0,
            id_galley,
            theme.palette.text,
        );
        ui.painter().galley(
            tag_rect.center() - tag_galley.size() / 2.0,
            tag_galley,
            tone.fg,
        );
        let whole = id_rect.union(tag_rect);
        let response = Tip::new()
            .mr(id, title)
            .chip(tag.clone(), *tone)
            .hint(tl("Open its page"))
            .show(
                ui.interact(
                    whole,
                    ui.scope_id()
                        .with(("pair", row.top() as i32, col.0 as i32, index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand),
            );
        if response.hovered() {
            ui.painter().rect_stroke(
                whole,
                CornerRadius::same(r),
                egui::Stroke::new(1.0, theme.palette.text_muted),
                egui::StrokeKind::Inside,
            );
        }
        if response.clicked() {
            clicked = Some(id.clone());
        }
        x = whole.right() + 6.0;
    }
    clicked
}
