//! The period's events as stacked bars by kind: by hour over a day, by day over a week or a month.

use super::*;

/// The events of the period, filters applied, as stacked bars by kind: one
/// bar an hour over 24 h, one a day over 7 or 30 days.
pub(super) fn activity_card(
    ui: &mut Ui,
    theme: &Theme,
    events: &[&Event],
    days: i64,
    now: DateTime<Utc>,
) {
    let hourly = days <= 1;
    let buckets = buckets(now, days);
    let step = if hourly {
        Duration::hours(1)
    } else {
        Duration::days(1)
    };
    let mut counts = vec![BTreeMap::<Kind, usize>::new(); buckets.len()];
    for e in events {
        let at = e.when.with_timezone(&Local);
        if let Some(i) = buckets.iter().position(|b| at >= *b && at < *b + step) {
            *counts[i].entry(e.kind).or_default() += 1;
        }
    }
    let total: usize = counts.iter().map(|c| c.values().sum::<usize>()).sum();
    let note = tn(total as u64, "{n} event", "{n} events");
    let title = if hourly {
        tl("Activity by hour")
    } else {
        tl("Activity by day")
    };
    board::card(ui, theme, title, &note, 0.0, |ui| {
        let max = counts
            .iter()
            .map(|c| c.values().sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(1);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 110.0), Sense::hover());
        let n = buckets.len().max(1);
        let gap = if n > 14 { 4.0 } else { 8.0 };
        let w = ((rect.width() - gap * (n - 1) as f32) / n as f32).max(2.0);
        for (i, day) in counts.iter().enumerate() {
            let x = rect.left() + i as f32 * (w + gap);
            let column =
                Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + w, rect.bottom()));
            let sum: usize = day.values().sum();
            if sum == 0 {
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        egui::pos2(x, rect.bottom() - 2.0),
                        egui::pos2(x + w, rect.bottom()),
                    ),
                    CornerRadius::same(1),
                    theme.palette.border,
                );
            }
            let mut y = rect.bottom();
            for kind in Kind::ALL {
                let k = day.get(&kind).copied().unwrap_or(0);
                if k == 0 {
                    continue;
                }
                let h = (rect.height() * k as f32 / max as f32).max(3.0);
                ui.painter().rect_filled(
                    Rect::from_min_max(egui::pos2(x, y - h), egui::pos2(x + w, y)),
                    CornerRadius::same(2),
                    kind.tone(theme).fg,
                );
                y -= h + 1.5;
            }
            let when = if hourly {
                buckets[i].format("%H:00").to_string()
            } else {
                crate::utils::day_text(buckets[i].date_naive())
            };
            let mut tip = Tip::new().title(format!(
                "{when} · {}",
                tn(sum as u64, "{n} event", "{n} events")
            ));
            for (kind, k) in day {
                tip = tip.dot(kind.tone(theme).fg, format!("{} · {k}", kind.short()));
            }
            ui.interact(column, ui.scope_id().with(("bucket", i)), Sense::hover())
                .rich(tip);
        }
        // The labels: as many as fit, evenly spread, the last one always.
        let (labels, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
        let font = FontId::proportional(theme.text_size - 2.5);
        let label = |i: usize| -> String {
            if hourly {
                buckets[i].format("%H:00").to_string()
            } else if n <= 7 {
                tc(
                    "date",
                    match buckets[i].weekday() {
                        chrono::Weekday::Mon => "Mon",
                        chrono::Weekday::Tue => "Tue",
                        chrono::Weekday::Wed => "Wed",
                        chrono::Weekday::Thu => "Thu",
                        chrono::Weekday::Fri => "Fri",
                        chrono::Weekday::Sat => "Sat",
                        chrono::Weekday::Sun => "Sun",
                    },
                )
                .to_string()
            } else {
                buckets[i].format("%d/%m").to_string()
            }
        };
        let room = 52.0;
        let every = ((room / (w + gap)).ceil() as usize).max(1);
        let mut right_edge = f32::NEG_INFINITY;
        let last_cx = labels.left() + (n - 1) as f32 * (w + gap) + w / 2.0;
        for i in (0..n).filter(|i| (n - 1 - i).is_multiple_of(every)) {
            let cx = labels.left() + i as f32 * (w + gap) + w / 2.0;
            let text = label(i);
            let galley = board::one_line(ui, &text, font.clone(), theme.palette.text_muted, room);
            let half = galley.size().x / 2.0;
            let left = (cx - half).clamp(labels.left(), labels.right() - galley.size().x);
            if left < right_edge + 8.0
                || (i + 1 < n && left + galley.size().x > last_cx - half - 8.0 && i != n - 1)
            {
                continue;
            }
            right_edge = left + galley.size().x;
            ui.painter().galley(
                egui::pos2(left, labels.center().y - galley.size().y / 2.0),
                galley,
                theme.palette.text_muted,
            );
        }
        ui.add_space(8.0);
        let legend: Vec<(Color32, String)> = Kind::ALL
            .iter()
            .map(|k| (k.tone(theme).fg, k.short().to_string()))
            .collect();
        board::legend(ui, theme, &legend);
    });
}

/// The bars' starts (local), oldest first: an hour each over 24 h, a day each
/// otherwise. The first holds the period's start, so every event the page
/// counts lands in a bar: "7 d" from this hour a week ago is today and the
/// seven days before, the oldest partly.
fn buckets(now: DateTime<Utc>, days: i64) -> Vec<chrono::DateTime<Local>> {
    let local_now = now.with_timezone(&Local);
    if days <= 1 {
        let hour = local_now
            .date_naive()
            .and_hms_opt(local_now.hour(), 0, 0)
            .and_then(|t| t.and_local_timezone(Local).earliest())
            .unwrap_or(local_now);
        (0..=24).rev().map(|h| hour - Duration::hours(h)).collect()
    } else {
        let today = local_now.date_naive();
        (0..=days)
            .rev()
            .filter_map(|d| today.checked_sub_days(chrono::Days::new(d as u64)))
            .filter_map(|d| d.and_hms_opt(0, 0, 0))
            .filter_map(|t| t.and_local_timezone(Local).earliest())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bars_start_no_later_than_the_period() {
        let now = Utc::now();
        for days in [1, 7, 30] {
            let b = buckets(now, days);
            assert!(
                b[0] <= (now - Duration::days(days)).with_timezone(&Local),
                "{days} d: the oldest bar starts after the period"
            );
        }
    }
}
