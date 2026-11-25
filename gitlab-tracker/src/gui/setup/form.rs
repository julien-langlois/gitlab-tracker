//! The form's building blocks: groups, folded rows, inputs, token inputs, banners.

use super::*;

/// A group of fields: its caption over a card (`aside` muted at the caption's
/// right end).
pub(super) fn group(
    ui: &mut Ui,
    theme: &Theme,
    caption: &str,
    aside: Option<&str>,
    add: impl FnOnce(&mut Ui),
) {
    group_with(
        ui,
        theme,
        |ui| {
            ui.label(
                semibold(caption.to_uppercase(), theme.text_size - 2.5)
                    .color(theme.palette.text_muted),
            );
            if let Some(aside) = aside {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(muted(theme, aside).size(theme.text_size - 2.0))
                            .truncate(),
                    );
                });
            }
        },
        add,
    );
}

/// A group of fields whose caption line `caption` draws, over a card.
pub(super) fn group_with(
    ui: &mut Ui,
    theme: &Theme,
    caption: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        caption(ui);
    });
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

/// A folded optional group: a dashed row with `icon` on a tile, `title` over
/// `detail`.
/// Whether it was clicked.
#[cfg(feature = "redmine")]
pub(super) fn fold_row(ui: &mut Ui, theme: &Theme, title: &str, detail: &str, icon: Icon) -> bool {
    let size = Vec2::new(ui.available_width(), 52.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let radius = CornerRadius::same(theme.radii.large);
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, radius, theme.palette.panel);
    }
    // Dashed outline: optional, not there yet.
    let stroke = Stroke::new(
        1.0,
        if hovered {
            theme.palette.accent.gamma_multiply(0.6)
        } else {
            theme.palette.border
        },
    );
    let r = rect.shrink(0.5);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for pair in corners.windows(2) {
        ui.painter().extend(egui::Shape::dashed_line(
            &[pair[0], pair[1]],
            stroke,
            5.0,
            4.0,
        ));
    }
    let tile = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        Vec2::splat(24.0),
    );
    ui.painter().rect_filled(
        tile,
        CornerRadius::same(theme.radii.medium),
        theme.palette.accent.gamma_multiply(0.14),
    );
    super::super::icons::paint(ui.painter(), tile.shrink(5.0), icon, theme.palette.accent);
    let x = rect.left() + 50.0;
    ui.painter().text(
        egui::pos2(x, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        bevel_theme::fonts::medium(theme.text_size),
        if hovered {
            theme.palette.text
        } else {
            theme.palette.text_secondary
        },
    );
    ui.painter().text(
        egui::pos2(x, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
    );
    super::super::widgets::focus_ring(ui, theme, &response, theme.radii.large);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A group's caption that folds it: a chevron, the caption, `aside` muted; the
/// whole line lights up on hover. Whether it was clicked.
#[cfg(feature = "redmine")]
pub(super) fn caption_toggle(ui: &mut Ui, theme: &Theme, caption: &str, aside: &str) -> bool {
    use super::super::tip::TipExt;
    // Lit from the last frame's hover: the line's size is known only once drawn.
    let id = ui.scope_id().with(("caption_toggle", caption));
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let tint = if hovered {
        theme.palette.text
    } else {
        theme.palette.text_muted
    };
    let rect = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (chevron, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
            super::super::icons::paint(ui.painter(), chevron, Icon::ArrowUp, tint);
            ui.label(semibold(caption.to_uppercase(), theme.text_size - 2.5).color(tint));
            ui.label(muted(theme, aside).size(theme.text_size - 2.0));
        })
        .response
        .rect
        .expand2(Vec2::new(4.0, 2.0));
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    super::super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
    response.tip(tl("Fold")).clicked()
}

/// How an [`input`] looks.
pub(super) struct Input<'a> {
    pub(super) label: &'a str,
    /// Says "optional" after the label.
    pub(super) optional: bool,
    /// Shown in the empty field.
    pub(super) hint: &'a str,
    /// Under the field, when nothing is wrong.
    pub(super) help: &'a str,
    pub(super) mono: bool,
    /// The field's mistake, said in its help's place.
    pub(super) error: Option<String>,
    /// Something is wrong with it: a red border.
    pub(super) bad: bool,
}

impl Input<'_> {
    pub(super) fn new(problem: Option<&Problem>) -> Self {
        Self {
            label: "",
            optional: false,
            hint: "",
            help: "",
            mono: false,
            error: problem.filter(|p| !p.remote).map(|p| p.text.clone()),
            bad: problem.is_some(),
        }
    }
}

/// A labelled field: the label (with "optional"), the field, then its help or
/// its mistake; `secret` masks what is typed.
pub(super) fn input(
    ui: &mut Ui,
    theme: &Theme,
    look: Input<'_>,
    text: &mut String,
) -> egui::Response {
    let response = input_only(ui, theme, &look, text, false);
    below(ui, theme, &look, |ui| {
        if !look.help.is_empty() {
            ui.add(egui::Label::new(muted(theme, look.help).size(theme.text_size - 1.5)).wrap());
        }
    });
    response
}

/// A token field: when the instance already has one (`known`), says so and lets
/// it be left empty; `None` while that is looked up.
pub(super) fn token_input(
    ui: &mut Ui,
    theme: &Theme,
    look: Input<'_>,
    text: &mut String,
    known: Option<bool>,
) {
    input_only(ui, theme, &look, text, true);
    below(ui, theme, &look, |ui| match known {
        Some(true) => {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "✔ {}",
                        tl("A token is already saved for this instance: no need to type it again.")
                    ))
                    .color(theme.tones.green.fg)
                    .size(theme.text_size - 1.5),
                )
                .wrap(),
            );
        }
        None => {
            ui.horizontal(|ui| {
                spinner(ui, 10.0, theme.palette.accent);
                ui.label(
                    muted(theme, tl("Looking for a saved token…")).size(theme.text_size - 1.5),
                );
            });
        }
        Some(false) => {
            ui.add(egui::Label::new(muted(theme, look.help).size(theme.text_size - 1.5)).wrap());
        }
    });
}

/// The label and the field, drawn like the window's other inputs: the input
/// fill, a border turning to the accent when focused, red when wrong.
pub(super) fn input_only(
    ui: &mut Ui,
    theme: &Theme,
    look: &Input<'_>,
    text: &mut String,
    secret: bool,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(look.label).font(bevel_theme::fonts::medium(theme.text_size)));
        if look.optional {
            ui.label(muted(theme, tl("optional")).size(theme.text_size - 2.0));
        }
    });
    ui.add_space(5.0);
    let height = theme.spacing.control_height + 6.0;
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let radius = CornerRadius::same(theme.radii.medium);
    ui.painter().rect_filled(rect, radius, theme.palette.input);
    let font = if look.mono {
        egui::FontId::monospace(theme.text_size - 0.5)
    } else {
        egui::FontId::proportional(theme.text_size)
    };
    let edit = egui::TextEdit::singleline(text)
        .id_salt(look.label)
        .frame(egui::Frame::NONE)
        .margin(Margin::ZERO)
        .password(secret)
        .font(font)
        .hint_text(RichText::new(look.hint).color(theme.palette.text_muted))
        .align(egui::Align2::LEFT_CENTER);
    let response = ui.put(rect.shrink2(Vec2::new(10.0, 0.0)), edit);
    let border = if look.bad {
        theme.tones.red.fg
    } else if response.has_focus() {
        theme.palette.accent
    } else {
        theme.palette.border
    };
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    response
}

/// Under a field: its mistake in red, else what `help` draws.
pub(super) fn below(ui: &mut Ui, theme: &Theme, look: &Input<'_>, help: impl FnOnce(&mut Ui)) {
    ui.add_space(5.0);
    match &look.error {
        Some(error) => {
            ui.add(
                egui::Label::new(
                    RichText::new(error)
                        .color(theme.tones.red.fg)
                        .size(theme.text_size - 1.5),
                )
                .wrap(),
            );
        }
        None => help(ui),
    }
}

/// A banner in `tone`: a mark on a round tile, the title, then what `lines` adds.
pub(super) fn banner(
    ui: &mut Ui,
    theme: &Theme,
    tone: Tone,
    mark: &str,
    title: &str,
    lines: impl FnOnce(&mut Ui),
) {
    egui::Frame::new()
        .fill(tone.bg)
        .stroke(Stroke::new(1.0, tone.fg.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(tile.center(), 13.0, tone.fg.gamma_multiply(0.2));
                ui.painter().text(
                    tile.center(),
                    egui::Align2::CENTER_CENTER,
                    mark,
                    bevel_theme::fonts::semibold(theme.text_size),
                    tone.fg,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(semibold(title, theme.text_size).color(tone.fg));
                    lines(ui);
                });
            });
        });
}
