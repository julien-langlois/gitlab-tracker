//! The ready MRs as a table: its columns, header and rows.

use super::*;

/// The ready table's columns, from the row's left edge: the box, the MR, the
/// title (the rest), then fixed ones.
pub(super) const COL_BOX: f32 = 28.0;
pub(super) const COL_MR: f32 = 56.0;
pub(super) const COL_SINCE: f32 = 140.0;
pub(super) const COL_TICKET: f32 = 240.0;
pub(super) const COL_EFFORT: f32 = 90.0;
pub(super) const COL_HOW: f32 = 110.0;
pub(super) const COL_PAD: f32 = 16.0;
pub(super) const COL_GAP: f32 = 10.0;

/// The x of each column in a row of `width`: box, MR, title, since, ticket,
/// effort, how; and the title's width.
pub(super) fn columns(left: f32, width: f32) -> ([f32; 7], f32) {
    let fixed = COL_BOX + COL_MR + COL_SINCE + COL_TICKET + COL_EFFORT + COL_HOW + COL_GAP * 6.0;
    let title = (width - COL_PAD * 2.0 - fixed).max(160.0);
    let mut x = left + COL_PAD;
    let mut xs = [0.0; 7];
    for (i, w) in [
        COL_BOX, COL_MR, title, COL_SINCE, COL_TICKET, COL_EFFORT, COL_HOW,
    ]
    .iter()
    .enumerate()
    {
        xs[i] = x;
        x += w + COL_GAP;
    }
    (xs, title)
}

pub(super) fn ready_header(ui: &mut Ui, theme: &Theme, from: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    let (xs, _) = columns(rect.left(), rect.width());
    let on = tf("On {branch} for", &[("branch", from)]);
    let names = [
        "",
        tl("MR"),
        tl("Title"),
        on.as_str(),
        tl("Ticket"),
        tl("Effort"),
        tl("How"),
    ];
    for (x, name) in xs.iter().zip(names) {
        painter.text(
            egui::pos2(*x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            name.to_uppercase(),
            fonts::medium(theme.text_size - 2.5),
            theme.palette.text_muted,
        );
    }
}

/// What a click on a ready row asked for.
pub(super) enum ReadyClick {
    Toggle,
    Open,
}

/// A ready MR as a table row: its box, the MR, its title over when it was
/// merged, how long it has been on the first branch, its ticket, its effort,
/// how it got there.
pub(super) fn ready_row(
    ui: &mut Ui,
    theme: &Theme,
    r: &Row,
    ticked: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<ReadyClick> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme.palette.raised);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    let (xs, title_width) = columns(rect.left(), rect.width());
    let cell = |i: usize, w: f32| {
        egui::Rect::from_min_size(egui::pos2(xs[i], rect.top()), Vec2::new(w, rect.height()))
    };
    let mut click = None;
    let child = |ui: &mut Ui, r: egui::Rect| {
        let mut c = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r)
                .layout(Layout::left_to_right(Align::Center)),
        );
        c.style_mut().interaction.selectable_labels = false;
        c
    };
    let mut c = child(ui, cell(0, COL_BOX));
    let mut checked = ticked;
    if c.checkbox(&mut checked, "")
        .tip(tl("In the release or not"))
        .changed()
    {
        click = Some(ReadyClick::Toggle);
    }
    let mut c = child(ui, cell(1, COL_MR));
    c.label(mono(format!("!{}", r.id), theme.text_size - 1.0).color(theme.palette.text_secondary));
    let mut c = child(ui, cell(2, title_width));
    c.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(RichText::new(&r.title).font(fonts::semibold(theme.text_size)))
                .truncate(),
        );
        let (name, _) = split_person(&r.author);
        let note = match r.merged_at.as_deref() {
            Some(at) => tf(
                "merged {when} · {who}",
                &[
                    ("when", &crate::utils::format_relative_date(at)),
                    ("who", name),
                ],
            ),
            None => name.to_string(),
        };
        ui.add(egui::Label::new(muted(theme, note).size(theme.text_size - 2.0)).truncate());
    });
    let soak = r
        .on_from_since
        .map(|since| (now - since).num_minutes().max(0) as f64 / 1440.0);
    let mut c = child(ui, cell(3, COL_SINCE));
    c.label(
        RichText::new(soak.map_or("—".into(), crate::next_step::days_text))
            .font(fonts::semibold(theme.text_size - 0.5))
            .color(match soak {
                Some(d) if d < 1.0 => theme.tones.amber.fg,
                Some(_) => theme.tones.green.fg,
                None => theme.palette.text_muted,
            }),
    )
    .tip(tl(
        "How long it has been tested there: under a day, in amber",
    ));
    let ticket_cell = cell(4, COL_TICKET);
    let mut c = child(ui, ticket_cell);
    // Kept to its column: a long status is cut, in full on hover.
    c.set_clip_rect(ticket_cell.intersect(c.clip_rect()));
    c.spacing_mut().item_spacing.x = 6.0;
    match &r.ticket {
        Some((id, status)) => {
            c.add(
                Chip::new(&format!("#{id}"), theme.tones.blue)
                    .font(fonts::medium(theme.text_size - 2.5)),
            );
            c.add(
                egui::Label::new(muted(theme, status).size(theme.text_size - 2.0))
                    .truncate()
                    .show_tooltip_when_elided(false),
            )
            .rich(
                Tip::new()
                    .ticket(id, "")
                    .chip(status.as_str(), theme.tones.blue),
            );
        }
        None => {
            c.label(muted(theme, "—"));
        }
    }
    let mut c = child(ui, cell(5, COL_EFFORT));
    match r.effort {
        Some(effort) => {
            let (label, tone) = tones::effort(effort, &theme.tones);
            c.label(RichText::new(label).color(tone.fg));
        }
        None => {
            c.label(muted(theme, "—"));
        }
    }
    let mut c = child(ui, cell(6, COL_HOW));
    let (_, label, tone) = tones::presence(r.on_from, false, &theme.tones);
    c.label(mono(label.to_lowercase(), theme.text_size - 1.5).color(tone.fg));
    if click.is_some() {
        return click;
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(Tip::new().mr(&r.id, &r.title).hint(tl("Open its page")))
        .clicked()
        .then_some(ReadyClick::Open)
}
