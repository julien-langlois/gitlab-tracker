//! The MR page's "Who could review": the people who know the files the MR
//! changes, how each stands against the project's `CODEOWNERS` (or the
//! owners deduced without one), their reviews waiting, and the files nobody
//! else knows. Worked out in [`crate::owners`]; this module draws it.
//!
//! The app reads GitLab only: a click copies the `/assign_reviewer` quick
//! action, to paste in a comment.

use std::collections::HashMap;

use bevel_theme::{fonts, Theme, Tone};
use chrono::Utc;
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};

use super::board;
use super::tip::TipExt;
use super::widgets::{avatar, button, mono, muted, semibold, spinner, split_person};
use super::GuiApp;
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, TrackedMr};
use crate::owners::{Candidate, Declared, Source, Standing, Suggestions};

/// People listed; the others are a click away.
const SHOWN: usize = 5;

/// The login in a display such as "Sarah Codewyn (@sarah_code)".
fn login(display: &str) -> &str {
    split_person(display).1.unwrap_or(display)
}

/// What the card shows, or why not yet.
enum State {
    Loading,
    NoFiles,
    Ready(Box<Suggestions>),
}

fn work_out(g: &mut GuiApp, mr: &TrackedMr, ctx: &egui::Context) -> State {
    let Some(stats) = mr.diff_stats.as_ref().filter(|s| !s.files.is_empty()) else {
        return State::NoFiles;
    };
    let files: Vec<String> = stats.files.iter().map(|f| f.path.clone()).collect();
    let declared = match g.details.declared(&g.app, &mr.target_branch, ctx) {
        None => return State::Loading,
        Some(Ok(declared)) => declared.clone(),
        // Not read (an error): owners deduced, as without a file.
        Some(Err(_)) => Declared::Absent,
    };
    let Some(history) = g.details.owner_history(&g.app, ctx) else {
        return State::Loading;
    };
    let mut waiting: HashMap<String, usize> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    for other in &g.app.mrs {
        for display in std::iter::once(&other.author).chain(&other.reviewers) {
            if let (name, Some(handle)) = split_person(display) {
                names.insert(handle.to_string(), name.to_string());
            }
        }
        if other.state != GitlabMrState::Opened {
            continue;
        }
        for reviewer in &other.reviewers {
            let who = login(reviewer);
            if !other.approved_by.iter().any(|a| a == who) {
                *waiting.entry(who.to_string()).or_default() += 1;
            }
        }
    }
    let reviewers: Vec<String> = mr.reviewers.iter().map(|r| login(r).to_string()).collect();
    let suggestions = crate::owners::suggest(&crate::owners::Input {
        files: &files,
        author: login(&mr.author),
        reviewers: &reviewers,
        history,
        declared: &declared,
        waiting: &waiting,
        names: &names,
        is_bot: &crate::utils::is_bot,
    });
    State::Ready(Box::new(suggestions))
}

/// The card, for an open MR. Whether it drew.
pub fn card(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) -> bool {
    if mr.state != GitlabMrState::Opened {
        return false;
    }
    let state = work_out(g, mr, ui.ctx());
    let theme = g.theme;
    let mut copied: Option<String> = None;
    theme.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(semibold(tl("Who could review"), theme.text_size));
            ui.label(muted(&theme, tl("who knows the files this MR changes")));
        });
        match &state {
            State::Loading => {
                ui.horizontal(|ui| {
                    spinner(ui, 12.0, theme.palette.accent);
                    ui.label(muted(&theme, tl("Reading CODEOWNERS and the merged MRs…")));
                });
            }
            State::NoFiles => {
                ui.label(muted(&theme, tl("The MR's files are not read yet.")));
            }
            State::Ready(s) => copied = body(ui, &theme, s, mr),
        }
    });
    if let Some(text) = copied {
        ui.ctx().copy_text(text.clone());
        g.app.flash(tf("Copied: {text}", &[("text", &text)]), false);
    }
    true
}

/// Where the owners come from, under the title.
fn source_line(ui: &mut Ui, theme: &Theme, s: &Suggestions) {
    let days = crate::owners::HISTORY_DAYS.to_string();
    ui.horizontal_wrapped(|ui| match &s.source {
        Source::File(path) => {
            board::chip(ui, theme, "CODEOWNERS", theme.tones.teal);
            ui.label(muted(
                theme,
                tf(
                    "{path}, compared with the {n} MRs merged in the last {days} days",
                    &[("path", path), ("n", &s.merges.to_string()), ("days", &days)],
                ),
            ));
        }
        Source::Deduced | Source::DeducedDenied => {
            board::chip(ui, theme, tl("Owners deduced"), theme.tones.purple);
            let why = if s.source == Source::DeducedDenied {
                tl("The token may not read the repository's files: owners deduced from the merged MRs.")
            } else {
                tl("No CODEOWNERS in this project: owners deduced from the merged MRs.")
            };
            ui.label(muted(theme, why))
                .tip(tf(
                    "In each folder, the one or two people who did the most in the last {days} days (author twice, reviewer or approver once), with two merged MRs at least.",
                    &[("days", &days)],
                ));
        }
    });
}

/// How a person stands, as a chip, with what it means on hover.
fn standing_chip(ui: &mut Ui, theme: &Theme, p: &Candidate) {
    let t = &theme.tones;
    let first = p.owns.first().map(String::as_str).unwrap_or_default();
    let (text, tone, tip): (String, Tone, String) = match p.standing {
        Standing::Owner => (
            tl("Code owner").to_string(),
            t.teal,
            tf("In CODEOWNERS for {path}, and knows these files.", &[("path", first)]),
        ),
        Standing::StaleOwner => (
            tl("Code owner, not seen here").to_string(),
            t.amber,
            tf(
                "In CODEOWNERS for {path}, yet in none of the merged MRs that changed these files in the period: CODEOWNERS may be out of date.",
                &[("path", first)],
            ),
        ),
        Standing::NotDeclared => (
            tl("Not in CODEOWNERS").to_string(),
            t.gray,
            tl("Knows these files, which CODEOWNERS gives to others.").to_string(),
        ),
        Standing::Deduced => (
            tl("Likely owner").to_string(),
            t.purple,
            tf(
                "Deduced: did the most in {path} lately. Not declared anywhere.",
                &[("path", first)],
            ),
        ),
        Standing::Knows => return,
    };
    board::chip(ui, theme, &text, tone).tip(tip);
}

/// The people's columns: who, what they know, their reviews waiting, the
/// button; what they know takes the room left.
const NAME_W: f32 = 220.0;
const LOAD_W: f32 = 160.0;
const BUTTON_W: f32 = 180.0;
const GAP: f32 = 16.0;
const ROW_H: f32 = 52.0;
/// The matrix's columns: the owners, then one per person.
const OWNERS_W: f32 = 280.0;
const PERSON_W: f32 = 30.0;
/// People in the matrix at most.
const MATRIX_PEOPLE: usize = 8;

/// A cell `width` wide and `height` tall, its content left to right (right
/// to left when `right`), centred on the row.
fn cell<R>(ui: &mut Ui, width: f32, height: f32, right: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if right {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    ui.allocate_ui_with_layout(Vec2::new(width, height), layout, |ui| {
        ui.set_min_size(Vec2::new(width, height));
        ui.set_max_width(width);
        add(ui)
    })
    .inner
}

/// A one-line label cut to its cell.
fn line(ui: &mut Ui, text: RichText) -> egui::Response {
    ui.add(egui::Label::new(text).truncate())
}

/// What a person knows of the MR's files, in words.
fn knowledge(p: &Candidate, total: usize) -> String {
    let total = total.to_string();
    let files = tn(
        p.known as u64,
        "{n} of {total} files",
        "{n} of {total} files",
    )
    .replace("{total}", &total);
    match (p.known, p.near) {
        (0, 0) => tl("none of these files lately").to_string(),
        (0, m) => tn(
            m as u64,
            "the folders of {n} of {total} files",
            "the folders of {n} of {total} files",
        )
        .replace("{total}", &total),
        (_, 0) => files,
        (_, m) => format!(
            "{files}, {}",
            tn(
                m as u64,
                "the folders of {n} more",
                "the folders of {n} more"
            )
        ),
    }
}

/// The card's content; the text to copy when a button was clicked.
fn body(ui: &mut Ui, theme: &Theme, s: &Suggestions, mr: &TrackedMr) -> Option<String> {
    let mut copied = None;
    source_line(ui, theme, s);
    ui.add_space(10.0);
    let total = s.files.len();
    if s.people.is_empty() {
        let text = if mr.reviewers.is_empty() {
            tl("Nobody else changed or reviewed these files lately.")
        } else {
            tl("Nobody else than the reviewers changed or reviewed these files lately.")
        };
        board::empty(ui, theme, text);
    }
    let unfold_id = ui.make_persistent_id(("reviewers-all", &mr.id));
    let all = ui.data(|d| d.get_temp::<bool>(unfold_id)).unwrap_or(false);
    let now = Utc::now();
    ui.spacing_mut().item_spacing.y = 2.0;
    for (i, p) in s
        .people
        .iter()
        .take(if all { usize::MAX } else { SHOWN })
        .enumerate()
    {
        if let Some(text) = person_row(ui, theme, p, total, now, i == 0) {
            copied = Some(text);
        }
    }
    ui.spacing_mut().item_spacing.y = 6.0;
    if s.people.len() > SHOWN {
        let label = if all {
            tl("Show fewer").to_string()
        } else {
            tn((s.people.len() - SHOWN) as u64, "{n} more", "{n} more")
        };
        if ui.link(label).clicked() {
            ui.data_mut(|d| d.insert_temp(unfold_id, !all));
        }
    }
    if let Some((a, b, n)) = &s.pair {
        ui.add_space(6.0);
        if pair_row(ui, theme, s, a, b, *n, total) {
            copied = Some(format!("/assign_reviewer @{a} @{b}"));
        }
    }
    agreement(ui, theme, s);
    lonely(ui, theme, s);
    if !s.lines.is_empty() {
        ui.add_space(10.0);
        ui.label(muted(theme, tl("To start a CODEOWNERS from these owners:")));
        // The box copies by itself.
        super::mr_page::command_box(ui, theme, &s.lines.join("\n"));
    }
    by_file(ui, theme, s, &mr.id);
    copied
}

/// One person, in the columns; the text to copy when its button was clicked.
fn person_row(
    ui: &mut Ui,
    theme: &Theme,
    p: &Candidate,
    total: usize,
    now: chrono::DateTime<Utc>,
    first: bool,
) -> Option<String> {
    let mut copied = None;
    let fill = if first {
        theme.palette.raised
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = ui.available_width();
            let know_w = (width - NAME_W - LOAD_W - BUTTON_W - 3.0 * GAP).max(140.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                cell(ui, NAME_W, ROW_H, false, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    avatar(ui, theme, &p.name, 24.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        line(
                            ui,
                            RichText::new(&p.name).font(fonts::medium(theme.text_size)),
                        );
                        standing_chip(ui, theme, p);
                    });
                });
                cell(ui, know_w, ROW_H, false, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        let mut detail = Vec::new();
                        if p.mrs > 0 {
                            detail.push(tn(p.mrs as u64, "in {n} MR", "in {n} MRs"));
                        }
                        if let Some(last) = p.last {
                            detail.push(tf("last {when}", &[("when", &board::ago(last, now))]));
                        }
                        ui.spacing_mut().item_spacing.y = 3.0;
                        line(
                            ui,
                            RichText::new(knowledge(p, total)).font(fonts::medium(theme.text_size)),
                        );
                        if !detail.is_empty() {
                            let detail = detail.join(", ");
                            line(
                                ui,
                                RichText::new(&detail)
                                    .size(theme.text_size - 1.5)
                                    .color(theme.palette.text_muted),
                            )
                            .tip(detail);
                        }
                        ui.add_space(2.0);
                        knowledge_bar(ui, theme, p, total, know_w);
                    });
                });
                cell(ui, LOAD_W, ROW_H, false, |ui| {
                    let (load, color) = match p.waiting {
                        0 => (tl("no review waiting").to_string(), theme.tones.green.fg),
                        n => (
                            tn(n as u64, "{n} review waiting", "{n} reviews waiting"),
                            if n >= 3 {
                                theme.tones.amber.fg
                            } else {
                                theme.palette.text_secondary
                            },
                        ),
                    };
                    let dot = if p.waiting >= 3 {
                        theme.tones.amber.fg
                    } else {
                        theme.tones.green.fg
                    };
                    ui.spacing_mut().item_spacing.x = 6.0;
                    super::widgets::dot(ui, dot, 8.0);
                    line(
                        ui,
                        RichText::new(load).color(if p.waiting == 0 {
                            theme.palette.text_secondary
                        } else {
                            color
                        }),
                    );
                });
                cell(ui, BUTTON_W, ROW_H, true, |ui| {
                    if button(ui, theme, None, tl("Copy /assign_reviewer"), false)
                        .tip(tf(
                            "/assign_reviewer @{login}, to paste in a comment",
                            &[("login", &p.login)],
                        ))
                        .clicked()
                    {
                        copied = Some(format!("/assign_reviewer @{}", p.login));
                    }
                });
            });
        });
    copied
}

/// The share of the files a person knows (full) and whose folder they know
/// (paler), on a track.
fn knowledge_bar(ui: &mut Ui, theme: &Theme, p: &Candidate, total: usize, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), Sense::hover());
    let radius = CornerRadius::same(3);
    ui.painter().rect_filled(rect, radius, theme.palette.border);
    let total = total.max(1) as f32;
    let teal = theme.tones.teal.fg;
    let known = rect.width() * p.known as f32 / total;
    let near = rect.width() * p.near as f32 / total;
    if near > 0.0 {
        let r = egui::Rect::from_min_size(rect.min, Vec2::new(known + near, rect.height()));
        ui.painter()
            .rect_filled(r, radius, teal.gamma_multiply(0.35));
    }
    if known > 0.0 {
        let r = egui::Rect::from_min_size(rect.min, Vec2::new(known, rect.height()));
        ui.painter().rect_filled(r, radius, teal);
    }
}

/// The two people who together know the most, with the button for both.
/// Whether it was clicked.
fn pair_row(
    ui: &mut Ui,
    theme: &Theme,
    s: &Suggestions,
    a: &str,
    b: &str,
    n: usize,
    total: usize,
) -> bool {
    let name = |login: &str| {
        s.names
            .get(login)
            .cloned()
            .unwrap_or_else(|| login.to_string())
    };
    let mut clicked = false;
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = ui.available_width();
            ui.horizontal(|ui| {
                cell(ui, width - BUTTON_W - GAP, ROW_H - 8.0, false, |ui| {
                    line(
                        ui,
                        RichText::new(tf(
                            "Together, {a} and {b} know {n} of {total} files.",
                            &[
                                ("a", &name(a)),
                                ("b", &name(b)),
                                ("n", &n.to_string()),
                                ("total", &total.to_string()),
                            ],
                        ))
                        .color(theme.palette.text_secondary),
                    );
                });
                cell(ui, BUTTON_W, ROW_H - 8.0, true, |ui| {
                    clicked = button(ui, theme, None, tl("Copy for both"), false).clicked();
                });
            });
        });
    clicked
}

/// With a `CODEOWNERS`: how the file and the history agree on these files.
fn agreement(ui: &mut Ui, theme: &Theme, s: &Suggestions) {
    if !matches!(s.source, Source::File(_)) {
        return;
    }
    let (known, stale, none) = crate::owners::agreement(s);
    let figures: Vec<(String, egui::Color32)> = [
        (
            known,
            tn(
                known as u64,
                "{n} file with an owner who knows it",
                "{n} files with an owner who knows them",
            ),
            theme.tones.teal.fg,
        ),
        (
            stale,
            tn(
                stale as u64,
                "{n} whose owners did not touch it lately",
                "{n} whose owners did not touch them lately",
            ),
            theme.tones.amber.fg,
        ),
        (
            none,
            tn(none as u64, "{n} without an owner", "{n} without an owner"),
            theme.palette.text_muted,
        ),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(_, text, color)| (text, color))
    .collect();
    if figures.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        ui.label(muted(theme, tl("CODEOWNERS against the history:")));
        for (text, color) in figures {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                super::widgets::dot(ui, color, 8.0);
                ui.label(RichText::new(text).color(color));
            });
        }
    });
}

/// The files only the author knows, or nobody, path and reason in columns.
fn lonely(ui: &mut Ui, theme: &Theme, s: &Suggestions) {
    let lonely: Vec<_> = s
        .files
        .iter()
        .filter(|f| f.only_author || f.unknown)
        .collect();
    if lonely.is_empty() {
        return;
    }
    ui.add_space(10.0);
    let amber = theme.tones.amber;
    let font = egui::FontId::monospace(theme.text_size - 1.5);
    let path_w = lonely
        .iter()
        .map(|f| {
            ui.painter()
                .layout_no_wrap(f.path.clone(), font.clone(), theme.palette.text)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    egui::Frame::new()
        .fill(amber.bg)
        .stroke(egui::Stroke::new(1.0, amber.fg.gamma_multiply(0.3)))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(tl("Known by nobody else yet"))
                    .font(fonts::medium(theme.text_size))
                    .color(amber.fg),
            );
            ui.add_space(4.0);
            let room = ui.available_width();
            let path_w = path_w.min(room * 0.6);
            for f in lonely {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    cell(ui, path_w, 20.0, false, |ui| {
                        line(ui, mono(&f.path, theme.text_size - 1.5));
                    });
                    let why = if f.only_author {
                        tl("only ever changed by the author")
                    } else {
                        tl("no merged MR touched it lately")
                    };
                    line(ui, muted(theme, why));
                });
            }
        });
}

/// Each file against the owners and the people, folded: a matrix.
fn by_file(ui: &mut Ui, theme: &Theme, s: &Suggestions, mr_id: &str) {
    ui.add_space(8.0);
    egui::CollapsingHeader::new(RichText::new(tl("By file")).font(fonts::medium(theme.text_size)))
        .id_salt(("reviewers-by-file", mr_id))
        .default_open(false)
        .show(ui, |ui| matrix(ui, theme, s));
}

/// A file's owners: each person by name with their avatar (groups, roles
/// and emails as written), the deduced ones marked so.
fn owners_cell(ui: &mut Ui, theme: &Theme, s: &Suggestions, f: &crate::owners::FileRow) {
    ui.spacing_mut().item_spacing.x = 5.0;
    let size = theme.text_size - 1.0;
    let (owners, deduced): (Vec<String>, bool) = if !f.declared.is_empty() {
        (f.declared.clone(), false)
    } else if !f.deduced.is_empty() {
        (f.deduced.iter().map(|l| format!("@{l}")).collect(), true)
    } else {
        line(ui, muted(theme, tl("no owner")).size(size));
        return;
    };
    let color = if deduced {
        theme.tones.purple.fg
    } else {
        theme.palette.text
    };
    for (i, owner) in owners.iter().enumerate() {
        if i > 0 {
            ui.add_space(4.0);
        }
        match crate::owners::owner_login(owner) {
            Some(login) => {
                let name = s.names.get(login).map_or(login, String::as_str);
                avatar(ui, theme, name, 16.0);
                line(ui, RichText::new(name).size(size).color(color)).tip(owner);
            }
            // A group, a role or an email: no one person to show.
            None => {
                line(ui, mono(owner, size - 0.5).color(theme.tones.teal.fg));
            }
        }
    }
    if deduced {
        line(
            ui,
            muted(theme, format!("({})", tl("deduced"))).size(size - 0.5),
        );
    }
}

fn matrix(ui: &mut Ui, theme: &Theme, s: &Suggestions) {
    // The people suggested first, then the others who know a file (the
    // reviewers already asked).
    let mut people: Vec<&str> = s.people.iter().map(|p| p.login.as_str()).collect();
    for f in &s.files {
        for k in f.knowers.iter().chain(&f.near) {
            if !people.contains(&k.as_str()) {
                people.push(k);
            }
        }
    }
    people.truncate(MATRIX_PEOPLE);
    let name = |login: &str| {
        s.names
            .get(login)
            .cloned()
            .unwrap_or_else(|| login.to_string())
    };
    let width = ui.available_width();
    let path_w = (width - OWNERS_W - PERSON_W * people.len() as f32 - 2.0 * 8.0).max(160.0);
    let header_h = 26.0;
    let row_h = 26.0;
    ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
    ui.horizontal(|ui| {
        cell(ui, path_w, header_h, false, |ui| {
            ui.label(muted(theme, tl("File")).size(theme.text_size - 2.0));
        });
        cell(ui, OWNERS_W, header_h, false, |ui| {
            ui.label(muted(theme, tl("Owners")).size(theme.text_size - 2.0));
        });
        ui.spacing_mut().item_spacing.x = 0.0;
        for login in &people {
            cell(ui, PERSON_W, header_h, false, |ui| {
                ui.add_space((PERSON_W - 20.0) / 2.0);
                let n = name(login);
                avatar(ui, theme, &n, 20.0).tip(n);
            });
        }
    });
    for (i, f) in s.files.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            egui::Stroke::new(1.0, theme.palette.border_soft),
        );
        let _ = i;
        ui.horizontal(|ui| {
            cell(ui, path_w, row_h, false, |ui| {
                line(ui, mono(&f.path, theme.text_size - 1.5)).tip(&f.path);
            });
            cell(ui, OWNERS_W, row_h, false, |ui| {
                owners_cell(ui, theme, s, f)
            });
            ui.spacing_mut().item_spacing.x = 0.0;
            for login in &people {
                let knows = f.knowers.iter().any(|k| k == login);
                let near = f.near.iter().any(|k| k == login);
                cell(ui, PERSON_W, row_h, false, |ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(PERSON_W, row_h), Sense::hover());
                    let center = r.center();
                    let teal = theme.tones.teal.fg;
                    if knows {
                        ui.painter().circle_filled(center, 4.5, teal);
                    } else if near {
                        ui.painter().circle_stroke(
                            center,
                            4.0,
                            egui::Stroke::new(1.5, teal.gamma_multiply(0.7)),
                        );
                    }
                });
            }
        });
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let teal = theme.tones.teal.fg;
        let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.5, teal);
        ui.label(muted(theme, tl("changed, reviewed or approved it")).size(theme.text_size - 1.5));
        ui.add_space(12.0);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().circle_stroke(
            r.center(),
            4.0,
            egui::Stroke::new(1.5, teal.gamma_multiply(0.7)),
        );
        ui.label(muted(theme, tl("worked in its folder")).size(theme.text_size - 1.5));
    });
}
