//! The feed: what changed while the app was closed, then during the session, newest first.

use super::*;

/// A line of the feed: about which MR (and tab), what, when.
pub(super) struct Change {
    pub(super) tab: usize,
    pub(super) project: String,
    pub(super) id: String,
    pub(super) title: String,
    pub(super) text: String,
    pub(super) color: egui::Color32,
    /// `None`: found at startup (while the app was closed).
    pub(super) when: Option<chrono::DateTime<chrono::Local>>,
}

/// What changed: while the app was closed (each tab's startup), then during the
/// session (the bell's notifications), newest first.
pub(super) fn feed(g: &GuiApp) -> Vec<Change> {
    use crate::app::ChangeKind;
    let t = &g.theme.tones;
    let mut session = Vec::new();
    let mut startup = Vec::new();
    for (index, slot) in g.tabs.iter().enumerate() {
        let (app, toasts) = match &slot.state {
            SlotState::Active => (&g.app, &g.toasts),
            SlotState::Parked(tab) => (&tab.app, &tab.toasts),
            _ => continue,
        };
        let project = app
            .project_name
            .clone()
            .unwrap_or_else(|| app.project_id.clone());
        for past in &toasts.history {
            if g.feed_seen_at.is_some_and(|seen| past.when <= seen) {
                continue;
            }
            session.push(Change {
                tab: index,
                project: project.clone(),
                id: past.id.clone(),
                title: past.title.clone(),
                text: past.text.clone(),
                color: past.tone.fg,
                when: Some(past.when),
            });
        }
        for change in &app.last_changes {
            let (text, color) = match &change.kind {
                ChangeKind::Merged => (tl("merged").to_string(), t.purple.fg),
                ChangeKind::Closed => (tl("closed").to_string(), t.gray.fg),
                ChangeKind::Arrived(branch) => {
                    (tf("reached {branch}", &[("branch", branch)]), t.green.fg)
                }
                ChangeKind::CiFailing => (tl("pipeline failed").to_string(), t.red.fg),
            };
            startup.push(Change {
                tab: index,
                project: project.clone(),
                id: change.id.clone(),
                title: change.title.clone(),
                text: format!("!{} {text}", change.id),
                color,
                when: None,
            });
        }
    }
    session.sort_by_key(|c| std::cmp::Reverse(c.when));
    session.extend(startup);
    session
}

/// The feed in a card; the MR clicked, with its tab.
/// What a click in the feed asked for.
pub(super) enum FeedClick {
    Open(usize, String),
    Seen,
}

pub(super) fn feed_card(
    ui: &mut Ui,
    theme: &Theme,
    changes: &[Change],
    several: bool,
) -> Option<FeedClick> {
    let mut picked = None;
    card(ui, theme, tl("Since your last visit"), |ui| {
        if changes.is_empty() {
            ui.label(muted(
                theme,
                tl("Nothing changed on your tracked MRs since the app was last opened."),
            ));
            return;
        }
        let mut heading = None;
        for change in changes {
            let group = if change.when.is_some() {
                tl("This session")
            } else {
                tl("While the app was closed")
            };
            if heading != Some(group) {
                if heading.is_some() {
                    ui.add_space(6.0);
                }
                ui.label(muted(theme, group).size(theme.text_size - 2.0));
                ui.add_space(2.0);
                heading = Some(group);
            }
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            ui.painter().circle_filled(
                egui::pos2(rect.left() + 8.0, rect.top() + 12.0),
                4.0,
                change.color,
            );
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(
                        rect.shrink2(Vec2::new(0.0, 3.0))
                            .with_min_x(rect.left() + 20.0),
                    )
                    .layout(Layout::top_down(Align::Min)),
            );
            inner.style_mut().interaction.selectable_labels = false;
            inner.spacing_mut().item_spacing.y = 1.0;
            inner.add(
                egui::Label::new(
                    RichText::new(&change.text).font(fonts::medium(theme.text_size - 1.0)),
                )
                .truncate(),
            );
            let mut detail = change.title.clone();
            if several {
                detail = format!("{} · {detail}", change.project);
            }
            if let Some(when) = change.when {
                detail = format!("{} · {detail}", when.format("%H:%M"));
            }
            inner.add(
                egui::Label::new(muted(theme, detail).size(theme.text_size - 2.0))
                    .truncate()
                    .show_tooltip_when_elided(false),
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .rich(
                    Tip::new()
                        .mr(&change.id, &change.title)
                        .text(&change.text)
                        .hint(tl("Open its page")),
                )
                .clicked()
            {
                picked = Some(FeedClick::Open(change.tab, change.id.clone()));
            }
        }
        ui.add_space(8.0);
        if super::super::widgets::button(ui, theme, None, tl("Mark all as seen"), false).clicked() {
            picked = Some(FeedClick::Seen);
        }
    });
    picked
}
