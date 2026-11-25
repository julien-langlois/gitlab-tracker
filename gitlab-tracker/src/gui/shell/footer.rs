//! The status bar at the bottom: auto-polling, the request estimates.

use super::*;

pub(super) fn status_bar(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    egui::Panel::bottom("status_bar")
        .exact_size(28.0)
        .frame(
            egui::Frame::new()
                .fill(theme.palette.chrome)
                .inner_margin(Margin::symmetric(14, 0)),
        )
        .show(ui, |ui| {
            let bar = ui.max_rect().expand2(Vec2::new(14.0, 0.0));
            ui.painter().hline(
                bar.x_range(),
                bar.top() + 0.5,
                Stroke::new(1.0, theme.palette.border),
            );
            ui.horizontal_centered(|ui| {
                if let Some(flash) = &g.app.flash {
                    let color = if flash.is_error {
                        theme.tones.red.fg
                    } else {
                        theme.palette.text_secondary
                    };
                    let text = egui::RichText::new(&flash.text)
                        .size(theme.text_size - 1.0)
                        .color(color);
                    ui.add(egui::Label::new(text).truncate());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    ui.label(muted(&theme, tl("? shortcuts")));
                    let mut filter = g.app.active_filter.label(&g.app.filter_defs);
                    if !g.app.filters.is_empty() {
                        filter.push_str(&format!(" + {}", g.app.filters.len()));
                    }
                    let tracked = tf(
                        "{n} tracked · View: {view}",
                        &[("n", &g.app.mrs.len().to_string()), ("view", &filter)],
                    );
                    let text = if g.app.config.hide_merged {
                        format!("{tracked} · {}", tl("merged hidden"))
                    } else {
                        tracked
                    };
                    ui.label(muted(&theme, text));
                    status_separator(ui, &theme);
                    auto_polling(ui, &theme, g.app.discovery_enabled);
                    if let Some(tracker) = &g.app.tracker {
                        request_estimate(
                            ui,
                            &theme,
                            tracker.name(),
                            g.app.estimated_tracker_calls,
                            g.app.saved_tracker_calls,
                            g.app.startup_tracker_estimate,
                        );
                    }
                    request_estimate(
                        ui,
                        &theme,
                        "GitLab",
                        g.app.estimated_gitlab_calls,
                        g.app.saved_gitlab_calls,
                        g.app.startup_gitlab_estimate,
                    );
                });
            });
        });
}

/// A thin vertical line between groups of the status bar.
pub(super) fn status_separator(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 14.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.palette.border);
}

/// "Auto-polling ON" in green, or "OFF" dimmed, as in the TUI's status bar.
/// Laid out right to left.
pub(super) fn auto_polling(ui: &mut Ui, theme: &Theme, on: bool) {
    let hint = tl(if on {
        "Each refresh also looks for the team's new MRs (discover_new_mrs)"
    } else {
        "Only the tracked MRs are refreshed; set discover_new_mrs to find the team's new ones"
    });
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (text, tone) = if on {
                (tl("ON"), theme.tones.green)
            } else {
                (tl("OFF"), theme.tones.gray)
            };
            ui.add(
                bevel_theme::Chip::new(text, tone)
                    .strong()
                    .font(fonts::semibold(theme.text_size - 2.5)),
            );
            ui.label(muted(theme, tl("Auto-polling")).size(theme.text_size - 1.0));
        })
        .response;
    response.tip(hint);
}

/// "GitLab ~12" — the requests the next refresh should send, the count in the
/// accent — with the share bulk reads and caching save ("−60%") and the estimate
/// at startup when it differs, as in the TUI. Laid out right to left.
pub(super) fn request_estimate(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    next: usize,
    saved: usize,
    startup: Option<usize>,
) {
    let size = theme.text_size - 1.0;
    let percent = crate::app::saved_percent(next, saved);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(startup) = startup.filter(|s| *s != next) {
                let text = tf("({n} at startup)", &[("n", &startup.to_string())]);
                ui.label(muted(theme, text).size(size - 0.5));
            }
            if percent > 0 {
                ui.label(
                    RichText::new(format!("−{percent}%"))
                        .size(size - 0.5)
                        .color(theme.tones.green.fg),
                );
            }
            ui.label(muted(theme, tl("req.")).size(size));
            ui.label(
                RichText::new(format!("~{next}"))
                    .font(fonts::semibold(size))
                    .color(theme.tones.teal.fg),
            );
            ui.label(muted(theme, format!("{name}:")).size(size));
        })
        .response;
    let mut hint = tf(
        "{name}: about {n} API requests at the next refresh",
        &[("name", name), ("n", &next.to_string())],
    );
    if saved > 0 {
        hint.push('\n');
        hint.push_str(&tf(
            "{saved} fewer than reading each item on its own ({n} instead of {before})",
            &[
                ("saved", &saved.to_string()),
                ("n", &next.to_string()),
                ("before", &(next + saved).to_string()),
            ],
        ));
    }
    if let Some(s) = startup {
        hint.push('\n');
        hint.push_str(&tf(
            "{n} at startup, from the saved state",
            &[("n", &s.to_string())],
        ));
    }
    response.tip_lines(hint);
}

// ── Icon rail ────────────────────────────────────────────────────────────────
