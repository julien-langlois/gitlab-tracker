//! The side cards: release notes of the selection, lines with how and when an MR got there, the share bar, id pills.

use super::*;

pub(super) enum Action {
    Copy,
    Show,
    ShowOpen,
}

pub(super) enum Copied {
    Commands,
    Notes,
}

/// The selected MRs as release notes: a Markdown list, newest merge last.
pub fn release_notes(rows: &[&Row], to: &str) -> String {
    let mut out = format!("## {}\n\n", tf("Released to {to}", &[("to", to)]));
    for r in rows {
        out.push_str(&format!("- {} (!{})", r.title, r.id));
        if let Some((id, _)) = &r.ticket {
            out.push_str(&format!(" · #{id}"));
        }
        out.push('\n');
    }
    out
}

/// Text in the accent that reads as a link.
pub(super) fn link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(theme.text_size - 1.0)
                .color(theme.palette.accent),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A line of a side card: the id, the title, then on the right a chip (how
/// it got there) and a note (when).
pub(super) fn side_row(
    ui: &mut Ui,
    theme: &Theme,
    r: &Row,
    chip: Option<(&str, bevel_theme::Tone)>,
    note: Option<String>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(6.0, 0.0)),
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let mut line = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    line.style_mut().interaction.selectable_labels = false;
    line.spacing_mut().item_spacing.x = 10.0;
    line.add_sized(
        Vec2::new(46.0, rect.height()),
        egui::Label::new(
            mono(format!("!{}", r.id), theme.text_size - 1.5).color(theme.palette.text_muted),
        ),
    );
    line.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(note) = &note {
            ui.label(muted(theme, note).size(theme.text_size - 2.0));
        }
        if let Some((label, tone)) = chip {
            ui.add(
                Chip::new(&label.to_lowercase(), tone).font(fonts::medium(theme.text_size - 3.0)),
            );
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&r.title).size(theme.text_size - 0.5)).truncate(),
            );
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(Tip::new().mr(&r.id, &r.title).hint(tl("Open its page")))
        .clicked()
}

/// The open MRs not on `from` yet, by what holds each back most (a failed
/// pipeline, else its status), the largest group first.
pub(super) fn open_groups<'a>(
    rows: &[&'a Row],
    theme: &Theme,
) -> Vec<(&'static str, bevel_theme::Tone, Vec<&'a Row>)> {
    let mut groups: Vec<(&'static str, bevel_theme::Tone, Vec<&'a Row>)> = Vec::new();
    let ci = tl("CI failed");
    for r in rows {
        let (label, tone) = if r.blockers.iter().any(|b| b == ci) {
            (ci, theme.tones.red)
        } else {
            tones::status(r.state, &r.mergeability, &theme.tones)
        };
        match groups.iter_mut().find(|g| g.0 == label) {
            Some(group) => group.2.push(r),
            None => groups.push((label, tone, vec![r])),
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.2.len()));
    groups
}

/// "CHANGES REQ." as "Changes req.": the status labels are written for chips.
/// Acronyms keep their capitals: "CI RUNNING" reads "CI running", not "Ci".
pub(super) fn sentence_case(label: &str) -> String {
    const ACRONYMS: [&str; 3] = ["CI", "MR", "MRS"];
    let mut out = String::with_capacity(label.len());
    for (i, word) in label.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let bare = word.trim_matches(|c: char| !c.is_alphanumeric());
        if ACRONYMS.contains(&bare) {
            out.push_str(word);
        } else if i == 0 {
            let lower = word.to_lowercase();
            let mut chars = lower.chars();
            if let Some(c) = chars.next() {
                out.extend(c.to_uppercase().chain(chars));
            }
        } else {
            out.push_str(&word.to_lowercase());
        }
    }
    out
}

/// A thin bar split by group, each segment as wide as its share.
pub(super) fn share_bar(ui: &mut Ui, groups: &[(&'static str, bevel_theme::Tone, Vec<&Row>)]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let total: usize = groups.iter().map(|g| g.2.len()).sum();
    let gap = 3.0;
    let room = rect.width() - gap * (groups.len() as f32 - 1.0);
    let mut x = rect.left();
    for (_, tone, rows) in groups {
        let w = (room * rows.len() as f32 / total as f32).max(4.0);
        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w, rect.height()));
        ui.painter()
            .rect_filled(seg, CornerRadius::same(3), tone.fg);
        x += w + gap;
    }
}

/// An MR id as a small pill in its group's tone, its title and what holds it
/// back on hover.
pub(super) fn id_pill(
    ui: &mut Ui,
    theme: &Theme,
    r: &Row,
    tone: bevel_theme::Tone,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        format!("!{}", r.id),
        egui::FontId::monospace(theme.text_size - 2.0),
        tone.fg,
    );
    let size = Vec2::new(galley.size().x + 14.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let stroke = if response.hovered() {
        egui::Stroke::new(1.0, tone.fg)
    } else {
        egui::Stroke::NONE
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(theme.radii.medium),
        tone.bg,
        stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, tone.fg);
    let (status, status_tone) = tones::status(r.state, &r.mergeability, &theme.tones);
    let why = r
        .blockers
        .iter()
        .fold(Tip::new().mr(&r.id, &r.title), |tip, b| {
            tip.chip(b.as_str(), theme.tones.red)
        })
        .chip(status, status_tone);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(why)
}
