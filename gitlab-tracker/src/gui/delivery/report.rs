//! The inspector beside the page, the DORA explainer and the weekly Markdown report.

use super::*;

pub(super) fn inspector(g: &mut GuiApp, ui: &mut Ui, d: &Delivery, now: DateTime<Utc>) {
    let theme = g.theme;
    let path = g.delivery.selected.as_deref().and_then(|id| {
        d.merged
            .iter()
            .chain(d.open_paths.iter())
            .find(|p| p.id == id)
    });
    let Some(path) = path else {
        about_dora(ui, &theme);
        return;
    };
    if board::inspector_title(ui, &theme, &format!("!{}", path.id)) {
        g.delivery.selected = None;
        return;
    }
    let mut open = false;
    board::section(ui, &theme, "", "", |ui| {
        ui.add(
            egui::Label::new(super::super::widgets::semibold(
                &path.title,
                theme.text_size,
            ))
            .wrap(),
        );
        ui.add_space(4.0);
        ui.label(muted(&theme, split_person(&path.author).0));
        ui.add_space(8.0);
        open = button(ui, &theme, None, tl("Open the MR"), false).clicked();
    });
    let stages = path.stages(now);
    board::section(ui, &theme, tl("Its way"), "", |ui| {
        let parts: Vec<(f32, Color32)> = Stage::ALL
            .iter()
            .map(|s| (stages[*s as usize].unwrap_or(0.0) as f32, s.tone(&theme).fg))
            .collect();
        board::stacked_bar(ui, &theme, &parts, 10.0, 1.0);
        ui.add_space(8.0);
        let rows: Vec<(&str, String)> = Stage::ALL
            .iter()
            .map(|s| {
                (
                    s.label(),
                    stages[*s as usize]
                        .map_or_else(|| board::dash().to_string(), board::duration_text),
                )
            })
            .collect();
        board::facts(ui, &theme, &rows);
    });
    board::section(ui, &theme, tl("Figures"), "", |ui| {
        let mut rows = vec![(tl("Opened"), board::ago(path.created, now))];
        if let Some(m) = path.merged {
            rows.push((tl("Merged"), board::ago(m, now)));
        }
        if let Some(t) = path.total() {
            rows.push((tl("Opening to production"), board::duration_text(t)));
        }
        rows.push((
            tl("Review rounds"),
            path.rounds
                .map_or_else(|| board::dash().to_string(), |n| n.to_string()),
        ));
        if path.merged.is_none() {
            let age = board::days_between(path.created, now);
            let against = match d.p85 {
                Some(p) if age >= p => tl("older than 85 % of the merged MRs").to_string(),
                Some(_) => match d.p50 {
                    Some(p) if age >= p => tl("older than half the merged MRs").to_string(),
                    _ => tl("within the usual time").to_string(),
                },
                None => board::dash().to_string(),
            };
            rows.push((tl("Against the others"), against));
        }
        board::facts(ui, &theme, &rows);
    });
    if open {
        let id = path.id.clone();
        g.open_mr(&id);
    }
}

pub(super) fn about_dora(ui: &mut Ui, theme: &Theme) {
    board::section(ui, theme, tl("The DORA measures"), "", |ui| {
        ui.add(
            egui::Label::new(muted(
                theme,
                tl("Four measures of a team's delivery, from the DORA research. Each is placed in one of four bands, from Elite to Low."),
            ))
            .wrap(),
        );
    });
    let rows: [(&str, [&str; 4]); 4] = [
        (
            "Deployment frequency",
            ["daily or more", "weekly", "monthly", "less"],
        ),
        (
            "Lead time for changes",
            ["< 1 day", "< 1 week", "< 1 month", "more"],
        ),
        ("Change failure rate", ["≤ 5 %", "≤ 10 %", "≤ 15 %", "more"]),
        ("Time to restore", ["< 1 h", "< 1 day", "< 1 week", "more"]),
    ];
    for (title, bands) in rows {
        board::section(ui, theme, tl(title), "", |ui| {
            let levels = [Level::Elite, Level::High, Level::Medium, Level::Low];
            for (level, band) in levels.iter().zip(bands) {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(line.left(), line.center().y),
                    level.label(),
                    level.tone(theme),
                    false,
                );
                board::paint_text(
                    ui,
                    egui::pos2(line.left() + w.max(64.0) + 10.0, line.center().y),
                    Align2::LEFT_CENTER,
                    tl(band),
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text_secondary,
                    line.width() - 80.0,
                );
            }
        });
    }
    board::section(ui, theme, tl("How they are measured"), "", |ui| {
        for text in [
            tl("Deployments: the successful deployments of the last tracked branch, else the MRs that reached it."),
            tl("Lead time: from an MR's first commit to its arrival on the last branch."),
            tl("Failures: failed deployments, reverts and hotfixes, over the deployments."),
            tl("Restore: from a failed deployment to the next good one, or from a reverted MR to its revert."),
        ] {
            ui.add(egui::Label::new(muted(theme, text).size(theme.text_size - 1.0)).wrap());
            ui.add_space(4.0);
        }
    });
}

// ── The weekly report ────────────────────────────────────────────────────────

/// The week as Markdown, to paste in a channel or a wiki: DORA, where the
/// time went, review rounds, CI, the MRs running late and the milestones.
pub fn weekly_report(
    app: &App,
    overview: Option<&ProjectOverview>,
    ci: &super::super::ci::Ci,
    now: DateTime<Utc>,
) -> String {
    use std::fmt::Write;
    let d = analyze(app, overview, now, 7);
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let dash = board::dash();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## {}",
        tf(
            "Weekly report · {project} · week of {date}",
            &[
                ("project", &project),
                (
                    "date",
                    &(now - Duration::days(6)).format("%Y-%m-%d").to_string()
                ),
            ],
        )
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "### {}", tl("Delivery"));
    let dora = &d.dora;
    let band = |level: Option<Level>| {
        level
            .map(|l| format!(" ({})", l.label()))
            .unwrap_or_default()
    };
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Deployment frequency"),
        tf("{n} / week", &[("n", &format!("{:.1}", dora.per_week))]),
        band((dora.per_week > 0.0).then(|| Level::of_frequency(dora.per_week)))
    );
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Lead time for changes"),
        dora.lead
            .map_or_else(|| dash.to_string(), board::duration_text),
        band(dora.lead.map(Level::of_lead))
    );
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Change failure rate"),
        dora.failure_rate
            .map_or_else(|| dash.to_string(), board::percent),
        band(dora.failure_rate.map(Level::of_failure_rate))
    );
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Time to restore"),
        dora.restore
            .map_or_else(|| dash.to_string(), board::duration_text),
        band(dora.restore.map(Level::of_restore))
    );
    let _ = writeln!(
        out,
        "- {}",
        tn(d.merged.len() as u64, "{n} MR merged", "{n} MRs merged")
    );
    if let Some((stage, share)) = slowest(&d.medians) {
        let _ = writeln!(
            out,
            "- {}",
            tf(
                "Most of the time: {stage}, {share} of the way",
                &[
                    ("stage", &stage.label().to_lowercase()),
                    ("share", &board::percent(share)),
                ],
            )
        );
    }
    for line in [drafts_text(&d), never_draft_text(&d)]
        .into_iter()
        .flatten()
    {
        let _ = writeln!(out, "- {line}");
    }
    if d.rounds.reviewed > 0 {
        let _ = writeln!(
            out,
            "- {}",
            tf(
                "Approved at the first round: {share} of the reviewed MRs",
                &[(
                    "share",
                    &board::percent(d.rounds.first_time as f64 / d.rounds.reviewed as f64)
                )],
            )
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "### {}", tl("CI"));
    let _ = writeln!(
        out,
        "- {}: {}",
        tl("Pipelines passing"),
        ci.success.map_or_else(|| dash.to_string(), board::percent)
    );
    let _ = writeln!(
        out,
        "- {}: {}",
        tl("Median duration"),
        ci.median
            .map_or_else(|| dash.to_string(), board::seconds_text)
    );
    let _ = writeln!(
        out,
        "- {}: {}",
        tl("Runner time"),
        super::super::ci::hours_text(ci.budget.runner_secs)
    );
    let _ = writeln!(out);
    let late: Vec<&Aging> = d
        .open
        .iter()
        .filter(|a| d.p85.is_some_and(|p| a.age >= p))
        .collect();
    let _ = writeln!(out, "### {}", tl("Running late"));
    if late.is_empty() {
        let _ = writeln!(out, "- {}", tl("No open MR is older than usual."));
    }
    for a in late {
        let _ = writeln!(
            out,
            "- !{} {} — {} · {}",
            a.id,
            a.title,
            a.column.label(),
            tf("open for {time}", &[("time", &board::duration_text(a.age))]),
        );
    }
    let _ = writeln!(out);
    let weekly = super::super::milestones::weekly_merges(app, overview, now);
    let milestones: Vec<_> = super::super::milestones::milestones(app)
        .into_iter()
        .filter(|m| !m.title.is_empty() && !m.is_done())
        .collect();
    if !milestones.is_empty() {
        let _ = writeln!(out, "### {}", tl("Milestones"));
        for m in milestones {
            let f = super::super::milestones::forecast(&m, &weekly, now);
            let due = m
                .due
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| dash.to_string());
            let odds = f
                .chance
                .map(|c| {
                    format!(
                        " · {}",
                        tf("{p} chance to make it", &[("p", &board::percent(c))])
                    )
                })
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "- {} — {} {due} · {}{odds}",
                m.title,
                tl("due"),
                f.p85.map_or_else(
                    || tl("no forecast").to_string(),
                    |p| tf(
                        "85 % sure by {date}",
                        &[("date", &p.format("%Y-%m-%d").to_string())]
                    )
                ),
            );
        }
    }
    out
}
