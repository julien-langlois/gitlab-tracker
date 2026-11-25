//! The linked ticket as a card, and its time log.

use super::*;

pub(in crate::gui) fn ticket(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let provider = g.app.tracker.as_ref().map(|t| t.name());
    super::super::widgets::rule(ui, &theme);
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                super::super::widgets::section_title(ui, &theme, tl("Tracker"));
                if let Some(provider) = provider {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(muted(&theme, provider));
                    });
                }
            });
            let Some(ticket) = &mr.linked_ticket else {
                ui.label(muted(
                    &theme,
                    tl("No ticket found in the MR's title, branch or description."),
                ));
                return;
            };
            ticket_card(ui, &theme, ticket);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if g.app.tracker.is_some()
                    && button(ui, &theme, Some(Icon::Plus), tl("Log time…"), true)
                        .tip(tl("Log time on this ticket (L)"))
                        .clicked()
                {
                    g.pending.push(Action::OpenLogTime);
                }
                if button(
                    ui,
                    &theme,
                    Some(Icon::ExternalLink),
                    tl("Open ticket"),
                    false,
                )
                .tip(tl("Open the ticket in the browser (T)"))
                .clicked()
                {
                    g.pending.push(Action::OpenSelectedTicket);
                }
            });
            if g.app.tracker.is_some() {
                let open = egui::CollapsingHeader::new(muted(&theme, tl("Time log")))
                    .id_salt("time_log")
                    .show(ui, |ui| time_log(g, ui, &ticket.id))
                    .fully_open();
                // The entries are fetched while the log is open, as in the TUI.
                g.app.tracker_view = if open {
                    TrackerView::TimeLog
                } else {
                    TrackerView::TicketInfo
                };
            }
        });
}

/// The ticket as a card: id and status, subject, progress and time, details.
pub fn ticket_card(ui: &mut Ui, theme: &Theme, ticket: &gitlab_tracker_core::LinkedTicket) {
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    mono(format!("#{}", ticket.id), theme.text_size - 1.0)
                        .color(theme.palette.accent),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.add(Chip::new(
                        &ticket.status,
                        tones::ticket_status(&ticket.status, &theme.tones),
                    ));
                    if let Some(kind) = &ticket.tracker_type {
                        ui.add(Chip::new(kind, theme.tones.gray));
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(semibold(&ticket.subject, theme.text_size));
            let fraction = match (ticket.done_ratio, ticket.time_spent, ticket.time_estimate) {
                (Some(done), _, _) => Some(done as f32 / 100.0),
                (None, Some(spent), Some(estimate)) if estimate > 0 => {
                    Some(spent as f32 / estimate as f32)
                }
                _ => None,
            };
            let time = match (ticket.time_spent, ticket.time_estimate) {
                (None, None) => None,
                (spent, estimate) => Some(format!(
                    "{} / {}",
                    spent.map_or("—".into(), duration),
                    estimate.map_or("—".into(), duration)
                )),
            };
            if fraction.is_some() || time.is_some() {
                ui.add_space(12.0);
                let over = matches!(
                    (ticket.time_spent, ticket.time_estimate),
                    (Some(s), Some(e)) if e > 0 && s > e
                );
                let color = if over {
                    theme.tones.red.fg
                } else {
                    theme.palette.accent
                };
                ui.horizontal(|ui| {
                    let size = theme.text_size - 1.5;
                    if let Some(time) = &time {
                        ui.label(muted(theme, tl("Spent / estimated")).size(size));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(time)
                                    .font(fonts::medium(size))
                                    .color(if over { color } else { theme.palette.text }),
                            );
                        });
                    } else if let Some(fraction) = fraction {
                        ui.label(muted(theme, tl("Done")).size(size));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{:.0}%", fraction * 100.0))
                                    .font(fonts::medium(size)),
                            );
                        });
                    }
                });
                if let Some(fraction) = fraction {
                    ui.add_space(6.0);
                    bar(ui, theme, fraction, color, 5.0);
                }
            }
            let details: Vec<(&str, &String)> = [
                ("Priority", ticket.priority.as_ref()),
                ("Version", ticket.version.as_ref()),
                ("Assignee", ticket.assignee.as_ref()),
            ]
            .into_iter()
            .filter_map(|(caption, value)| value.map(|v| (caption, v)))
            .collect();
            if !details.is_empty() {
                ui.add_space(12.0);
                super::super::widgets::rule(ui, theme);
                ui.add_space(10.0);
                key_values(ui, theme, |rows| {
                    for (caption, value) in details {
                        rows.row(tl(caption), |ui| {
                            if caption == "Assignee" {
                                ui.spacing_mut().interact_size.y = 18.0;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    avatar(ui, theme, value, 18.0);
                                    ui.label(value);
                                });
                            } else {
                                ui.label(value);
                            }
                        });
                    }
                });
            }
        });
}

/// "1h 30m" for a number of seconds.
pub fn duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

pub(super) fn time_log(g: &GuiApp, ui: &mut Ui, ticket_id: &str) {
    let theme = g.theme;
    match g.app.time_entries.get(ticket_id) {
        None | Some(TimeLogState::Loading) => {
            ui.horizontal(|ui| {
                spinner(
                    ui,
                    ui.spacing().interact_size.y,
                    ui.visuals().strong_text_color(),
                );
                ui.label(muted(&theme, tl("Loading the time entries…")));
            });
        }
        Some(TimeLogState::Failed(error)) => {
            ui.label(RichText::new(error).color(theme.tones.red.fg));
        }
        Some(TimeLogState::Loaded(entries)) if entries.is_empty() => {
            ui.label(muted(&theme, tl("No time logged yet.")));
        }
        Some(TimeLogState::Loaded(entries)) => {
            let total: f32 = entries.iter().map(|e| e.hours).sum();
            ui.label(muted(
                &theme,
                format!(
                    "{total:.2} h · {}",
                    tn(entries.len() as u64, "{n} entry", "{n} entries")
                ),
            ));
            for entry in entries {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        mono(&entry.spent_on, theme.text_size - 1.5)
                            .color(theme.palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!("{:.2} h", entry.hours))
                            .font(fonts::medium(theme.text_size - 1.0)),
                    );
                    ui.label(muted(
                        &theme,
                        format!("{} · {}", entry.user, entry.activity.name),
                    ));
                });
                if !entry.comment.trim().is_empty() {
                    ui.label(
                        RichText::new(entry.comment.trim()).color(theme.palette.text_secondary),
                    );
                }
            }
        }
    }
}
