//! The counts of the day, and the rows: one MR on two lines, why it is here, since when it waits, the button that helps.

use super::*;

/// A count of one kind, in its tone.
/// Lists the MRs I merged today on the merge requests page of the tab `tab`:
/// "Merged by me" with today's days, the latest merge first.
pub(in crate::gui) fn merged_by_me(g: &mut GuiApp, tab: usize) {
    g.activate(tab);
    let today = crate::merged_range::Period::Today
        .range(chrono::Local::now().date_naive())
        .query();
    super::super::merged_list::show(g, "merged_by_me", &[("merged_between", today)], true);
}

/// A count of the day; `true` when clicked ("Merged today" alone is a link).
pub(super) fn tile(ui: &mut Ui, theme: &Theme, kind: Kind, n: usize) -> bool {
    let tone = kind.tone(theme);
    let response = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_min_width(170.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(n.to_string())
                        .font(fonts::semibold(theme.text_size + 10.0))
                        .color(if n > 0 {
                            tone.fg
                        } else {
                            theme.palette.text_secondary
                        }),
                );
                ui.label(muted(theme, kind.title()));
            });
        })
        .response;
    if kind != Kind::Merged {
        return false;
    }
    response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl(
            "Everything I merged, by merge day, in the merge requests",
        ))
        .clicked()
}

/// The project and id columns of a row.
pub(super) const PROJECT_WIDTH: f32 = 92.0;
pub(super) const ID_WIDTH: f32 = 44.0;

/// What a click on a row asked for.
pub(super) enum RowClick {
    /// The MR's page, on the tab it was last left on.
    Open,
    /// The MR's page, on this tab.
    Page(super::super::mr_page::Tab),
    /// Its reminder was copied.
    Copied,
    /// Snooze it until then.
    Snooze(Until),
    /// End its snooze.
    Wake,
}

/// One MR, on two lines as in the mockup: its title, then why it is here in the
/// section's tone; on the right since when it waits (amber when late) and the
/// button that helps (the threads, the new commits, a reminder).
pub(super) fn row(ui: &mut Ui, theme: &Theme, item: &Item, show_project: bool) -> Option<RowClick> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    // A line between rows, as in the mockup's list.
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, theme.palette.border_soft),
    );
    let tone = item.kind.tone(theme);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 11.0), Vec2::new(3.0, 28.0)),
        CornerRadius::same(2),
        tone.fg,
    );
    let mut line = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The row takes the click: its texts, selectable, would take it over them.
    line.style_mut().interaction.selectable_labels = false;
    line.spacing_mut().item_spacing.x = 10.0;
    if show_project {
        // A column of its own, so that the titles line up.
        line.allocate_ui_with_layout(
            Vec2::new(PROJECT_WIDTH, 20.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(PROJECT_WIDTH);
                ui.add(
                    Chip::new(&item.project, theme.tones.gray)
                        .font(fonts::medium(theme.text_size - 2.5)),
                );
            },
        );
    }
    line.allocate_ui_with_layout(
        Vec2::new(ID_WIDTH, 20.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(ID_WIDTH);
            ui.label(
                mono(format!("!{}", item.id), theme.text_size - 1.0)
                    .color(theme.palette.text_secondary),
            );
        },
    );
    let (name, _) = split_person(&item.author);
    avatar(&mut line, theme, name, 18.0).rich(Tip::new().person(&item.author));
    let mut click = None;
    line.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if let Some(reminder) = &item.reminder {
            if super::super::widgets::button(ui, theme, None, tl("Copy a reminder"), false)
                .tip(reminder)
                .clicked()
            {
                ui.ctx().copy_text(reminder.clone());
                click = Some(RowClick::Copied);
            }
        } else if let Some((tab, label)) = item.action {
            if super::super::widgets::button(ui, theme, None, label, false).clicked() {
                click = Some(RowClick::Page(tab));
            }
        }
        if item.state == GitlabMrState::Opened {
            if let Some(until) = snooze_menu(ui, theme, &item.id, &item.title) {
                click = Some(RowClick::Snooze(until));
            }
        }
        if let Some(since) = &item.since {
            let color = if item.late {
                theme.tones.amber.fg
            } else {
                theme.palette.text_muted
            };
            ui.label(
                RichText::new(since)
                    .size(theme.text_size - 1.5)
                    .color(color),
            )
            .tip(tl("Since when it waits"));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            super::super::widgets::centered_block(ui, ("today", &item.project, &item.id), |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&item.title).font(fonts::semibold(theme.text_size)),
                    )
                    .truncate(),
                );
                // Why: the reasons, then the status, in the section's tone.
                let (status, _) = tones::status(item.state, &item.mergeability, &theme.tones);
                let mut why: Vec<String> = item.reasons.clone();
                why.push(status.to_string());
                ui.add(
                    egui::Label::new(
                        RichText::new(why.join(" · "))
                            .size(theme.text_size - 1.5)
                            .color(tone.fg),
                    )
                    .truncate(),
                );
            });
        });
    });
    if click.is_some() {
        return click;
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(
            Tip::new()
                .mr(&item.id, &item.title)
                .hint(tl("Open its page")),
        )
        .clicked()
        .then_some(RowClick::Open)
}
