//! The Cockpit's "by label" card: for each label of the open MRs, how many,
//! where they wait (the next step's buckets, as the table's chips), their
//! median age and the oldest. A click on a label lists its MRs in the table.

use bevel_theme::Theme;
use chrono::{DateTime, Utc};
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Ui, Vec2};

use super::board;
use super::widgets::{label_tone, muted};
use crate::app::App;
use crate::i18n::{tl, tn};
use crate::models::GitlabMrState;
use crate::next_step::Bucket;

/// Labels shown before "N more".
const SHOWN: usize = 8;

/// The open MRs carrying one label.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRow {
    pub label: String,
    /// Open MRs per bucket, in [`Bucket::ALL`]'s order.
    pub buckets: [usize; 7],
    pub open: usize,
    /// Median days since the MRs were opened.
    pub median_age: Option<f64>,
    /// The oldest: (id, title, days).
    pub oldest: Option<(String, String, f64)>,
}

/// How the rows are sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Count,
    Age,
}

/// The labels of the open MRs of `app`, most MRs first (then by name), and how
/// many open MRs carry no label.
pub fn analyze(app: &App, now: DateTime<Utc>) -> (Vec<LabelRow>, usize) {
    let me = app.config.gitlab_username.as_deref();
    let mut rows: Vec<LabelRow> = Vec::new();
    let mut ages: Vec<Vec<f64>> = Vec::new();
    let mut unlabelled = 0;
    for mr in &app.mrs {
        if mr.state != GitlabMrState::Opened || mr.title.is_empty() {
            continue;
        }
        if mr.labels.is_empty() {
            unlabelled += 1;
            continue;
        }
        let bucket = crate::next_step::of(app, mr, me).bucket();
        let slot = Bucket::ALL.iter().position(|b| *b == bucket).unwrap_or(6);
        let age = mr
            .created_at
            .as_deref()
            .and_then(board::parse_time)
            .map(|at| board::days_between(at, now).max(0.0));
        // A label written twice on an MR counts once.
        let mut seen: Vec<String> = Vec::new();
        for label in &mr.labels {
            let key = label.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            let i = match rows.iter().position(|r| r.label.to_lowercase() == key) {
                Some(i) => i,
                None => {
                    rows.push(LabelRow {
                        label: label.clone(),
                        buckets: [0; 7],
                        open: 0,
                        median_age: None,
                        oldest: None,
                    });
                    ages.push(Vec::new());
                    rows.len() - 1
                }
            };
            let row = &mut rows[i];
            row.open += 1;
            row.buckets[slot] += 1;
            if let Some(age) = age {
                ages[i].push(age);
                if row.oldest.as_ref().is_none_or(|o| age > o.2) {
                    row.oldest = Some((mr.id.clone(), mr.title.clone(), age));
                }
            }
        }
    }
    for (row, ages) in rows.iter_mut().zip(ages) {
        row.median_age = crate::next_step::median(ages);
    }
    sort(&mut rows, Sort::Count);
    (rows, unlabelled)
}

pub fn sort(rows: &mut [LabelRow], by: Sort) {
    match by {
        Sort::Count => rows.sort_by(|a, b| {
            b.open
                .cmp(&a.open)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        }),
        Sort::Age => rows.sort_by(|a, b| {
            b.median_age
                .unwrap_or(0.0)
                .total_cmp(&a.median_age.unwrap_or(0.0))
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        }),
    }
}

/// What a click on the card asked for.
pub enum Click {
    /// List the MRs of this label.
    Label(String),
    /// Show this MR.
    Mr(String),
}

/// The color of a median age: amber past the stale days' half, red past them.
fn age_color(theme: &Theme, app: &App, days: f64) -> egui::Color32 {
    let stale = app.config.activity_stale_days.max(1) as f64;
    if days >= stale {
        theme.tones.red.fg
    } else if days >= stale / 2.0 {
        theme.tones.amber.fg
    } else {
        theme.palette.text
    }
}

/// The card. `None` when no open MR carries a label.
pub fn card(ui: &mut Ui, theme: &Theme, app: &App) -> Option<Click> {
    let (mut rows, unlabelled) = analyze(app, Utc::now());
    if rows.is_empty() {
        return None;
    }
    let sort_id = ui.make_persistent_id("cockpit-labels-sort");
    let by_age = ui.data(|d| d.get_temp::<bool>(sort_id)).unwrap_or(false);
    if by_age {
        sort(&mut rows, Sort::Age);
    }
    let mut click = None;
    theme
        .card_frame()
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Title, then the sort on the right.
            ui.horizontal(|ui| {
                ui.label(super::widgets::semibold(
                    tl("Open MRs by label"),
                    theme.text_size,
                ));
                ui.label(
                    muted(theme, tl("where they wait, and for how long"))
                        .size(theme.text_size - 1.5),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.allocate_ui(Vec2::new(320.0, 28.0), |ui| {
                        let labels = [tl("Most MRs"), tl("Oldest first")];
                        if let Some(i) = board::segmented(
                            ui,
                            theme,
                            "cockpit_labels_sort",
                            &labels,
                            usize::from(by_age),
                        ) {
                            ui.data_mut(|d| d.insert_temp(sort_id, i == 1));
                        }
                    });
                });
            });
            ui.add_space(8.0);
            legend(ui, theme, &rows);
            ui.add_space(6.0);
            let most = rows.iter().map(|r| r.open).max().unwrap_or(1).max(1);
            // Columns, right to left.
            let (oldest_w, age_w, open_w, label_w) = (200.0, 90.0, 76.0, 190.0);
            {
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                let y = head.center().y;
                let font = FontId::proportional(theme.text_size - 3.0);
                let color = theme.palette.text_muted;
                let right = head.right() - 8.0;
                let caption = |x: f32, align: Align2, text: &str, w: f32| {
                    board::paint_text(
                        ui,
                        egui::pos2(x, y),
                        align,
                        &text.to_uppercase(),
                        font.clone(),
                        color,
                        w,
                    );
                };
                caption(head.left() + 8.0, Align2::LEFT_CENTER, tl("Label"), label_w);
                caption(
                    head.left() + label_w + 16.0,
                    Align2::LEFT_CENTER,
                    tl("Where they wait"),
                    200.0,
                );
                caption(
                    right - oldest_w - age_w - 16.0,
                    Align2::RIGHT_CENTER,
                    tl("Open"),
                    open_w,
                );
                caption(
                    right - oldest_w - 8.0,
                    Align2::RIGHT_CENTER,
                    tl("Median age"),
                    age_w,
                );
                caption(right, Align2::RIGHT_CENTER, tl("Oldest"), oldest_w);
            }
            for row in rows.iter().take(SHOWN) {
                let (rect, response) = board::row(ui, theme, 34.0, false);
                let y = rect.center().y;
                let right = rect.right() - 8.0;
                // The label, as a chip in its GitLab color.
                let tone = label_tone(app, &row.label, theme);
                board::paint_tag_within(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 8.0, y),
                    &row.label,
                    tone,
                    false,
                    label_w,
                );
                // Oldest MR: a pill, its own click.
                let mut on_pill = false;
                if let Some((id, title, days)) = &row.oldest {
                    let text = format!("!{id} · {}", board::duration_text(*days));
                    let galley = ui.painter().layout_no_wrap(
                        text,
                        FontId::monospace(theme.text_size - 2.0),
                        theme.palette.text_secondary,
                    );
                    let pill = Rect::from_min_size(
                        egui::pos2(right - galley.size().x - 12.0, y - 10.0),
                        Vec2::new(galley.size().x + 12.0, 20.0),
                    );
                    let hit = ui
                        .interact(
                            pill,
                            ui.make_persistent_id(("label-oldest", &row.label)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(title);
                    ui.painter().rect_filled(
                        pill,
                        CornerRadius::same(theme.radii.small),
                        if hit.hovered() {
                            theme.palette.border
                        } else {
                            theme.palette.raised
                        },
                    );
                    ui.painter().galley(
                        pill.center() - galley.size() / 2.0,
                        galley,
                        theme.palette.text_secondary,
                    );
                    on_pill = hit.hovered();
                    if hit.clicked() {
                        click = Some(Click::Mr(id.clone()));
                    }
                }
                let age = row
                    .median_age
                    .map_or_else(|| board::dash().to_string(), board::duration_text);
                let age_color = row
                    .median_age
                    .map_or(theme.palette.text_muted, |d| age_color(theme, app, d));
                board::paint_text(
                    ui,
                    egui::pos2(right - oldest_w - 8.0, y),
                    Align2::RIGHT_CENTER,
                    &age,
                    FontId::proportional(theme.text_size - 1.0),
                    age_color,
                    age_w,
                );
                board::paint_text(
                    ui,
                    egui::pos2(right - oldest_w - age_w - 16.0, y),
                    Align2::RIGHT_CENTER,
                    &row.open.to_string(),
                    bevel_theme::fonts::semibold(theme.text_size),
                    theme.palette.text,
                    open_w,
                );
                // Where they wait: a bar as long as the count, cut by bucket.
                let bar_left = rect.left() + label_w + 16.0;
                let bar_room = (right - oldest_w - age_w - open_w - 32.0 - bar_left).max(0.0);
                if bar_room > 20.0 {
                    let width = (bar_room * row.open as f32 / most as f32).max(6.0);
                    let bar =
                        Rect::from_min_size(egui::pos2(bar_left, y - 6.0), Vec2::new(width, 12.0));
                    let parts: Vec<(f32, egui::Color32)> = Bucket::ALL
                        .iter()
                        .zip(row.buckets)
                        .map(|(b, n)| (n as f32, bucket_color(*b, theme)))
                        .collect();
                    board::paint_stacked_bar(ui, theme, bar, &parts, 1.0);
                }
                let hover: Vec<String> = Bucket::ALL
                    .iter()
                    .zip(row.buckets)
                    .filter(|(_, n)| *n > 0)
                    .map(|(b, n)| format!("{} {n}", b.label()))
                    .collect();
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!(
                        "{}\n{}",
                        hover.join(" · "),
                        tl("Click: list them in the table")
                    ));
                if response.clicked() && !on_pill && click.is_none() {
                    click = Some(Click::Label(row.label.clone()));
                }
            }
            ui.add_space(6.0);
            let mut notes = Vec::new();
            if rows.len() > SHOWN {
                notes.push(tn(
                    (rows.len() - SHOWN) as u64,
                    "{n} more label",
                    "{n} more labels",
                ));
            }
            if unlabelled > 0 {
                notes.push(tn(
                    unlabelled as u64,
                    "{n} open MR without a label",
                    "{n} open MRs without a label",
                ));
            }
            notes.push(tl("An MR with two labels counts in both.").to_string());
            ui.label(muted(theme, notes.join(" · ")).size(theme.text_size - 1.5));
        });
    click
}

/// A bucket's color on this card: hues far apart (red, yellow, purple, green,
/// blue), so the segments of a bar read at a glance. The table's chips keep
/// their softer tones, where the name is written.
fn bucket_color(bucket: Bucket, theme: &Theme) -> egui::Color32 {
    let t = &theme.tones;
    match bucket {
        Bucket::OnMe => t.red.fg,
        Bucket::OnAuthor => t.amber.fg,
        Bucket::OnReviewers => t.purple.fg,
        Bucket::ToMerge => t.green.fg,
        Bucket::ToDeploy => t.blue.fg,
        Bucket::Done => t.teal.fg,
        Bucket::Unknown => t.gray.fg,
    }
}

/// The buckets the rows use: a dot and the name.
fn legend(ui: &mut Ui, theme: &Theme, rows: &[LabelRow]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for (i, bucket) in Bucket::ALL.iter().enumerate() {
            if rows.iter().all(|r| r.buckets[i] == 0) {
                continue;
            }
            let color = bucket_color(*bucket, theme);
            super::widgets::dot_label(ui, theme, color, bucket.label());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_demo_sorts_its_labels_and_counts_each_mr_once_per_label() {
        let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        // A label written twice on an MR counts once.
        let first = app
            .mrs
            .iter_mut()
            .find(|m| m.state == GitlabMrState::Opened && !m.labels.is_empty())
            .unwrap();
        let twice = first.labels[0].to_uppercase();
        first.data.labels.push(twice);
        let (rows, _) = analyze(&app, Utc::now());
        assert!(!rows.is_empty());
        for pair in rows.windows(2) {
            assert!(pair[0].open >= pair[1].open);
        }
        let open = app
            .mrs
            .iter()
            .filter(|m| m.state == GitlabMrState::Opened && !m.title.is_empty())
            .count();
        for row in &rows {
            assert_eq!(row.buckets.iter().sum::<usize>(), row.open, "{}", row.label);
            assert!(row.open <= open);
            assert!(row.oldest.is_some() && row.median_age.is_some());
        }
        let unique: std::collections::HashSet<String> =
            rows.iter().map(|r| r.label.to_lowercase()).collect();
        assert_eq!(unique.len(), rows.len(), "a label listed twice");
    }

    #[test]
    fn by_age_puts_the_oldest_label_first() {
        let row = |label: &str, open, age| LabelRow {
            label: label.into(),
            buckets: [open, 0, 0, 0, 0, 0, 0],
            open,
            median_age: Some(age),
            oldest: None,
        };
        let mut rows = vec![row("a", 5, 1.0), row("b", 1, 9.0), row("c", 5, 3.0)];
        sort(&mut rows, Sort::Count);
        assert_eq!(rows[0].label, "a");
        assert_eq!(rows[2].label, "b");
        sort(&mut rows, Sort::Age);
        assert_eq!(rows[0].label, "b");
    }
}
