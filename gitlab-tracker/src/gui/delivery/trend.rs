//! The DORA figures month by month, under each tile: read from the stats
//! database with its report (`App::dora_months`), one point a month, a month
//! with too few cases left out.

use super::*;
use crate::gui::board::Spark;
use chrono::Datelike;
use gitlab_tracker_stats::DoraMonth;

/// The trend lengths offered, in months (0: off).
pub(super) const TRENDS: [(usize, &str); 3] = [(0, "Off"), (6, "6 months"), (12, "12 months")];

/// The four figures' lines over the last `months` months, and a sentence for
/// each (how it went since the first month with a point); `None` when the
/// database covers no month of them.
pub(super) fn sparks(theme: &Theme, all: &[DoraMonth], months: usize) -> Option<Trend> {
    let shown = &all[all.len().saturating_sub(months)..];
    if months == 0 || !shown.iter().any(DoraMonth::covered) {
        return None;
    }
    let name = |m: &DoraMonth| super::super::widgets::month_short(m.start.month0() as usize);
    let freq_text = |per_week: f64| {
        if per_week >= 1.0 || per_week == 0.0 {
            tf("{n} / week", &[("n", &format!("{per_week:.1}"))])
        } else {
            tf(
                "{n} / month",
                &[("n", &format!("{:.1}", per_week * 30.0 / 7.0))],
            )
        }
    };
    // One figure: its value each month, what each point says, how a value
    // reads, and whether a rise is good news.
    let figure = |value: &dyn Fn(&DoraMonth) -> Option<f64>,
                  tip: &dyn Fn(&DoraMonth) -> String,
                  text: &dyn Fn(f64) -> String,
                  better_up: bool|
     -> (Option<Spark>, Option<(String, Color32)>) {
        let points: Vec<Option<f64>> = shown.iter().map(value).collect();
        let tips = shown
            .iter()
            .map(|m| {
                let what = if m.covered() {
                    tip(m)
                } else {
                    tl("not kept yet").to_string()
                };
                format!("{} · {what}", name(m))
            })
            .collect();
        let read: Vec<(usize, f64)> = points
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.map(|v| (i, v)))
            .collect();
        let sentence = match (read.first(), read.last()) {
            (Some(&(i, first)), Some(&(j, last))) if i != j => {
                let from = name(&shown[i]);
                let moved = (last - first).abs() > first.abs() * 0.05;
                Some(if !moved {
                    (
                        tf("as in {month}", &[("month", from)]),
                        theme.palette.text_muted,
                    )
                } else {
                    let up = last > first;
                    (
                        tf(
                            if up {
                                "▲ from {value} in {month}"
                            } else {
                                "▼ from {value} in {month}"
                            },
                            &[("value", &text(first)), ("month", from)],
                        ),
                        if up == better_up {
                            theme.tones.green.fg
                        } else {
                            theme.tones.orange.fg
                        },
                    )
                })
            }
            _ => None,
        };
        let spark = Spark {
            points,
            first: shown.first().map(name).unwrap_or_default().to_string(),
            last: shown.last().map(name).unwrap_or_default().to_string(),
            tips,
            color: theme.palette.accent,
        };
        (Some(spark), sentence)
    };
    let freq = figure(
        &|m| m.per_week,
        &|m| {
            let shipped = if m.from_deployments {
                tn(m.shipped as u64, "{n} deployment", "{n} deployments")
            } else {
                tn(
                    m.shipped as u64,
                    "{n} MR reaching production",
                    "{n} MRs reaching production",
                )
            };
            format!("{} ({shipped})", freq_text(m.per_week.unwrap_or(0.0)))
        },
        &freq_text,
        true,
    );
    let lead = figure(
        &|m| m.lead,
        &|m| match m.lead {
            Some(days) => format!(
                "{} ({})",
                board::duration_text(days),
                tn(m.leads as u64, "{n} MR", "{n} MRs")
            ),
            None => too_few(m.leads),
        },
        &|days| board::duration_text(days),
        false,
    );
    let failure = figure(
        &|m| m.failure_rate,
        &|m| match m.failure_rate {
            Some(rate) => format!(
                "{} ({})",
                board::percent(rate),
                tf(
                    "{n} of {total}",
                    &[
                        ("n", &m.failures.to_string()),
                        ("total", &m.changes.to_string())
                    ]
                )
            ),
            None => too_few(m.changes),
        },
        &|rate| board::percent(rate),
        false,
    );
    let restore = figure(
        &|m| m.restore,
        &|m| match m.restore {
            Some(days) => format!(
                "{} ({})",
                board::duration_text(days),
                tn(m.restores as u64, "{n} incident", "{n} incidents")
            ),
            None if m.failures == 0 => tl("no incident").to_string(),
            None => tl("not restored yet").to_string(),
        },
        &|days| board::duration_text(days),
        false,
    );
    let (sparks, sentences) = [freq, lead, failure, restore].into_iter().unzip();
    Some((sparks, sentences))
}

fn too_few(n: usize) -> String {
    tn(
        n as u64,
        "too few to tell ({n} case)",
        "too few to tell ({n} cases)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(start: &str, lead: Option<f64>) -> DoraMonth {
        DoraMonth {
            start: start.parse().unwrap(),
            shipped: 4,
            from_deployments: true,
            per_week: Some(1.0),
            lead,
            leads: 4,
            failures: 0,
            changes: 4,
            failure_rate: Some(0.0),
            restore: None,
            restores: 0,
        }
    }

    #[test]
    fn a_trend_shows_the_last_months_and_how_each_figure_went() {
        let theme = Theme::dark();
        let mut uncovered = month("2026-05-01", None);
        uncovered.per_week = None;
        let all = vec![
            month("2026-04-01", Some(20.0)),
            uncovered,
            month("2026-06-01", Some(12.0)),
            month("2026-07-01", None),
            month("2026-08-01", Some(6.0)),
        ];
        assert!(sparks(&theme, &all, 0).is_none());
        let (lines, sentences) = sparks(&theme, &all, 4).expect("a trend");
        let lead = lines[1].as_ref().expect("the lead time's line");
        // The last four months; a month without a point is a gap.
        assert_eq!(lead.points, vec![None, Some(12.0), None, Some(6.0)]);
        assert_eq!(lead.tips.len(), 4);
        // Faster since June: better, in green.
        let (text, color) = sentences[1].clone().expect("a sentence");
        assert!(
            text.contains(super::super::super::widgets::month_short(5)),
            "{text}"
        );
        assert_eq!(color, theme.tones.green.fg);
        // The frequency did not move.
        assert_eq!(
            sentences[0].as_ref().map(|s| s.1),
            Some(theme.palette.text_muted)
        );
        // Nothing covered in the months shown: no trend.
        assert!(sparks(&theme, &all[1..2], 1).is_none());
    }
}
