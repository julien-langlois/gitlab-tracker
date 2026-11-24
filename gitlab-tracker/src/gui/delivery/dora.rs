//! The undo card (reverts and hotfixes), the trend arrows and the DORA figures.

use super::*;

/// The reverts and hotfixes merged in the period: what each undoes or patches,
/// how long after, and how far the faulty MR had gone.
pub(super) fn undo_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    hotfix_branches: &str,
    selected: Option<&str>,
    picked: &mut Option<String>,
) {
    let counted = d.undos.iter().filter(|u| u.counted).count();
    let note = if d.undos.is_empty() {
        tl("what had to be undone or patched").to_string()
    } else {
        let note = tn(
            counted as u64,
            "{n} counted in the change failure rate",
            "{n} counted in the change failure rate",
        );
        match d.dora.failed_deploys {
            0 => note,
            n => format!(
                "{note} · {}",
                tn(
                    n as u64,
                    "with {n} failed deployment",
                    "with {n} failed deployments"
                )
            ),
        }
    };
    board::card(ui, theme, tl("Reverts and hotfixes"), &note, 0.0, |ui| {
        if d.undos.is_empty() {
            board::empty(ui, theme, tl("No revert or hotfix merged in the period."));
        }
        let font = FontId::proportional(theme.text_size - 1.0);
        let small = FontId::proportional(theme.text_size - 2.0);
        // The columns, right to left; the same in the heading and the rows.
        let (reached_w, after_w, undoes_w) = (120.0, 70.0, 170.0);
        if !d.undos.is_empty() {
            let (head, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
            let right = head.right() - 8.0;
            let y = head.center().y;
            let caption = |ui: &Ui, x: f32, align: Align2, text: &str, width: f32| {
                board::paint_text(
                    ui,
                    egui::pos2(x, y),
                    align,
                    &text.to_uppercase(),
                    FontId::proportional(theme.text_size - 3.0),
                    theme.palette.text_muted,
                    width,
                );
            };
            caption(ui, right, Align2::RIGHT_CENTER, tl("How far"), reached_w);
            caption(
                ui,
                right - reached_w - 8.0,
                Align2::RIGHT_CENTER,
                tl("After"),
                after_w,
            );
            caption(
                ui,
                right - reached_w - after_w - 16.0 - undoes_w,
                Align2::LEFT_CENTER,
                tl("Undoes"),
                undoes_w,
            );
        }
        for undo in d.undos.iter().take(12) {
            let (rect, response) = board::row(ui, theme, 34.0, selected == Some(undo.id.as_str()));
            let y = rect.center().y;
            let (kind, tone) = match undo.kind {
                UndoKind::Revert => (tl("Revert"), theme.tones.red),
                UndoKind::Hotfix => (tl("Hotfix"), theme.tones.amber),
            };
            // The kind, in a column of its own so the ids line up.
            board::paint_tag(
                ui,
                theme,
                egui::pos2(rect.left() + 6.0, y),
                &kind.to_uppercase(),
                tone,
                false,
            );
            let mut x = rect.left() + 84.0;
            x += board::paint_tag(
                ui,
                theme,
                egui::pos2(x, y),
                &format!("!{}", undo.id),
                board::id_tone(theme),
                true,
            ) + 10.0;
            // Right to left: where the faulty MR got, after how long, what it was.
            let right = rect.right() - 8.0;
            // On production, users saw it: in red.
            let (reached, reached_color) = match &undo.reached {
                Some(branch) if *branch == d.production => (branch.clone(), theme.tones.red.fg),
                Some(branch) => (branch.clone(), theme.palette.text_secondary),
                None => (board::dash().to_string(), theme.palette.text_muted),
            };
            board::paint_text(
                ui,
                egui::pos2(right, y),
                Align2::RIGHT_CENTER,
                &reached,
                small.clone(),
                reached_color,
                reached_w,
            );
            let after = undo
                .after
                .map_or_else(|| board::dash().to_string(), board::duration_text);
            board::paint_text(
                ui,
                egui::pos2(right - reached_w - 8.0, y),
                Align2::RIGHT_CENTER,
                &after,
                font.clone(),
                theme.palette.text_secondary,
                after_w,
            );
            let undoes_left = right - reached_w - after_w - 16.0 - undoes_w;
            let mut undone_click = None;
            match &undo.undoes {
                Some((id, title, found)) => {
                    let w = board::paint_tag(
                        ui,
                        theme,
                        egui::pos2(undoes_left, y),
                        &format!("!{id}"),
                        board::id_tone(theme),
                        true,
                    );
                    let tag =
                        Rect::from_min_size(egui::pos2(undoes_left, y - 10.0), Vec2::new(w, 20.0));
                    let hint = match found {
                        Found::Description => tl("named in its description"),
                        Found::Commit => tl("one of its commits is reverted"),
                        Found::Title => tl("its title is quoted"),
                        Found::SameFiles => tl(
                            "a guess: the latest MR merged before it that changed the same files",
                        ),
                    };
                    let r = ui
                        .interact(
                            tag,
                            ui.make_persistent_id(("undoes", &undo.id)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .rich(Tip::new().mr(id, title).text(hint));
                    if r.clicked() {
                        undone_click = Some(id.clone());
                    }
                    if *found == Found::SameFiles {
                        board::paint_text(
                            ui,
                            egui::pos2(undoes_left + w + 6.0, y),
                            Align2::LEFT_CENTER,
                            tl("guess"),
                            small.clone(),
                            theme.palette.text_muted,
                            undoes_w - w - 6.0,
                        );
                    }
                }
                None => {
                    board::paint_text(
                        ui,
                        egui::pos2(undoes_left, y),
                        Align2::LEFT_CENTER,
                        match undo.kind {
                            UndoKind::Revert => tl("not found"),
                            UndoKind::Hotfix => tl("no guess"),
                        },
                        small.clone(),
                        theme.palette.text_muted,
                        undoes_w,
                    );
                }
            }
            board::paint_text(
                ui,
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                &undo.title,
                font.clone(),
                theme.palette.text,
                (undoes_left - x - 12.0).max(0.0),
            );
            if undone_click.is_some() {
                *picked = undone_click;
            } else if response.clicked() {
                *picked = Some(undo.id.clone());
            }
        }
        ui.add_space(6.0);
        let patterns = if hotfix_branches.trim().is_empty() {
            tl("Hotfixes: the MRs labelled hotfix (no hotfix branch set).").to_string()
        } else {
            tf(
                "Hotfixes: the MRs labelled hotfix or from {branches}. The project settings change the branches.",
                &[("branches", hotfix_branches.trim())],
            )
        };
        ui.label(muted(theme, patterns).size(theme.text_size - 1.5));
    });
}

/// "+2 on the period before" in green, or worse in red, `better_up` telling
/// which way is better.
pub(super) fn trend(
    now: f64,
    before: f64,
    better_up: bool,
    text: String,
    theme: &Theme,
) -> (String, Option<Color32>) {
    if (now - before).abs() < f64::EPSILON {
        return (tl("as the period before").to_string(), None);
    }
    let better = (now > before) == better_up;
    (
        text,
        Some(if better {
            theme.tones.green.fg
        } else {
            theme.tones.red.fg
        }),
    )
}

pub(super) fn dora_figures(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
) {
    let dora = &d.dora;
    // Compared: the period before says it all, under each figure.
    let comparing = before.is_some();
    let level_caption = |level: Option<Level>, detail: String| -> (String, Option<Color32>) {
        match level {
            Some(l) => (
                if detail.is_empty() {
                    l.label().to_string()
                } else {
                    format!("{} · {detail}", l.label())
                },
                Some(l.tone(theme).fg),
            ),
            None => (detail, None),
        }
    };
    let freq = if dora.per_week >= 1.0 || dora.per_week == 0.0 {
        tf("{n} / week", &[("n", &format!("{:.1}", dora.per_week))])
    } else {
        tf(
            "{n} / month",
            &[("n", &format!("{:.1}", dora.per_week * 30.0 / 7.0))],
        )
    };
    let shipped_before = dora.deploys_before;
    let (freq_caption, freq_color) = if dora.has_deployments {
        let delta = dora.deploys as i64 - shipped_before as i64;
        let detail = tn(dora.deploys as u64, "{n} deployment", "{n} deployments");
        if delta == 0 || comparing {
            level_caption(Some(Level::of_frequency(dora.per_week)), detail)
        } else {
            level_caption(
                Some(Level::of_frequency(dora.per_week)),
                format!("{detail} ({delta:+})"),
            )
        }
    } else {
        level_caption(
            (dora.per_week > 0.0).then(|| Level::of_frequency(dora.per_week)),
            tl("MRs reaching production").to_string(),
        )
    };
    let (lead_caption, lead_color) = match (dora.lead, dora.lead_before) {
        (Some(a), Some(b)) if (a - b).abs() >= 0.05 && !comparing => {
            let (text, color) = trend(
                a,
                b,
                false,
                tf("{d} before", &[("d", &board::duration_text(b))]),
                theme,
            );
            (
                format!("{} · {text}", Level::of_lead(a).label()),
                color.or(Some(Level::of_lead(a).tone(theme).fg)),
            )
        }
        (Some(a), _) => level_caption(Some(Level::of_lead(a)), tl("from the first commit").into()),
        _ => (tl("nothing on production in the period").to_string(), None),
    };
    let failures = dora.failed_deploys + dora.reverts.len() + dora.hotfixes.len();
    let (cfr_caption, cfr_color) = match dora.failure_rate {
        Some(rate) => level_caption(
            Some(Level::of_failure_rate(rate)),
            tf(
                "{n} of {total}",
                &[
                    ("n", &failures.to_string()),
                    ("total", &dora.changes.to_string()),
                ],
            ),
        ),
        None => (tl("no change in the period").to_string(), None),
    };
    let (restore_caption, restore_color) = match dora.restore {
        Some(days) => level_caption(
            Some(Level::of_restore(days)),
            tn(dora.restores as u64, "{n} incident", "{n} incidents"),
        ),
        None if failures == 0 => (tl("no failure to recover from").to_string(), None),
        None => (tl("not restored yet").to_string(), Some(theme.tones.red.fg)),
    };
    let figures = [
        Figure {
            label: tl("Deployment frequency").into(),
            value: freq,
            value_color: None,
            caption: freq_caption,
            caption_color: freq_color,
        },
        Figure {
            label: tl("Lead time for changes").into(),
            value: dora
                .lead
                .map_or_else(|| board::dash().to_string(), board::duration_text),
            value_color: None,
            caption: lead_caption,
            caption_color: lead_color,
        },
        Figure {
            label: tl("Change failure rate").into(),
            value: dora
                .failure_rate
                .map_or_else(|| board::dash().to_string(), board::percent),
            value_color: None,
            caption: cfr_caption,
            caption_color: cfr_color,
        },
        Figure {
            label: tl("Time to restore").into(),
            value: dora
                .restore
                .map_or_else(|| board::dash().to_string(), board::duration_text),
            value_color: None,
            caption: restore_caption,
            caption_color: restore_color,
        },
    ];
    let Some(b) = before.map(|b| &b.dora) else {
        board::figures(ui, theme, &figures);
        return;
    };
    let freq_text = |per_week: f64| {
        if per_week >= 1.0 || per_week == 0.0 {
            tf("{n} / week", &[("n", &format!("{per_week:.1}"))])
        } else {
            tf(
                "{n} / month",
                &[("n", &format!("{:.1}", per_week * 30.0 / 7.0))],
            )
        }
    };
    let notes = [
        Some(versus_note(
            theme,
            versus(
                dora.per_week,
                b.per_week,
                (dora.shipped, b.shipped),
                true,
                0.05,
            ),
            &freq_text(b.per_week),
            period,
        )),
        dora.lead.zip(b.lead).map(|(now, then)| {
            versus_note(
                theme,
                versus(now, then, (dora.leads, b.leads), false, 0.05),
                &board::duration_text(then),
                period,
            )
        }),
        dora.failure_rate.zip(b.failure_rate).map(|(now, then)| {
            versus_note(
                theme,
                versus(now, then, (dora.changes, b.changes), false, 0.005),
                &board::percent(then),
                period,
            )
        }),
        dora.restore.zip(b.restore).map(|(now, then)| {
            versus_note(
                theme,
                versus(now, then, (dora.restores, b.restores), false, 0.05),
                &board::duration_text(then),
                period,
            )
        }),
    ];
    // A figure with nothing to compare says so.
    let notes: Vec<Option<(String, Color32)>> = notes
        .into_iter()
        .map(|n| {
            n.or_else(|| {
                Some((
                    tl("nothing to compare before").to_string(),
                    theme.palette.text_muted,
                ))
            })
        })
        .collect();
    board::figures_with(ui, theme, &figures, &notes);
}
