//! The left column's tabs: the work card, the description, the discussions.

use super::*;

pub(super) fn work(
    g: &mut GuiApp,
    ui: &mut Ui,
    jumps: &mut super::super::jump::Jumps,
    mr: &TrackedMr,
    data: Option<&PageData>,
) {
    if mr.state == GitlabMrState::Opened {
        let picked = jumps.section(ui, "blockers", |ui| blockers(g, ui, mr, data));
        // A file or the commits picked: their tab, brought into view.
        if let Some(path) = picked.file {
            g.mr_tab = Tab::Changes;
            g.details.open_file(&mr.id, &path);
            jumps.ask(TABS);
        }
        if picked.commits {
            g.mr_tab = Tab::Commits;
            jumps.ask(TABS);
        }
        if let Some(id) = picked.mr {
            g.open_mr(&id);
        }
        if picked.copied {
            g.app.flash(tl("Commands copied"), false);
        }
        ui.add_space(16.0);
    }
    if jumps.section(ui, "reviewers", |ui| {
        super::super::reviewers::card(g, ui, mr)
    }) {
        ui.add_space(16.0);
    }
    if !mr.description.trim().is_empty() {
        jumps.section(ui, "description", |ui| description(g, ui, mr));
        ui.add_space(16.0);
    }
    let tabs = tabs(g, mr, data);
    jumps.section(ui, TABS, |ui| tabs_card(g, ui, mr, data, tabs));
}

/// The card of the tabs: discussions, changes, commits, pipelines.
pub(super) fn tabs_card(
    g: &mut GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    data: Option<&PageData>,
    tabs: [TabLine; 4],
) {
    let theme = g.theme;
    theme.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, label, n, alert, _) in tabs {
                let on = g.mr_tab == tab;
                let text = RichText::new(format!("{label} · {n}"))
                    .font(if on {
                        fonts::medium(theme.text_size)
                    } else {
                        egui::FontId::proportional(theme.text_size)
                    })
                    .color(if on {
                        theme.palette.text
                    } else {
                        theme.palette.text_secondary
                    });
                let response = ui
                    .add(egui::Label::new(text).sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if let Some(color) = alert {
                    let r = response.rect;
                    ui.painter().circle_filled(
                        egui::pos2(r.right() + 6.0, r.center().y),
                        3.5,
                        color,
                    );
                    ui.add_space(8.0);
                }
                if on {
                    let r = response.rect;
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 6.0,
                        egui::Stroke::new(2.0, theme.palette.accent),
                    );
                }
                if response.clicked() {
                    g.mr_tab = tab;
                }
                ui.add_space(14.0);
            }
        });
        ui.add_space(6.0);
        super::super::widgets::rule(ui, &theme);
        ui.add_space(10.0);
        match g.mr_tab {
            Tab::Discussions => discussions(g, ui, mr, data),
            Tab::Changes => {
                g.details.open_first_file(&mr.id);
                details::changes(
                    &mut g.details,
                    &g.app,
                    ui,
                    &theme,
                    &mr.id,
                    &mr.web_url,
                    true,
                )
            }
            Tab::Commits => commits(g, ui, mr),
            Tab::Pipelines => super::super::mr_pipelines::tab(g, ui, mr),
        }
    });
}

/// The description's height while folded, and how much taller it may be and
/// still show in full: folding would hide only a line or two.
pub(super) const FOLDED_DESCRIPTION: f32 = 180.0;
pub(super) const FOLD_SLACK: f32 = 48.0;

/// The MR's description across the main column, where its lists, tables and
/// code have room. Folded when long, with a fade over its last lines, so the
/// tabs below stay in view; "Read more" shows it in full, until another MR.
pub(super) fn description(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let open = g.mr_description_open.as_deref() == Some(mr.id.as_str());
    // The full height, measured at the last frame: whether there is anything to fold
    // is known before the title row is drawn.
    let height_id = egui::Id::unique(("mr_description_height", &mr.id));
    let full: Option<f32> = ui.ctx().data(|d| d.get_temp(height_id));
    let foldable = full.is_some_and(|h| h > FOLDED_DESCRIPTION + FOLD_SLACK);
    let mut toggle = false;
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                tile_title(ui, &theme, tl("Description"));
                if foldable && open {
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        toggle |= text_link(ui, &theme, tl("Show less")).clicked();
                    });
                }
            });
            let width = ui.available_width();
            let top = ui.cursor().min;
            let mut body = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        top,
                        Vec2::new(width, f32::INFINITY),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            let folded = !open && full.is_none_or(|h| h > FOLDED_DESCRIPTION + FOLD_SLACK);
            if folded {
                // Clipped: what lies below can be neither seen nor clicked.
                let shown = egui::Rect::from_min_size(top, Vec2::new(width, FOLDED_DESCRIPTION));
                body.set_clip_rect(body.clip_rect().intersect(shown));
            }
            g.description.show(&mut body, &theme, mr, true);
            let measured = body.min_rect().height();
            ui.ctx().data_mut(|d| d.insert_temp(height_id, measured));
            let height = if folded {
                measured.min(FOLDED_DESCRIPTION)
            } else {
                measured
            };
            let rect = egui::Rect::from_min_size(top, Vec2::new(width, height));
            ui.allocate_rect(rect, Sense::hover());
            if folded && measured > FOLDED_DESCRIPTION {
                fade(ui, rect, theme.palette.panel);
                ui.add_space(4.0);
                toggle |= text_link(ui, &theme, &format!("{} ▾", tl("Read more"))).clicked();
            } else if foldable && open {
                ui.add_space(6.0);
                toggle |= text_link(ui, &theme, &format!("{} ▴", tl("Show less"))).clicked();
            }
        });
    if toggle {
        g.mr_description_open = (!open).then(|| mr.id.clone());
    }
}

/// The last lines of a folded block fading into its background.
pub(super) fn fade(ui: &Ui, rect: egui::Rect, background: egui::Color32) {
    let band = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 56.0), rect.max);
    let mut mesh = egui::Mesh::default();
    let clear = background.gamma_multiply(0.0);
    mesh.colored_vertex(band.left_top(), clear);
    mesh.colored_vertex(band.right_top(), clear);
    mesh.colored_vertex(band.right_bottom(), background);
    mesh.colored_vertex(band.left_bottom(), background);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Text in the accent that reads as a link.
pub(super) fn text_link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(theme.text_size - 1.0)
                .color(theme.palette.accent),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn discussions(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) {
    let theme = g.theme;
    let threads = match data.map(|d| &d.threads) {
        None => {
            ui.horizontal(|ui| {
                spinner(ui, 12.0, theme.palette.accent);
                ui.label(muted(&theme, tl("Reading the threads…")));
            });
            return;
        }
        Some(Err(error)) => {
            ui.label(RichText::new(format!("⚠ {error}")).color(theme.tones.red.fg));
            return;
        }
        Some(Ok(threads)) => threads,
    };
    let open = threads.iter().filter(|t| t.is_open()).count();
    let resolved = threads.iter().filter(|t| t.resolved).count();
    let comments = threads.iter().filter(|t| !t.resolvable).count();
    ui.horizontal(|ui| {
        let mut summary = tf(
            "{open} open · {resolved} resolved",
            &[
                ("open", &open.to_string()),
                ("resolved", &resolved.to_string()),
            ],
        );
        if comments > 0 {
            summary = format!(
                "{summary} · {}",
                tn(comments as u64, "{n} comment", "{n} comments")
            );
        }
        ui.label(muted(&theme, summary));
        if resolved > 0 {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if super::super::widgets::switch(ui, &theme, g.show_resolved, tl("Show resolved"))
                    .clicked()
                {
                    g.show_resolved = !g.show_resolved;
                }
            });
        }
    });
    ui.add_space(8.0);
    if threads.is_empty() {
        ui.label(muted(&theme, tl("No review thread on this MR.")));
        return;
    }
    let me = g.app.config.gitlab_username.clone();
    // The threads waiting for me first, then the other open ones, then the rest.
    let waits_for_me = |t: &Thread| {
        t.is_open()
            && t.notes
                .last()
                .is_some_and(|last| me.as_deref().is_some_and(|u| last.username != u))
    };
    let mut shown: Vec<&Thread> = threads
        .iter()
        .filter(|t| g.show_resolved || !t.resolved)
        .collect();
    shown.sort_by_key(|t| (!waits_for_me(t), t.resolved, !t.resolvable));
    for thread in shown {
        // The code it is about, from the diff once read.
        let code: Vec<String> = match (&thread.path, thread.line, g.details.diff(&mr.id)) {
            (Some(path), Some(line), Some(files)) => files
                .iter()
                .find(|f| &f.new_path == path || &f.old_path == path)
                .map(|f| {
                    details::snippet(&f.diff, line, 3)
                        .into_iter()
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        thread_card(ui, &theme, thread, &code, me.as_deref());
        ui.add_space(10.0);
    }
}

pub(super) fn thread_card(
    ui: &mut Ui,
    theme: &Theme,
    thread: &Thread,
    code: &[String],
    me: Option<&str>,
) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .fill(theme.palette.raised)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Whose turn first, on the right; the path takes what is
                    // left and is shortened when too long.
                    let last = thread.notes.last();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(last) = last {
                                let waiting =
                                    thread.is_open() && me.is_some_and(|u| last.username != u);
                                let when = format_relative_date(&last.created_at);
                                if waiting {
                                    ui.label(muted(theme, when));
                                    ui.add(Chip::new(tl("Your turn"), theme.tones.orange))
                                        .tip(tl("The last note is someone else's"));
                                } else {
                                    ui.label(muted(theme, when));
                                }
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                let (label, tone) = if !thread.resolvable {
                                    (tl("Comment"), theme.tones.gray)
                                } else if thread.resolved {
                                    (tl("Resolved"), theme.tones.green)
                                } else {
                                    (tl("Unresolved"), theme.tones.amber)
                                };
                                ui.add(Chip::new(label, tone));
                                match (&thread.path, thread.line) {
                                    (Some(path), line) => {
                                        let text = match line {
                                            Some(n) => format!(
                                                "{path} · {}",
                                                tf("line {n}", &[("n", &n.to_string())])
                                            ),
                                            None => path.clone(),
                                        };
                                        ui.add(
                                            egui::Label::new(mono(text, theme.text_size - 1.5))
                                                .truncate()
                                                .show_tooltip_when_elided(false),
                                        )
                                        .rich(Tip::new().mono(path));
                                    }
                                    _ => {
                                        ui.label(muted(theme, tl("General")));
                                    }
                                }
                            });
                        });
                    });
                });
            if !code.is_empty() {
                let lines: Vec<&str> = code.iter().map(String::as_str).collect();
                details::code(ui, theme, &lines, true);
            }
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 10,
                    right: 10,
                    top: 6,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for note in &thread.notes {
                        ui.add_space(6.0);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            avatar(ui, theme, &note.author, 20.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&note.author)
                                            .font(fonts::medium(theme.text_size)),
                                    );
                                    ui.label(
                                        muted(theme, format_relative_date(&note.created_at))
                                            .size(theme.text_size - 1.5),
                                    );
                                });
                                ui.label(
                                    RichText::new(note.body.trim())
                                        .color(theme.palette.text_secondary),
                                );
                            });
                        });
                    }
                });
        });
}
