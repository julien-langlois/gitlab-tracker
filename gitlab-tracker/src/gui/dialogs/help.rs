//! The keyboard shortcuts dialog: each section a card of keycaps and what they do.

use super::*;

/// The window's shortcuts: the TUI's, minus those for moving the focus.
pub(super) const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Merge requests",
        &[
            ("↑ / ↓  or  k / j", "Select the previous / next MR"),
            (
                "Enter  or  double click",
                "Open the MR's page (Esc: back to the list)",
            ),
            ("O", "Open the selected MR in the browser"),
            ("Space", "Flag / unflag the selected MR"),
            ("Y", "Copy the git clone command"),
            ("E", "Copy the visible MRs as a Markdown checklist"),
            (
                "Ctrl+Shift+C",
                "Copy the MRs shown in the export's last format",
            ),
            ("Del", "Stop tracking the selected MR"),
            ("R", "Refresh now"),
            (
                "Ctrl+click  or  Shift+click",
                "Pick several MRs, for the bulk actions",
            ),
        ],
    ),
    (
        "Views",
        &[
            ("/  or  I", "Search (Enter on a number tracks the MR)"),
            ("F", "Show / hide the views panel"),
            ("M", "Hide / show merged MRs"),
            ("S / Shift+S", "Next sort column / reverse the order"),
            ("C", "Columns"),
            ("P", "Inspector: details / pipelines"),
            ("T", "Inspector: linked ticket"),
            ("[ / ]", "Previous / next section of a long page"),
            ("Home", "Back to the top of a long page"),
        ],
    ),
    (
        "Projects",
        &[
            ("W", "Open or add a project"),
            ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab"),
            ("Ctrl+1 … 9", "Go to that tab (9: the last)"),
            ("Ctrl+W  or  middle click", "Close the tab"),
        ],
    ),
    (
        "Elsewhere",
        &[
            ("Ctrl+K", "Command palette: pages, views, MRs and commands"),
            ("L", "Log time on the linked ticket"),
            ("G", "Statistics (Tab: next tab, W: time window)"),
            (",", "Settings"),
            ("?", "This list"),
            (
                "Esc",
                "Close a dialog / leave the statistics / unpick the MRs",
            ),
        ],
    ),
];

/// Width of the keys' column, the same in every section.
pub(super) const KEYS_WIDTH: f32 = 190.0;

/// The keys of a shortcut as keycaps: "↑ / ↓  or  k / j" gives four caps, the
/// "/" and "or" between them as plain text.
pub(super) fn keys(ui: &mut Ui, theme: &Theme, keys: &str) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (i, token) in keys.split_whitespace().enumerate() {
        // Between keys, "/" and "or" are words; first, "/" is the search key.
        if i > 0 && (token == "/" || token == "or") {
            ui.label(muted(theme, tl(token)));
            continue;
        }
        let galley = ui.painter().layout_no_wrap(
            token.to_string(),
            egui::FontId::monospace(theme.text_size - 1.5),
            theme.palette.text,
        );
        let size = Vec2::new((galley.size().x + 12.0).max(22.0), 22.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let radius = CornerRadius::same(theme.radii.small);
        ui.painter().rect_filled(rect, radius, theme.palette.panel);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme.palette.border),
            egui::StrokeKind::Inside,
        );
        // The cap's lower edge, a little thicker.
        ui.painter().hline(
            rect.x_range().shrink(2.0),
            rect.bottom() - 1.0,
            Stroke::new(1.5, theme.palette.border),
        );
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            theme.palette.text,
        );
    }
}

/// The shortcuts, by section, each a card of rows: the keys, what they do.
/// Two columns once the dialog is wide enough.
pub(super) fn help(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let height = (ui.ctx().content_rect().height() - 240.0).clamp(280.0, 640.0);
    egui::ScrollArea::vertical()
        .max_height(height)
        .show(ui, |ui| {
            body(ui, |ui| {
                let width = ui.available_width();
                if width < 740.0 {
                    for (i, (section, entries)) in SHORTCUTS.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(16.0);
                        }
                        shortcut_section(ui, &theme, section, entries);
                    }
                    return;
                }
                // Merge requests and projects on the left, the rest on the right:
                // the two columns about as long.
                let gap = 20.0;
                let column = (width - gap) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for half in [[0, 2], [1, 3]] {
                        ui.allocate_ui_with_layout(
                            Vec2::new(column, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(column);
                                for (j, i) in half.into_iter().enumerate() {
                                    if let Some((section, entries)) = SHORTCUTS.get(i) {
                                        if j > 0 {
                                            ui.add_space(16.0);
                                        }
                                        shortcut_section(ui, &theme, section, entries);
                                    }
                                }
                            },
                        );
                    }
                });
            });
        });
    footer(ui, &theme, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        keys(ui, &theme, "?");
        ui.label(muted(&theme, tl("This list")));
        buttons(ui, |ui| {
            if primary_button(ui, &theme, tl("Close")).clicked() {
                close(g);
            }
        });
    });
}

pub(super) fn shortcut_section(
    ui: &mut Ui,
    theme: &Theme,
    section: &str,
    entries: &[(&str, &str)],
) {
    caption(ui, theme, tl(section));
    ui.add_space(2.0);
    card(ui, theme, |ui| {
        for (i, (key, what)) in entries.iter().enumerate() {
            if i > 0 {
                rule(ui, theme);
            }
            egui::Frame::new()
                .inner_margin(Margin::symmetric(12, 7))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(KEYS_WIDTH, 24.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_width(KEYS_WIDTH);
                                keys(ui, theme, key);
                            },
                        );
                        let width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 24.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(width);
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new(tl(what)).color(theme.palette.text_secondary),
                                );
                            },
                        );
                    });
                });
        }
    });
}
