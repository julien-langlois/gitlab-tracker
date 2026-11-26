//! The panel beside the page (the period in figures, the most active MRs) and the rail's badge.

use super::*;

pub(super) fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    shown: &[&Event],
    selected: Option<&Event>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    board::section(
        ui,
        &theme,
        tl("In this period"),
        &tn(shown.len() as u64, "{n} event", "{n} events"),
        |ui| {
            let mut by_kind: BTreeMap<Kind, usize> = BTreeMap::new();
            for e in shown {
                *by_kind.entry(e.kind).or_default() += 1;
            }
            if by_kind.is_empty() {
                ui.label(muted(&theme, tl("Nothing yet.")));
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for (kind, n) in &by_kind {
                    board::chip(
                        ui,
                        &theme,
                        &format!("{n} · {}", kind.short()),
                        kind.tone(&theme),
                    );
                }
            });
        },
    );
    board::section(ui, &theme, tl("Most active"), "", |ui| {
        let mut by_mr: HashMap<(usize, &str), (usize, &str)> = HashMap::new();
        for e in shown {
            let entry = by_mr
                .entry((e.tab, e.id.as_str()))
                .or_insert((0, e.title.as_str()));
            entry.0 += 1;
        }
        let mut list: Vec<_> = by_mr.into_iter().collect();
        list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0 .1.cmp(b.0 .1)));
        if list.is_empty() {
            ui.label(muted(&theme, tl("Nothing yet.")));
        }
        let mut open = None;
        for ((tab, id), (n, title)) in list.into_iter().take(5) {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect.expand2(Vec2::new(6.0, 0.0)),
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            let w = board::paint_tag(
                ui,
                &theme,
                egui::pos2(rect.left(), rect.center().y),
                &format!("!{id}"),
                board::id_tone(&theme),
                true,
            );
            let count = n.to_string();
            board::paint_text(
                ui,
                egui::pos2(rect.right(), rect.center().y),
                Align2::RIGHT_CENTER,
                &count,
                fonts::medium(theme.text_size - 1.5),
                theme.palette.text_muted,
                30.0,
            );
            board::paint_text(
                ui,
                egui::pos2(rect.left() + w + 8.0, rect.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text,
                rect.width() - w - 44.0,
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some((tab, id.to_string()));
            }
        }
        if let Some((tab, id)) = open {
            g.open_link(tab, &id);
        }
    });
    let Some(event) = selected else {
        return;
    };
    let mut open = false;
    let mut web = false;
    board::section(
        ui,
        &theme,
        tl("Selected"),
        &board::ago(event.when, now),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                board::mr_pill(ui, &theme, &event.id, &event.title);
                if let Some((label, tone)) = &event.chip {
                    board::chip(ui, &theme, label, *tone);
                }
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(super::super::widgets::semibold(
                    &event.title,
                    theme.text_size,
                ))
                .wrap(),
            );
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("!{} {}", event.id, event.what))
                        .color(theme.palette.text_secondary),
                )
                .wrap(),
            );
            if !event.detail.is_empty() {
                ui.add(egui::Label::new(muted(&theme, &event.detail)).wrap());
            }
            ui.add(egui::Label::new(
                muted(&theme, crate::utils::moment_text(event.when)).size(theme.text_size - 2.0),
            ));
            ui.add_space(GAP);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                open = primary_button(ui, &theme, tl("Show the MR")).clicked();
                web = button(
                    ui,
                    &theme,
                    Some(super::super::icons::Icon::ExternalLink),
                    tl("Open in GitLab"),
                    false,
                )
                .clicked();
            });
        },
    );
    if web {
        board::open_url(ui, &event.url);
    }
    if open {
        g.open_link(event.tab, &event.id);
    }
}

/// The rail's badge: the events since the journal was last marked as read (the
/// last day at most when it never was), others' and not the bots'; red when a
/// pipeline failed among them, else blue.
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let floor = now - Duration::days(PERIODS[1].0);
    let since = match g.feed_seen_at {
        Some(seen) => seen.with_timezone(&Utc).max(floor),
        None => now - Duration::days(1),
    };
    let me = g.app.config.gitlab_username.as_deref();
    let (all, _) = collect(g, since);
    let new: Vec<&Event> = all
        .iter()
        .filter(|e| e.when > since)
        .filter(|e| {
            e.people
                .first()
                .is_none_or(|who| !board::is_bot(who) && me.is_none_or(|me| login(who) != me))
        })
        .collect();
    if new.is_empty() {
        return None;
    }
    let failed = new
        .iter()
        .filter(|e| e.kind == Kind::Pipeline && e.glyph == "✕")
        .count();
    let tones = &g.theme.tones;
    let mut hint = tn(
        new.len() as u64,
        "{n} new event since the journal was last read",
        "{n} new events since the journal was last read",
    );
    if failed > 0 {
        hint.push('\n');
        hint.push_str(&tn(
            failed as u64,
            "{n} failed pipeline among them",
            "{n} failed pipelines among them",
        ));
    }
    Some(Badge {
        n: new.len(),
        tone: if failed > 0 { tones.red } else { tones.blue },
        hint,
    })
}
