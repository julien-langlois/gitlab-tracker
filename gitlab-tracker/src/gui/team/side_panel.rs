//! The panel beside the page: one person's waiting list, reminders, the summary.

use super::*;

pub(super) fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    analysis: &Analysis,
    shown: Option<&str>,
    now: DateTime<Utc>,
    reminder: f64,
) {
    let theme = g.theme;
    let Some(person) = shown.and_then(|s| analysis.people.iter().find(|p| p.login == s)) else {
        board::inspector_empty(
            ui,
            &theme,
            tl("Pick someone in the list to see what waits on them."),
        );
        return;
    };
    // Who.
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                avatar(ui, &theme, &person.name, 42.0);
                let chip_w = 90.0;
                ui.allocate_ui(Vec2::new(ui.available_width() - chip_w, 42.0), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add_space(3.0);
                        ui.add(
                            egui::Label::new(semibold(&person.name, theme.text_size + 2.0))
                                .truncate(),
                        );
                        let last = match person.weeks.iter().rposition(|w| *w > 0) {
                            Some(_) => format!("@{}", person.login),
                            None => format!("@{} · {}", person.login, tl("no review lately")),
                        };
                        ui.add(egui::Label::new(muted(&theme, last)).truncate());
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    board::chip(ui, &theme, person.level.label(), person.level.tone(&theme));
                });
            });
        });
    super::super::widgets::rule(ui, &theme);
    let mut open_mr = None;
    board::section(
        ui,
        &theme,
        tl("Waiting on them"),
        &person.reviews.len().to_string(),
        |ui| {
            waiting_list(ui, &theme, person, now, reminder, &mut open_mr);
        },
    );
    board::section(
        ui,
        &theme,
        tl("Their MRs"),
        &person.mrs.len().to_string(),
        |ui| {
            if person.mrs.is_empty() {
                ui.label(muted(&theme, tl("No open MR.")));
            }
            for m in &person.mrs {
                let caption = super::super::next::caption(&m.step);
                let summary = m.step.summary();
                let detail = if summary.is_empty() {
                    caption
                } else {
                    format!("{} · {summary}", super::super::next::caption(&m.step))
                };
                if line(
                    ui,
                    &theme,
                    &m.id,
                    &m.title,
                    &detail,
                    Some(super::super::next::tone(&m.step, &theme).fg),
                ) {
                    open_mr = Some(m.id.clone());
                }
            }
        },
    );
    // Only when there is one: an empty "Helped on" says nothing worth the room.
    if !person.helped.is_empty() {
        board::section(
            ui,
            &theme,
            tl("Helped on"),
            &person.helped.len().to_string(),
            |ui| {
                for h in &person.helped {
                    let detail = format!(
                        "{} · {}",
                        name_of(&h.author),
                        tf(
                            "pushed {time} ago",
                            &[(
                                "time",
                                &board::duration_text(board::days_between(h.at, now))
                            )]
                        )
                    );
                    if line(ui, &theme, &h.id, &h.title, &detail, None) {
                        open_mr = Some(h.id.clone());
                    }
                }
            },
        );
    }
    board::section(ui, &theme, tl("Reviews"), tl("8 weeks"), |ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
        let max = person.weeks.iter().copied().max().unwrap_or(0).max(1);
        let gap = 6.0;
        let w = (rect.width() - gap * 7.0) / 8.0;
        for (i, n) in person.weeks.iter().enumerate() {
            let h = if *n == 0 {
                2.0
            } else {
                (rect.height() * *n as f32 / max as f32).max(4.0)
            };
            let bar = Rect::from_min_max(
                egui::pos2(rect.left() + i as f32 * (w + gap), rect.bottom() - h),
                egui::pos2(rect.left() + i as f32 * (w + gap) + w, rect.bottom()),
            );
            let color = if i == 7 {
                theme.palette.accent
            } else {
                theme.palette.accent.gamma_multiply(0.5)
            };
            ui.painter().rect_filled(
                bar,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                if *n == 0 { theme.palette.border } else { color },
            );
            let when = if i == 7 {
                tl("this week").to_string()
            } else {
                tn(7 - i as u64, "{n} week ago", "{n} weeks ago")
            };
            Tip::new()
                .title(tn(u64::from(*n), "{n} review", "{n} reviews"))
                .text(when)
                .show(ui.interact(
                    bar.expand2(Vec2::new(gap / 2.0, 30.0)).intersect(rect),
                    ui.scope_id().with(("week", i)),
                    Sense::hover(),
                ));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(muted(&theme, tl("8 weeks ago")).size(theme.text_size - 2.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    muted(
                        &theme,
                        tf("this week: {n}", &[("n", &person.weeks[7].to_string())]),
                    )
                    .size(theme.text_size - 2.0),
                );
            });
        });
    });
    let mut page = false;
    let mut show = false;
    let mut remind = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                page = Tip::new()
                    .text(tl("Or double click their row, or press Enter"))
                    .show(primary_button(ui, &theme, tl("Open their page")))
                    .clicked();
                show = button(ui, &theme, None, tl("Show their MRs"), false).clicked();
                remind = button(
                    ui,
                    &theme,
                    Some(super::super::icons::Icon::Copy),
                    tl("Copy a reminder"),
                    false,
                )
                .clicked();
            });
        });
    if page {
        g.open_person(&person.login);
    } else if show {
        show_mrs(g, person);
    } else if remind {
        copy_reminder(g, ui, person, now);
    } else if let Some(id) = open_mr {
        g.open_mr(&id);
    }
}

/// The MRs list, narrowed to `person`'s MRs and the reviews waiting on them.
pub(in crate::gui) fn show_mrs(g: &mut GuiApp, person: &Person) {
    let ids = person
        .mrs
        .iter()
        .map(|m| m.id.clone())
        .chain(person.reviews.iter().map(|r| r.id.clone()))
        .collect();
    g.next_focus = Some(Some(Focus {
        key: String::new(),
        label: tf("{name}: their MRs and reviews", &[("name", &person.name)]),
        ids,
    }));
    g.open_page(super::super::Page::MergeRequests);
}

/// Copies the reminder of the reviews waiting on `person`.
pub(in crate::gui) fn copy_reminder(g: &mut GuiApp, ui: &Ui, person: &Person, now: DateTime<Utc>) {
    ui.ctx()
        .copy_text(board::Links::of(&g.app).apply(&reminder_text(person, now)));
    g.app.flash(
        tl("Reminder copied: paste it in a chat or a comment"),
        false,
    );
}

/// The reviews waiting on `person`, oldest first.
pub(in crate::gui) fn waiting_list(
    ui: &mut Ui,
    theme: &Theme,
    person: &Person,
    now: DateTime<Utc>,
    reminder: f64,
    open_mr: &mut Option<String>,
) {
    if person.reviews.is_empty() {
        ui.label(muted(theme, tl("No review waiting on them.")));
        return;
    }
    let mut reviews: Vec<&Waiting> = person.reviews.iter().collect();
    reviews.sort_by_key(|r| r.since.unwrap_or(now));
    for r in reviews {
        let age = r.since.map(|s| board::days_between(s, now));
        let color = match age {
            Some(a) if a >= reminder => Some(theme.tones.red.fg),
            Some(a) if a >= reminder / 2.0 => Some(theme.tones.amber.fg),
            _ => None,
        };
        let detail = match age {
            Some(a) => format!(
                "{} · {}",
                name_of(&r.author),
                tf("asked {time} ago", &[("time", &board::duration_text(a))])
            ),
            None => name_of(&r.author),
        };
        if line(ui, theme, &r.id, &r.title, &detail, color) {
            *open_mr = Some(r.id.clone());
        }
    }
}

/// A two-line entry of the inspector: the id and the title, a detail under
/// it (its leading dot in `color`); whether it was clicked.
pub(in crate::gui) fn line(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    title: &str,
    detail: &str,
    color: Option<Color32>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(Vec2::new(6.0, 0.0)),
            CornerRadius::same(theme.radii.medium),
            theme.palette.raised,
        );
    }
    let id_w = board::paint_tag(
        ui,
        theme,
        egui::pos2(rect.left(), rect.top() + 12.0),
        &format!("!{id}"),
        board::id_tone(theme),
        true,
    );
    let left = rect.left() + id_w + 8.0;
    board::paint_text(
        ui,
        egui::pos2(left, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        title,
        fonts::medium(theme.text_size - 0.5),
        theme.palette.text,
        rect.right() - left,
    );
    let mut x = left;
    if let Some(color) = color {
        ui.painter()
            .circle_filled(egui::pos2(x + 3.0, rect.top() + 31.0), 3.0, color);
        x += 11.0;
    }
    board::paint_text(
        ui,
        egui::pos2(x, rect.top() + 31.0),
        Align2::LEFT_CENTER,
        detail,
        FontId::proportional(theme.text_size - 2.0),
        theme.palette.text_muted,
        rect.right() - x,
    );
    Tip::new()
        .mr(id, title)
        .text(detail)
        .hint(tl("Open its page"))
        .show(response.on_hover_cursor(egui::CursorIcon::PointingHand))
        .clicked()
}

/// The reminder to paste: the reviews waiting on `person`.
pub(super) fn reminder_text(person: &Person, now: DateTime<Utc>) -> String {
    let mut text = tf(
        "Hi {name}, these reviews are waiting on you:",
        &[("name", board::first_name(&person.name))],
    );
    text.push('\n');
    for r in &person.reviews {
        let age = r
            .since
            .map(|s| {
                format!(
                    " ({})",
                    tf(
                        "{time} ago",
                        &[("time", &board::duration_text(board::days_between(s, now)))]
                    )
                )
            })
            .unwrap_or_default();
        text.push_str(&format!("- !{} {}{age}\n", r.id, r.title));
    }
    if person.reviews.is_empty() {
        text = tf(
            "Hi {name}, nothing waits on you right now.",
            &[("name", board::first_name(&person.name))],
        );
    }
    text
}

/// The team's summary to paste: who waits on whom.
pub(super) fn summary(analysis: &Analysis, now: DateTime<Utc>) -> String {
    let mut text = format!("{}\n", tl("Reviews waiting"));
    let mut any = false;
    for p in &analysis.people {
        if p.reviews.is_empty() {
            continue;
        }
        any = true;
        let list = p
            .reviews
            .iter()
            .map(|r| match r.since {
                Some(s) => format!(
                    "!{} ({})",
                    r.id,
                    board::duration_text(board::days_between(s, now))
                ),
                None => format!("!{}", r.id),
            })
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!("- {}: {list}\n", p.name));
    }
    if !any {
        text.push_str(&format!("- {}\n", tl("none")));
    }
    text
}

/// The rail's badge: the reviews waiting past the reminder delay (red past
/// twice that delay, else orange), else the overloaded reviewers (amber).
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let days = PERIODS[g.team.period.min(PERIODS.len() - 1)].0;
    let analysis = g.memo.team.get(&g.app.project_id, (days, false), || {
        analyze(&g.app, now, days, false)
    });
    let reminder = g.app.config.review_reminder_hours.max(1) as f64 / 24.0;
    let tones = &g.theme.tones;
    let mut late: Vec<(&Person, &Waiting, f64)> = analysis
        .people
        .iter()
        .filter(|p| p.level != Level::Bot)
        .flat_map(|p| p.reviews.iter().map(move |r| (p, r)))
        .filter_map(|(p, r)| r.since.map(|s| (p, r, board::days_between(s, now))))
        .filter(|(_, _, d)| *d >= reminder)
        .collect();
    late.sort_by(|a, b| b.2.total_cmp(&a.2));
    if let Some((p, r, worst)) = late.first() {
        let mut hint = tn(
            late.len() as u64,
            "{n} review waits past the reminder delay",
            "{n} reviews wait past the reminder delay",
        );
        hint.push('\n');
        hint.push_str(&tf(
            "The oldest: !{id}, for {name}'s review, {time}",
            &[
                ("id", &r.id),
                ("name", board::first_name(&p.name)),
                ("time", &board::duration_text(*worst)),
            ],
        ));
        let tone = if *worst >= 2.0 * reminder {
            tones.red
        } else {
            tones.orange
        };
        return Some(Badge {
            n: late.len(),
            tone,
            hint,
        });
    }
    let over: Vec<String> = analysis
        .people
        .iter()
        .filter(|p| p.level == Level::Overloaded)
        .map(|p| board::first_name(&p.name).to_string())
        .collect();
    (!over.is_empty()).then(|| Badge {
        n: over.len(),
        tone: tones.amber,
        hint: rail_badges::listed(
            tn(
                over.len() as u64,
                "{n} reviewer overloaded",
                "{n} reviewers overloaded",
            ),
            rail_badges::names(&over, 4),
        ),
    })
}
