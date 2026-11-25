//! A patch's lines coloured as in GitLab, and the code a thread is about.

use super::*;

/// A patch, its lines coloured as in GitLab.
pub(super) fn patch(ui: &mut Ui, theme: &Theme, diff: &str, web_url: &str) {
    if diff.trim().is_empty() {
        ui.label(muted(
            theme,
            tl("No text diff: a binary file, or too large for GitLab to show."),
        ));
        return;
    }
    let size = theme.text_size - 2.5;
    let green = theme.tones.green;
    let red = theme.tones.red;
    egui::Frame::new()
        .fill(theme.palette.chrome)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(diff.len())
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for line in diff.lines().take(PATCH_LINES) {
                        let (fill, color) = match line.chars().next() {
                            Some('+') => (green.bg, green.fg),
                            Some('-') => (red.bg, red.fg),
                            Some('@') => (Color32::TRANSPARENT, theme.tones.blue.fg),
                            _ => (Color32::TRANSPARENT, theme.palette.text_secondary),
                        };
                        egui::Frame::new()
                            .fill(fill)
                            .inner_margin(Margin::symmetric(8, 0))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        mono(line.replace('\t', "    "), size).color(color),
                                    )
                                    .extend(),
                                );
                            });
                    }
                });
        });
    let total = diff.lines().count();
    if total > PATCH_LINES {
        ui.horizontal(|ui| {
            ui.label(muted(
                theme,
                tn(
                    (total - PATCH_LINES) as u64,
                    "{n} more line",
                    "{n} more lines",
                ),
            ));
            ui.hyperlink_to(tl("in GitLab"), format!("{web_url}/diffs"));
        });
    }
    ui.add_space(4.0);
}

/// The lines of `diff` up to the new file's line `line`, at most `before` of them
/// before it: the code a thread on that line is about. Empty when the line is not
/// in the patch.
pub fn snippet(diff: &str, line: u32, before: usize) -> Vec<&str> {
    let mut numbered: Vec<(Option<u32>, &str)> = Vec::new();
    let mut new = 0u32;
    for text in diff.lines() {
        if let Some(rest) = text.strip_prefix("@@") {
            // `@@ -a,b +c,d @@`: the hunk's new lines start at c.
            new = rest
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            numbered.push((None, text));
        } else if text.starts_with('-') {
            numbered.push((None, text));
        } else {
            numbered.push((Some(new), text));
            new += 1;
        }
    }
    let Some(at) = numbered.iter().position(|(n, _)| *n == Some(line)) else {
        return Vec::new();
    };
    numbered[at.saturating_sub(before)..=at]
        .iter()
        .map(|(_, text)| *text)
        .collect()
}

/// Code lines coloured as in GitLab, the last one marked when `mark_last`.
pub fn code(ui: &mut Ui, theme: &Theme, lines: &[&str], mark_last: bool) {
    let size = theme.text_size - 2.5;
    egui::Frame::new()
        .fill(theme.palette.chrome)
        .inner_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, line) in lines.iter().enumerate() {
                let (fill, color) = match line.chars().next() {
                    Some('+') => (theme.tones.green.bg, theme.tones.green.fg),
                    Some('-') => (theme.tones.red.bg, theme.tones.red.fg),
                    Some('@') => (Color32::TRANSPARENT, theme.tones.blue.fg),
                    _ => (Color32::TRANSPARENT, theme.palette.text_secondary),
                };
                let marked = mark_last && i + 1 == lines.len();
                let response = egui::Frame::new()
                    .fill(fill)
                    .inner_margin(Margin::symmetric(10, 0))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add(
                            egui::Label::new(mono(line.replace('\t', "    "), size).color(color))
                                .truncate(),
                        );
                    })
                    .response;
                if marked {
                    let r = response.rect;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(r.min, Vec2::new(3.0, r.height())),
                        0.0,
                        theme.palette.accent,
                    );
                }
            }
        });
}

/// Lines added and removed by a patch.
pub fn count(diff: &str) -> (usize, usize) {
    diff.lines().fold((0, 0), |(a, d), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (a + 1, d)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (a, d + 1)
        } else {
            (a, d)
        }
    })
}

/// Whether a log line tells what failed.
pub fn is_failure(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "failed",
        "failure",
        "panicked",
        "fatal",
        "exception",
        "exit code",
    ]
    .iter()
    .any(|word| lower.contains(word))
}
