//! Who an MR waits on, drawn: the inspector's card and the MR page's banner
//! (see [`crate::next_step`]). Each lists the todos in order, what comes after,
//! and offers the action that helps with the first todo.

use bevel_theme::{fonts, Theme, Tone};
use egui::{CornerRadius, Margin, RichText, Stroke, Ui};

use super::widgets::{button, muted};
use crate::i18n::{tf, tl};
use crate::models::TrackedMr;
use crate::next_step::{days_text, first_name, Actor, NextStep, Todo};

/// What a button of the card asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// The threads waiting for an answer.
    OpenThreads,
    /// The failed pipeline.
    OpenPipelines,
    /// The commands to rebase the source branch, copied (already done).
    Copied,
    /// The Promote page, from the branch reached to the next one.
    Promote { from: String, to: String },
}

/// The tone of a step: mine in orange, then by actor.
pub fn tone(step: &NextStep, theme: &Theme) -> Tone {
    let t = &theme.tones;
    if step.mine {
        return t.orange;
    }
    match step.actor {
        Actor::Author => t.amber,
        Actor::Reviewers => t.purple,
        Actor::Merger => t.teal,
        Actor::Deploy => t.blue,
        Actor::Nobody if step.closed => t.gray,
        Actor::Nobody => t.green,
        Actor::Unknown => t.gray,
    }
}

/// "Waiting on you · for 3 d", "To deploy · on staging for 4 d", "Done".
pub fn caption(step: &NextStep) -> String {
    let waited = step.waited_days(chrono::Utc::now()).map(days_text);
    match step.actor {
        Actor::Deploy => match (&step.reached, waited) {
            (Some(b), Some(w)) => tf(
                "To deploy · on {branch} for {time}",
                &[("branch", b), ("time", &w)],
            ),
            _ => tl("To deploy").into(),
        },
        Actor::Nobody if step.closed => tl("Closed without merging").into(),
        Actor::Nobody => match &step.reached {
            Some(b) => tf("Done · on {branch}", &[("branch", b)]),
            None => tl("Done").into(),
        },
        Actor::Unknown => tl("Known once the MR is loaded").into(),
        _ => {
            let who = if step.mine {
                tl("you").to_string()
            } else {
                people(step)
            };
            match waited {
                Some(w) => tf(
                    "Waiting on {who} · for {time}",
                    &[("who", &who), ("time", &w)],
                ),
                None => tf("Waiting on {who}", &[("who", &who)]),
            }
        }
    }
}

fn people(step: &NextStep) -> String {
    match step.people.as_slice() {
        [] => step.who(),
        list => list
            .iter()
            .map(|p| first_name(p).to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The git commands to bring `mr`'s source branch up to its target.
pub fn rebase_commands(mr: &TrackedMr) -> String {
    format!(
        "git fetch origin\ngit switch {src}\ngit rebase origin/{target}\n# resolve, then: git rebase --continue\ngit push --force-with-lease origin {src}\n",
        src = mr.source_branch,
        target = mr.target_branch
    )
}

/// A secondary button that shows on the card's tinted ground: filled, outlined.
fn secondary(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    let font = fonts::medium(theme.text_size - 0.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, theme.palette.text);
    let size = egui::vec2(galley.size().x + 20.0, theme.spacing.control_height + 2.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let radius = CornerRadius::same(theme.radii.medium);
    let fill = if response.hovered() {
        theme.palette.raised
    } else {
        theme.palette.panel
    };
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme.palette.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme.palette.text,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The wide banner's caption: who has the ball.
fn headline(step: &NextStep) -> String {
    match step.actor {
        Actor::Author | Actor::Reviewers | Actor::Merger if step.mine => {
            tl("Ball in your court").into()
        }
        Actor::Author | Actor::Reviewers | Actor::Merger => {
            tf("Waiting on {who}", &[("who", &people(step))])
        }
        Actor::Deploy => tl("To deploy").into(),
        Actor::Nobody if step.closed => tl("Closed without merging").into(),
        Actor::Nobody => tl("Done").into(),
        Actor::Unknown => String::new(),
    }
}

/// The todos numbered, then what comes after as the last, muted, line.
fn steps_list(ui: &mut Ui, theme: &Theme, step: &NextStep) {
    ui.spacing_mut().item_spacing.y = 5.0;
    let mut n = 0;
    for todo in &step.todos {
        n += 1;
        ui.add(
            egui::Label::new(
                RichText::new(format!("{n}. {}", todo.text()))
                    .font(fonts::semibold(theme.text_size))
                    .color(theme.palette.text),
            )
            .wrap(),
        );
    }
    if step.actor == Actor::Nobody && !step.closed {
        ui.label(
            RichText::new(tl("Nothing left: it reached every branch of its flow."))
                .color(theme.palette.text),
        );
    }
    if !step.then.is_empty() {
        let prefix = if n > 0 {
            format!("{}. ", n + 1)
        } else {
            String::new()
        };
        ui.add(
            egui::Label::new(muted(
                theme,
                format!(
                    "{prefix}{}",
                    tf("Then: {steps}", &[("steps", &step.then.join(" → "))])
                ),
            ))
            .wrap(),
        );
    }
}

/// The card: caption, todos (numbered), then, and the helping buttons.
/// `wide` lays the buttons beside the todos (the MR page).
pub fn card(
    ui: &mut Ui,
    theme: &Theme,
    mr: &TrackedMr,
    step: &NextStep,
    wide: bool,
) -> Option<NextAction> {
    if step.actor == Actor::Unknown {
        return None;
    }
    let tone = tone(step, theme);
    let mut action = None;
    egui::Frame::new()
        .fill(tone.bg.gamma_multiply(0.55))
        .stroke(Stroke::new(1.0, tone.fg.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let body = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    ui.label(
                        RichText::new(caption(step).to_uppercase())
                            .font(fonts::semibold(theme.text_size - 2.5))
                            .color(tone.fg),
                    );
                    let several = step.todos.len() > 1;
                    for (i, todo) in step.todos.iter().enumerate() {
                        let text = if several {
                            format!("{}. {}", i + 1, todo.text())
                        } else {
                            todo.text()
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(text)
                                    .font(fonts::semibold(theme.text_size))
                                    .color(theme.palette.text),
                            )
                            .wrap(),
                        );
                    }
                    if step.actor == Actor::Nobody && step.todos.is_empty() && !step.closed {
                        ui.label(
                            RichText::new(tl("Nothing left: it reached every branch of its flow."))
                                .color(theme.palette.text),
                        );
                    }
                    if !step.then.is_empty() {
                        ui.add(
                            egui::Label::new(muted(
                                theme,
                                tf("Then: {steps}", &[("steps", &step.then.join(" → "))]),
                            ))
                            .wrap(),
                        );
                    }
                });
            };
            let buttons = |ui: &mut Ui, action: &mut Option<NextAction>| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // The main action first (threads, promotion), then the helpers.
                let mut todos: Vec<&Todo> = step.todos.iter().collect();
                todos.sort_by_key(|t| {
                    !matches!(
                        t,
                        Todo::AnswerThreads { .. }
                            | Todo::ReadAnswers { .. }
                            | Todo::Promote { .. }
                    )
                });
                for todo in todos {
                    match todo {
                        Todo::AnswerThreads { .. } | Todo::ReadAnswers { .. } => {
                            if button(ui, theme, None, tl("Open the threads"), true).clicked() {
                                *action = Some(NextAction::OpenThreads);
                            }
                        }
                        Todo::Rebase { .. } => {
                            if secondary(ui, theme, tl("Copy the rebase commands"))
                                .on_hover_text(rebase_commands(mr))
                                .clicked()
                            {
                                ui.ctx().copy_text(rebase_commands(mr));
                                *action = Some(NextAction::Copied);
                            }
                        }
                        Todo::FixPipeline => {
                            if secondary(ui, theme, tl("See the pipeline")).clicked() {
                                *action = Some(NextAction::OpenPipelines);
                            }
                        }
                        Todo::Promote { to }
                            if button(ui, theme, None, tl("Open Promote"), true).clicked() =>
                        {
                            *action = Some(NextAction::Promote {
                                from: step.reached.clone().unwrap_or_default(),
                                to: to.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            };
            if wide {
                // As the mockup: how long, big, on the left; the steps; the buttons.
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 24.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(170.0, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(170.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                RichText::new(headline(step).to_uppercase())
                                    .font(fonts::semibold(theme.text_size - 2.5))
                                    .color(tone.fg),
                            );
                            let big = step
                                .waited_days(chrono::Utc::now())
                                .map(days_text)
                                .unwrap_or_else(|| "—".into());
                            ui.label(
                                RichText::new(big)
                                    .font(fonts::semibold(theme.text_size + 8.0))
                                    .color(tone.fg),
                            );
                            if let Some(since) = step.since {
                                let date = since
                                    .with_timezone(&chrono::Local)
                                    .format("%d %b")
                                    .to_string();
                                let text = match (&step.actor, &step.reached) {
                                    (Actor::Deploy | Actor::Nobody, Some(branch)) => tf(
                                        "on {branch} since {date}",
                                        &[("branch", branch), ("date", &date)],
                                    ),
                                    _ => tf("since {date}", &[("date", &date)]),
                                };
                                ui.label(muted(theme, text));
                            }
                        },
                    );
                    let buttons_width = 330.0;
                    let middle = (ui.available_width() - buttons_width - 24.0).max(240.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(middle, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(middle);
                            steps_list(ui, theme, step);
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        buttons(ui, &mut action);
                    });
                });
            } else {
                body(ui);
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| buttons(ui, &mut action));
            }
        });
    action
}

/// Applies what the card asked for; `on_page`: from the MR page (its tabs),
/// else from the inspector (its sections).
pub fn apply(g: &mut super::GuiApp, action: NextAction, on_page: bool) {
    use super::{mr_page, InspectorSection, Page};
    match action {
        NextAction::OpenThreads if on_page => g.mr_tab = mr_page::Tab::Discussions,
        NextAction::OpenThreads => g.scroll_to = Some(InspectorSection::Threads),
        NextAction::OpenPipelines if on_page => g.mr_tab = mr_page::Tab::Pipelines,
        NextAction::OpenPipelines => g.scroll_to = Some(InspectorSection::Pipelines),
        NextAction::Copied => g.app.flash(tl("Rebase commands copied"), false),
        NextAction::Promote { from, to } => {
            if !from.is_empty() {
                g.promote = Some((from, to));
            }
            g.open_page(Page::Promote);
        }
    }
}

/// A stage of an MR's life, for the lifecycle strip.
#[derive(Debug, Clone)]
pub struct Stage {
    pub label: String,
    /// When it was reached.
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    /// Days from the previous stage (running for the current one).
    pub took: Option<f64>,
    /// The project's median for that hop, in days.
    pub usual: Option<f64>,
    pub reached: bool,
    /// The stage the MR waits for now.
    pub current: bool,
}

fn parse(at: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(at?)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

/// A stage's name, when it was reached, and whether it was.
type Milestone = (String, Option<chrono::DateTime<chrono::Utc>>, bool);

/// When each stage of `mr` was reached: created, first review, approved,
/// merged, then each branch up its flow. `None`: not reached (or not known).
fn milestones(app: &crate::app::App, mr: &TrackedMr) -> Vec<Milestone> {
    let review = mr.review.as_ref();
    let merged = mr.state == crate::models::GitlabMrState::Merged;
    let mut out = vec![
        (
            tl("Created").to_string(),
            parse(mr.created_at.as_deref()),
            true,
        ),
        (
            tl("First review").to_string(),
            parse(review.and_then(|r| r.first_review_at.as_deref())),
            review.is_some_and(|r| r.first_review_at.is_some()) || merged,
        ),
        (
            tl("Approved").to_string(),
            parse(review.and_then(|r| r.approved_at.as_deref())),
            !mr.approved_by.is_empty() || merged,
        ),
        (
            tf("Merged into {branch}", &[("branch", &mr.target_branch)]),
            parse(mr.merged_at.as_deref()),
            merged,
        ),
    ];
    for branch in app.branch_flow().path(&mr.target_branch).promotion {
        if !app.branches.contains(&branch) {
            continue;
        }
        let reached = merged
            && app
                .branch_presence(mr, &branch)
                .is_some_and(|p| !p.is_missing());
        let at = app
            .branch_arrivals
            .get(&(mr.id.clone(), branch.clone()))
            .copied();
        out.push((tf("On {branch}", &[("branch", &branch)]), at, reached));
    }
    out
}

/// The lifecycle of `mr`: its stages, how long each hop took, and the usual
/// time for that hop over the other tracked MRs.
pub fn lifecycle(app: &crate::app::App, mr: &TrackedMr, step: &NextStep) -> Vec<Stage> {
    let mut stages = stages_of(app, mr);
    // Who it waits on now, as a stage of its own before the next one.
    if matches!(
        step.actor,
        Actor::Author | Actor::Reviewers | Actor::Merger | Actor::Deploy
    ) {
        if let Some(i) = stages.iter().position(|s| s.current) {
            stages[i].current = false;
            stages[i].took = None;
            let usual = (step.actor == Actor::Reviewers)
                .then(|| crate::next_step::usual_first_review_days(app))
                .flatten();
            let label = match step.actor {
                Actor::Deploy => caption_short(step),
                _ if step.mine => tl("Waiting on you").to_string(),
                _ => tf("Waiting on {who}", &[("who", &people(step))]),
            };
            stages.insert(
                i,
                Stage {
                    label,
                    at: step.since,
                    took: step.waited_days(chrono::Utc::now()),
                    usual,
                    reached: false,
                    current: true,
                },
            );
        }
    }
    stages
}

fn caption_short(step: &NextStep) -> String {
    match step.todos.first() {
        Some(Todo::Promote { to }) => tf("Waiting for {branch}", &[("branch", to)]),
        _ => tl("To deploy").into(),
    }
}

fn stages_of(app: &crate::app::App, mr: &TrackedMr) -> Vec<Stage> {
    let mine = milestones(app, mr);
    // The usual time of each hop, over the other MRs that made it.
    let others: Vec<Vec<Milestone>> = app
        .mrs
        .iter()
        .filter(|m| m.id != mr.id && !m.title.is_empty())
        .map(|m| milestones(app, m))
        .collect();
    let usual = |i: usize| -> Option<f64> {
        crate::next_step::median(
            others
                .iter()
                .filter_map(|stages| {
                    // From the latest dated stage before this one.
                    let to = stages.get(i)?.1?;
                    let from = stages[..i].iter().rev().find_map(|s| s.1)?;
                    Some((to - from).num_minutes().max(0) as f64 / 1440.0)
                })
                .collect(),
        )
    };
    let now = chrono::Utc::now();
    let closed = mr.state == crate::models::GitlabMrState::Closed;
    let current = (!closed).then(|| mine.iter().position(|s| !s.2)).flatten();
    mine.iter()
        .enumerate()
        .map(|(i, (label, at, reached))| {
            let from = mine[..i].iter().rev().find_map(|s| s.1);
            let is_current = current == Some(i);
            let took = match (from, at) {
                (Some(from), Some(at)) if *reached => {
                    Some((*at - from).num_minutes().max(0) as f64 / 1440.0)
                }
                (Some(from), _) if is_current => {
                    Some((now - from).num_minutes().max(0) as f64 / 1440.0)
                }
                _ => None,
            };
            Stage {
                label: label.clone(),
                at: *at,
                took,
                usual: if i == 0 { None } else { usual(i) },
                reached: *reached,
                current: is_current,
            }
        })
        .collect()
}

/// The lifecycle as a line of steps, each with the time the hop took and the
/// usual time: the current one in `tone`, slower than usual in amber.
pub fn lifecycle_strip(ui: &mut Ui, theme: &Theme, stages: &[Stage], tone: Tone) {
    let n = stages.len().max(1);
    let width = ui.available_width();
    let step = (width / n as f32).max(110.0);
    let height = 74.0;
    egui::ScrollArea::horizontal()
        .id_salt("lifecycle")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(step * n as f32, height), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            let y = rect.top() + 8.0;
            for (i, s) in stages.iter().enumerate() {
                let x = rect.left() + i as f32 * step;
                let center = egui::pos2(x + 7.0, y);
                // The line to the next stage.
                if i + 1 < n {
                    let next_reached = stages[i + 1].reached;
                    painter.line_segment(
                        [center + egui::vec2(9.0, 0.0), egui::pos2(x + step - 2.0, y)],
                        Stroke::new(
                            2.0,
                            if next_reached {
                                theme.tones.green.fg.gamma_multiply(0.5)
                            } else {
                                theme.palette.border
                            },
                        ),
                    );
                }
                if s.reached {
                    painter.circle_filled(center, 6.0, theme.tones.green.fg);
                } else if s.current {
                    painter.circle_filled(center, 6.0, tone.bg);
                    painter.circle_stroke(center, 6.0, Stroke::new(2.0, tone.fg));
                } else {
                    painter.circle_stroke(center, 5.5, Stroke::new(1.5, theme.palette.border));
                }
                let text_x = x;
                let mut line_y = y + 14.0;
                let label_color = if s.current {
                    tone.fg
                } else if s.reached {
                    theme.palette.text
                } else {
                    theme.palette.text_muted
                };
                let max = step - 10.0;
                let mut text = |t: String, font: egui::FontId, color: egui::Color32| {
                    let galley = painter.layout(t, font, color, max);
                    let h = galley.size().y;
                    painter.galley(egui::pos2(text_x, line_y), galley, color);
                    line_y += h + 1.0;
                };
                text(
                    s.label.clone(),
                    fonts::semibold(theme.text_size - 1.5),
                    label_color,
                );
                let when =
                    s.at.map(|at| {
                        at.with_timezone(&chrono::Local)
                            .format("%d %b %Y")
                            .to_string()
                    })
                    .unwrap_or_else(|| {
                        if s.reached {
                            "—".into()
                        } else {
                            String::new()
                        }
                    });
                if !when.is_empty() {
                    text(
                        when,
                        egui::FontId::proportional(theme.text_size - 2.5),
                        theme.palette.text_muted,
                    );
                }
                if let Some(took) = s.took {
                    let slow = s.usual.is_some_and(|u| took > u * 1.5 && took - u > 0.5);
                    let color = if s.current {
                        tone.fg
                    } else if slow {
                        theme.tones.amber.fg
                    } else {
                        theme.palette.text_secondary
                    };
                    let prefix = if s.current { "" } else { "+" };
                    let first = painter.layout_no_wrap(
                        format!("{prefix}{}", days_text(took)),
                        fonts::semibold(theme.text_size - 2.0),
                        color,
                    );
                    let w = first.size().x;
                    painter.galley(egui::pos2(text_x, line_y), first, color);
                    if let Some(u) = s.usual {
                        let rest = painter.layout(
                            tf("(median {time})", &[("time", &days_text(u))]),
                            egui::FontId::proportional(theme.text_size - 2.5),
                            theme.palette.text_muted,
                            (max - w - 4.0).max(40.0),
                        );
                        painter.galley(
                            egui::pos2(text_x + w + 4.0, line_y + 1.0),
                            rest,
                            theme.palette.text_muted,
                        );
                    }
                } else if let Some(u) = s.usual.filter(|_| !s.reached) {
                    text(
                        tf("median {time}", &[("time", &days_text(u))]),
                        egui::FontId::proportional(theme.text_size - 2.5),
                        theme.palette.text_muted,
                    );
                }
            }
        });
}
