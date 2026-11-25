//! The inspector's top: the MR's header, byline, branch route, ticket shortcut.

use super::*;

pub(super) fn header(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let tones = &theme.tones;
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    mono(format!("!{}", mr.id), theme.text_size - 1.0)
                        .color(theme.palette.text_secondary),
                );
                let (label, tone) = tones::status(mr.state, &mr.mergeability, tones);
                badge(ui, &theme, label, tone);
                let (label, tone) = activity(&g.app, mr.updated_at.as_deref(), &theme);
                ui.spacing_mut().item_spacing.x = 5.0;
                dot(ui, tone.fg, 6.0);
                ui.label(
                    RichText::new(label)
                        .size(theme.text_size - 1.5)
                        .color(tone.fg),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let style = theme.icon_button();
                    let (icon, hint) = if mr.flagged {
                        (Icon::Star, tl("Unflag (Space)"))
                    } else {
                        (Icon::StarOutline, tl("Flag (Space)"))
                    };
                    let response =
                        bevel_widgets::buttons::IconButton::new(hint, Vec2::splat(24.0), &style)
                            .show(ui, |ui, rect, tint| {
                                let color = if mr.flagged { tones.amber.fg } else { tint };
                                icons::paint(ui.painter(), rect.shrink(4.0), icon, color);
                            })
                            .tip(hint);
                    if response.clicked() {
                        g.pending.push(Action::ToggleFlagSelected);
                    }
                });
            });
            ui.label(semibold(&mr.title, theme.text_size + 2.0));
            byline(ui, &theme, mr);
            branch_route(ui, &theme, &mr.source_branch, &mr.target_branch);
            if let MrStatus::Error(message) = &mr.status {
                ui.label(
                    RichText::new(format!("{} — {message}", tl("Last fetch failed")))
                        .color(tones.red.fg),
                );
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let clone = crate::utils::git_clone_command(&mr.web_url, &mr.source_branch);
                let outlined = theme.outlined_icon_button();
                let height = theme.spacing.control_height + 2.0;
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if super::super::widgets::icon_button_tip(
                        ui,
                        tl("Copy the git clone command (Y)"),
                        Icon::Copy,
                        &outlined,
                        height,
                        Tip::plain(tl("Copy the git clone command (Y)")).mono(clone),
                    )
                    .clicked()
                    {
                        g.pending.push(Action::YankCloneCommand);
                    }
                    // Its page, where the inspector is too narrow (not on the page itself).
                    if g.page() != Page::Mr
                        && icon_button(
                            ui,
                            tl("Open the MR's page (Enter, or a double click on its row)"),
                            Icon::Maximize,
                            &outlined,
                            height,
                        )
                        .clicked()
                    {
                        g.open_page(Page::Mr);
                    }
                    ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                        let response = full_width_button(ui, &theme, tl("Open in GitLab"));
                        if response.tip(tl("Open in the browser (O)")).clicked() {
                            g.pending.push(Action::OpenSelectedMr);
                        }
                    });
                });
            });
            if let Some(ticket) = &mr.linked_ticket {
                if ticket_shortcut(ui, &theme, ticket).clicked() {
                    g.pending.push(Action::OpenSelectedTicket);
                }
            }
        });
}

/// The MR's ticket as a one-line button: id and subject, shortened to fit.
pub(super) fn ticket_shortcut(
    ui: &mut Ui,
    theme: &Theme,
    ticket: &gitlab_tracker_core::LinkedTicket,
) -> egui::Response {
    let pad = 10.0;
    let icon = 13.0;
    let height = theme.spacing.control_height + 2.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let painter = ui.painter();
    let fill = if response.hovered() {
        theme.palette.raised
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect(
        rect,
        egui::CornerRadius::same(theme.radii.medium),
        fill,
        Stroke::new(1.0, theme.palette.border),
        egui::StrokeKind::Inside,
    );
    let elided = |text: String, font: egui::FontId, color: egui::Color32, max: f32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(max.max(0.0));
        painter.layout_job(job)
    };
    let y = rect.center().y;
    let room = rect.width() - 2.0 * pad - icon - 2.0 * 8.0;
    let id = elided(
        format!("#{}", ticket.id),
        egui::FontId::monospace(theme.text_size - 1.0),
        theme.palette.accent,
        room / 2.0,
    );
    let id_width = id.size().x;
    let subject = elided(
        ticket.subject.clone(),
        egui::FontId::proportional(theme.text_size - 0.5),
        theme.palette.text_secondary,
        room - id_width,
    );
    let left = rect.left() + pad;
    painter.galley(
        egui::pos2(left, y - id.size().y / 2.0),
        id,
        theme.palette.accent,
    );
    painter.galley(
        egui::pos2(left + id_width + 8.0, y - subject.size().y / 2.0),
        subject,
        theme.palette.text_secondary,
    );
    icons::paint(
        painter,
        egui::Rect::from_center_size(
            egui::pos2(rect.right() - pad - icon / 2.0, y),
            Vec2::splat(icon),
        ),
        Icon::ExternalLink,
        theme.palette.text_secondary,
    );
    response
        .tip(tl("Open the ticket in the browser (T)"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The author's avatar and name, then when the MR last moved.
pub(super) fn byline(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let size = theme.text_size - 1.0;
        let (name, handle) = split_person(&mr.author);
        let response = avatar(ui, theme, name, 20.0);
        if handle.is_some() {
            response.rich(Tip::new().person(&mr.author));
        }
        ui.label(
            RichText::new(name)
                .font(fonts::medium(size))
                .color(theme.palette.text_secondary),
        );
        let Some(updated) = mr.updated_at.as_deref() else {
            return;
        };
        ui.label(
            RichText::new("·")
                .size(size)
                .color(theme.palette.text_muted),
        );
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
        icons::paint(ui.painter(), rect, Icon::Clock, theme.palette.text_muted);
        ui.label(
            RichText::new(tf(
                "updated {when}",
                &[("when", &format_relative_date(updated))],
            ))
            .size(size)
            .color(theme.palette.text_muted),
        );
    });
}

/// Where the MR goes: the source branch, an arrow, the target branch (accented),
/// in a soft box. Each name is shortened with an ellipsis to fit, in full on hover.
pub(super) fn branch_route(ui: &mut Ui, theme: &Theme, source: &str, target: &str) {
    let font = egui::FontId::monospace(theme.text_size - 1.5);
    let pad = 8.0;
    let pill_height = 22.0;
    let arrow_width = 26.0;
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(egui::CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (rect, response) = ui
                .allocate_exact_size(Vec2::new(ui.available_width(), pill_height), Sense::hover());
            let painter = ui.painter();
            let elided = |text: &str, max: f32, color: egui::Color32| {
                let mut job =
                    egui::text::LayoutJob::simple_singleline(text.to_owned(), font.clone(), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(max.max(0.0));
                painter.layout_job(job)
            };
            // The target is usually short: it keeps its width up to half the row.
            let room = rect.width() - arrow_width - 4.0 * pad;
            let target_galley = elided(target, room / 2.0, theme.tones.blue.fg);
            let source_galley = elided(
                source,
                room - target_galley.size().x,
                theme.palette.text_secondary,
            );
            let pill = |left: f32, galley: std::sync::Arc<egui::Galley>, fill, stroke| {
                let pill = egui::Rect::from_min_size(
                    egui::pos2(left, rect.top()),
                    Vec2::new(galley.size().x + 2.0 * pad, pill_height),
                );
                painter.rect(
                    pill,
                    egui::CornerRadius::same(theme.radii.small),
                    fill,
                    Stroke::new(1.0, stroke),
                    egui::StrokeKind::Inside,
                );
                let text_pos =
                    egui::pos2(pill.left() + pad, pill.center().y - galley.size().y / 2.0);
                painter.galley(text_pos, galley, theme.palette.text);
                pill
            };
            let source_pill = pill(
                rect.left(),
                source_galley,
                theme.palette.panel,
                theme.palette.border_soft,
            );
            let arrow = egui::Rect::from_min_size(
                egui::pos2(source_pill.right(), rect.top()),
                Vec2::new(arrow_width, pill_height),
            );
            let (y, x0, x1) = (arrow.center().y, arrow.left() + 7.0, arrow.right() - 7.0);
            let stroke = Stroke::new(1.5, theme.palette.text_muted);
            painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
            painter.line_segment([egui::pos2(x1 - 4.0, y - 4.0), egui::pos2(x1, y)], stroke);
            painter.line_segment([egui::pos2(x1 - 4.0, y + 4.0), egui::pos2(x1, y)], stroke);
            pill(
                arrow.right(),
                target_galley,
                theme.tones.blue.bg,
                theme.tones.blue.fg.gamma_multiply(0.35),
            );
            response.rich(Tip::new().mono(format!("{source} → {target}")));
        });
}

/// The primary button stretched to the width left.
pub(super) fn full_width_button(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    let width = ui.available_width();
    let height = theme.spacing.control_height + 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let fill = if response.hovered() {
        theme.palette.accent_fill.gamma_multiply(1.15)
    } else {
        theme.palette.accent_fill
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(theme.radii.medium), fill);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        fonts::medium(theme.text_size - 0.5),
        egui::Color32::WHITE,
    );
    let total = 13.0 + 6.0 + galley.size().x;
    let left = rect.center().x - total / 2.0;
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(egui::pos2(left + 6.5, rect.center().y), Vec2::splat(13.0)),
        Icon::ExternalLink,
        egui::Color32::WHITE,
    );
    ui.painter().galley(
        egui::pos2(left + 19.0, rect.center().y - galley.size().y / 2.0),
        galley,
        egui::Color32::WHITE,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
