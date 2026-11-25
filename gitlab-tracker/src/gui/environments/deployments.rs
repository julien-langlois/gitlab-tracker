//! The period's deployments, one lane per environment, and the MRs waiting longest for the next branch.

use super::*;

/// The deployments of the period, one lane per environment, dots by date.
pub(super) fn deployments_card(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    overview: Option<&ProjectOverview>,
    days: i64,
    failed: bool,
    now: DateTime<Utc>,
) {
    let note = tf("last {n} days", &[("n", &days.to_string())]);
    board::card(ui, theme, tl("Deployments"), &note, 0.0, |ui| {
        let Some(overview) = overview else {
            ui.horizontal(|ui| {
                super::super::widgets::spinner(ui, 14.0, theme.palette.text_muted);
                ui.label(muted(theme, tl("Reading the deployments…")));
            });
            return;
        };
        let start = now - Duration::days(days);
        let lanes: Vec<(&Branch, &String)> = model
            .branches
            .iter()
            .flat_map(|b| b.environments.iter().map(move |e| (b, e)))
            .collect();
        if lanes.is_empty() {
            board::empty(
                ui,
                theme,
                tl("GitLab lists no deployment of the tracked branches."),
            );
            return;
        }
        // The names' column as wide as the longest name (two fifths of the
        // card at most), then a gap before the lanes start.
        let width = ui.available_width();
        let label_font = FontId::proportional(theme.text_size - 1.5);
        let gap = 18.0;
        let longest = lanes
            .iter()
            .map(|(b, env)| {
                ui.painter()
                    .layout_no_wrap(
                        format!("{env} · {}", b.alias),
                        label_font.clone(),
                        theme.palette.text_secondary,
                    )
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max);
        let label_w = (14.0 + longest + gap).clamp(110.0, (width * 0.4).max(110.0));
        let track_w = width - label_w - 8.0;
        for (b, env) in lanes.iter().rev() {
            let (line, _) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
            board::row_rule(ui, theme, line);
            ui.painter().circle_filled(
                egui::pos2(line.left() + 4.0, line.center().y),
                4.0,
                role_color(b.role, theme),
            );
            let name = format!("{env} · {}", b.alias);
            let galley = board::one_line(
                ui,
                &name,
                label_font.clone(),
                theme.palette.text_secondary,
                label_w - 14.0 - gap,
            );
            let cut = galley.elided;
            let name_rect = Rect::from_min_size(
                egui::pos2(line.left() + 14.0, line.center().y - galley.size().y / 2.0),
                galley.size(),
            );
            ui.painter()
                .galley(name_rect.min, galley, theme.palette.text_secondary);
            if cut {
                ui.interact(
                    name_rect,
                    ui.scope_id().with(("lane", env.as_str())),
                    Sense::hover(),
                )
                .tip(&name);
            }
            let track = Rect::from_min_max(
                egui::pos2(line.left() + label_w, line.top()),
                egui::pos2(line.left() + label_w + track_w, line.bottom()),
            );
            ui.painter().hline(
                track.x_range(),
                track.center().y,
                Stroke::new(1.0, theme.palette.border),
            );
            let lane: Vec<&DeploymentRecord> = overview
                .deployments
                .iter()
                .filter(|d| &d.environment == *env && d.reference == b.name)
                .filter(|d| board::parse_time(&d.created_at).is_some_and(|at| at >= start))
                .filter(|d| failed || d.status == "success")
                .collect();
            // Smaller dots when they crowd the lane.
            let radius = (track.width() / lane.len().max(1) as f32 / 2.6).clamp(3.0, 5.5);
            for d in lane {
                let Some(at) = board::parse_time(&d.created_at) else {
                    continue;
                };
                if at < start {
                    continue;
                }
                let ok = d.status == "success";
                if !ok && !failed {
                    continue;
                }
                let x = track.left()
                    + 6.0
                    + (track.width() - 12.0)
                        * ((at - start).num_minutes() as f32
                            / (now - start).num_minutes().max(1) as f32);
                let color = match d.status.as_str() {
                    "success" => theme.tones.green.fg,
                    "failed" => theme.tones.red.fg,
                    _ => theme.tones.amber.fg,
                };
                let center = egui::pos2(x, track.center().y);
                ui.painter()
                    .circle(center, radius, color, Stroke::new(1.5, theme.palette.panel));
                let hit = Rect::from_center_size(center, Vec2::splat(14.0));
                ui.interact(
                    hit,
                    ui.scope_id()
                        .with(("dep", env.as_str(), d.created_at.as_str())),
                    Sense::hover(),
                )
                .rich(
                    Tip::new()
                        .title(&d.environment)
                        .dot(color, d.status.as_str())
                        .mono(d.sha.chars().take(8).collect::<String>())
                        .text(format!(
                            "{} {}",
                            board::ago(at, now),
                            if d.user.is_empty() {
                                String::new()
                            } else {
                                tf("by {name}", &[("name", &d.user)])
                            },
                        )),
                );
            }
        }
        // The dates under the lanes: today on the right, then the others
        // where they do not run into a neighbour.
        let (axis, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
        let track_left = axis.left() + label_w + 6.0;
        let track_width = track_w - 12.0;
        let font = FontId::proportional(theme.text_size - 2.5);
        let today_w = board::paint_text(
            ui,
            egui::pos2(track_left + track_width, axis.center().y),
            Align2::RIGHT_CENTER,
            tl("today"),
            font.clone(),
            theme.palette.text_muted,
            90.0,
        );
        let limit = track_left + track_width - today_w - 10.0;
        let mut previous_right = f32::NEG_INFINITY;
        for k in 0..4 {
            let f = k as f32 / 4.0;
            let at = start + Duration::minutes(((now - start).num_minutes() as f32 * f) as i64);
            let text = at.format("%d/%m").to_string();
            let galley = board::one_line(ui, &text, font.clone(), theme.palette.text_muted, 60.0);
            let w = galley.size().x;
            let x = track_left + track_width * f;
            let left = if k == 0 { x } else { x - w / 2.0 };
            if left < previous_right + 10.0 || left + w > limit {
                continue;
            }
            ui.painter().galley(
                egui::pos2(left, axis.center().y - galley.size().y / 2.0),
                galley,
                theme.palette.text_muted,
            );
            previous_right = left + w;
        }
        ui.add_space(6.0);
        board::legend(
            ui,
            theme,
            &[
                (theme.tones.green.fg, tl("Succeeded").to_string()),
                (theme.tones.red.fg, tl("Failed").to_string()),
            ],
        );
    });
}

/// The MRs that wait the longest on a branch for the next one.
pub(super) fn waiting_card(
    ui: &mut Ui,
    theme: &Theme,
    model: &Model,
    now: DateTime<Utc>,
    open: &mut Option<String>,
    picked: &mut Option<(usize, usize)>,
) {
    board::card(
        ui,
        theme,
        tl("Waiting the longest"),
        tl("on a branch, not on the next one"),
        0.0,
        |ui| {
            let mut list: Vec<(usize, &Waiting)> = (0..model.branches.len().saturating_sub(1))
                .flat_map(|i| model.gap(i, i + 1).iter().map(move |r| (i, r)))
                .collect();
            list.sort_by_key(|(_, r)| r.2.unwrap_or(now));
            if list.is_empty() {
                board::empty(
                    ui,
                    theme,
                    tl("Every branch has what the one before it has."),
                );
                return;
            }
            for (i, (id, title, since)) in list.into_iter().take(6) {
                let (rect, response) = board::row(ui, theme, 36.0, false);
                board::row_rule(ui, theme, rect);
                let cy = rect.center().y;
                let left = rect.left() + 8.0;
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(left, cy),
                    &format!("!{id}"),
                    board::id_tone(theme),
                    true,
                );
                let wait = since.map(|s| board::days_between(s, now));
                let late = wait.is_some_and(|w| w >= LATE_DAYS);
                let right_text = match wait {
                    Some(w) => tf(
                        "on {branch} for {time}",
                        &[
                            ("branch", &model.branches[i].alias),
                            ("time", &board::duration_text(w)),
                        ],
                    ),
                    None => tf("on {branch}", &[("branch", &model.branches[i].alias)]),
                };
                let to = &model.branches[i + 1].alias;
                let tag = tf("to {branch}", &[("branch", to)]);
                let tag_galley = ui.painter().layout_no_wrap(
                    tag.clone(),
                    fonts::medium(theme.text_size - 2.5),
                    theme.tones.amber.fg,
                );
                let tag_w = tag_galley.size().x + 12.0;
                board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.right() - 8.0 - tag_w, cy),
                    &tag,
                    if late {
                        theme.tones.red
                    } else {
                        theme.tones.amber
                    },
                    false,
                );
                let text_right = rect.right() - 8.0 - tag_w - 12.0;
                let rw = board::paint_text(
                    ui,
                    egui::pos2(text_right, cy),
                    Align2::RIGHT_CENTER,
                    &right_text,
                    FontId::proportional(theme.text_size - 1.5),
                    if late {
                        theme.tones.red.fg
                    } else {
                        theme.palette.text_secondary
                    },
                    220.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(left + w + 10.0, cy),
                    Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(theme.text_size - 0.5),
                    theme.palette.text,
                    text_right - rw - 16.0 - (left + w + 10.0),
                );
                let response = response.tip(tl("Click: pick the pair · double click: show the MR"));
                if super::super::widgets::double_clicked(ui, &response, ("env", &id, i)) {
                    *open = Some(id.clone());
                } else if response.clicked() {
                    *picked = Some((i, i + 1));
                }
            }
        },
    );
}
