//! The panel beside the page: where two MRs meet, line by line, and the report.

use super::*;

pub(super) fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    overlaps: &Overlaps,
    pair: Option<&(String, String)>,
) {
    let theme = g.theme;
    let Some((a, b)) = pair else {
        board::inspector_empty(
            ui,
            &theme,
            tl("No two open MRs change the same file: nothing to pick."),
        );
        return;
    };
    let Some(p) = overlaps.pair(a, b) else {
        return;
    };
    let ca = theme.tones.blue.fg;
    let cb = theme.tones.purple.fg;
    board::section(
        ui,
        &theme,
        tl("Where they meet"),
        &tn(p.files.len() as u64, "{n} file", "{n} files"),
        |ui| {
            // How many files of each kind, before the detail.
            let count = |level: Level| p.files.iter().filter(|f| f.1 == level).count();
            let summary: Vec<(Level, String)> = [
                (
                    Level::SameLines,
                    tn(
                        count(Level::SameLines) as u64,
                        "{n} on the same lines",
                        "{n} on the same lines",
                    ),
                ),
                (
                    Level::Near,
                    tn(
                        count(Level::Near) as u64,
                        "{n} on close lines",
                        "{n} on close lines",
                    ),
                ),
                (
                    Level::SameFile,
                    tn(
                        count(Level::SameFile) as u64,
                        "{n} only in the same file",
                        "{n} only in the same file",
                    ),
                ),
            ]
            .into_iter()
            .filter(|(level, _)| count(*level) > 0)
            .collect();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for (level, text) in &summary {
                    board::chip(ui, &theme, text, level.tone(&theme));
                }
            });
            ui.add_space(4.0);
            board::caption(
                ui,
                &theme,
                tl("The files where they change the most lines in common first."),
            );
            ui.add_space(10.0);
            // The files, the largest overlap first.
            let mut files: Vec<Meeting> = p
                .files
                .iter()
                .filter_map(|(path, level)| {
                    let shared = overlaps.file(path)?;
                    let hunks = |id: &str| {
                        shared
                            .mrs
                            .iter()
                            .find(|m| m.0 == id)
                            .map(|m| m.1.clone())
                            .unwrap_or_default()
                    };
                    let (ha, hb) = (hunks(a), hunks(b));
                    let both = common_lines(&ha, &hb);
                    Some((path, *level, ha, hb, both))
                })
                .collect();
            let size = |r: &[(u32, u32)]| r.iter().map(|(s, e)| e - s + 1).sum::<u32>();
            files.sort_by(|x, y| {
                x.1.cmp(&y.1)
                    .then(size(&y.4).cmp(&size(&x.4)))
                    .then(x.0.cmp(y.0))
            });
            for (path, level, hunks_a, hunks_b, both) in files.iter().take(4) {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
                let galley = ui.painter().layout_no_wrap(
                    level.label().to_string(),
                    fonts::medium(theme.text_size - 2.5),
                    level.tone(&theme).fg,
                );
                let tag_w = galley.size().x + 12.0;
                board::paint_tag(
                    ui,
                    &theme,
                    egui::pos2(line.right() - tag_w, line.center().y),
                    level.label(),
                    level.tone(&theme),
                    false,
                );
                path_text(
                    ui,
                    &theme,
                    egui::pos2(line.left(), line.center().y),
                    path,
                    line.width() - tag_w - 10.0,
                );
                ui.add_space(6.0);
                file_lanes(ui, &theme, (a, hunks_a, ca), (b, hunks_b, cb), both);
                let caption = if both.is_empty() {
                    if *level == Level::Near {
                        tl("A few lines apart: git may stop.").to_string()
                    } else {
                        tl("Far apart: it most likely merges on its own.").to_string()
                    }
                } else {
                    let ranges = both
                        .iter()
                        .take(3)
                        .map(|(s, e)| {
                            if s == e {
                                tf("line {n}", &[("n", &s.to_string())])
                            } else {
                                tf(
                                    "lines {from}–{to}",
                                    &[("from", &s.to_string()), ("to", &e.to_string())],
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let more = if both.len() > 3 {
                        format!(
                            " {}",
                            tf("and {n} more", &[("n", &(both.len() - 3).to_string())])
                        )
                    } else {
                        String::new()
                    };
                    format!(
                        "{}{more}",
                        tf("Changed by both: {ranges}", &[("ranges", &ranges)])
                    )
                };
                ui.add_space(4.0);
                board::caption(ui, &theme, &caption);
                ui.add_space(12.0);
            }
            if files.len() > 4 {
                ui.label(muted(
                    &theme,
                    tf("and {n} more", &[("n", &(files.len() - 4).to_string())]),
                ));
            }
        },
    );
}

/// A file two MRs share: its path, how they meet, the hunks of each, and the
/// lines both change.
pub(super) type Meeting<'a> = (
    &'a String,
    Level,
    Vec<(u32, u32)>,
    Vec<(u32, u32)>,
    Vec<(u32, u32)>,
);

/// The lines (first, last) both hunk lists change, merged and in order.
pub(super) fn common_lines(a: &[(u32, u32)], b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut both: Vec<(u32, u32)> = a
        .iter()
        .flat_map(|x| {
            b.iter().filter_map(move |y| {
                let (s, e) = (x.0.max(y.0), x.1.min(y.1));
                (s <= e).then_some((s, e))
            })
        })
        .collect();
    both.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (s, e) in both {
        match merged.last_mut() {
            Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// The part of a file the two MRs change: one lane per MR, its id on the
/// left in its color and its hunks as bars; the lines both change shaded red
/// across the two lanes; the first and last line under it.
pub(super) fn file_lanes(
    ui: &mut Ui,
    theme: &Theme,
    (a_id, a, ca): (&str, &[(u32, u32)], Color32),
    (b_id, b, cb): (&str, &[(u32, u32)], Color32),
    both: &[(u32, u32)],
) {
    let all: Vec<&(u32, u32)> = a.iter().chain(b).collect();
    let lo = all.iter().map(|h| h.0).min().unwrap_or(1);
    let hi = all.iter().map(|h| h.1).max().unwrap_or(1).max(lo);
    let width = ui.available_width();
    let id_font = egui::FontId::monospace(theme.text_size - 2.5);
    let label_w = [a_id, b_id]
        .iter()
        .map(|id| {
            ui.painter()
                .layout_no_wrap(format!("!{id}"), id_font.clone(), theme.palette.text)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        + 12.0;
    let lane_h = 14.0;
    let gap = 6.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, lane_h * 2.0 + gap + 12.0), Sense::hover());
    let track = Rect::from_min_max(egui::pos2(rect.left() + label_w, rect.top()), rect.max);
    ui.painter().rect_filled(
        track,
        CornerRadius::same(theme.radii.medium),
        theme.palette.input,
    );
    let inner = track.shrink2(Vec2::new(8.0, 6.0));
    // Zoomed on the changes; a single line still gets some width.
    let (from, to) = if hi == lo {
        (lo as f32 - 3.0, hi as f32 + 2.0)
    } else {
        let pad = ((hi - lo) as f32 * 0.06).max(1.0);
        ((lo as f32 - 1.0 - pad).max(0.0), hi as f32 + pad)
    };
    let x_of =
        |line: f32| inner.left() + inner.width() * ((line - from) / (to - from)).clamp(0.0, 1.0);
    let span = |s: u32, e: u32| {
        let x0 = x_of(s as f32 - 1.0);
        (x0, x_of(e as f32).max(x0 + 4.0))
    };
    let top_lane = inner.top();
    let bottom_lane = inner.bottom() - lane_h;
    // The lines both change, behind the bars, over the two lanes.
    let red = theme.tones.red.fg;
    for (s, e) in both {
        let (x0, x1) = span(*s, *e);
        let band = Rect::from_min_max(
            egui::pos2(x0 - 2.0, inner.top() - 3.0),
            egui::pos2(x1 + 2.0, inner.bottom() + 3.0),
        );
        ui.painter()
            .rect_filled(band, CornerRadius::same(3), red.gamma_multiply(0.22));
        ui.painter().rect_stroke(
            band,
            CornerRadius::same(3),
            Stroke::new(1.5, red),
            egui::StrokeKind::Inside,
        );
    }
    for (id, hunks, lane_top, color) in [(a_id, a, top_lane, ca), (b_id, b, bottom_lane, cb)] {
        board::paint_text(
            ui,
            egui::pos2(rect.left(), lane_top + lane_h / 2.0),
            Align2::LEFT_CENTER,
            &format!("!{id}"),
            id_font.clone(),
            color,
            label_w - 6.0,
        );
        for (s, e) in hunks {
            let (x0, x1) = span(*s, *e);
            ui.painter().rect_filled(
                Rect::from_min_max(egui::pos2(x0, lane_top), egui::pos2(x1, lane_top + lane_h)),
                CornerRadius::same(3),
                color,
            );
        }
    }
    let hover = Tip::new()
        .dot(ca, format!("!{a_id} · {}", ranges_text(a)))
        .dot(cb, format!("!{b_id} · {}", ranges_text(b)));
    ui.interact(
        track,
        ui.scope_id()
            .with(("lanes", a_id, b_id, lo, hi, a.len(), b.len())),
        Sense::hover(),
    )
    .rich(hover);
    // The line numbers: both ends, or the one line.
    let (axis, _) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::hover());
    let font = egui::FontId::monospace(theme.text_size - 3.5);
    let axis_left = axis.left() + label_w + 8.0;
    let axis_right = axis.right() - 8.0;
    if hi == lo {
        board::paint_text(
            ui,
            egui::pos2(x_of(lo as f32 - 0.5), axis.center().y),
            Align2::CENTER_CENTER,
            &tf("line {n}", &[("n", &lo.to_string())]),
            font,
            theme.palette.text_muted,
            axis.width() / 2.0,
        );
    } else {
        board::paint_text(
            ui,
            egui::pos2(axis_left, axis.center().y),
            Align2::LEFT_CENTER,
            &tf("line {n}", &[("n", &lo.to_string())]),
            font.clone(),
            theme.palette.text_muted,
            (axis_right - axis_left) / 2.0,
        );
        board::paint_text(
            ui,
            egui::pos2(axis_right, axis.center().y),
            Align2::RIGHT_CENTER,
            &tf("line {n}", &[("n", &hi.to_string())]),
            font,
            theme.palette.text_muted,
            (axis_right - axis_left) / 2.0,
        );
    }
}

/// "lines 1–5, 12" for a tooltip.
pub(super) fn ranges_text(hunks: &[(u32, u32)]) -> String {
    hunks
        .iter()
        .map(|(s, e)| {
            if s == e {
                s.to_string()
            } else {
                format!("{s}–{e}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The overlaps as Markdown, to paste.
pub(super) fn report(overlaps: &Overlaps) -> String {
    let mut out = format!("### {}\n\n", tl("Overlaps between open MRs"));
    if overlaps.pairs.is_empty() {
        out.push_str(&format!(
            "{}\n",
            tl("No two open MRs change the same file.")
        ));
    }
    for p in &overlaps.pairs {
        let files = p
            .files
            .iter()
            .map(|(f, l)| format!("`{f}` ({})", l.label()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("- !{} × !{} · {files}\n", p.a, p.b));
    }
    out
}

/// The rail's badge: the pairs of open MRs changing the same files (only those
/// with one of mine when the username is known), in the tone of the worst: the
/// same lines, close lines, the same file.
pub fn badge(g: &GuiApp) -> Option<Badge> {
    let key = (g.overlaps.lockfiles, g.overlaps.drafts);
    let overlaps = g
        .memo
        .overlaps
        .get(&g.app.project_id, key, || analyze(&g.app, key.0, key.1));
    let known = me(&g.app).is_some();
    let pairs: Vec<Level> = overlaps
        .pairs
        .iter()
        .filter(|p| !known || mine(&g.app, &overlaps, p))
        .map(Pair::level)
        .collect();
    let worst = pairs.iter().min()?;
    let mut hint = if known {
        tn(
            pairs.len() as u64,
            "{n} pair of open MRs, one of them yours, change the same files",
            "{n} pairs of open MRs, one of them yours, change the same files",
        )
    } else {
        tn(
            pairs.len() as u64,
            "{n} pair of open MRs change the same files",
            "{n} pairs of open MRs change the same files",
        )
    };
    for level in [Level::SameLines, Level::Near, Level::SameFile] {
        let n = pairs.iter().filter(|l| **l == level).count();
        if n > 0 {
            hint.push('\n');
            hint.push_str(&rail_badges::listed(level.label(), n.to_string()));
        }
    }
    Some(Badge {
        n: pairs.len(),
        tone: worst.tone(&g.theme),
        hint,
    })
}
