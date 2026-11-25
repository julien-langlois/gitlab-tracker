//! The figures, the list rows and the unreleased card.

use super::*;

pub(super) fn figures(
    ui: &mut Ui,
    theme: &Theme,
    m: &Model,
    pending: Option<&Contents>,
    now: DateTime<Utc>,
) {
    let latest = &m.releases[0];
    let mut latest_caption = latest.at.map(|at| board::ago(at, now)).unwrap_or_default();
    if let Some(env) = latest.deployed.first() {
        latest_caption = format!("{latest_caption} · {}", tf("on {env}", &[("env", env)]));
    }
    let oldest = pending.and_then(|c| c.mrs.iter().filter_map(|i| i.at).min());
    let late = match (oldest, m.cadence) {
        (Some(at), Some(c)) => board::days_between(at, now) > (2.0 * c).max(7.0),
        (Some(at), None) => board::days_between(at, now) > 14.0,
        _ => false,
    };
    let (waiting, waiting_caption) = match (&m.pending, pending) {
        (_, Some(c)) => (
            tn(c.mrs.len() as u64, "{n} MR", "{n} MRs"),
            match oldest {
                Some(at) => tf(
                    "the oldest merged {time} ago",
                    &[("time", &board::duration_text(board::days_between(at, now)))],
                ),
                None => tf(
                    "nothing merged into {branch} since",
                    &[("branch", &m.integration)],
                ),
            },
        ),
        (Some(Read::Failed(_)), _) => (
            board::dash().to_string(),
            tl("the commits could not be read").to_string(),
        ),
        _ => (
            board::dash().to_string(),
            tl("reading the commits…").to_string(),
        ),
    };
    let (next, next_caption) = match (&m.next, pending) {
        (Some((v, bump)), Some(c)) => {
            let why = match bump {
                Bump::Major => tn(
                    c.count(Kind::Breaking) as u64,
                    "{n} breaking change",
                    "{n} breaking changes",
                ),
                Bump::Minor => tn(c.count(Kind::Feature) as u64, "{n} feature", "{n} features"),
                Bump::Patch => tl("fixes and other changes only").to_string(),
            };
            (
                v.clone(),
                tf("{bump}: {why}", &[("bump", bump.label()), ("why", &why)]),
            )
        }
        (None, Some(c)) if c.mrs.is_empty() => (
            board::dash().to_string(),
            tl("nothing to release").to_string(),
        ),
        _ => (board::dash().to_string(), String::new()),
    };
    let figures = [
        Figure {
            label: tl("Latest release").into(),
            value: latest.record.tag.clone(),
            value_color: None,
            caption: latest_caption,
            caption_color: None,
        },
        Figure {
            label: tl("Waiting for a release").into(),
            value: waiting,
            value_color: late.then_some(theme.tones.amber.fg),
            caption: waiting_caption,
            caption_color: late.then_some(theme.tones.amber.fg),
        },
        Figure {
            label: tl("Cadence").into(),
            value: m
                .cadence
                .map_or_else(|| board::dash().to_string(), board::duration_text),
            value_color: None,
            caption: tl("median, last 90 days").to_string(),
            caption_color: None,
        },
        Figure {
            label: tl("Next version").into(),
            value: next,
            value_color: None,
            caption: next_caption,
            caption_color: None,
        },
    ];
    board::figures(ui, theme, &figures);
}

/// A list row: the kind's tag, the id, the title, `right` on the right.
pub(super) fn item_row(
    ui: &mut Ui,
    theme: &Theme,
    item: &Item,
    right: Option<(&str, egui::Color32)>,
) -> egui::Response {
    let (rect, response) = board::row(ui, theme, 30.0, false);
    let y = rect.center().y;
    board::paint_tag_within(
        ui,
        theme,
        egui::pos2(rect.left() + 6.0, y),
        item.kind.label(),
        item.kind.tone(theme),
        false,
        70.0,
    );
    let mut x = rect.left() + 84.0;
    x += board::paint_tag(
        ui,
        theme,
        egui::pos2(x, y),
        &format!("!{}", item.id),
        board::id_tone(theme),
        true,
    ) + 10.0;
    let right_w = if right.is_some() { 110.0 } else { 0.0 };
    if let Some((text, color)) = right {
        board::paint_text(
            ui,
            egui::pos2(rect.right() - 8.0, y),
            Align2::RIGHT_CENTER,
            text,
            FontId::proportional(theme.text_size - 2.0),
            color,
            right_w,
        );
    }
    board::paint_text(
        ui,
        egui::pos2(x, y),
        Align2::LEFT_CENTER,
        &clean_title(&item.title),
        FontId::proportional(theme.text_size - 0.5),
        if item.tracked {
            theme.palette.text
        } else {
            theme.palette.text_secondary
        },
        (rect.right() - 16.0 - right_w - x).max(40.0),
    );
    let hint = if item.tracked {
        tl("Open the MR")
    } else {
        tl("Not tracked by the app")
    };
    response.rich(Tip::new().mr(&item.id, &item.title).hint(hint))
}

pub(super) fn unreleased_card(
    ui: &mut Ui,
    theme: &Theme,
    app: &App,
    m: &Model,
    now: DateTime<Utc>,
    open: &mut Option<String>,
    show: &mut Option<Vec<String>>,
) {
    let latest = &m.releases[0].record.tag;
    let note = tf(
        "merged into {branch} since {tag}",
        &[("branch", &m.integration), ("tag", latest)],
    );
    let production = m.ordered.last().cloned().unwrap_or_default();
    board::card(ui, theme, tl("Unreleased"), &note, 0.0, |ui| {
        let contents = match &m.pending {
            Some(Read::Ready(c)) => c,
            Some(Read::Failed(e)) => {
                board::empty(
                    ui,
                    theme,
                    &tf("The commits could not be read: {error}", &[("error", e)]),
                );
                return;
            }
            _ => {
                board::empty(ui, theme, tl("Reading the commits…"));
                return;
            }
        };
        if contents.mrs.is_empty() {
            board::empty(ui, theme, tl("Nothing merged since the latest release."));
        }
        let mut sorted: Vec<&Item> = contents.mrs.iter().collect();
        sorted.sort_by_key(|i| i.kind);
        for item in sorted.iter().take(PENDING_ROWS) {
            let place = reached(app, &item.id, &m.ordered);
            let right = place.as_deref().map(|b| {
                let color = if *b == production {
                    theme.tones.green.fg
                } else {
                    theme.palette.text_muted
                };
                (b, color)
            });
            if item_row(ui, theme, item, right).clicked() && item.tracked {
                *open = Some(item.id.clone());
            }
        }
        if sorted.len() > PENDING_ROWS {
            board::caption(
                ui,
                theme,
                &tn(
                    (sorted.len() - PENDING_ROWS) as u64,
                    "and {n} more",
                    "and {n} more",
                ),
            );
        }
        let mut foot = Vec::new();
        if let Some(at) = contents.mrs.iter().filter_map(|i| i.at).min() {
            foot.push(tf(
                "the oldest merged {time} ago",
                &[("time", &board::duration_text(board::days_between(at, now)))],
            ));
        }
        if contents.direct > 0 {
            foot.push(tn(
                contents.direct as u64,
                "{n} commit outside an MR",
                "{n} commits outside an MR",
            ));
        }
        if contents.truncated {
            foot.push(tl("more commits than were read").to_string());
        }
        let tracked: Vec<String> = contents
            .mrs
            .iter()
            .filter(|i| i.tracked)
            .map(|i| i.id.clone())
            .collect();
        ui.add_space(8.0);
        // Right to left: the button keeps its room, the line is cut short.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !tracked.is_empty()
                && button(ui, theme, None, tl("Show in the table"), false).clicked()
            {
                *show = Some(tracked.clone());
            }
            let line = foot.join(" · ");
            ui.add(
                egui::Label::new(muted(theme, &line).size(theme.text_size - 1.5))
                    .truncate()
                    .show_tooltip_when_elided(false),
            )
            .tip(line);
        });
    });
}
