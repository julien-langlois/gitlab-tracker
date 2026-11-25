//! How they work: review partners, their MRs' time, how they review, their latest MRs.

use super::*;

/// Who asks them for reviews, whom they ask, and who could take some.
pub(super) fn partners_card(
    ui: &mut Ui,
    theme: &Theme,
    analysis: &Analysis,
    person: &Person,
    go: &mut Go,
) {
    let (asked_by, asks) = partners(analysis, &person.login);
    let name = |login: &str| -> String {
        analysis
            .people
            .iter()
            .find(|p| p.login == login)
            .map_or(login.to_string(), |p| p.name.clone())
    };
    board::card(
        ui,
        theme,
        tl("Who they work with"),
        tl("review requests in the period"),
        0.0,
        |ui| {
            if asked_by.is_empty() && asks.is_empty() {
                board::empty(ui, theme, tl("No review request in the period."));
                return;
            }
            for (title, list, color) in [
                (tl("Ask them for a review"), &asked_by, theme.palette.accent),
                (tl("They ask for a review"), &asks, theme.tones.purple.fg),
            ] {
                if list.is_empty() {
                    continue;
                }
                subheading(ui, theme, title);
                let max = list.first().map_or(1, |l| l.1) as f32;
                for (login, n) in list {
                    let who = name(login);
                    let known = analysis.people.iter().any(|p| &p.login == login);
                    let response = bar(
                        ui,
                        theme,
                        &who,
                        *n as f32,
                        max,
                        &n.to_string(),
                        color,
                        known,
                    );
                    if known && response.tip(tl("Open their page")).clicked() {
                        *go = Go::Person(login.clone());
                    }
                }
                ui.add_space(8.0);
            }
            if let Some(text) = relief(analysis, person) {
                board::caption(ui, theme, &text);
            }
        },
    );
}

/// Where their MRs spend their time, and what they are like.
pub(super) fn authoring_card(ui: &mut Ui, theme: &Theme, profile: &Profile) {
    board::card(
        ui,
        theme,
        tl("Their MRs: where the time goes"),
        "",
        0.0,
        |ui| {
            let failed = (profile.pipelines.1 > 0)
                .then(|| f64::from(profile.pipelines.0) / f64::from(profile.pipelines.1));
            stats(
                ui,
                theme,
                &[
                    (
                        profile
                            .first_review
                            .map_or_else(|| board::dash().to_string(), board::duration_text),
                        tl("to the first review"),
                    ),
                    (
                        profile
                            .rounds
                            .map_or_else(|| board::dash().to_string(), |r| format!("{r:.1}")),
                        tl("pushes after a review, on average"),
                    ),
                    (
                        failed.map_or_else(|| board::dash().to_string(), board::percent),
                        tl("pipelines failed"),
                    ),
                ],
            );
            ui.add_space(10.0);
            subheading(
                ui,
                theme,
                &tn(
                    u64::from(profile.merged),
                    "Each step, median of their {n} MR merged in the period",
                    "Each step, median of their {n} MRs merged in the period",
                ),
            );
            let t = &theme.tones;
            let max = profile
                .stages
                .iter()
                .flatten()
                .copied()
                .fold(0.0_f64, f64::max) as f32;
            for (stage, label, color) in [
                (profile.stages[0], tl("Waiting for a review"), t.blue.fg),
                (profile.stages[1], tl("Review to approval"), t.amber.fg),
                (profile.stages[2], tl("Approval to merge"), t.green.fg),
            ] {
                let text = stage.map_or_else(|| board::dash().to_string(), board::duration_text);
                bar(
                    ui,
                    theme,
                    label,
                    stage.unwrap_or(0.0) as f32,
                    max,
                    &text,
                    color,
                    false,
                );
            }
            let total: u32 = profile.efforts.iter().sum();
            if total > 0 || !profile.labels.is_empty() {
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    if total > 0 {
                        ui.label(muted(theme, tl("Size:")).size(theme.text_size - 1.5));
                        for (n, label, tone) in [
                            (profile.efforts[0], tl("Easy"), t.green),
                            (profile.efforts[1], tl("Medium"), t.amber),
                            (profile.efforts[2], tl("Complex"), t.red),
                        ] {
                            if n > 0 {
                                board::chip(ui, theme, &format!("{label} · {n}"), tone);
                            }
                        }
                    }
                    if !profile.labels.is_empty() {
                        ui.label(muted(theme, tl("Labels:")).size(theme.text_size - 1.5));
                        for (label, n) in &profile.labels {
                            board::chip(ui, theme, &format!("{label} · {n}"), t.gray);
                        }
                    }
                });
            }
        },
    );
}

/// How they review: what they were asked, how fast they answer.
pub(super) fn reviewing_card(ui: &mut Ui, theme: &Theme, profile: &Profile) {
    board::card(ui, theme, tl("As a reviewer"), "", 0.0, |ui| {
        stats(
            ui,
            theme,
            &[
                (
                    profile.requested.to_string(),
                    tl("reviews asked in the period"),
                ),
                (profile.approved.to_string(), tl("approved by them")),
                (
                    profile.threads_opened.to_string(),
                    tl("threads opened on others' MRs"),
                ),
            ],
        );
        ui.add_space(10.0);
        subheading(ui, theme, tl("Time to answer a review request"));
        let t = &theme.tones;
        let max = profile.buckets.iter().copied().max().unwrap_or(0) as f32;
        for (n, label, color) in [
            (profile.buckets[0], tl("under 4 h"), t.green.fg),
            (profile.buckets[1], tl("4 h to 1 d"), t.blue.fg),
            (profile.buckets[2], tl("1 to 3 d"), t.amber.fg),
            (profile.buckets[3], tl("over 3 d"), t.red.fg),
        ] {
            bar(
                ui,
                theme,
                label,
                n as f32,
                max,
                &n.to_string(),
                color,
                false,
            );
        }
        if profile.unanswered > 0 {
            ui.add_space(4.0);
            board::caption(
                ui,
                theme,
                &tn(
                    u64::from(profile.unanswered),
                    "{n} request of the period not answered yet.",
                    "{n} requests of the period not answered yet.",
                ),
            );
        }
    });
}

/// Their latest MRs, open or not.
pub(super) fn recent_card(
    ui: &mut Ui,
    theme: &Theme,
    profile: &Profile,
    now: DateTime<Utc>,
    go: &mut Go,
) {
    board::card(ui, theme, tl("Their latest MRs"), "", 0.0, |ui| {
        if profile.recent.is_empty() {
            board::empty(ui, theme, tl("No tracked MR of theirs."));
            return;
        }
        let t = &theme.tones;
        for r in &profile.recent {
            let (state, color) = match r.state {
                GitlabMrState::Merged => (tc("state", "Merged"), t.green.fg),
                GitlabMrState::Closed => (tc("state", "Closed"), t.gray.fg),
                _ => (tc("state", "Open"), t.blue.fg),
            };
            let detail = match r.at {
                Some(at) => format!("{state} · {}", board::ago(at, now)),
                None => state.to_string(),
            };
            if team::line(ui, theme, &r.id, &r.title, &detail, Some(color)) {
                *go = Go::Mr(r.id.clone());
            }
        }
    });
}

/// Values over their captions, side by side.
pub(super) fn stats(ui: &mut Ui, theme: &Theme, items: &[(String, &str)]) {
    board::columns(ui, items.len(), |ui, i| {
        let (value, caption) = &items[i];
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(egui::Label::new(semibold(value, theme.text_size + 4.0)).truncate());
        ui.add(egui::Label::new(muted(theme, *caption).size(theme.text_size - 2.0)).wrap());
    });
}
