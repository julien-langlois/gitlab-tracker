//! A card's lines, their trends, a release's block and bar, what each counter means.

use super::*;

/// One line of a card. In the Flow card (`history`), the figures without a key
/// come from the stats' history, not the listed MRs: a clock says so.
pub(super) fn cockpit_row(
    ui: &mut Ui,
    theme: &Theme,
    row: &CockpitRow,
    counts: &Counts<'_>,
    history: bool,
) -> Option<Pick> {
    match row {
        CockpitRow::Heading { text, color, alert } => {
            let color = colors::foreground(*color, theme).unwrap_or(theme.palette.text);
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ROW_HEIGHT),
                Layout::left_to_right(Align::Center),
                |ui| {
                    let text = if *alert {
                        format!("⚠ {text}")
                    } else {
                        text.clone()
                    };
                    ui.add(
                        egui::Label::new(semibold(text, theme.text_size).color(color)).truncate(),
                    );
                },
            );
            None
        }
        CockpitRow::Metric {
            label,
            value,
            color,
            nested,
            key,
            history: of_history,
        } => {
            let zero = value == "0";
            if zero && counts.hide_zeros && key.is_some() {
                return None;
            }
            let value_color = if zero {
                theme.palette.text_muted
            } else {
                colors::foreground(*color, theme).unwrap_or(theme.palette.text)
            };
            let label_color = if zero {
                theme.palette.text_muted
            } else {
                theme.palette.text_secondary
            };
            // A count of the history lists the tracked MRs among those it counts.
            let of_history = of_history.filter(|_| counts.app.is_some());
            let clickable = key.as_ref().is_some_and(|k| counts.members.contains_key(k))
                || of_history.is_some();
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), ROW_HEIGHT),
                if clickable {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            if clickable && response.hovered() {
                ui.painter().rect_filled(
                    rect.expand2(Vec2::new(6.0, 0.0)),
                    CornerRadius::same(theme.radii.small),
                    theme.palette.raised,
                );
            }
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            // The row takes the click, its label too.
            inner.style_mut().interaction.selectable_labels = false;
            if *nested {
                inner.add_space(12.0);
            }
            if history && key.is_none() {
                let (clock, response) =
                    inner.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                super::super::icons::paint(
                    inner.painter(),
                    clock,
                    super::super::icons::Icon::Clock,
                    theme.palette.text_muted,
                );
                response.tip(tl(
                    "From the stats history since recording started, not the listed MRs",
                ));
            }
            inner.label(RichText::new(label).color(label_color));
            inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(semibold(value, theme.text_size).color(value_color));
                if let Some(key) = key {
                    trend(ui, theme, counts, key);
                }
            });
            let counted: usize = value.parse().unwrap_or(0);
            if let (Some(history), Some(app)) = (of_history, counts.app) {
                let response = response
                    .rich(
                        Tip::new()
                            .text(super::super::merged_list::tip(app, history, counted))
                            .hint(tl("Click to list them")),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                return response
                    .clicked()
                    .then(|| Pick::History(history, counted, label.clone()));
            }
            let hint = key.as_deref().and_then(explain);
            let response = match (hint, clickable) {
                (Some(hint), true) => response
                    .rich(Tip::new().text(tl(hint)).hint(tl("Click to list them")))
                    .on_hover_cursor(egui::CursorIcon::PointingHand),
                (Some(hint), false) => response.tip(tl(hint)),
                (None, true) => response
                    .tip(tl("Click to list them"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand),
                (None, false) => response,
            };
            (clickable && response.clicked())
                .then(|| Pick::Focus(key.clone().unwrap_or_default(), label.clone()))
        }
    }
}

/// "▲2" or "▼1" before a counter that moved since the list was first complete:
/// green when it moved the right way, amber otherwise.
pub(super) fn trend(ui: &mut Ui, theme: &Theme, counts: &Counts<'_>, key: &str) {
    let delta = counts.delta(key);
    if delta == 0 || key.starts_with("release:") {
        return;
    }
    let good = matches!(key, "Mergeable" | "Recently updated" | "With ticket") == (delta > 0);
    let tone = if good {
        theme.tones.green
    } else {
        theme.tones.amber
    };
    let arrow = if delta > 0 { "▲" } else { "▼" };
    let since = counts.since.unwrap_or("");
    ui.label(
        RichText::new(format!("{arrow}{}", delta.abs()))
            .size(theme.text_size - 2.5)
            .color(tone.fg),
    )
    .tip_lines(format!(
        "{}\n{}",
        tf(
            "{delta} since {time}",
            &[("delta", &format!("{delta:+}")), ("time", since)],
        ),
        tl("Green: good news · amber: to watch")
    ));
}

/// A release: its title and due date, a bar of merged / in progress / review /
/// blocked MRs, and the four counts, each a link to its MRs.
pub(super) fn release_block(
    ui: &mut Ui,
    theme: &Theme,
    r: &ReleaseView,
    counts: &Counts<'_>,
) -> Option<Pick> {
    let t = &theme.tones;
    let mut pick = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let color = if r.at_risk {
            t.red.fg
        } else {
            theme.palette.accent
        };
        let title = if r.at_risk {
            format!("⚠ {}", r.title)
        } else {
            r.title.clone()
        };
        let response = ui
            .add(
                egui::Label::new(semibold(title, theme.text_size).color(color))
                    .sense(Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .tip(tl("List its MRs"));
        if response.hovered() {
            ui.painter().hline(
                response.rect.x_range(),
                response.rect.bottom(),
                Stroke::new(1.0, color),
            );
        }
        if response.clicked() {
            pick = Some(Pick::Release(r.title.clone()));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (caption, tone) = due_caption(r, t);
            ui.add(Chip::new(&caption, tone.unwrap_or(t.gray)).pill());
        });
    });
    ui.add_space(6.0);
    let parts = [
        ("Merged", r.merged, t.purple),
        ("WIP", r.in_progress, t.blue),
        ("Review", r.waiting_review, t.amber),
        ("Blocked", r.blocked, t.red),
    ];
    stacked_bar(ui, theme, &parts);
    ui.add_space(6.0);
    // Each entry's width (dot, name, count), to start a new line before one
    // that would not fit: a nested `horizontal` only knows its width once drawn.
    let entry_width = |bucket: &str, count: usize| {
        let font = egui::FontId::proportional(theme.text_size - 1.0);
        let text = |s: String| {
            ui.painter()
                .layout_no_wrap(s, font.clone(), Color32::WHITE)
                .size()
                .x
        };
        8.0 + 4.0 + text(tl(bucket).to_string()) + 4.0 + text(count.to_string()) + 2.0
    };
    let widths: Vec<f32> = parts.iter().map(|(b, c, _)| entry_width(b, *c)).collect();
    let max_x = ui.max_rect().right();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for ((bucket, count, tone), width) in parts.into_iter().zip(widths) {
            if ui.cursor().left() + width > max_x {
                ui.end_row();
            }
            let key = crate::ui::cockpit::release_metric_key(&r.title, bucket);
            let clickable = counts.members.contains_key(&key);
            let color = if count == 0 {
                theme.palette.text_muted.gamma_multiply(0.7)
            } else {
                theme.palette.text_secondary
            };
            let response = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, tone.fg);
                    ui.label(
                        RichText::new(tl(bucket))
                            .size(theme.text_size - 1.0)
                            .color(color),
                    );
                    ui.label(
                        semibold(count.to_string(), theme.text_size - 1.0).color(if count == 0 {
                            color
                        } else {
                            tone.fg
                        }),
                    );
                })
                .response;
            if clickable {
                let response = ui
                    .interact(response.rect, ui.scope_id().with(&key), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .tip(tl("Click to list them"));
                if response.hovered() {
                    ui.painter().hline(
                        response.rect.x_range(),
                        response.rect.bottom(),
                        Stroke::new(1.0, tone.fg),
                    );
                }
                if response.clicked() {
                    pick = Some(Pick::Focus(key, format!("{} · {}", r.title, tl(bucket))));
                }
            }
        }
    });
    ui.add_space(10.0);
    pick
}

/// One bar split by `parts`, in their tones; an empty track without MRs.
pub(super) fn stacked_bar(ui: &mut Ui, theme: &Theme, parts: &[(&str, usize, Tone)]) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    let radius = CornerRadius::same(4);
    ui.painter()
        .rect_filled(rect, radius, theme.palette.border_soft);
    let total: usize = parts.iter().map(|p| p.1).sum();
    if total == 0 {
        return;
    }
    let mut x = rect.left();
    let painter = ui.painter().with_clip_rect(rect);
    for (_, count, tone) in parts {
        if *count == 0 {
            continue;
        }
        let w = rect.width() * *count as f32 / total as f32;
        let part =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w, rect.height()));
        painter.rect_filled(part, CornerRadius::ZERO, tone.fg);
        x += w;
    }
    // Rounded ends over the square parts.
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.5, theme.palette.panel),
        egui::StrokeKind::Outside,
    );
    let tip = parts.iter().fold(Tip::new(), |tip, (name, count, tone)| {
        tip.dot(tone.fg, format!("{} · {count}", tl(name)))
    });
    response.rich(tip);
}

/// What a counter means, for its tooltip.
pub(super) fn explain(key: &str) -> Option<&'static str> {
    Some(match key {
        "Open" => "Open merge requests in the list",
        "Mergeable" => "Open MRs GitLab could merge now",
        "Draft" => "Open MRs still marked as draft",
        "Blocked" => "Conflicts, rebase needed, failing CI, open discussions or changes requested",
        "Conflicts" => "Open MRs in conflict with their target branch",
        "Needs rebase" => "Open MRs GitLab asks to rebase before merging",
        "CI failing" => "Open MRs whose last pipeline failed",
        "Discussions" => "Open MRs held back by unresolved discussions",
        "Changes req." => "Open MRs whose reviewers asked for changes",
        "Needs review" => "Open MRs still missing their approvals",
        "To review (me)" => "Open MRs waiting for your review",
        "No reviewer" => "Open MRs nobody was asked to review",
        "No assignee" => "Open MRs assigned to nobody",
        "Flagged" => "MRs you starred",
        "Stale" => "Open MRs nobody touched for a while",
        "No milestone" => "Open MRs planned in no milestone",
        "Due soon" => "Open MRs whose milestone is due within the next days",
        "Overdue" => "Open MRs whose milestone is past due",
        "Behind target" => "Open MRs whose branch lags behind its target",
        "Hot threads" => "Open MRs with many comment threads",
        "With ticket" => "MRs linked to a tracker ticket",
        "No ticket" => "MRs linked to no tracker ticket",
        "Over estimate" => "MRs whose ticket took longer than estimated",
        "Complex" => "Open MRs with a high review effort",
        "Large diff" => "Open MRs with many changed lines",
        "Many commits" => "Open MRs with many commits",
        "Many files" => "Open MRs touching many files",
        "No diff stats" => "Open MRs whose diff is not loaded yet",
        "Pipeline unknown" => "Open MRs without a known pipeline",
        "CI skipped" => "Open MRs whose last pipeline was skipped",
        "Recently updated" => "MRs changed at the last refresh",
        _ => return None,
    })
}
