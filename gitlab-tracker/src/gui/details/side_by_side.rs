//! A patch with the old file on the left and the new one on the right, lines numbered.

use super::*;

/// A row of a side-by-side patch: a hunk's header across both sides, or a line
/// of the old file (number and text) beside one of the new file, either missing.
pub(super) enum SplitRow<'a> {
    Hunk(&'a str),
    Lines(Option<(u32, &'a str)>, Option<(u32, &'a str)>),
}

/// The rows of a side-by-side patch: unchanged lines face themselves; the lines
/// removed face the lines added in their place, one for one.
pub(super) fn split_rows(diff: &str) -> Vec<SplitRow<'_>> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    let mut removed: Vec<(u32, &str)> = Vec::new();
    let mut added: Vec<(u32, &str)> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("@@") {
            flush_split(&mut rows, &mut removed, &mut added);
            // `@@ -a,b +c,d @@`: the hunk's old lines start at a, its new at c.
            let start = |sign: char| {
                rest.split_whitespace()
                    .find_map(|part| part.strip_prefix(sign))
                    .and_then(|range| range.split(',').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0)
            };
            old = start('-');
            new = start('+');
            rows.push(SplitRow::Hunk(line));
        } else if line.starts_with("---") || line.starts_with("+++") || line.starts_with('\\') {
            continue;
        } else if let Some(text) = line.strip_prefix('-') {
            removed.push((old, text));
            old += 1;
        } else if let Some(text) = line.strip_prefix('+') {
            added.push((new, text));
            new += 1;
        } else {
            flush_split(&mut rows, &mut removed, &mut added);
            let text = line.strip_prefix(' ').unwrap_or(line);
            rows.push(SplitRow::Lines(Some((old, text)), Some((new, text))));
            old += 1;
            new += 1;
        }
    }
    flush_split(&mut rows, &mut removed, &mut added);
    rows
}

/// The lines removed facing the lines added in their place, one for one.
pub(super) fn flush_split<'a>(
    rows: &mut Vec<SplitRow<'a>>,
    removed: &mut Vec<(u32, &'a str)>,
    added: &mut Vec<(u32, &'a str)>,
) {
    for i in 0..removed.len().max(added.len()) {
        rows.push(SplitRow::Lines(
            removed.get(i).copied(),
            added.get(i).copied(),
        ));
    }
    removed.clear();
    added.clear();
}

/// A patch with the old file on the left and the new one on the right, their
/// lines numbered, coloured as in GitLab.
pub(super) fn split_patch(ui: &mut Ui, theme: &Theme, diff: &str, web_url: &str) {
    if diff.trim().is_empty() {
        ui.label(muted(
            theme,
            tl("No text diff: a binary file, or too large for GitLab to show."),
        ));
        return;
    }
    let rows = split_rows(diff);
    let font = egui::FontId::monospace(theme.text_size - 2.5);
    let (height, digit) = ui.fonts_mut(|f| (f.row_height(&font) + 2.0, f.glyph_width(&font, '0')));
    let widest = rows
        .iter()
        .filter_map(|row| match row {
            SplitRow::Lines(a, b) => Some(a.map_or(0, |l| l.0).max(b.map_or(0, |l| l.0))),
            SplitRow::Hunk(_) => None,
        })
        .max()
        .unwrap_or(0);
    let gutter = (widest.to_string().len() as f32 + 1.5) * digit;
    let green = theme.tones.green;
    let red = theme.tones.red;
    let missing = theme.palette.text_muted.gamma_multiply(0.06);
    egui::Frame::new()
        .fill(theme.palette.chrome)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            let half = (width / 2.0).floor();
            for row in rows.iter().take(PATCH_LINES) {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                let painter = ui.painter_at(rect);
                let y = rect.center().y;
                match row {
                    SplitRow::Hunk(text) => {
                        painter.text(
                            egui::pos2(rect.left() + 8.0, y),
                            egui::Align2::LEFT_CENTER,
                            text.replace('\t', "    "),
                            font.clone(),
                            theme.tones.blue.fg,
                        );
                    }
                    SplitRow::Lines(left, right) => {
                        // Changed on a side when the other does not hold it alike.
                        let changed = left.map(|l| l.1) != right.map(|r| r.1);
                        let mut full = String::new();
                        for (side, line, tone, sign) in
                            [(0.0, left, red, '-'), (half, right, green, '+')]
                        {
                            let cell = egui::Rect::from_min_size(
                                egui::pos2(rect.left() + side, rect.top()),
                                Vec2::new(if side == 0.0 { half } else { width - half }, height),
                            );
                            let Some((number, text)) = line else {
                                painter.rect_filled(cell, 0.0, missing);
                                continue;
                            };
                            let color = if changed {
                                painter.rect_filled(cell, 0.0, tone.bg);
                                tone.fg
                            } else {
                                theme.palette.text_secondary
                            };
                            painter.text(
                                egui::pos2(cell.left() + gutter, y),
                                egui::Align2::RIGHT_CENTER,
                                number.to_string(),
                                font.clone(),
                                theme.palette.text_muted,
                            );
                            let text = text.replace('\t', "    ");
                            let marked = if changed { sign } else { ' ' };
                            ui.painter_at(cell.with_min_x(cell.left() + gutter + 6.0))
                                .text(
                                    egui::pos2(cell.left() + gutter + 8.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    format!("{marked} {text}"),
                                    font.clone(),
                                    color,
                                );
                            if !full.is_empty() {
                                full.push('\n');
                            }
                            full.push_str(&format!("{marked}{number:>5}  {text}"));
                        }
                        // The line in full, should it be cut.
                        if response.hovered() && !full.is_empty() {
                            super::super::tip::Tip::new().mono(full).show(response);
                        }
                    }
                }
            }
            // The line between both sides.
            let frame = ui.min_rect();
            ui.painter().vline(
                frame.left() + half,
                frame.y_range(),
                egui::Stroke::new(1.0, theme.palette.border),
            );
        });
    if rows.len() > PATCH_LINES {
        ui.horizontal(|ui| {
            ui.label(muted(
                theme,
                tn(
                    (rows.len() - PATCH_LINES) as u64,
                    "{n} more line",
                    "{n} more lines",
                ),
            ));
            ui.hyperlink_to(tl("in GitLab"), format!("{web_url}/diffs"));
        });
    }
    ui.add_space(4.0);
}
