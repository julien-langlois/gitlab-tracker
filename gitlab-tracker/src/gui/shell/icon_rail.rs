//! The icon rail: its entries, their badges, captions on a tall window.

use super::*;

pub(super) fn rail(g: &mut GuiApp, ui: &mut Ui, page: Page) {
    let theme = g.theme;
    egui::Panel::left("rail")
        .exact_size(RAIL_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme.palette.rail)
                .inner_margin(Margin::symmetric(6, 10)),
        )
        .show(ui, |ui| {
            let edge = ui.max_rect().expand2(Vec2::new(6.0, 10.0));
            ui.painter().vline(
                edge.right() - 0.5,
                edge.y_range(),
                Stroke::new(1.0, theme.palette.border),
            );
            ui.spacing_mut().item_spacing.y = 6.0;
            let count = |key: &str| g.members.get(key).map_or(0, Vec::len);
            let tones = &theme.tones;
            // What waits on each page: my reviews, the blocked MRs, the tickets
            // over their estimate.
            let today = super::super::today::waiting_count(g);
            let fixed = [
                (Page::Today, today, tones.amber, tl("waiting for you")),
                (
                    Page::MergeRequests,
                    count("To review (me)"),
                    tones.amber,
                    tl("waiting for your review"),
                ),
                (Page::Cockpit, count("Blocked"), tones.red, tl("blocked")),
                (
                    Page::Tracker,
                    count("Over estimate"),
                    tones.red,
                    tl("over their estimate"),
                ),
            ];
            // The pages under the rule count what their own page flags.
            let badges: Vec<(Page, usize, bevel_theme::Tone, String)> = fixed
                .iter()
                .filter(|b| b.1 > 0)
                .map(|(page, n, tone, what)| (*page, *n, *tone, format!("{n} {what}")))
                .chain(
                    [
                        Page::Team,
                        Page::Journal,
                        Page::Environments,
                        Page::Ci,
                        Page::Milestones,
                        Page::Overlaps,
                        Page::Delivery,
                        Page::Releases,
                        Page::Stacks,
                    ]
                    .into_iter()
                    .filter_map(|page| {
                        let b = g.rail_badges.get(page).filter(|b| b.n > 0)?;
                        Some((page, b.n, b.tone, b.hint.clone()))
                    }),
                )
                .collect();
            ui.vertical_centered(|ui| {
                let mut items = vec![
                    (
                        Page::Today,
                        Icon::Clock,
                        tl("Today"),
                        tl("What waits for you, in every tab"),
                    ),
                    (
                        Page::MergeRequests,
                        Icon::MergeRequests,
                        tl("MRs"),
                        tl("Merge requests"),
                    ),
                    (Page::Cockpit, Icon::Cockpit, tl("Cockpit"), tl("Cockpit")),
                ];
                if g.app.branches.len() >= 2 {
                    items.push((
                        Page::Promote,
                        Icon::ArrowUp,
                        tl("Promote"),
                        tl("What is ready to go to the next branch"),
                    ));
                }
                if g.app.tracker.is_some() {
                    items.push((
                        Page::Tracker,
                        Icon::Ticket,
                        tl("Tracker"),
                        tl("Linked tickets"),
                    ));
                }
                #[cfg(feature = "stats")]
                items.push((Page::Stats, Icon::Stats, tl("Stats"), tl("Statistics (G)")));
                // The pages that look at the project from one angle, under a rule.
                let first_extra = items.len();
                items.extend([
                    (
                        Page::Team,
                        Icon::People,
                        tl("Team"),
                        tl("Who waits on whom, and who can take the next review"),
                    ),
                    (
                        Page::Journal,
                        Icon::History,
                        tl("Journal"),
                        tl("What happened, in the order it happened"),
                    ),
                    (
                        Page::Environments,
                        Icon::Layers,
                        tl("Envs"),
                        tl("Environments: what is where, from feature to production"),
                    ),
                    (
                        Page::Ci,
                        Icon::Pulse,
                        tl("CI"),
                        tl("CI / Pipelines: what breaks, what slows down"),
                    ),
                    (
                        Page::Milestones,
                        Icon::Flag,
                        tl("Plan"),
                        tl("Milestones: progress, and whether they make their date"),
                    ),
                    (
                        Page::Overlaps,
                        Icon::Overlap,
                        // A shorter word than the page's title where it is long.
                        crate::i18n::tc("rail", "Overlaps"),
                        tl("Open MRs that change the same files"),
                    ),
                    (
                        Page::Delivery,
                        Icon::Gauge,
                        crate::i18n::tc("rail", "Delivery"),
                        tl("Delivery: DORA, and where the time goes"),
                    ),
                    (
                        Page::Releases,
                        Icon::Tag,
                        crate::i18n::tc("rail", "Releases"),
                        tl("Releases: what shipped, and what the next one will hold"),
                    ),
                    (
                        Page::Stacks,
                        Icon::Stack,
                        crate::i18n::tc("rail", "Stacks"),
                        tl("Stacked MRs: built on each other, and the order to merge them"),
                    ),
                ]);
                // Captions while every entry fits with them, else icons alone,
                // smaller still on a short window.
                let room = ui.available_height() - 2.0 * (34.0 + 6.0) - 16.0;
                let needed = |height: f32| items.len() as f32 * (height + 6.0) + 9.0;
                let size = if needed(46.0) <= room {
                    RailSize::Labeled
                } else if needed(34.0) <= room {
                    RailSize::Icon
                } else {
                    RailSize::Small
                };
                for (index, (target, icon, label, hint)) in items.into_iter().enumerate() {
                    if index == first_extra {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(28.0, 3.0), Sense::hover());
                        ui.painter().hline(
                            rect.x_range(),
                            rect.center().y,
                            Stroke::new(1.0, theme.palette.border),
                        );
                    }
                    let label = (size == RailSize::Labeled).then_some(label);
                    let badge = badges
                        .iter()
                        .find(|b| b.0 == target)
                        .map(|b| (b.1, b.2, b.3.clone()));
                    let mut hint = Tip::new().title(items_label(target)).text(hint);
                    if let Some((_, tone, what)) = &badge {
                        hint = hint.dot(tone.fg, what.as_str());
                    }
                    let response = rail_item(
                        ui,
                        &theme,
                        icon,
                        label,
                        items_label(target),
                        size.height(),
                        page == target,
                    );
                    if let Some((n, tone, _)) = badge {
                        rail_badge(ui, &theme, response.rect, n, tone);
                    }
                    if response.rich(hint).clicked() {
                        g.open_page(target);
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    if rail_item(
                        ui,
                        &theme,
                        Icon::Settings,
                        None,
                        tl("Settings"),
                        34.0,
                        false,
                    )
                    .tip(tl("Settings (,)"))
                    .clicked()
                    {
                        g.pending.push(Action::OpenSettings);
                    }
                    if rail_item(
                        ui,
                        &theme,
                        Icon::Help,
                        None,
                        tl("Keyboard shortcuts"),
                        34.0,
                        false,
                    )
                    .tip(tl("Keyboard shortcuts (?)"))
                    .clicked()
                    {
                        g.app.input_mode = InputMode::Help;
                    }
                });
            });
        });
}

/// A count on a rail entry's top-right corner, in `tone`.
pub(super) fn rail_badge(ui: &Ui, theme: &Theme, item: Rect, n: usize, tone: bevel_theme::Tone) {
    let text = badge_text(n);
    let text_color = if theme.mode == bevel_theme::Mode::Dark {
        theme.palette.rail
    } else {
        Color32::WHITE
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, fonts::semibold(9.5), text_color);
    let ink = ink_rect(&galley);
    let width = (ink.map_or(galley.size().x, |r| r.width()) + 9.0).max(16.0);
    // A wider number grows the badge leftwards: its right edge stays inside
    // the rail.
    let rect = Rect::from_min_max(
        egui::pos2(item.right() + 2.0 - width, item.top() - 2.0),
        egui::pos2(item.right() + 2.0, item.top() + 14.0),
    );
    ui.painter()
        .rect_filled(rect.expand(1.5), CornerRadius::same(10), theme.palette.rail);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), tone.fg);
    // Centred on the glyphs' ink, not on the line box: the line box has room
    // under the baseline that digits never use, which pushed them up.
    let at = rect.center() - ink.map_or(galley.size() / 2.0, |r| r.center().to_vec2());
    let ppp = ui.ctx().pixels_per_point();
    let at = egui::pos2((at.x * ppp).round() / ppp, (at.y * ppp).round() / ppp);
    ui.painter().galley(at, galley, text_color);
}

/// A badge's number: "99+" past 99.
pub(super) fn badge_text(n: usize) -> String {
    if n > 99 {
        "99+".to_string()
    } else {
        n.to_string()
    }
}

/// Where `galley`'s glyphs draw, relative to its top-left corner.
pub(super) fn ink_rect(galley: &egui::Galley) -> Option<Rect> {
    galley
        .rows
        .iter()
        .flat_map(|row| {
            row.row
                .glyphs
                .iter()
                .filter(|g| !g.uv_rect.is_nothing())
                .map(move |g| {
                    Rect::from_min_size(
                        row.pos + g.pos.to_vec2() + g.uv_rect.offset,
                        g.uv_rect.size,
                    )
                })
        })
        .reduce(|a, b| a.union(b))
}

/// How the rail's entries fit the window's height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RailSize {
    /// An icon over its caption.
    Labeled,
    /// The icon alone.
    Icon,
    /// The icon alone, packed tighter: a short window.
    Small,
}

impl RailSize {
    pub(super) fn height(self) -> f32 {
        match self {
            RailSize::Labeled => 46.0,
            RailSize::Icon => 34.0,
            RailSize::Small => 28.0,
        }
    }
}

/// The name of a page, for the rail's tooltip when its caption is hidden.
pub(super) fn items_label(page: Page) -> &'static str {
    match page {
        Page::Today => tl("Today"),
        Page::MergeRequests | Page::Mr => tl("Merge requests"),
        Page::Cockpit => tl("Cockpit"),
        Page::Promote => tl("Promote"),
        Page::Tracker => tl("Linked tickets"),
        #[cfg(feature = "stats")]
        Page::Stats => tl("Statistics"),
        Page::Team | Page::Person => tl("Team"),
        Page::Journal => tl("Journal"),
        Page::Environments => tl("Environments"),
        Page::Ci => tl("CI / Pipelines"),
        Page::Milestones => tl("Milestones"),
        Page::Overlaps => tl("Overlaps"),
        Page::Delivery => tl("Delivery"),
        Page::Releases => tl("Releases"),
        Page::Stacks => tl("Stacked MRs"),
    }
}

/// A rail entry: an icon over a short caption, highlighted when `active`.
/// An entry of the icon rail. `name` is what a screen reader announces, the
/// caption being hidden on a short window.
pub(super) fn rail_item(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    label: Option<&str>,
    name: &str,
    height: f32,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(40.0, height), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), active, name)
    });
    let p = &theme.palette;
    let (fill, color) = if active {
        (p.rail_active, p.accent)
    } else if response.hovered() {
        (p.raised, p.text)
    } else {
        (Color32::TRANSPARENT, p.text_secondary)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    let icon_center = match label {
        Some(_) => egui::pos2(rect.center().x, rect.top() + 16.0),
        None => rect.center(),
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(icon_center, Vec2::splat(18.0)),
        icon,
        color,
    );
    if let Some(label) = label {
        // The caption fits the rail: a smaller size for a long word, cut short
        // below 8.5 points.
        let room = RAIL_WIDTH - 6.0;
        let font = |size: f32| {
            if active {
                fonts::medium(size)
            } else {
                egui::FontId::proportional(size)
            }
        };
        let natural = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font(10.0), color)
            .size()
            .x;
        let size = if natural > room {
            (10.0 * room / natural).max(8.5)
        } else {
            10.0
        };
        let galley = super::super::board::one_line(ui, label, font(size), color, room);
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.bottom() - 10.0 - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    super::super::widgets::focus_ring(ui, theme, &response, 8);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
