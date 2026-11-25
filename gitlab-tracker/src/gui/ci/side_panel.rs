//! The panel beside the page (the selected job, or the project's overview) and the rail's badge.

use super::*;

pub(super) fn inspector(
    g: &mut GuiApp,
    ui: &mut Ui,
    ci: &Ci,
    overview: Option<&crate::gitlab_detail::ProjectOverview>,
    selected: Option<&str>,
    now: DateTime<Utc>,
) {
    let theme = g.theme;
    board::section(
        ui,
        &theme,
        tl("Tracked branches"),
        tl("latest pipeline"),
        |ui| {
            let Some(overview) = overview else {
                ui.horizontal(|ui| {
                    super::super::widgets::spinner(ui, 14.0, theme.palette.text_muted);
                    ui.label(muted(&theme, tl("Reading the branches' pipelines…")));
                });
                return;
            };
            let flow = g.app.branch_flow();
            let ordered: Vec<String> = flow.in_order().iter().map(|b| b.name.clone()).collect();
            for branch in &ordered {
                let pipeline = overview.pipelines.iter().find(|p| &p.branch == branch);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
                let (color, line2) = match pipeline {
                    Some(p) => {
                        let (label, tone) = super::super::tones::pipeline(p.status, &theme.tones);
                        let when = board::parse_time(&p.created_at)
                            .map(|t| board::ago(t, now))
                            .unwrap_or_default();
                        let jobs = if p.failed_jobs.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", p.failed_jobs.join(", "))
                        };
                        (
                            tone.fg,
                            format!("#{} · {}{jobs} · {when}", p.id, label.to_lowercase()),
                        )
                    }
                    None => (theme.palette.text_muted, tl("no pipeline").to_string()),
                };
                ui.painter().circle_filled(
                    egui::pos2(rect.left() + 4.0, rect.top() + 12.0),
                    4.0,
                    color,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 16.0, rect.top() + 12.0),
                    Align2::LEFT_CENTER,
                    branch,
                    egui::FontId::monospace(theme.text_size - 1.0),
                    theme.tones.blue.fg,
                    rect.width() - 16.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + 16.0, rect.top() + 30.0),
                    Align2::LEFT_CENTER,
                    &line2,
                    FontId::proportional(theme.text_size - 2.0),
                    if pipeline.is_some_and(|p| p.status == PipelineState::Failed) {
                        theme.tones.red.fg
                    } else {
                        theme.palette.text_muted
                    },
                    rect.width() - 16.0,
                );
            }
        },
    );
    let mut open = None;
    board::section(
        ui,
        &theme,
        tl("Held back by the CI"),
        &ci.blocked.len().to_string(),
        |ui| {
            if ci.blocked.is_empty() {
                ui.label(muted(
                    &theme,
                    tl("No open MR has its latest pipeline failed."),
                ));
            }
            for b in &ci.blocked {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(
                        rect.expand2(Vec2::new(6.0, 0.0)),
                        CornerRadius::same(theme.radii.medium),
                        theme.palette.raised,
                    );
                }
                let w = board::paint_tag(
                    ui,
                    &theme,
                    egui::pos2(rect.left(), rect.top() + 12.0),
                    &format!("!{}", b.id),
                    board::id_tone(&theme),
                    true,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + w + 8.0, rect.top() + 12.0),
                    Align2::LEFT_CENTER,
                    &b.title,
                    fonts::medium(theme.text_size - 0.5),
                    theme.palette.text,
                    rect.width() - w - 8.0,
                );
                let since = b
                    .since
                    .map(|s| board::duration_text(board::days_between(s, now)))
                    .unwrap_or_default();
                let mut text = format!("{} · {}", board::first_name(&b.author), b.jobs.join(", "));
                if !since.is_empty() {
                    text = format!("{text} · {}", tf("for {time}", &[("time", &since)]));
                }
                let mut x = rect.left() + w + 8.0;
                if b.flaky {
                    x += board::paint_tag(
                        ui,
                        &theme,
                        egui::pos2(x, rect.top() + 31.0),
                        tl("rerun?"),
                        theme.tones.amber,
                        false,
                    ) + 6.0;
                }
                board::paint_text(
                    ui,
                    egui::pos2(x, rect.top() + 31.0),
                    Align2::LEFT_CENTER,
                    &text,
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    rect.right() - x,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .tip(tl("Open its pipelines"))
                    .clicked()
                {
                    open = Some(b.id.clone());
                }
            }
        },
    );
    if let Some(id) = open {
        g.open_mr(&id);
        g.mr_tab = super::super::mr_page::Tab::Pipelines;
        return;
    }
    let Some(job) = selected.and_then(|s| ci.jobs.iter().find(|j| j.name == s)) else {
        return;
    };
    let mut show = false;
    board::section(ui, &theme, tl("Job picked"), &job.stage, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(
                super::super::widgets::mono(&job.name, theme.text_size).color(theme.palette.text),
            );
            if job.is_flaky() {
                board::chip(ui, &theme, tl("flaky"), theme.tones.amber);
            }
        });
        ui.add_space(8.0);
        let failures = job.failures();
        let last_failure = job
            .runs
            .iter()
            .rev()
            .find(|r| r.status == "failed")
            .map(|r| board::ago(r.at, now));
        board::facts(
            ui,
            &theme,
            &[
                (tl("Runs"), job.runs.len().to_string()),
                (
                    tl("Failures"),
                    format!("{failures} · {}", board::percent(job.failure_rate)),
                ),
                (
                    tl("Flaky"),
                    tn(
                        job.flaky_runs as u64,
                        "{n} rerun fixed it",
                        "{n} reruns fixed it",
                    ),
                ),
                (
                    tl("Median"),
                    job.median
                        .map_or_else(|| board::dash().to_string(), board::seconds_text),
                ),
                (
                    tl("Last failure"),
                    last_failure
                        .clone()
                        .unwrap_or_else(|| board::dash().to_string()),
                ),
            ],
        );
        let mut mrs: Vec<(&str, &str)> = job
            .runs
            .iter()
            .filter(|r| r.status == "failed")
            .map(|r| (r.mr.as_str(), r.mr_title.as_str()))
            .collect();
        mrs.sort_unstable();
        mrs.dedup();
        if !mrs.is_empty() {
            ui.add_space(10.0);
            ui.label(muted(&theme, tl("Failed on")).size(theme.text_size - 1.5));
            ui.add_space(2.0);
            let mut clicked = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for (id, title) in &mrs {
                    if board::mr_pill(ui, &theme, id, title).clicked() {
                        clicked = Some(id.to_string());
                    }
                }
            });
            if let Some(id) = clicked {
                open = Some(id);
            }
        }
        ui.add_space(GAP);
        show = primary_button(ui, &theme, tl("Show these MRs")).clicked();
    });
    if show {
        let ids = job
            .runs
            .iter()
            .filter(|r| r.status == "failed")
            .map(|r| r.mr.clone())
            .collect();
        g.next_focus = Some(Some(Focus {
            key: String::new(),
            label: tf("{job} failed on them", &[("job", &job.name)]),
            ids,
        }));
        g.open_page(super::super::Page::MergeRequests);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

/// The rail's badge: the open MRs held back by a failed pipeline (red, orange
/// when each failed on a flaky job: a rerun may do), else the jobs failing more
/// often than in the first half of the period (amber).
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let days = PERIODS[g.ci.period.min(PERIODS.len() - 1)].0;
    let ci = g
        .memo
        .ci
        .get(&g.app.project_id, days, || analyze(&g.app, now, days));
    let tones = &g.theme.tones;
    if !ci.blocked.is_empty() {
        let flaky = ci.blocked.iter().filter(|b| b.flaky).count();
        let mut hint = tn(
            ci.blocked.len() as u64,
            "{n} open MR held back by a failed pipeline",
            "{n} open MRs held back by a failed pipeline",
        );
        if flaky > 0 {
            hint.push('\n');
            hint.push_str(&tn(
                flaky as u64,
                "{n} of them failed on a flaky job: a rerun may do",
                "{n} of them failed on a flaky job: a rerun may do",
            ));
        }
        return Some(Badge {
            n: ci.blocked.len(),
            tone: if flaky == ci.blocked.len() {
                tones.orange
            } else {
                tones.red
            },
            hint,
        });
    }
    let rising: Vec<String> = ci
        .jobs
        .iter()
        .filter(|j| {
            j.failure_rate >= 0.2
                && j.rate_before
                    .is_some_and(|before| j.failure_rate >= before + 0.1)
        })
        .map(|j| j.name.clone())
        .collect();
    (!rising.is_empty()).then(|| Badge {
        n: rising.len(),
        tone: tones.amber,
        hint: rail_badges::listed(
            tn(
                rising.len() as u64,
                "{n} job fails more often than before",
                "{n} jobs fail more often than before",
            ),
            rail_badges::names(&rising, 3),
        ),
    })
}
