//! Why it cannot merge: merge checks, signatures, blockers, commands, conflicting files.

use super::*;

/// A merge check: met, failed, or a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Met,
    Blocks,
    Warns,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub verdict: Verdict,
    pub what: String,
    pub why: String,
    /// GitLab logins shown after `what`, by name with their avatar.
    pub who: Vec<String>,
    /// The files in conflict, under the conflict.
    pub files: Vec<ConflictFile>,
    /// "N commits behind": a click lists them.
    pub behind: bool,
    /// A command that fixes it, shown to copy.
    pub command: Option<String>,
    /// About the commits: a link opens the Commits tab.
    pub to_commits: bool,
    /// Another MR it is about: a button opens its page.
    pub open_mr: Option<String>,
}

/// What the commits' signatures warn about, when the project enforces signed
/// commits: a line per kind of anomaly, never blocking (whether to merge is
/// the team's call), else a line that every commit is signed.
pub fn signature_checks(mr: &TrackedMr, report: &crate::signatures::Report) -> Vec<Check> {
    let summary = report.summary();
    let shas = |list: &[String]| {
        list.iter()
            .map(|s| s.get(..8).unwrap_or(s))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let check = |verdict, what: String, why: String, command: Option<String>| Check {
        verdict,
        what,
        why,
        who: Vec::new(),
        files: Vec::new(),
        behind: false,
        command,
        to_commits: true,
        open_mr: None,
    };
    let mut list = Vec::new();
    // From the branch as GitLab has it, signed again over the target, pushed
    // back in place of the unsigned one (refused if someone pushed meanwhile).
    let resign = format!(
        "git fetch origin\n\
         git switch {src}\n\
         git merge --ff-only origin/{src}\n\
         git rebase --exec 'git commit --amend --no-edit -S' origin/{target}\n\
         git push --force-with-lease origin {src}",
        src = mr.source_branch,
        target = mr.target_branch
    );
    if !summary.unsigned.is_empty() {
        list.push(check(
            Verdict::Warns,
            tn(
                summary.unsigned.len() as u64,
                "{n} commit without a signature",
                "{n} commits without a signature",
            ),
            tf(
                "{shas}: no signature, and no earlier version of the MR held a signed copy. \
                 Sign the branch again, then force-push:",
                &[("shas", &shas(&summary.unsigned))],
            ),
            Some(resign.clone()),
        ));
    }
    if !summary.system.is_empty() {
        list.push(check(
            Verdict::Warns,
            tn(
                summary.system.len() as u64,
                "{n} commit signed by GitLab only",
                "{n} commits signed by GitLab only",
            ),
            tf(
                "{shas}: signed with the instance's key, not its author's. \
                 Sign the branch again, then force-push:",
                &[("shas", &shas(&summary.system))],
            ),
            Some(resign),
        ));
    }
    if !summary.unverified.is_empty() {
        list.push(check(
            Verdict::Warns,
            tn(
                summary.unverified.len() as u64,
                "{n} signature not verified",
                "{n} signatures not verified",
            ),
            tf(
                "{shas}: GitLab cannot verify it (unknown or revoked key, another user's, \
                 another e-mail). The author's public key must be in their GitLab profile.",
                &[("shas", &shas(&summary.unverified))],
            ),
            None,
        ));
    }
    if summary.anomalies() == 0 && summary.pending == 0 {
        let mut why = Vec::new();
        if summary.rebased > 0 {
            why.push(tn(
                summary.rebased as u64,
                "{n} lost its signature in a rebase from GitLab, signed before it",
                "{n} lost their signature in a rebase from GitLab, signed before it",
            ));
        }
        if summary.exempt > 0 {
            why.push(tn(
                summary.exempt as u64,
                "{n} from an exempted bot",
                "{n} from an exempted bot",
            ));
        }
        list.push(check(
            Verdict::Met,
            tl("Every commit signed").into(),
            why.join(" · "),
            None,
        ));
    }
    list
}

/// Worst first: what blocks, what warns, what is met.
pub(super) fn rank(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::Blocks => 0,
        Verdict::Warns => 1,
        Verdict::Met => 2,
    }
}

/// What stands between the MR and its merge, worst first, then what is met.
pub fn checks(
    mr: &TrackedMr,
    overview: Option<&Overview>,
    rules: Option<&[Rule]>,
    threads: Option<&[Thread]>,
    conflicts: Option<&[ConflictFile]>,
) -> Vec<Check> {
    let status = overview.map_or("", |o| o.detailed_merge_status.as_str());
    let mut list = Vec::new();
    let mut push = |verdict, what: String, why: String| {
        list.push(Check {
            verdict,
            what,
            why,
            who: Vec::new(),
            files: Vec::new(),
            behind: false,
            command: None,
            to_commits: false,
            open_mr: None,
        })
    };

    let draft = overview.is_some_and(|o| o.draft) || mr.mergeability == MergeabilityStatus::Draft;
    if draft {
        push(
            Verdict::Blocks,
            tl("Still a draft").into(),
            tl("Mark it as ready in GitLab once it is.").into(),
        );
    }
    let conflict = overview.is_some_and(|o| o.has_conflicts)
        || mr.mergeability == MergeabilityStatus::Conflict;
    let conflict_title = tf("Conflicts with {target}", &[("target", &mr.target_branch)]);
    if conflict {
        push(
            Verdict::Blocks,
            conflict_title.clone(),
            tf(
                "Rebase {source} on {target}, or merge {target} into it, and resolve them.",
                &[("source", &mr.source_branch), ("target", &mr.target_branch)],
            ),
        );
    }
    if mr.mergeability == MergeabilityStatus::RequestedChanges || status == "requested_changes" {
        push(
            Verdict::Blocks,
            tl("Changes requested").into(),
            tl("A reviewer asked for changes: their review must be withdrawn or redone.").into(),
        );
    }
    match mr.pipelines.first() {
        Some(p) if p.status == PipelineState::Failed => {
            let failed: Vec<&str> = p
                .jobs
                .iter()
                .filter(|j| j.status == "failed")
                .map(|j| j.name.as_str())
                .collect();
            let why = if failed.is_empty() {
                tf("Pipeline #{id}.", &[("id", &p.id.to_string())])
            } else {
                tf(
                    "Pipeline #{id}: {jobs}. Its log is in the Pipelines tab.",
                    &[("id", &p.id.to_string()), ("jobs", &failed.join(", "))],
                )
            };
            push(Verdict::Blocks, tl("Pipeline failed").into(), why);
        }
        Some(p) if matches!(p.status, PipelineState::Running | PipelineState::Pending) => {
            push(
                Verdict::Warns,
                tl("Pipeline still running").into(),
                tf("Pipeline #{id}.", &[("id", &p.id.to_string())]),
            );
        }
        Some(p) if p.status == PipelineState::Success => {
            push(Verdict::Met, tl("Pipeline passed").into(), String::new());
        }
        _ => {}
    }
    let open = match threads {
        Some(threads) => threads.iter().filter(|t| t.is_open()).count(),
        None => mr.user_notes_count as usize,
    };
    if open > 0 {
        let blocking = overview.is_some_and(|o| !o.blocking_discussions_resolved)
            || status == "discussions_not_resolved";
        push(
            if blocking {
                Verdict::Blocks
            } else {
                Verdict::Warns
            },
            tn(
                open as u64,
                "{n} unresolved thread",
                "{n} unresolved threads",
            ),
            if blocking {
                tl("The project requires every thread resolved before merging.").into()
            } else {
                tl("They do not block the merge here.").into()
            },
        );
    } else {
        push(
            Verdict::Met,
            tl("Every thread resolved").into(),
            String::new(),
        );
    }
    // Plain comments cannot be resolved and never block, but one the author
    // has not answered yet may still ask for something.
    let unanswered = threads.map_or(0, |threads| {
        threads
            .iter()
            .filter(|t| !t.resolvable)
            .filter(|t| {
                t.notes.last().is_some_and(|last| {
                    !crate::utils::matches_gitlab_username(&mr.author, &last.username)
                })
            })
            .count()
    });
    if unanswered > 0 {
        push(
            Verdict::Warns,
            tn(
                unanswered as u64,
                "{n} general comment unanswered",
                "{n} general comments unanswered",
            ),
            tl("They do not block the merge, but may ask for something.").into(),
        );
    }
    match rules {
        Some(rules) => {
            let met = rules.iter().filter(|r| r.approved).count();
            if met == rules.len() {
                push(
                    Verdict::Met,
                    tl("Every approval rule met").into(),
                    String::new(),
                );
            } else {
                let waiting: Vec<String> = rules
                    .iter()
                    .filter(|r| !r.approved)
                    .map(|r| {
                        let who: Vec<&str> = r
                            .eligible_approvers
                            .iter()
                            .map(|u| u.name.as_str())
                            .take(3)
                            .collect();
                        if who.is_empty() {
                            format!("{} ({})", r.name, r.approvals_required)
                        } else {
                            format!("{} ({}: {})", r.name, r.approvals_required, who.join(", "))
                        }
                    })
                    .collect();
                push(
                    Verdict::Warns,
                    tn(
                        rules.len() as u64,
                        "Approvals: {met} of {n} rule met",
                        "Approvals: {met} of {n} rules met",
                    )
                    .replace("{met}", &met.to_string()),
                    waiting.join(" · "),
                );
            }
        }
        None if !mr.approved_by.is_empty() => {
            push(Verdict::Met, tl("Approved by").into(), String::new());
        }
        None if status == "not_approved" || mr.mergeability == MergeabilityStatus::NotApproved => {
            push(Verdict::Warns, tl("Not approved yet").into(), String::new());
        }
        None => {}
    }
    let behind = overview
        .and_then(|o| o.diverged_commits_count)
        .or(mr.diff_stats.as_ref().and_then(|s| s.commits_behind));
    let mut behind_title = None;
    if let Some(n) = behind.filter(|n| *n > 0) {
        let title = tn(
            n as u64,
            "{n} commit behind {target}",
            "{n} commits behind {target}",
        )
        .replace("{target}", &mr.target_branch);
        behind_title = Some(title.clone());
        let rebase = status == "need_rebase" || mr.mergeability == MergeabilityStatus::NeedsRebase;
        push(
            if rebase {
                Verdict::Blocks
            } else {
                Verdict::Warns
            },
            title,
            if rebase {
                tl("The project merges fast-forward only: rebase first.").into()
            } else {
                tl("It merges anyway; a rebase tests it against the latest code.").into()
            },
        );
    }
    if !draft {
        push(Verdict::Met, tl("Not a draft").into(), String::new());
    }
    if overview.and_then(|o| o.force_remove_source_branch) == Some(true) {
        push(
            Verdict::Met,
            tl("The source branch is deleted after the merge").into(),
            String::new(),
        );
    }
    if let Some(check) = list
        .iter_mut()
        .find(|c| Some(&c.what) == behind_title.as_ref())
    {
        check.behind = true;
    }
    if let Some(check) = list.iter_mut().find(|c| c.what == conflict_title) {
        check.files = conflicts.unwrap_or_default().to_vec();
    }
    // The approvers by name, next to "Approved by".
    if let Some(check) = list.iter_mut().find(|c| c.what == tl("Approved by")) {
        check.who = mr.approved_by.clone();
    }
    list.sort_by_key(|c| rank(c.verdict));
    list
}

/// What a click in the checks asks for.
#[derive(Default)]
pub(super) struct Picked {
    /// The file in conflict, to show in Changes.
    pub(super) file: Option<String>,
    /// The MR picked in the "behind" popup, to open.
    pub(super) mr: Option<String>,
    /// The Commits tab, for the signatures.
    pub(super) commits: bool,
    /// A command was copied: the window says so.
    pub(super) copied: bool,
}

/// The checks, worst first, and what was clicked in them.
pub(super) fn blockers(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr, data: Option<&PageData>) -> Picked {
    let theme = g.theme;
    let mut list = checks(
        mr,
        data.and_then(|d| d.overview.as_ref()),
        data.and_then(|d| d.rules.as_deref()),
        data.and_then(|d| d.threads.as_deref().ok()),
        data.and_then(|d| d.conflicts.as_deref()),
    );
    if let Some(report) = g.app.signature_report(mr) {
        list.extend(signature_checks(mr, &report));
    }
    // First among the warnings: it says why "N commits behind" matters now.
    let mut ahead = conflicts_ahead(&g.app, mr);
    ahead.append(&mut list);
    let mut list = ahead;
    list.sort_by_key(|c| rank(c.verdict));
    let mut picked = None;
    let mut open = None;
    let mut to_commits = false;
    let mut copied = false;
    let ready = !list.iter().any(|c| c.verdict != Verdict::Met);
    let title = if ready {
        tl("Nothing blocks the merge")
    } else {
        tl("Why it can't merge")
    };
    let stroke = if ready {
        theme.tones.green.fg
    } else {
        theme.tones.red.fg
    };
    theme
        .card_frame()
        .stroke(egui::Stroke::new(1.0, stroke.gamma_multiply(0.45)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(semibold(title, theme.text_size));
                if data.is_none() {
                    spinner(ui, 12.0, theme.palette.accent);
                    ui.label(muted(&theme, tl("Asking GitLab…")));
                }
            });
            ui.add_space(6.0);
            for (i, check) in list.iter().enumerate() {
                if i > 0 {
                    super::super::widgets::rule(ui, &theme);
                }
                let (glyph, color) = match check.verdict {
                    Verdict::Met => ("✓", theme.tones.green.fg),
                    Verdict::Blocks => ("✕", theme.tones.red.fg),
                    Verdict::Warns => ("!", theme.tones.amber.fg),
                };
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add_sized(
                        [14.0, 18.0],
                        egui::Label::new(
                            RichText::new(glyph)
                                .font(fonts::semibold(theme.text_size))
                                .color(color),
                        ),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let what = RichText::new(&check.what).font(fonts::medium(theme.text_size));
                        if check.behind {
                            // A link: the commits it lacks, in a popup.
                            let response = ui
                                .add(
                                    egui::Label::new(what.underline())
                                        .sense(Sense::click())
                                        .selectable(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .tip(tl("See what it lacks"));
                            if let Some(id) = behind_popup(g, &response, mr, data) {
                                open = Some(id);
                            }
                        } else if check.who.is_empty() {
                            ui.label(what);
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                // As tall as a text line, like the checks around.
                                ui.spacing_mut().interact_size.y = 18.0;
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(what);
                                for login in &check.who {
                                    person_chip(ui, &theme, &display_of(mr, data, login));
                                }
                            });
                        }
                        if !check.why.is_empty() {
                            ui.label(muted(&theme, &check.why));
                        }
                        if let Some(command) = &check.command {
                            copied |= command_box(ui, &theme, command);
                        }
                        if let Some(id) = &check.open_mr {
                            ui.add_space(4.0);
                            if button(
                                ui,
                                &theme,
                                Some(Icon::MergeRequests),
                                &tf("See !{id}", &[("id", id)]),
                                false,
                            )
                            .clicked()
                            {
                                open = Some(id.clone());
                            }
                        }
                        if check.to_commits && check.verdict != Verdict::Met {
                            ui.add_space(4.0);
                            if button(
                                ui,
                                &theme,
                                Some(Icon::Shield),
                                tl("See them in Commits"),
                                false,
                            )
                            .clicked()
                            {
                                to_commits = true;
                            }
                        }
                        if !check.files.is_empty() {
                            picked = conflict_files(ui, &theme, &mr.id, &check.files);
                        }
                    });
                });
            }
        });
    Picked {
        file: picked,
        mr: open,
        commits: to_commits,
        copied,
    }
}

/// A warning per merged MR the branch will conflict with at its next rebase
/// or merge (see `overlaps::ahead`): GitLab does not see it yet.
pub fn conflicts_ahead(app: &crate::app::App, mr: &TrackedMr) -> Vec<Check> {
    super::super::overlaps::ahead(app, mr)
        .into_iter()
        .map(|a| {
            let lines: Vec<String> = a
                .files
                .iter()
                .map(|f| {
                    tf(
                        "{path}: lines {theirs} there, {mine} here",
                        &[
                            ("path", &f.path),
                            ("theirs", &super::super::overlaps::lines_text(&f.theirs)),
                            ("mine", &super::super::overlaps::lines_text(&f.mine)),
                        ],
                    )
                })
                .collect();
            Check {
                verdict: Verdict::Warns,
                what: tf(
                    "A conflict is coming with {target}",
                    &[("target", &mr.target_branch)],
                ),
                why: format!(
                    "{}\n{}",
                    tf(
                        "GitLab sees no conflict yet: it compares with {target} as of the last push. \
                         Since then !{id} {title}, merged {when}, changed the same lines: the next \
                         rebase or merge stops on them.",
                        &[
                            ("target", &mr.target_branch),
                            ("id", &a.merged),
                            ("title", &a.title),
                            ("when", &crate::utils::format_relative_date(&a.merged_at.to_rfc3339())),
                        ],
                    ),
                    lines.join("\n")
                ),
                who: Vec::new(),
                files: Vec::new(),
                behind: false,
                command: Some(super::super::next::rebase_commands(mr)),
                to_commits: false,
                open_mr: Some(a.merged),
            }
        })
        .collect()
}

/// A command to copy: monospace in a box, with the copy icon in its corner;
/// copied on click (`true` then).
pub(in crate::gui) fn command_box(ui: &mut Ui, theme: &Theme, command: &str) -> bool {
    const ICON: f32 = 14.0;
    ui.add_space(2.0);
    let response = egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.small))
        .inner_margin(Margin {
            left: 8,
            right: 8 + ICON as i8 + 6,
            top: 5,
            bottom: 5,
        })
        .show(ui, |ui| {
            ui.add(egui::Label::new(mono(command, theme.text_size - 1.5)).wrap());
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl("Copy"));
    let corner = egui::Rect::from_min_size(
        egui::pos2(
            response.rect.right() - 8.0 - ICON,
            response.rect.top() + 6.0,
        ),
        Vec2::splat(ICON),
    );
    // For a moment after a click, "✓ Copied" in place of the icon.
    let copied_at = response.id.with("copied at");
    let now = ui.input(|i| i.time);
    if response.clicked() {
        ui.ctx().copy_text(command.to_string());
        ui.data_mut(|d| d.insert_temp(copied_at, now));
    }
    let since = ui
        .data(|d| d.get_temp::<f64>(copied_at))
        .map(|at| now - at)
        .filter(|since| *since < COPIED_FOR);
    if let Some(since) = since {
        let galley = ui.painter().layout_no_wrap(
            format!("✓ {}", tc("clipboard", "Copied")),
            fonts::medium(theme.text_size - 1.5),
            theme.tones.green.fg,
        );
        let pos = egui::pos2(
            corner.right() - galley.size().x,
            corner.center().y - galley.size().y / 2.0,
        );
        let behind = egui::Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(6.0, 2.0));
        ui.painter().rect_filled(
            behind,
            CornerRadius::same(theme.radii.small),
            theme.palette.content,
        );
        ui.painter().galley(pos, galley, theme.tones.green.fg);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(COPIED_FOR - since));
    } else {
        let color = if response.hovered() {
            theme.palette.text
        } else {
            theme.palette.text_muted
        };
        super::super::icons::paint(ui.painter(), corner, Icon::Copy, color);
    }
    response.clicked()
}

/// How long a command box says it was copied, in seconds.
pub(super) const COPIED_FOR: f64 = 1.5;

/// How many files in conflict show at once; the box scrolls past them.
pub(super) const CONFLICTS_SHOWN: usize = 10;

/// The files in conflict, all of them, in a box that scrolls past
/// [`CONFLICTS_SHOWN`]; the files changed on both sides that merge anyway, on
/// one line. Returns the file clicked.
pub(super) fn conflict_files(
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    files: &[ConflictFile],
) -> Option<String> {
    let (clashes, others): (Vec<&ConflictFile>, Vec<&ConflictFile>) =
        files.iter().partition(|f| f.clash);
    let mut picked = None;
    ui.add_space(6.0);
    if !clashes.is_empty() {
        ui.label(
            RichText::new(tn(
                clashes.len() as u64,
                "{n} file in conflict",
                "{n} files in conflict",
            ))
            .font(fonts::medium(theme.text_size - 1.0))
            .color(theme.palette.text_secondary),
        )
        .tip(tl(
            "Worked out from what each side changed since they parted: the same or \
             adjacent lines. GitLab lists them exactly when you resolve the conflicts.",
        ));
        ui.add_space(2.0);
        let row = ui
            .painter()
            .layout_no_wrap("x".into(), conflict_font(theme), egui::Color32::WHITE)
            .size()
            .y
            + ui.spacing().item_spacing.y;
        egui::Frame::new()
            .fill(theme.palette.raised)
            .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
            .corner_radius(CornerRadius::same(theme.radii.small))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt(("mr page conflicts", mr_id))
                    .max_height(CONFLICTS_SHOWN as f32 * row - ui.spacing().item_spacing.y)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // Rows as tall as their text, not as a control.
                        ui.spacing_mut().interact_size.y = 0.0;
                        let tree = Folder::of(clashes.iter().map(|f| f.path.as_str()));
                        picked = tree_rows(ui, theme, mr_id, &tree, "", 0);
                    });
            });
    }
    if !others.is_empty() {
        ui.add_space(4.0);
        let names: Vec<&str> = others.iter().map(|f| file_name(&f.path)).collect();
        ui.add(
            egui::Label::new(muted(
                theme,
                format!(
                    "{} {}",
                    tn(
                        others.len() as u64,
                        "Also changed on both sides, likely merging on its own ({n}):",
                        "Also changed on both sides, likely merging on their own ({n}):",
                    ),
                    names.join(", ")
                ),
            ))
            .truncate()
            .show_tooltip_when_elided(false),
        )
        .rich(
            Tip::new().mono(
                others
                    .iter()
                    .map(|f| f.path.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        );
    }
    picked
}

pub(super) fn conflict_font(theme: &Theme) -> egui::FontId {
    egui::FontId::monospace(theme.text_size - 1.5)
}

/// A file's last path segment.
pub(super) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Indentation of one tree level.
pub(super) const TREE_INDENT: f32 = 16.0;

/// The rows of `folder` (at `path`, `depth` deep): each sub-folder (a chain of
/// folders that hold only one folder on one row, as `src/api/handlers`), its
/// content under it unless folded, then the files. Returns the file clicked.
pub(super) fn tree_rows(
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    folder: &Folder,
    path: &str,
    depth: usize,
) -> Option<String> {
    let mut picked = None;
    for (name, mut sub) in &folder.folders {
        let mut label = (*name).to_string();
        while sub.files.is_empty() && sub.folders.len() == 1 {
            let (next, deeper) = sub.folders.iter().next().expect("one folder");
            label = format!("{label}/{next}");
            sub = deeper;
        }
        let full = format!("{path}{label}/");
        let id = egui::Id::unique(("mr page conflict folder", mr_id, &full));
        let folded = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let chevron = if folded { "▸" } else { "▾" };
        let response = tree_row(
            ui,
            theme,
            depth,
            &format!("{chevron} {label}/"),
            theme.palette.text_muted,
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            ui.data_mut(|d| d.insert_temp(id, !folded));
        }
        if !folded {
            picked = tree_rows(ui, theme, mr_id, sub, &full, depth + 1).or(picked);
        }
    }
    for path in &folder.files {
        let response = tree_row(ui, theme, depth, file_name(path), theme.tones.red.fg)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .rich(Tip::new().mono(*path).hint(tl("Show it in Changes")));
        if response.clicked() {
            picked = Some((*path).to_string());
        }
    }
    picked
}

/// One row of the tree, indented, with a guide for each level above it.
pub(super) fn tree_row(
    ui: &mut Ui,
    theme: &Theme,
    depth: usize,
    text: &str,
    color: egui::Color32,
) -> egui::Response {
    let indent = depth as f32 * TREE_INDENT;
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(conflict_font(theme), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(ui.available_width() - indent);
    let response = ui
        .horizontal(|ui| {
            ui.add_space(indent);
            ui.add(egui::Label::new(job).sense(Sense::click()))
        })
        .inner;
    // The guides, through the item spacing so that they join from row to row.
    let gap = ui.spacing().item_spacing.y;
    let y = egui::Rangef::new(
        response.rect.top() - gap / 2.0,
        response.rect.bottom() + gap / 2.0,
    );
    for level in 0..depth {
        let x = response.rect.left() - indent + level as f32 * TREE_INDENT + 4.0;
        ui.painter()
            .vline(x, y, egui::Stroke::new(1.0, theme.palette.border));
    }
    response
}
