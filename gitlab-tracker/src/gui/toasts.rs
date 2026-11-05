//! Toasts: what the refreshes changed (an MR merged, a pipeline failed…), in the
//! window's corner for a few seconds, and kept for the bell. A click opens the
//! MR's page.

use std::collections::HashMap;

use bevel_theme::{fonts, Theme, Tone};
use egui::{CornerRadius, Margin, RichText, Sense, Stroke, Vec2};

use super::widgets::muted;
use super::{tones, GuiApp};
use crate::app::App;
use crate::i18n::{tf, tl, tn};
use crate::models::PipelineState;

/// Seconds a toast stays, the last of them fading out.
const LIFETIME: f64 = 8.0;
const FADE: f64 = 1.0;
/// Toasts shown at once; older ones go first.
const MAX_SHOWN: usize = 4;

pub struct Toast {
    pub id: String,
    pub text: String,
    pub tone: Tone,
    born: f64,
}

/// What a notification is about, for its icon and the bell's filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    PipelineFailed,
    PipelinePassed,
    Merged,
    /// Another status change (closed, conflict, approved…).
    Status,
}

/// A toast kept for the bell: what, about which MR, and when.
#[derive(Clone)]
pub struct Past {
    pub id: String,
    /// The MR's title.
    pub title: String,
    pub kind: Kind,
    pub text: String,
    pub tone: Tone,
    pub when: chrono::DateTime<chrono::Local>,
    /// Seen in the bell's panel.
    pub read: bool,
}

/// Toasts the bell keeps; older ones go first.
const HISTORY: usize = 50;

/// The toasts shown, and what each MR looked like at the last frame.
#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
    /// Every toast of the session, newest last, for the bell.
    pub history: Vec<Past>,
    /// Status badge and last pipeline, by MR id.
    seen: HashMap<String, (&'static str, Option<PipelineState>)>,
}

impl Toasts {
    /// Notifications not seen in the bell's panel yet.
    pub fn unread(&self) -> usize {
        self.history.iter().filter(|p| !p.read).count()
    }

    /// Compares each MR with the last frame and adds a toast for each change, once
    /// the startup fetches are over (they would all look like changes).
    pub fn watch(&mut self, app: &App, theme: &Theme, now: f64) {
        let quiet = !app.pending_initial_fetches.is_empty();
        for mr in &app.mrs {
            if mr.title.is_empty() {
                continue;
            }
            let (status, tone) = tones::status(mr.state, &mr.mergeability, &theme.tones);
            let pipeline = mr.pipelines.first().map(|p| p.status);
            let before = self.seen.insert(mr.id.clone(), (status, pipeline));
            let Some((was_status, was_pipeline)) = before else {
                continue;
            };
            if quiet {
                continue;
            }
            if was_pipeline != pipeline {
                match pipeline {
                    Some(PipelineState::Failed) => self.push(
                        &mr.id,
                        &mr.title,
                        Kind::PipelineFailed,
                        tf("!{id} · pipeline failed", &[("id", &mr.id)]),
                        theme.tones.red,
                        now,
                    ),
                    Some(PipelineState::Success) => self.push(
                        &mr.id,
                        &mr.title,
                        Kind::PipelinePassed,
                        tf("!{id} · pipeline passed", &[("id", &mr.id)]),
                        theme.tones.green,
                        now,
                    ),
                    _ => {}
                }
            }
            if was_status != status {
                let text = tf(
                    "!{id} · now {status}",
                    &[("id", &mr.id), ("status", status)],
                );
                let kind = if mr.state == crate::models::GitlabMrState::Merged {
                    Kind::Merged
                } else {
                    Kind::Status
                };
                self.push(&mr.id, &mr.title, kind, text, tone, now);
            }
        }
        self.list.retain(|t| now - t.born < LIFETIME);
    }

    pub fn push(&mut self, id: &str, title: &str, kind: Kind, text: String, tone: Tone, now: f64) {
        self.history.push(Past {
            id: id.to_string(),
            title: title.to_string(),
            kind,
            text: text.clone(),
            tone,
            when: chrono::Local::now(),
            read: false,
        });
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.list.push(Toast {
            id: id.to_string(),
            text,
            tone,
            born: now,
        });
        if self.list.len() > MAX_SHOWN {
            self.list.remove(0);
        }
    }
}

/// The toasts in the bottom-right corner, above the status bar, newest at the bottom.
pub fn show(g: &mut GuiApp, ctx: &egui::Context) {
    if g.toasts.list.is_empty() {
        return;
    }
    let theme = g.theme;
    let now = ctx.input(|i| i.time);
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    let mut clicked = None;
    let mut closed = None;
    egui::Area::new(egui::Id::unique("toasts"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -40.0))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for (i, toast) in g.toasts.list.iter().enumerate() {
                let mut close_rect = egui::Rect::NOTHING;
                let left = LIFETIME - (now - toast.born);
                let alpha = (left / FADE).clamp(0.0, 1.0) as f32;
                let response = egui::Frame::new()
                    .fill(theme.palette.raised.gamma_multiply(alpha))
                    .stroke(Stroke::new(1.0, theme.palette.border.gamma_multiply(alpha)))
                    .corner_radius(CornerRadius::same(theme.radii.large))
                    .inner_margin(Margin {
                        left: 16,
                        right: 12,
                        top: 10,
                        bottom: 10,
                    })
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: egui::Color32::from_black_alpha((60.0 * alpha) as u8),
                    })
                    .show(ui, |ui| {
                        ui.set_width(300.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter().circle_filled(
                                dot.center(),
                                4.0,
                                toast.tone.fg.gamma_multiply(alpha),
                            );
                            ui.label(
                                RichText::new(&toast.text)
                                    .font(fonts::medium(theme.text_size - 0.5))
                                    .color(theme.palette.text.gamma_multiply(alpha)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    close_rect = ui
                                        .label(RichText::new("✕").color(theme.palette.text_muted))
                                        .rect;
                                },
                            );
                        });
                    })
                    .response;
                // The stripe in the toast's tone, along its left edge.
                let stripe = egui::Rect::from_min_size(
                    response.rect.min + Vec2::new(0.0, 8.0),
                    Vec2::new(3.0, response.rect.height() - 16.0),
                );
                ui.painter().rect_filled(
                    stripe,
                    CornerRadius::same(2),
                    toast.tone.fg.gamma_multiply(alpha),
                );
                let response = ui
                    .interact(
                        response.rect,
                        ui.scope_id().with(("toast", i)),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tl("Open this MR's page · ✕ dismisses it"));
                if response.clicked() {
                    let on_close = response
                        .interact_pointer_pos()
                        .is_some_and(|p| close_rect.expand(4.0).contains(p));
                    if on_close {
                        closed = Some(i);
                    } else {
                        clicked = Some(i);
                    }
                }
            }
        });
    if let Some(i) = closed {
        g.toasts.list.remove(i);
    } else if let Some(i) = clicked {
        let toast = g.toasts.list.remove(i);
        g.open_mr(&toast.id);
    }
}

// ── The bell's panel ─────────────────────────────────────────────────────────

/// The bell panel's filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Failures,
    Merges,
    Changes,
}

impl Filter {
    fn keeps(self, kind: Kind) -> bool {
        match self {
            Filter::All => true,
            Filter::Failures => kind == Kind::PipelineFailed,
            Filter::Merges => kind == Kind::Merged,
            Filter::Changes => matches!(kind, Kind::Status | Kind::PipelinePassed),
        }
    }
}

/// A notification of any tab, as the panel lists it.
struct Line {
    tab: usize,
    project: String,
    past: Past,
}

/// What the panel asked for.
enum Ask {
    /// Its tab, MR, and whether it is a failed pipeline (opened on the pipelines).
    Open(usize, String, bool),
    ReadAll,
    Clear,
}

const PANEL_WIDTH: f32 = 420.0;
const ROW_HEIGHT: f32 = 58.0;

/// The notifications of every open project, unread first highlighted, under the
/// bell. A click opens the MR's page in its tab; they count as read once the
/// panel closes.
pub fn panel(g: &mut GuiApp, button: &egui::Response) {
    let theme = g.theme;
    let mut lines: Vec<Line> = Vec::new();
    let several = g.tabs.len() > 1;
    for (tab, slot) in g.tabs.iter().enumerate() {
        let (toasts, project) = match &slot.state {
            super::SlotState::Active => (&g.toasts, super::shell::project_name(g)),
            super::SlotState::Parked(t) => (
                &t.toasts,
                t.app
                    .project_name
                    .clone()
                    .unwrap_or_else(|| t.app.project_id.clone()),
            ),
            _ => continue,
        };
        lines.extend(toasts.history.iter().map(|past| Line {
            tab,
            project: project.clone(),
            past: past.clone(),
        }));
    }
    lines.sort_by_key(|l| std::cmp::Reverse(l.past.when));
    let unread = lines.iter().filter(|l| !l.past.read).count();
    let count = |f: Filter| lines.iter().filter(|l| f.keeps(l.past.kind)).count();

    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .shadow(egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(90),
        });
    let mut ask = None;
    let mut filter = g.bell_filter;
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_PAGE=bell` keeps it open.
    #[cfg(debug_assertions)]
    let forced = std::env::var("GITLAB_TRACKER_GUI_PAGE").as_deref() == Ok("bell");
    #[cfg(not(debug_assertions))]
    let forced = false;
    let shown = egui::Popup::menu(button)
        .open_memory(if forced {
            Some(egui::SetOpenCommand::Bool(true))
        } else {
            button.clicked().then_some(egui::SetOpenCommand::Toggle)
        })
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(PANEL_WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            // Header: title, unread count, actions.
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 16,
                    right: 12,
                    top: 14,
                    bottom: 10,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            RichText::new(tl("Notifications"))
                                .font(fonts::semibold(theme.text_size + 2.0)),
                        );
                        if unread > 0 {
                            ui.add(
                                bevel_theme::Chip::new(
                                    &tn(unread as u64, "{n} new", "{n} new"),
                                    theme.tones.blue,
                                )
                                .pill(),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if !lines.is_empty() {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if link(ui, &theme, tl("Clear")).clicked() {
                                    ask = Some(Ask::Clear);
                                }
                                if unread > 0 {
                                    ui.label(muted(&theme, "·"));
                                    if link(ui, &theme, tl("Mark all read")).clicked() {
                                        ask = Some(Ask::ReadAll);
                                    }
                                }
                            }
                        });
                    });
                    if !lines.is_empty() {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for (f, label, tone) in [
                                (Filter::All, tl("All"), theme.tones.blue),
                                (Filter::Failures, tl("Failures"), theme.tones.red),
                                (Filter::Merges, tl("Merges"), theme.tones.purple),
                                (Filter::Changes, tl("Changes"), theme.tones.teal),
                            ] {
                                let text = format!("{label} {}", count(f));
                                if super::widgets::pill_toggle(ui, &theme, &text, tone, filter == f)
                                    .clicked()
                                {
                                    filter = f;
                                }
                            }
                        });
                    }
                });
            super::widgets::rule(ui, &theme);

            let kept: Vec<&Line> = lines.iter().filter(|l| filter.keeps(l.past.kind)).collect();
            if kept.is_empty() {
                empty(ui, &theme, lines.is_empty());
            } else {
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        let today = chrono::Local::now().date_naive();
                        let mut day = None;
                        for line in kept {
                            let date = line.past.when.date_naive();
                            if day != Some(date) {
                                day = Some(date);
                                day_heading(ui, &theme, date, today);
                            }
                            if row(ui, &theme, line, several) {
                                ask = Some(Ask::Open(
                                    line.tab,
                                    line.past.id.clone(),
                                    line.past.kind == Kind::PipelineFailed,
                                ));
                            }
                        }
                        ui.add_space(6.0);
                    });
            }
            super::widgets::rule(ui, &theme);
            egui::Frame::new()
                .inner_margin(Margin::symmetric(16, 9))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            muted(&theme, tl("Click an entry to open its MR page"))
                                .size(theme.text_size - 1.5),
                        );
                    });
                });
            if matches!(ask, Some(Ask::Open(..))) {
                ui.close();
            }
        });
    g.bell_filter = filter;
    let open = shown.is_some();
    let closed = g.bell_open && !open;
    g.bell_open = open;

    let mut each = |f: &mut dyn FnMut(&mut Toasts)| {
        f(&mut g.toasts);
        for slot in &mut g.tabs {
            if let super::SlotState::Parked(t) = &mut slot.state {
                f(&mut t.toasts);
            }
        }
    };
    match ask {
        Some(Ask::ReadAll) => each(&mut |t| t.history.iter_mut().for_each(|p| p.read = true)),
        Some(Ask::Clear) => each(&mut |t| t.history.clear()),
        Some(Ask::Open(tab, id, failed)) => {
            each(&mut |t| t.history.iter_mut().for_each(|p| p.read = true));
            g.open_link(tab, &id);
            if failed {
                g.mr_tab = super::mr_page::Tab::Pipelines;
            }
            return;
        }
        None => {}
    }
    if closed {
        each(&mut |t| t.history.iter_mut().for_each(|p| p.read = true));
    }
}

/// A text button in the accent color.
fn link(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .color(theme.palette.accent)
                .size(theme.text_size - 1.0),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn day_heading(
    ui: &mut egui::Ui,
    theme: &Theme,
    date: chrono::NaiveDate,
    today: chrono::NaiveDate,
) {
    let text = if date == today {
        tl("Today").to_string()
    } else if Some(date) == today.pred_opt() {
        tl("Yesterday").to_string()
    } else {
        date.format("%Y-%m-%d").to_string()
    };
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 8,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(muted(theme, text.to_uppercase()).size(theme.text_size - 2.5));
        });
}

/// "just now", "12 min ago", "14:05", or the date and time.
fn ago(when: chrono::DateTime<chrono::Local>) -> String {
    let now = chrono::Local::now();
    let minutes = (now - when).num_minutes();
    if minutes < 1 {
        tl("just now").to_string()
    } else if minutes < 60 {
        tn(minutes as u64, "{n} min ago", "{n} min ago")
    } else if when.date_naive() == now.date_naive() {
        when.format("%H:%M").to_string()
    } else {
        when.format("%m-%d %H:%M").to_string()
    }
}

/// One notification: its icon in a circle of its tone, what happened and to
/// which MR, when, and from which project; `true` when clicked.
fn row(ui: &mut egui::Ui, theme: &Theme, line: &Line, several: bool) -> bool {
    let past = &line.past;
    let (outer, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    let rect = outer.shrink2(Vec2::new(8.0, 2.0));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.border_soft,
        );
    } else if !past.read {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.accent.gamma_multiply(0.07),
        );
    }
    // Unread: a dot on the left edge.
    if !past.read {
        painter.circle_filled(
            egui::pos2(rect.left() + 7.0, rect.center().y),
            3.0,
            theme.palette.accent,
        );
    }
    // The icon.
    let center = egui::pos2(rect.left() + 30.0, rect.center().y);
    painter.circle_filled(center, 15.0, past.tone.bg);
    let glyph_rect = egui::Rect::from_center_size(center, Vec2::splat(14.0));
    match past.kind {
        Kind::Merged => super::icons::paint(
            painter,
            glyph_rect,
            super::icons::Icon::MergeRequests,
            past.tone.fg,
        ),
        kind => {
            let glyph = match kind {
                Kind::PipelineFailed => "✕",
                Kind::PipelinePassed => "✓",
                _ => "●",
            };
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                glyph,
                fonts::semibold(theme.text_size),
                past.tone.fg,
            );
        }
    }
    // What happened: the text after "!id · ", capitalised.
    let what = past
        .text
        .split_once(" · ")
        .map_or(past.text.as_str(), |(_, w)| w);
    let mut what_chars = what.chars();
    let what = match what_chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + what_chars.as_str(),
        None => String::new(),
    };
    let text_left = rect.left() + 56.0;
    let right = rect.right() - 10.0;
    let time = ago(past.when);
    let time_galley = painter.layout_no_wrap(
        time,
        egui::FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
    );
    let top = rect.top() + 11.0;
    painter.galley(
        egui::pos2(right - time_galley.size().x, top + 1.0),
        time_galley.clone(),
        theme.palette.text_muted,
    );

    let mut first = egui::text::LayoutJob::default();
    first.append(
        &what,
        0.0,
        egui::TextFormat::simple(fonts::semibold(theme.text_size), theme.palette.text),
    );
    first.append(
        &format!("  !{}", past.id),
        0.0,
        egui::TextFormat::simple(
            egui::FontId::monospace(theme.text_size - 1.5),
            theme.palette.text_muted,
        ),
    );
    first.wrap = egui::text::TextWrapping::truncate_at_width(
        right - text_left - time_galley.size().x - 10.0,
    );
    let first = painter.layout_job(first);
    painter.galley(egui::pos2(text_left, top), first, theme.palette.text);

    // The MR's title, and the project when several are open.
    let mut project_width = 0.0;
    if several {
        let galley = painter.layout_no_wrap(
            line.project.clone(),
            fonts::medium(theme.text_size - 2.5),
            theme.palette.text_secondary,
        );
        let size = galley.size() + Vec2::new(12.0, 4.0);
        let chip = egui::Rect::from_min_size(egui::pos2(right - size.x, rect.top() + 32.0), size);
        painter.rect_filled(chip, CornerRadius::same(4), theme.tones.gray.bg);
        painter.galley(
            chip.min + Vec2::new(6.0, 2.0),
            galley,
            theme.palette.text_secondary,
        );
        project_width = size.x + 8.0;
    }
    let mut second = egui::text::LayoutJob::simple_singleline(
        past.title.clone(),
        egui::FontId::proportional(theme.text_size - 1.0),
        theme.palette.text_secondary,
    );
    second.wrap = egui::text::TextWrapping::truncate_at_width(right - text_left - project_width);
    let second = painter.layout_job(second);
    painter.galley(
        egui::pos2(text_left, rect.top() + 33.0),
        second,
        theme.palette.text_secondary,
    );

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open its MR page"))
        .clicked()
}

/// Nothing to list: a quiet bell and why.
fn empty(ui: &mut egui::Ui, theme: &Theme, nothing_at_all: bool) {
    ui.add_space(26.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 26.0, theme.palette.border_soft);
        super::icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), Vec2::splat(22.0)),
            super::icons::Icon::Bell,
            theme.palette.text_muted,
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new(if nothing_at_all {
                tl("You're all caught up")
            } else {
                tl("Nothing of this kind")
            })
            .font(fonts::semibold(theme.text_size + 1.0)),
        );
        ui.add_space(4.0);
        ui.label(
            muted(
                theme,
                tl("Merges, status changes and pipelines of every open project show up here."),
            )
            .size(theme.text_size - 1.0),
        );
    });
    ui.add_space(26.0);
}
