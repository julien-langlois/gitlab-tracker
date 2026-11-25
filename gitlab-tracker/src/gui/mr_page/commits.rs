//! The Commits tab: authors, signatures, presence on each branch.

use super::*;

/// The MR's commits, and where each one is.
pub(super) fn commits(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    if mr.commits.is_empty() {
        ui.label(muted(
            &theme,
            tl("The commits are read with the next refresh."),
        ));
        return;
    }
    let branches = g.app.branches.clone();
    // Fixed columns, the title taking the rest (never under TITLE_MIN: the
    // table scrolls sideways instead).
    const SHA: f32 = 76.0;
    const DATE_MIN: f32 = 104.0;
    const GAP: f32 = 16.0;
    const TITLE_MIN: f32 = 220.0;
    const ROW: f32 = 32.0;
    let branch_font = egui::FontId::monospace(theme.text_size - 2.0);
    let branch_widths: Vec<f32> = branches
        .iter()
        .map(|b| text_width(ui, b, branch_font.clone()).max(36.0))
        .collect();
    // The dates as shown: the column is as wide as the longest ("about a month
    // ago"), so that it never pushes the branches' columns of its row.
    let dates: Vec<String> = mr
        .commits
        .iter()
        .map(|commit| {
            chrono::DateTime::from_timestamp(commit.fingerprint.authored_at, 0)
                .map(|d| format_relative_date(&d.to_rfc3339()))
                .unwrap_or_default()
        })
        .collect();
    // Who wrote each commit (an MR is often shared): avatar and full name, the
    // column as wide as the longest name, up to AUTHOR_MAX.
    const AVATAR: f32 = 22.0;
    const AUTHOR_MAX: f32 = 200.0;
    let author_font = egui::FontId::proportional(theme.text_size - 1.0);
    let people = known_people(&g.app);
    let authors: Vec<(String, Option<String>)> = mr
        .commits
        .iter()
        .map(|c| commit_author(&people, c))
        .collect();
    let author_widths: Vec<f32> = authors
        .iter()
        .map(|(name, _)| AVATAR + 6.0 + text_width(ui, name, author_font.clone()).ceil() + 2.0)
        .collect();
    let author_width = author_widths
        .iter()
        .copied()
        .fold(
            text_width(ui, &tl("Author").to_uppercase(), author_font.clone()),
            f32::max,
        )
        .min(AUTHOR_MAX);
    let date_font = egui::FontId::proportional(theme.text_size - 1.0);
    let date_width = dates
        .iter()
        .map(|d| text_width(ui, d, date_font.clone()).ceil() + 2.0)
        .fold(DATE_MIN, f32::max);
    // Each commit's signature, when the project enforces signed commits.
    let report = g.app.signature_report(mr);
    let signatures: std::collections::HashMap<&str, &crate::signatures::CommitCheck> = report
        .iter()
        .flat_map(|r| &r.commits)
        .map(|c| (c.sha.as_str(), c))
        .collect();
    let sig_font = egui::FontId::proportional(theme.text_size - 1.5);
    let sig_width = report.as_ref().map_or(0.0, |r| {
        r.commits
            .iter()
            .map(|c| {
                let (glyph, label, _) = tones::signature(c, &theme.tones);
                text_width(ui, &format!("{glyph} {label}"), sig_font.clone()).ceil() + 16.0
            })
            .fold(
                text_width(ui, &tl("Signature").to_uppercase(), sig_font.clone()),
                f32::max,
            )
    });
    let sig_gap = if report.is_some() { GAP } else { 0.0 };
    let fixed_width = SHA
        + author_width
        + GAP
        + date_width
        + sig_width
        + sig_gap
        + branch_widths.iter().sum::<f32>()
        + GAP * (2 + branches.len()) as f32;
    let title = (ui.available_width() - fixed_width).max(TITLE_MIN);
    let total = fixed_width + title;

    egui::ScrollArea::horizontal()
        .id_salt(("mr_commits", &mr.id))
        .show(ui, |ui| {
            ui.set_width(total);
            ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.0);
            // Header.
            ui.horizontal(|ui| {
                ui.set_height(24.0);
                let head = |ui: &mut Ui, width: f32, text: &str| {
                    fixed(ui, width, |ui| {
                        ui.label(muted(&theme, text.to_uppercase()).size(theme.text_size - 2.5));
                    });
                };
                head(ui, SHA, "SHA");
                head(ui, title, tl("Title"));
                centered(ui, author_width, |ui| {
                    ui.label(
                        muted(&theme, tl("Author").to_uppercase()).size(theme.text_size - 2.5),
                    );
                });
                head(ui, date_width, tl("Authored"));
                if report.is_some() {
                    head(ui, sig_width, tl("Signature"));
                }
                for (b, w) in branches.iter().zip(&branch_widths) {
                    centered(ui, *w, |ui| {
                        ui.label(
                            RichText::new(b)
                                .font(branch_font.clone())
                                .color(theme.palette.text_muted),
                        );
                    });
                }
            });
            super::super::widgets::rule(ui, &theme);
            for (((commit, date), (author, display)), author_w) in mr
                .commits
                .iter()
                .zip(&dates)
                .zip(&authors)
                .zip(&author_widths)
            {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(total, ROW), Sense::hover());
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
                }
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                row.spacing_mut().item_spacing.x = GAP;
                fixed(&mut row, SHA, |ui| {
                    ui.label(
                        mono(
                            commit.sha.get(..8).unwrap_or(&commit.sha),
                            theme.text_size - 1.5,
                        )
                        .color(theme.palette.text_secondary),
                    );
                });
                fixed(&mut row, title, |ui| {
                    ui.add(
                        egui::Label::new(&commit.fingerprint.title)
                            .truncate()
                            .show_tooltip_when_elided(false),
                    )
                    .rich(
                        Tip::new()
                            .title(&commit.fingerprint.title)
                            .text(&commit.fingerprint.author_email),
                    );
                });
                fixed(&mut row, author_width, |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // Centred under the column's title, as one block.
                    ui.add_space(((author_width - author_w) / 2.0).max(0.0));
                    let author = author.as_str();
                    let who = || {
                        Tip::new()
                            .person(display.as_deref().unwrap_or(author))
                            .text(&commit.fingerprint.author_email)
                    };
                    avatar(ui, &theme, author, AVATAR).rich(who());
                    ui.add(
                        egui::Label::new(
                            RichText::new(author)
                                .font(author_font.clone())
                                .color(theme.palette.text_secondary),
                        )
                        .truncate()
                        .show_tooltip_when_elided(false),
                    )
                    .rich(who());
                });
                fixed(&mut row, date_width, |ui| {
                    ui.label(muted(&theme, date.as_str()));
                });
                if report.is_some() {
                    fixed(&mut row, sig_width, |ui| {
                        if let Some(check) = signatures.get(commit.sha.as_str()) {
                            signature_chip(ui, &theme, check, sig_font.clone());
                        }
                    });
                }
                for (b, w) in branches.iter().zip(&branch_widths) {
                    let presence = commit_presence(&g.app, mr, b, commit);
                    let (glyph, label, tone) = match presence {
                        Some(p) => tones::presence(Some(p), false, &theme.tones),
                        None => ("…", tl("Not checked yet"), theme.tones.gray),
                    };
                    centered(&mut row, *w, |ui| {
                        ui.label(RichText::new(glyph).color(tone.fg))
                            .rich(Tip::new().mono(b).chip(format!("{glyph} {label}"), tone));
                    });
                }
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom(),
                    egui::Stroke::new(1.0, theme.palette.border_soft),
                );
            }
        });
    ui.add_space(10.0);
    ui.label(muted(&theme, tl("● same SHA · ◆ cherry-picked with -x · ≈ copied (same author, date and title) · ○ absent")));
}

/// Everyone the tracking knows by "Full Name (@login)": lower-cased login to
/// that display string.
pub(super) fn known_people(app: &App) -> std::collections::HashMap<String, String> {
    let mut people = std::collections::HashMap::new();
    for mr in &app.mrs {
        let all = std::iter::once(&mr.author)
            .chain(&mr.reviewers)
            .chain(&mr.approvers)
            .chain(&mr.assignee)
            .chain(&mr.merged_by);
        for display in all {
            if let (name, Some(login)) = split_person(display) {
                if !name.trim().is_empty() {
                    people
                        .entry(login.to_lowercase())
                        .or_insert_with(|| display.clone());
                }
            }
        }
    }
    people
}

/// A commit's author by their GitLab full name when git recorded their login
/// (as its author name or its e-mail's local part), with their "Full Name
/// (@login)"; else as git recorded them.
pub(super) fn commit_author(
    people: &std::collections::HashMap<String, String>,
    commit: &crate::models::MrCommit,
) -> (String, Option<String>) {
    let email = &commit.fingerprint.author_email;
    let local = email.split('@').next().unwrap_or(email);
    [commit.author_name.trim(), local]
        .into_iter()
        .filter(|key| !key.is_empty())
        .find_map(|key| people.get(&key.to_lowercase()))
        .map_or_else(
            || (commit.author().to_string(), None),
            |display| (split_person(display).0.to_string(), Some(display.clone())),
        )
}

/// A commit's signature: glyph and label on its tone, why on hover.
pub(super) fn signature_chip(
    ui: &mut Ui,
    theme: &Theme,
    check: &crate::signatures::CommitCheck,
    font: egui::FontId,
) {
    let (glyph, label, tone) = tones::signature(check, &theme.tones);
    egui::Frame::new()
        .fill(tone.bg)
        .corner_radius(CornerRadius::same(theme.radii.small))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{glyph} {label}"))
                    .font(font)
                    .color(tone.fg),
            );
        })
        .response
        .rich(
            Tip::new()
                .chip(format!("{glyph} {label}"), tone)
                .text(tones::signature_detail(check)),
        );
}

/// A column `width` wide, its content centered.
pub(super) fn centered(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 20.0),
        Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

/// Where one of the MR's commits is on `branch` (`None`: not scanned yet).
pub(super) fn commit_presence(
    app: &App,
    mr: &TrackedMr,
    branch: &str,
    commit: &crate::models::MrCommit,
) -> Option<BranchPresence> {
    if matches!(&mr.status, MrStatus::MergedIn(found) if found.contains(branch)) {
        return Some(BranchPresence::Present);
    }
    let index = app.branch_commits.get(branch)?;
    Some(if index.shas.contains(&commit.sha) {
        BranchPresence::Present
    } else if index.picked_from.contains_key(&commit.sha) {
        BranchPresence::CherryPicked
    } else if index.fingerprints.contains_key(&commit.fingerprint) {
        BranchPresence::Copied
    } else {
        BranchPresence::Absent
    })
}
