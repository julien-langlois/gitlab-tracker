//! The folded sections: unresolved threads, pipelines, people, planning.

use super::*;

/// The unresolved threads; a click opens one in GitLab.
pub(super) fn threads(ui: &mut Ui, theme: &Theme, mr: &TrackedMr, me: Option<&str>) {
    use crate::next_step::{thread_turn, Turn};
    let author_is_me = me.is_some_and(|me| matches_gitlab_username(&mr.author, me));
    let open: Vec<_> = mr.threads.iter().filter(|t| !t.resolved).collect();
    if open.is_empty() {
        return;
    }
    section(ui, theme, tl("Unresolved threads"), |ui| {
        for thread in open {
            let response = egui::Frame::new()
                .fill(theme.palette.content)
                .stroke(Stroke::new(1.0, theme.palette.border_soft))
                .corner_radius(egui::CornerRadius::same(theme.radii.medium))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        icons::paint(ui.painter(), rect, Icon::Comment, theme.tones.amber.fg);
                        ui.label(
                            RichText::new(format!("@{}", thread.author))
                                .font(fonts::medium(theme.text_size - 1.0)),
                        );
                        ui.label(muted(
                            theme,
                            format!(
                                "{} · {}",
                                tn(thread.notes as u64, "{n} note", "{n} notes"),
                                format_relative_date(&thread.last_note_at)
                            ),
                        ));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            let turn = thread_turn(mr, thread);
                            let mine = match turn {
                                Turn::Author => author_is_me,
                                Turn::Others => me.is_some_and(|me| thread.author == me),
                            };
                            let (text, tone) = match (turn, mine) {
                                (_, true) => (tl("Your turn"), theme.tones.orange),
                                (Turn::Author, false) => (tl("Author's turn"), theme.tones.amber),
                                (Turn::Others, false) => {
                                    (tl("Reviewer's turn"), theme.tones.purple)
                                }
                            };
                            ui.add(
                                Chip::new(text, tone).font(fonts::medium(theme.text_size - 2.5)),
                            );
                        });
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(&thread.excerpt).color(theme.palette.text_secondary),
                        )
                        .truncate(),
                    );
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .tip(tl("Open the thread in GitLab"));
            if response.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                    "{}#note_{}",
                    mr.web_url, thread.note_id
                )));
            }
        }
    });
}

/// The pipelines, drawn as in the MR page's tab, narrowed to the inspector.
pub(super) fn pipelines(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    section(ui, &theme, tl("Pipelines"), |ui| {
        super::super::mr_pipelines::compact(g, ui, mr);
    });
}

/// An initials avatar and the name, the handle on hover; me stands out, as in
/// the TUI.
pub(super) fn person(ui: &mut Ui, theme: &Theme, me: Option<&str>, display: &str) {
    let is_me = me.is_some_and(|username| matches_gitlab_username(display, username));
    let (name, handle) = split_person(display);
    // `horizontal` is at least `interact_size.y` tall (the theme's 28-point
    // control height): keep it to the avatar's, as the text rows around.
    ui.spacing_mut().interact_size.y = 18.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let response = avatar(ui, theme, name, 18.0);
        let mut text = RichText::new(name).font(fonts::medium(theme.text_size - 0.5));
        if is_me {
            text = text.color(theme.tones.blue.fg);
        }
        let response = response | ui.label(text);
        if is_me {
            ui.add(Chip::new(tl("you"), theme.tones.blue).pill());
        }
        if handle.is_some() {
            response.rich(Tip::new().person(display));
        }
    });
}

pub(in crate::gui) fn people(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let me = g.app.config.gitlab_username.clone();
    let person = |ui: &mut Ui, display: &str| person(ui, &theme, me.as_deref(), display);
    section(ui, &theme, tl("People"), |ui| {
        key_values(ui, &theme, |rows| {
            rows.row(tl("Author"), |ui| person(ui, &mr.author));
            rows.row(tl("Assignee"), |ui| match &mr.assignee {
                Some(assignee) => person(ui, assignee),
                None => {
                    ui.label(muted(&theme, "—"));
                }
            });
            rows.row(tl("Reviewers"), |ui| {
                if mr.reviewers.is_empty() {
                    ui.label(muted(&theme, tl("None")));
                    return;
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for reviewer in &mr.reviewers {
                        person(ui, reviewer);
                    }
                });
            });
        });
    });
}

pub(in crate::gui) fn planning(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    section(ui, theme, tl("Planning"), |ui| planning_body(ui, theme, mr));
}

/// The milestone, its due date and description.
pub(in crate::gui) fn planning_body(ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
    let tones = &theme.tones;
    {
        key_values(ui, theme, |rows| {
            rows.row(tl("Milestone"), |ui| match &mr.milestone {
                Some(milestone) => {
                    ui.label(RichText::new(milestone).color(theme.palette.accent));
                }
                None => {
                    ui.label(muted(theme, "—"));
                }
            });
            rows.row(tl("Due"), |ui| match mr.milestone_due_date.as_deref() {
                None | Some("") => {
                    ui.label(muted(theme, tl("Not set")));
                }
                Some(date) => {
                    // Red when past, amber within a week (YYYY-MM-DD compares as text).
                    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let soon = (chrono::Utc::now() + chrono::Duration::days(7))
                        .format("%Y-%m-%d")
                        .to_string();
                    let color = if date < today.as_str() {
                        tones.red.fg
                    } else if date <= soon.as_str() {
                        tones.amber.fg
                    } else {
                        tones.green.fg
                    };
                    let relative = format_relative_date(&format!("{date}T00:00:00Z"));
                    let shown = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                        .map_or_else(|_| date.to_string(), crate::utils::day_text);
                    ui.label(RichText::new(format!("{shown} ({relative})")).color(color));
                }
            });
        });
        if let Some(description) = mr
            .milestone_description
            .as_deref()
            .filter(|d| !d.is_empty())
        {
            // A blank line between the dates and the description.
            ui.add_space(theme.text_size);
            ui.label(muted(theme, description));
        }
    }
}
