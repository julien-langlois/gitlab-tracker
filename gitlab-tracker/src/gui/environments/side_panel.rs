//! The panel beside the page (the selected pair of branches: where their MRs stand) and the rail's badge.

use super::*;

pub(super) fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    model: &Model,
    pair: Option<(usize, usize)>,
    overview: Option<&ProjectOverview>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    let Some((i, j)) = pair else {
        board::inspector_empty(
            ui,
            &theme,
            tl("Track at least two branches to compare them."),
        );
        return;
    };
    let from = &model.branches[i];
    let to = &model.branches[j];
    let rows: Vec<Row> = promote::rows(&g.app, &from.name, &to.name);
    let ready: Vec<&Row> = rows.iter().filter(|r| r.stage == Stage::Ready).collect();
    let upstream = rows
        .iter()
        .filter(|r| r.stage == Stage::Upstream && r.state == GitlabMrState::Opened)
        .count();
    let partial = ready
        .iter()
        .filter(|r| r.on_to == Some(crate::models::BranchPresence::Partial))
        .count();
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
            let font = egui::FontId::monospace(theme.text_size - 0.5);
            let w = board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &from.name,
                font.clone(),
                theme.tones.blue.fg,
                line.width() / 2.0 - 16.0,
            );
            board::paint_text(
                ui,
                egui::pos2(line.left() + w + 8.0, line.center().y),
                Align2::LEFT_CENTER,
                "→",
                FontId::proportional(theme.text_size),
                theme.palette.text_muted,
                20.0,
            );
            board::paint_text(
                ui,
                egui::pos2(line.left() + w + 28.0, line.center().y),
                Align2::LEFT_CENTER,
                &to.name,
                font,
                theme.tones.blue.fg,
                line.width() - w - 28.0,
            );
            ui.add_space(6.0);
            let title = if j > i {
                tn(
                    ready.len() as u64,
                    "{n} MR ahead of {to}",
                    "{n} MRs ahead of {to}",
                )
                .replace("{to}", &to.alias)
            } else {
                tn(
                    ready.len() as u64,
                    "{n} MR to bring back to {to}",
                    "{n} MRs to bring back to {to}",
                )
                .replace("{to}", &to.alias)
            };
            ui.add(
                egui::Label::new(super::super::widgets::semibold(
                    title,
                    theme.text_size + 2.0,
                ))
                .wrap(),
            );
            let last = to
                .last
                .as_ref()
                .and_then(|d| board::parse_time(&d.created_at));
            let detail = match (to.environments.first(), last) {
                (Some(env), Some(at)) => tf(
                    "Last deployment of {env}: {when}",
                    &[("env", env), ("when", &board::ago(at, now))],
                ),
                (Some(env), None) => tf("{env} was never deployed", &[("env", env)]),
                _ => tl("No environment deployed from it").to_string(),
            };
            ui.add(egui::Label::new(muted(&theme, detail)).wrap());
        });
    super::super::widgets::rule(ui, &theme);
    board::section(ui, &theme, tl("Where they stand"), "", |ui| {
        let t = &theme.tones;
        board::stacked_bar(
            ui,
            &theme,
            &[
                ((ready.len() - partial) as f32, t.green.fg),
                (partial as f32, t.amber.fg),
                (upstream as f32, t.gray.fg),
            ],
            10.0,
            2.0,
        );
        ui.add_space(8.0);
        board::legend(
            ui,
            &theme,
            &[
                (
                    t.green.fg,
                    tn((ready.len() - partial) as u64, "{n} ready", "{n} ready"),
                ),
                (
                    t.amber.fg,
                    tn(partial as u64, "{n} partly there", "{n} partly there"),
                ),
                (
                    t.gray.fg,
                    tf(
                        "{n} not on {from} yet",
                        &[("n", &upstream.to_string()), ("from", &from.alias)],
                    ),
                ),
            ],
        );
    });
    let mut open = None;
    board::section(ui, &theme, tl("MRs"), tl("in merge order"), |ui| {
        if ready.is_empty() {
            ui.label(muted(
                &theme,
                tf(
                    "{to} has everything {from} has.",
                    &[("to", &to.alias), ("from", &from.alias)],
                ),
            ));
        }
        for r in ready.iter().take(12) {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect.expand2(Vec2::new(6.0, 0.0)),
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            let cy = rect.center().y;
            let w = board::paint_tag(
                ui,
                &theme,
                egui::pos2(rect.left(), cy),
                &format!("!{}", r.id),
                board::id_tone(&theme),
                true,
            );
            let since = r
                .on_from_since
                .map(|s| board::duration_text(board::days_between(s, now)));
            let note_w = since.as_ref().map_or(0.0, |s| {
                board::paint_text(
                    ui,
                    egui::pos2(rect.right(), cy),
                    Align2::RIGHT_CENTER,
                    s,
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    70.0,
                ) + 8.0
            });
            board::paint_text(
                ui,
                egui::pos2(rect.left() + w + 8.0, cy),
                Align2::LEFT_CENTER,
                &r.title,
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text,
                rect.width() - w - 8.0 - note_w,
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some(r.id.clone());
            }
        }
        if ready.len() > 12 {
            ui.label(muted(
                &theme,
                tf("and {n} more", &[("n", &(ready.len() - 12).to_string())]),
            ));
        }
    });
    if let Some(env) = to.environments.first() {
        let dates: Vec<DateTime<Utc>> = overview
            .map(|o| {
                o.deployments
                    .iter()
                    .filter(|d| &d.environment == env && d.status == "success")
                    .filter_map(|d| board::parse_time(&d.created_at))
                    .collect()
            })
            .unwrap_or_default();
        let gaps: Vec<f64> = dates
            .windows(2)
            .map(|w| board::days_between(w[1], w[0]))
            .collect();
        let month = dates
            .iter()
            .filter(|d| **d >= now - Duration::days(30))
            .count();
        board::section(
            ui,
            &theme,
            &tf("Rhythm of {env}", &[("env", env)]),
            "",
            |ui| {
                board::columns(ui, 2, |ui, k| {
                    let (value, caption) = if k == 0 {
                        (
                            crate::next_step::median(gaps.clone())
                                .map_or_else(|| board::dash().to_string(), board::duration_text),
                            tl("between two deployments"),
                        )
                    } else {
                        (month.to_string(), tl("deployments in 30 days"))
                    };
                    ui.label(super::super::widgets::semibold(
                        value,
                        theme.text_size + 7.0,
                    ));
                    ui.add(
                        egui::Label::new(muted(&theme, caption).size(theme.text_size - 2.0)).wrap(),
                    );
                });
            },
        );
    }
    let mut promote_it = false;
    let mut show = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                promote_it = primary_button(ui, &theme, tl("Open in Promote")).clicked();
                show = button(ui, &theme, None, tl("Show in the table"), false).clicked();
            });
        });
    if promote_it {
        g.promote = Some((from.name.clone(), to.name.clone()));
        g.open_page(super::super::Page::Promote);
    }
    if show {
        g.next_focus = Some(Some(Focus {
            key: String::new(),
            label: tf(
                "On {from}, not on {to}",
                &[("from", &from.name), ("to", &to.name)],
            ),
            ids: ready.iter().map(|r| r.id.clone()).collect(),
        }));
        g.open_page(super::super::Page::MergeRequests);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

/// The state of the environments as Markdown, to paste.
pub(super) fn state_text(model: &Model, now: DateTime<Utc>) -> String {
    let mut out = format!("### {}\n\n", tl("Environments"));
    for (i, b) in model.branches.iter().enumerate() {
        let env = b
            .environments
            .first()
            .map_or(String::new(), |e| format!(" ({e})"));
        let deployed = b
            .last
            .as_ref()
            .and_then(|d| board::parse_time(&d.created_at))
            .map_or(String::new(), |at| {
                format!(
                    " · {}",
                    tf("deployed {when}", &[("when", &board::ago(at, now))])
                )
            });
        out.push_str(&format!("- `{}`{env}{deployed}", b.name));
        if i + 1 < model.branches.len() {
            let list = model.gap(i, i + 1);
            if !list.is_empty() {
                let ids = list
                    .iter()
                    .map(|r| format!("!{}", r.0))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    " · {}: {ids}",
                    tf("to {branch}", &[("branch", &model.branches[i + 1].name)])
                ));
            }
        }
        out.push('\n');
    }
    out
}

/// Waiting this long on a branch already shows.
pub(super) const SLOW_DAYS: f64 = 3.0;

/// The rail's badge: the environments whose last deployment failed (red), and
/// the MRs waiting to go one branch further for [`LATE_DAYS`] (orange) or
/// [`SLOW_DAYS`] (amber).
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let overview = g.details.project_ready();
    let mut failed: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    // The deployments come newest first: an environment's first is its last.
    for d in overview.map_or(&[][..], |o| o.deployments.as_slice()) {
        if seen.contains(&d.environment.as_str()) {
            continue;
        }
        seen.push(&d.environment);
        if d.status == "failed" {
            failed.push(d.environment.clone());
        }
    }
    let model = model(&g.app, overview);
    let mut waiting: Vec<(&str, f64)> = Vec::new();
    for i in 0..model.branches.len().saturating_sub(1) {
        for (id, _, since) in model.gap(i, i + 1) {
            let Some(days) = since.map(|s| board::days_between(s, now)) else {
                continue;
            };
            if days >= SLOW_DAYS && !waiting.iter().any(|w| w.0 == id) {
                waiting.push((id, days));
            }
        }
    }
    let late = waiting.iter().filter(|w| w.1 >= LATE_DAYS).count();
    let tones = &g.theme.tones;
    let tone = if !failed.is_empty() {
        tones.red
    } else if late > 0 {
        tones.orange
    } else if !waiting.is_empty() {
        tones.amber
    } else {
        return None;
    };
    let mut lines = Vec::new();
    if !failed.is_empty() {
        lines.push(rail_badges::listed(
            tn(
                failed.len() as u64,
                "{n} environment whose last deployment failed",
                "{n} environments whose last deployment failed",
            ),
            rail_badges::names(&failed, 3),
        ));
    }
    if late > 0 {
        lines.push(tn(
            late as u64,
            "{n} MR waits for a week or more to go one branch further",
            "{n} MRs wait for a week or more to go one branch further",
        ));
    }
    if waiting.len() > late {
        lines.push(tn(
            (waiting.len() - late) as u64,
            "{n} MR waits for 3 days or more to go one branch further",
            "{n} MRs wait for 3 days or more to go one branch further",
        ));
    }
    Some(Badge {
        n: failed.len() + waiting.len(),
        tone,
        hint: lines.join("\n"),
    })
}
