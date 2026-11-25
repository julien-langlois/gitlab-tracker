//! The panel beside the page: the selected release, its tag, commit, where it is deployed, its GitLab release.

use super::*;

pub(super) fn inspector(g: &mut GuiApp, ui: &mut Ui, m: &Model, now: DateTime<Utc>) {
    let theme = g.theme;
    let release = g
        .releases
        .selected
        .as_deref()
        .and_then(|tag| m.releases.iter().find(|s| s.record.tag == tag));
    let Some(s) = release else {
        board::section(ui, &theme, tl("Releases"), "", |ui| {
            ui.add(
                egui::Label::new(muted(
                    &theme,
                    tl("Pick a release to see what it holds, where it went and its notes."),
                ))
                .wrap(),
            );
        });
        return;
    };
    if board::inspector_title(ui, &theme, &s.record.tag) {
        g.releases.selected = None;
        return;
    }
    let url = release_url(&g.app, &s.record);
    let mut open_url = false;
    let mut copy = None;
    let mut open = None;
    board::section(ui, &theme, "", "", |ui| {
        if let Some(name) = &s.record.name {
            ui.add(egui::Label::new(semibold(name, theme.text_size)).wrap());
            ui.add_space(6.0);
        }
        let mut rows = Vec::new();
        if let Some(at) = s.at {
            rows.push((
                tl("Released"),
                format!("{} · {}", local_date(at), board::ago(at, now)),
            ));
        }
        rows.push((tl("Commit"), s.record.sha.chars().take(8).collect()));
        rows.push((
            tl("Deployed to"),
            if s.deployed.is_empty() {
                board::dash().to_string()
            } else {
                s.deployed.join(", ")
            },
        ));
        rows.push((
            tl("Kind"),
            if s.record.is_release {
                tl("GitLab release").to_string()
            } else {
                tl("Tag").to_string()
            },
        ));
        board::facts(ui, &theme, &rows);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if url.is_some() {
                open_url = button(
                    ui,
                    &theme,
                    Some(Icon::ExternalLink),
                    tl("Open in GitLab"),
                    false,
                )
                .clicked();
            }
            if let Some(Read::Ready(c)) = &s.contents {
                if !c.mrs.is_empty()
                    && button(ui, &theme, Some(Icon::Copy), tl("Copy its notes"), false).clicked()
                {
                    copy = Some(notes(
                        &s.record.tag,
                        &c.mrs,
                        g.app.config.release_note_tickets,
                    ));
                }
            }
        });
    });
    board::section(ui, &theme, tl("What it holds"), "", |ui| {
        match &s.contents {
            Some(Read::Ready(c)) => {
                if c.mrs.is_empty() {
                    board::empty(ui, &theme, tl("No MR: commits only."));
                }
                let mut sorted: Vec<&Item> = c.mrs.iter().collect();
                sorted.sort_by_key(|i| i.kind);
                for item in sorted {
                    if item_row(ui, &theme, item, None).clicked() && item.tracked {
                        open = Some(item.id.clone());
                    }
                }
                if c.direct > 0 {
                    board::caption(
                        ui,
                        &theme,
                        &tn(
                            c.direct as u64,
                            "{n} commit outside an MR",
                            "{n} commits outside an MR",
                        ),
                    );
                }
            }
            Some(Read::Loading) => board::empty(ui, &theme, tl("Reading the commits…")),
            Some(Read::Failed(e)) => board::empty(
                ui,
                &theme,
                &tf("The commits could not be read: {error}", &[("error", e)]),
            ),
            None => board::empty(
                ui,
                &theme,
                tl("The oldest release read: what came before it is not known."),
            ),
        }
    });
    if let Some(text) = &s.record.notes {
        board::section(ui, &theme, tl("Notes in GitLab"), "", |ui| {
            ui.add(egui::Label::new(muted(&theme, text.as_str())).wrap());
        });
    }
    if open_url {
        if let Some(url) = &url {
            board::open_url(ui, url);
        }
    }
    if let Some(text) = copy {
        ui.ctx().copy_text(board::Links::of(&g.app).apply(&text));
        g.app.flash(tl("Release notes copied"), false);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}
