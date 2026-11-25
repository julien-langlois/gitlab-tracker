//! The draft notes card and the release history.

use super::*;

/// The draft notes and their copy button; the text to copy when clicked.
pub(super) fn notes_card(
    ui: &mut Ui,
    theme: &Theme,
    m: &Model,
    draft: Option<&str>,
) -> Option<String> {
    let mut copied = None;
    let note = m.next.as_ref().map(|(v, _)| v.clone()).unwrap_or_default();
    board::card(ui, theme, tl("Draft release notes"), &note, 0.0, |ui| {
        let Some(text) = draft else {
            board::empty(
                ui,
                theme,
                tl("The notes write themselves once an MR is merged."),
            );
            return;
        };
        egui::Frame::new()
            .fill(theme.palette.panel)
            .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
            .corner_radius(egui::CornerRadius::same(theme.radii.medium))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // A preview: the headings as headings, the items as a list.
                ui.spacing_mut().item_spacing.y = 3.0;
                for line in text.lines().filter(|l| !l.trim().is_empty()) {
                    if let Some(title) = line.strip_prefix("### ") {
                        ui.add_space(6.0);
                        ui.label(
                            semibold(title.to_uppercase(), theme.text_size - 2.5)
                                .color(theme.palette.text_muted),
                        );
                    } else if let Some(title) = line.strip_prefix("## ") {
                        ui.label(semibold(title, theme.text_size + 1.0));
                    } else {
                        let item = unlink(line.strip_prefix("- ").unwrap_or(line));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("•  {item}"))
                                    .size(theme.text_size - 1.0)
                                    .color(theme.palette.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                }
            });
        ui.add_space(8.0);
        // Right to left: the button keeps its room, the hint is cut short.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, theme, Some(Icon::Copy), tl("Copy Markdown"), false).clicked() {
                copied = Some(text.to_string());
            }
            ui.add(
                egui::Label::new(
                    muted(theme, tl("Paste it in GitLab when you tag."))
                        .size(theme.text_size - 1.5),
                )
                .truncate(),
            );
        });
    });
    copied
}

pub(super) fn history_card(
    ui: &mut Ui,
    theme: &Theme,
    m: &Model,
    selected: Option<&str>,
    now: DateTime<Utc>,
    picked: &mut Option<String>,
) {
    let note = match m.cadence {
        Some(c) => tf("one every {time}", &[("time", &board::duration_text(c))]),
        None => String::new(),
    };
    board::card(ui, theme, tl("History"), &note, 0.0, |ui| {
        let font = FontId::proportional(theme.text_size - 1.0);
        let small = FontId::proportional(theme.text_size - 2.0);
        for s in m.releases.iter().take(HISTORY_ROWS) {
            let (rect, response) =
                board::row(ui, theme, 34.0, selected == Some(s.record.tag.as_str()));
            let y = rect.center().y;
            let tone = if s.record.is_release {
                theme.tones.blue
            } else {
                board::id_tone(theme)
            };
            board::paint_tag_within(
                ui,
                theme,
                egui::pos2(rect.left() + 6.0, y),
                &s.record.tag,
                tone,
                true,
                110.0,
            );
            let when =
                s.at.map(|at| format!("{} · {}", local_date(at), board::ago(at, now)))
                    .unwrap_or_else(|| board::dash().to_string());
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 130.0, y),
                Align2::LEFT_CENTER,
                &when,
                font.clone(),
                theme.palette.text_secondary,
                170.0,
            );
            let right = rect.right() - 8.0;
            let envs = if s.deployed.is_empty() {
                board::dash().to_string()
            } else {
                s.deployed.join(", ")
            };
            board::paint_text(
                ui,
                egui::pos2(right, y),
                Align2::RIGHT_CENTER,
                &envs,
                small.clone(),
                if s.deployed.is_empty() {
                    theme.palette.text_muted
                } else {
                    theme.tones.green.fg
                },
                150.0,
            );
            let (count, detail) = match &s.contents {
                Some(Read::Ready(c)) if c.mrs.is_empty() => {
                    (tl("no MR").to_string(), String::new())
                }
                Some(Read::Ready(c)) => (tn(c.mrs.len() as u64, "{n} MR", "{n} MRs"), breakdown(c)),
                Some(Read::Loading) => ("…".to_string(), String::new()),
                Some(Read::Failed(_)) => (board::dash().to_string(), tl("not read").to_string()),
                None => (board::dash().to_string(), String::new()),
            };
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 310.0, y),
                Align2::LEFT_CENTER,
                &count,
                font.clone(),
                theme.palette.text,
                80.0,
            );
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 395.0, y),
                Align2::LEFT_CENTER,
                &detail,
                small.clone(),
                theme.palette.text_muted,
                (right - 160.0 - rect.left() - 395.0).max(0.0),
            );
            if response.clicked() {
                *picked = Some(s.record.tag.clone());
            }
        }
        if m.releases.len() > HISTORY_ROWS {
            board::caption(
                ui,
                theme,
                &tn(
                    (m.releases.len() - HISTORY_ROWS) as u64,
                    "and {n} older",
                    "and {n} older",
                ),
            );
        }
    });
}
