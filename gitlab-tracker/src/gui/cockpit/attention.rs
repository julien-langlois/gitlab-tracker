//! Needs attention: the listed MRs that weigh most, in captioned columns, who each waits on.

use super::*;

/// How much each counter weighs in the "Needs attention" ranking.
pub(super) const URGENCY: &[(&str, u32)] = &[
    ("Conflicts", 5),
    ("Changes req.", 5),
    ("CI failing", 4),
    ("Overdue", 3),
    ("To review (me)", 3),
    ("Needs rebase", 3),
    ("Over estimate", 2),
    ("Discussions", 2),
    ("Stale", 2),
    ("Hot threads", 1),
    ("Flagged", 1),
];

/// An MR of the "Needs attention" list.
pub(super) struct Urgent {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) author: String,
    pub(super) status: (&'static str, Tone),
    /// The counters it is behind, heaviest first, less the one its status
    /// badge already says ("Conflicts" next to `CONFLICT`).
    pub(super) reasons: Vec<(&'static str, u32)>,
    /// Who it waits on, since when ("3 d"), in their tone.
    pub(super) next: Option<Waiting>,
    pub(super) kind: crate::status::StatusKind,
    /// The weight of its heaviest counter, the said one included: its stripe.
    pub(super) heaviest: u32,
}

/// Who an MR of the "Needs attention" list waits on.
pub(super) struct Waiting {
    /// "You", a full name, "3 reviewers", "Deploy"…
    pub(super) who: String,
    /// The people behind it, as display strings ("Full Name (@login)"), for
    /// their avatars: me alone when it waits on me.
    pub(super) people: Vec<String>,
    pub(super) since: Option<String>,
    pub(super) tone: Tone,
}

impl Waiting {
    pub(super) fn of(step: &crate::next_step::NextStep, me: Option<&str>, theme: &Theme) -> Self {
        use crate::next_step::Actor;
        let full = |display: &str| split_person(display).0.to_string();
        let people: Vec<String> = match step.actor {
            Actor::Author | Actor::Merger | Actor::Reviewers => step.people.clone(),
            _ => Vec::new(),
        };
        let (who, people) = if step.mine {
            let mine = people
                .iter()
                .find(|p| me.is_some_and(|me| crate::utils::matches_gitlab_username(p, me)))
                .cloned();
            (tl("You").to_string(), mine.into_iter().collect())
        } else {
            match people.as_slice() {
                [one] => (full(one), people.clone()),
                // The first of several: the author's, or the merger's.
                [first, ..] if step.actor != Actor::Reviewers => (full(first), vec![first.clone()]),
                _ => (step.who(), people.clone()),
            }
        };
        Self {
            who,
            people,
            since: step
                .waited_days(chrono::Utc::now())
                .map(crate::next_step::days_text),
            tone: super::super::next::tone(step, theme),
        }
    }
}

/// The counter a status badge already says, left out of the row's reasons.
pub(super) fn said_by(kind: crate::status::StatusKind) -> Option<&'static str> {
    use crate::status::StatusKind as K;
    match kind {
        K::Conflict => Some("Conflicts"),
        K::NeedsRebase => Some("Needs rebase"),
        K::RequestedChanges => Some("Changes req."),
        K::Discussions => Some("Discussions"),
        _ => None,
    }
}

/// The listed MRs that weigh most in [`URGENCY`], heaviest first.
pub(super) fn urgent(g: &GuiApp) -> Vec<Urgent> {
    let mut by_mr: HashMap<&str, Vec<(&'static str, u32)>> = HashMap::new();
    for (key, weight) in URGENCY {
        for id in g.members.get(*key).into_iter().flatten() {
            by_mr.entry(id.as_str()).or_default().push((key, *weight));
        }
    }
    let mut list: Vec<Urgent> = g
        .app
        .visible_mrs()
        // Snoozed from Today: out of the list until it wakes.
        .filter(|mr| mr.snooze.is_none())
        .filter_map(|mr| {
            let reasons = by_mr.get(mr.id.as_str())?.clone();
            let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
            // Who has to act: nobody once done, closed or not loaded.
            let acts = !matches!(
                step.actor,
                crate::next_step::Actor::Unknown | crate::next_step::Actor::Nobody
            );
            let next =
                acts.then(|| Waiting::of(&step, g.app.config.gitlab_username.as_deref(), &g.theme));
            let kind = crate::status::StatusKind::of(mr.state, &mr.mergeability);
            Some(Urgent {
                next,
                id: mr.id.clone(),
                title: mr.title.clone(),
                author: mr.author.clone(),
                status: tones::status(mr.state, &mr.mergeability, &g.theme.tones),
                reasons,
                kind,
                heaviest: 0,
            })
        })
        .collect();
    list.sort_by_key(|u| std::cmp::Reverse(u.reasons.iter().map(|r| r.1).sum::<u32>()));
    list.truncate(URGENT_COUNT);
    for u in &mut list {
        u.reasons.sort_by_key(|r| std::cmp::Reverse(r.1));
        u.heaviest = u.reasons.first().map_or(0, |r| r.1);
        let said = said_by(u.kind);
        u.reasons.retain(|r| Some(r.0) != said);
        u.reasons.truncate(3);
    }
    list
}

pub(super) fn needs_attention(ui: &mut Ui, theme: &Theme, urgent: &[Urgent]) -> Option<Pick> {
    let mut pick = None;
    card(ui, theme, tl("Needs attention"), |ui| {
        if urgent.is_empty() {
            ui.label(muted(
                theme,
                tl("Nothing urgent: no MR is blocked, late or waiting for you."),
            ));
            return;
        }
        ui.label(muted(
            theme,
            tl("The MRs that are blocked, late or waiting for you, the most urgent first."),
        ));
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let cols = Columns::measure(ui, theme, urgent);
        let (head, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        let x = cols.place(head.shrink2(Vec2::new(12.0, 0.0)));
        cols.captions(ui, theme, head, &x);
        let (line, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
        ui.painter().hline(
            line.x_range(),
            line.top() + 1.0,
            Stroke::new(1.0, theme.palette.border_soft),
        );
        for u in urgent {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            let stripe = weight_tone(theme, u.heaviest);
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 6.0), Vec2::new(3.0, 24.0)),
                CornerRadius::same(2),
                stripe.fg,
            );
            urgent_row(ui, theme, rect.shrink2(Vec2::new(12.0, 0.0)), &cols, &x, u);
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .rich(Tip::new().mr(&u.id, &u.title).hint(tl("Open its page")))
                .clicked()
            {
                pick = Some(Pick::Mr(u.id.clone()));
            }
        }
    });
    pick
}

/// Red from "CI failing" up, amber under.
pub(super) fn weight_tone(theme: &Theme, weight: u32) -> Tone {
    if weight >= 4 {
        theme.tones.red
    } else {
        theme.tones.amber
    }
}

/// The widths of the right-hand columns of the "Needs attention" rows, the
/// widest of the listed MRs' (or of the column's caption): every row starts
/// its reasons, its waiter and its status at the same x.
pub(super) struct Columns {
    pub(super) reasons: f32,
    pub(super) who: f32,
    pub(super) since: f32,
    pub(super) status: f32,
}

/// Where the columns of a "Needs attention" row start, and where its title ends.
pub(super) struct Xs {
    pub(super) title_end: f32,
    /// `None`: too narrow, the reasons are left out.
    pub(super) reasons: Option<f32>,
    pub(super) who: f32,
    pub(super) since: f32,
    pub(super) status: f32,
}

impl Columns {
    /// Space between two columns.
    pub(super) const GAP: f32 = 24.0;
    /// Space between two chips.
    pub(super) const CHIP_GAP: f32 = 4.0;
    /// The title keeps at least this much, else the reasons go.
    pub(super) const TITLE_MIN: f32 = 260.0;
    /// An avatar of the "Waiting on" column, and the space after it.
    pub(super) const AVATAR: f32 = 20.0;
    /// The widest a name gets before it is cut.
    pub(super) const WHO_MAX: f32 = 220.0;

    pub(super) fn measure(ui: &Ui, theme: &Theme, urgent: &[Urgent]) -> Self {
        let width = |text: &str, font: egui::FontId| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
                .size()
                .x
        };
        // A chip: its text plus 7 px of padding on each side.
        let chip = |text: &str, font: egui::FontId, spacing: f32| {
            width(text, font) + 14.0 + spacing * text.chars().count() as f32
        };
        let caption = |key: &str| width(&tl(key).to_uppercase(), Self::caption_font(theme));
        let max = |it: &mut dyn Iterator<Item = f32>| it.fold(0.0_f32, f32::max);
        let reasons = max(&mut urgent.iter().map(|u| {
            u.reasons
                .iter()
                .map(|(key, _)| chip(tl(key), Self::chip_font(theme), 0.0) + Self::CHIP_GAP)
                .sum::<f32>()
        }));
        let who = max(&mut urgent
            .iter()
            .filter_map(|u| u.next.as_ref())
            .map(|n| Self::avatars_width(n.people.len()) + width(&n.who, Self::who_font(theme))));
        let since = max(&mut urgent
            .iter()
            .filter_map(|u| u.next.as_ref()?.since.as_deref())
            .map(|t| width(t, Self::since_font(theme))));
        let status = max(&mut urgent
            .iter()
            .map(|u| chip(u.status.0, fonts::semibold(theme.text_size - 2.5), 0.3)));
        Self {
            reasons: reasons.max(caption("Why")),
            // A long name is cut: the column stays the width of a usual one.
            who: who.min(Self::WHO_MAX).max(caption("Waiting on")).ceil(),
            since: since.max(caption("Since")).ceil(),
            status: status.max(caption("Status")).ceil(),
        }
    }

    /// The avatars of `n` people: one, or up to three overlapping.
    pub(super) fn avatars_width(n: usize) -> f32 {
        match n {
            0 => 0.0,
            n => Self::AVATAR + 12.0 * (n.min(3) - 1) as f32 + 6.0,
        }
    }

    /// The columns' x in `rect`, from the right: status, since, who, reasons.
    pub(super) fn place(&self, rect: egui::Rect) -> Xs {
        let status = rect.right() - self.status;
        let since = status - Self::GAP - self.since;
        let who = since - 12.0 - self.who;
        let reasons = who - Self::GAP - self.reasons;
        let reasons = (reasons - rect.left() >= Self::TITLE_MIN).then_some(reasons);
        Xs {
            title_end: reasons.unwrap_or(who) - Self::GAP,
            reasons,
            who,
            since,
            status,
        }
    }

    /// The captions over the columns, in small caps like the tables'.
    pub(super) fn captions(&self, ui: &Ui, theme: &Theme, head: egui::Rect, x: &Xs) {
        let paint = |x: f32, align: egui::Align2, key: &str| {
            ui.painter().text(
                egui::pos2(x, head.center().y),
                align,
                tl(key).to_uppercase(),
                Self::caption_font(theme),
                theme.palette.text_muted,
            );
        };
        // Past the stripe and the `!id`: over the title.
        paint(head.left() + 12.0, egui::Align2::LEFT_CENTER, "MR");
        if let Some(reasons) = x.reasons {
            paint(reasons, egui::Align2::LEFT_CENTER, "Why");
        }
        paint(x.who, egui::Align2::LEFT_CENTER, "Waiting on");
        paint(x.since + self.since, egui::Align2::RIGHT_CENTER, "Since");
        paint(x.status, egui::Align2::LEFT_CENTER, "Status");
    }

    pub(super) fn caption_font(theme: &Theme) -> egui::FontId {
        egui::FontId::proportional(theme.text_size - 3.0)
    }
    pub(super) fn chip_font(theme: &Theme) -> egui::FontId {
        fonts::medium(theme.text_size - 2.5)
    }
    pub(super) fn who_font(theme: &Theme) -> egui::FontId {
        fonts::medium(theme.text_size - 1.5)
    }
    pub(super) fn since_font(theme: &Theme) -> egui::FontId {
        egui::FontId::proportional(theme.text_size - 1.5)
    }
}

/// One row of "Needs attention": `!id`, author, title on the left; the reasons,
/// who it waits on, since when and the status in columns on the right.
pub(super) fn urgent_row(
    ui: &mut Ui,
    theme: &Theme,
    rect: egui::Rect,
    cols: &Columns,
    x: &Xs,
    u: &Urgent,
) {
    let cell = |ui: &mut Ui, left: f32, width: f32, layout: Layout| {
        let rect = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(left + width.max(0.0), rect.bottom()),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
        // The row takes the click: its texts, selectable, would take it over them.
        child.style_mut().interaction.selectable_labels = false;
        child
    };
    let left_aligned = Layout::left_to_right(Align::Center);

    let mut c = cell(ui, x.status, cols.status, left_aligned);
    badge(&mut c, theme, u.status.0, u.status.1);

    if let Some(next) = &u.next {
        let mut c = cell(ui, x.who, cols.who, left_aligned);
        c.spacing_mut().item_spacing.x = 6.0;
        if !next.people.is_empty() {
            c.horizontal(|ui| {
                // Overlapping, the first on top.
                ui.spacing_mut().item_spacing.x = 12.0 - Columns::AVATAR + 2.0;
                for person in next.people.iter().take(3) {
                    let (name, _) = split_person(person);
                    avatar(ui, theme, name, Columns::AVATAR - 2.0).rich(Tip::new().person(person));
                }
            });
        }
        let label = c.add(
            egui::Label::new(
                RichText::new(&next.who)
                    .font(Columns::who_font(theme))
                    .color(next.tone.fg),
            )
            .truncate()
            .show_tooltip_when_elided(false),
        );
        let chip = match &next.since {
            Some(since) => format!("{} · {since}", next.who),
            None => next.who.clone(),
        };
        let mut tip = Tip::new()
            .chip(&chip, next.tone)
            .text(tl("Who it waits on, and since when"));
        for person in next.people.iter().skip(1) {
            tip = tip.person(person);
        }
        label.rich(tip);
        if let Some(since) = &next.since {
            let mut c = cell(
                ui,
                x.since,
                cols.since,
                Layout::right_to_left(Align::Center),
            );
            c.label(
                RichText::new(since)
                    .font(Columns::since_font(theme))
                    .color(theme.palette.text_secondary),
            );
        }
    }

    if let Some(reasons_x) = x.reasons {
        let mut c = cell(ui, reasons_x, cols.reasons, left_aligned);
        c.spacing_mut().item_spacing.x = Columns::CHIP_GAP;
        for (key, weight) in &u.reasons {
            c.add(Chip::new(tl(key), weight_tone(theme, *weight)).font(Columns::chip_font(theme)));
        }
    }

    let mut c = cell(ui, rect.left(), x.title_end - rect.left(), left_aligned);
    c.spacing_mut().item_spacing.x = 8.0;
    c.label(mono(format!("!{}", u.id), theme.text_size - 1.0).color(theme.palette.text_secondary));
    let (name, _) = split_person(&u.author);
    avatar(&mut c, theme, name, 18.0).rich(Tip::new().person(&u.author));
    c.add(
        egui::Label::new(RichText::new(&u.title).font(fonts::medium(theme.text_size))).truncate(),
    );
}
