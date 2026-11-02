//! Toasts: what the refreshes changed (an MR merged, a pipeline failed…), in the
//! window's corner for a few seconds. A click selects the MR.

use std::collections::HashMap;

use bevel_theme::{fonts, Theme, Tone};
use egui::{CornerRadius, Margin, RichText, Sense, Stroke, Vec2};

use super::{tones, GuiApp};
use crate::app::App;
use crate::i18n::{tf, tl};
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

/// A toast kept for the bell: what and when.
pub struct Past {
    pub id: String,
    pub text: String,
    pub tone: Tone,
    /// "14:05".
    pub at: String,
}

/// Toasts the bell keeps; older ones go first.
const HISTORY: usize = 50;

/// The toasts shown, and what each MR looked like at the last frame.
#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
    /// Every toast of the session, newest last, for the bell.
    pub history: Vec<Past>,
    /// Toasts since the bell was last opened.
    pub unread: usize,
    /// Status badge and last pipeline, by MR id.
    seen: HashMap<String, (&'static str, Option<PipelineState>)>,
}

impl Toasts {
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
                        tf("!{id} · pipeline failed", &[("id", &mr.id)]),
                        theme.tones.red,
                        now,
                    ),
                    Some(PipelineState::Success) => self.push(
                        &mr.id,
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
                self.push(&mr.id, text, tone, now);
            }
        }
        self.list.retain(|t| now - t.born < LIFETIME);
    }

    pub fn push(&mut self, id: &str, text: String, tone: Tone, now: f64) {
        self.history.push(Past {
            id: id.to_string(),
            text: text.clone(),
            tone,
            at: chrono::Local::now().format("%H:%M").to_string(),
        });
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.unread += 1;
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
                    .on_hover_text(tl("Show this merge request · ✕ dismisses it"));
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
        g.show_mr(&toast.id);
    }
}
