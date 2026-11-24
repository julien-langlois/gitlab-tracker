//! The stand-up: what I did since a moment (merged, reviewed and merged, pushed,
//! reviewed, opened), what is on today and what waits on others, across every
//! open tab, as text for the team's chat. Today's "Copy my stand-up" copies it
//! with the saved settings; its ▾ opens a panel to pick the period, the
//! sections and the format, with a preview of what the chat will show.

use bevel_theme::{fonts, Theme, Tone};
use egui::text::LayoutJob;
use egui::{
    Align, Color32, CornerRadius, FontId, Layout, Margin, Rect, RichText, Sense, Stroke,
    TextFormat, Ui, Vec2,
};
use serde::{Deserialize, Serialize};

use super::icons::{self, Icon};
use super::tip::{Tip, TipExt};
use super::today::{Item, Kind};
use super::widgets::{muted, split_person};
use super::{GuiApp, SlotState};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, ReviewActivity, TrackedMr};
use crate::utils::matches_gitlab_username;

/// How far back "what I did" looks.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    /// Since the last working day, at midnight: Friday on a Monday.
    #[default]
    LastWorkingDay,
    /// The last 24 hours.
    Day,
    /// The last 7 days, for a weekly meeting.
    Week,
    /// Since the stand-up was last copied.
    SinceLastCopy,
}

/// The chat the text is for.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// `**bold**`, `[!12](url)`: GitLab, Mattermost, Teams, Discord.
    #[default]
    Markdown,
    /// `*bold*`, `<url|!12>`: Slack does not read Markdown links.
    Slack,
    /// No markup and no links.
    Plain,
}

/// A part of the stand-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Merged,
    ReviewedMerged,
    Pushed,
    Reviewed,
    Opened,
    ToReview,
    ToFix,
    ToMerge,
    Waiting,
}

impl Section {
    /// What I did, in the order shown.
    const DONE: [Section; 5] = [
        Section::Merged,
        Section::ReviewedMerged,
        Section::Pushed,
        Section::Reviewed,
        Section::Opened,
    ];
    /// What is on today, in the order shown.
    const TODAY: [Section; 3] = [Section::ToReview, Section::ToFix, Section::ToMerge];

    fn bit(self) -> u16 {
        1 << self as u16
    }

    /// Its heading in the text.
    fn heading(self) -> &'static str {
        match self {
            Section::Merged => tl("Merged:"),
            Section::ReviewedMerged => tl("Reviewed & merged:"),
            Section::Pushed => tl("Pushed to:"),
            Section::Reviewed => tl("Reviewed:"),
            Section::Opened => tl("Opened:"),
            Section::ToReview => tl("To review:"),
            Section::ToFix => tl("To fix:"),
            Section::ToMerge => tl("To merge:"),
            Section::Waiting => tl("Waiting on others"),
        }
    }

    /// Its name in the panel: the heading without its colon.
    fn label(self) -> &'static str {
        self.heading().trim_end_matches([':', '：', ' ', '\u{a0}'])
    }

    fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Section::Merged => t.purple,
            Section::ReviewedMerged => t.blue,
            Section::Pushed => t.teal,
            Section::Reviewed => t.amber,
            Section::Opened => t.green,
            Section::ToReview => t.amber,
            Section::ToFix => t.red,
            Section::ToMerge => t.green,
            Section::Waiting => t.gray,
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Merged | Section::ReviewedMerged | Section::ToMerge => Icon::MergeRequests,
            Section::Pushed => Icon::ArrowUp,
            Section::Reviewed => Icon::Comment,
            Section::Opened => Icon::Plus,
            Section::ToReview => Icon::Search,
            Section::ToFix => Icon::Flag,
            Section::Waiting => Icon::Clock,
        }
    }
}

/// How the stand-up is written; kept with the window's preferences.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub period: Period,
    /// The sections left out, a bit each ([`Section::bit`]).
    pub hidden: u16,
    pub format: Format,
    /// The MRs and tickets link to GitLab and the tracker.
    pub links: bool,
    /// When it was last copied (Unix seconds), where "since the last copy" starts.
    pub last_copied: Option<i64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            period: Period::LastWorkingDay,
            hidden: 0,
            format: Format::Markdown,
            links: true,
            last_copied: None,
        }
    }
}

impl Settings {
    fn shows(&self, section: Section) -> bool {
        self.hidden & section.bit() == 0
    }

    fn last_copied(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.last_copied
            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
    }

    /// Where "what I did" starts; "since the last copy" with none yet starts at
    /// the last working day.
    fn since(&self, now: chrono::DateTime<chrono::Local>) -> chrono::DateTime<chrono::Utc> {
        let utc = now.with_timezone(&chrono::Utc);
        match self.period {
            Period::LastWorkingDay => last_working_day(now),
            Period::Day => utc - chrono::Duration::hours(24),
            Period::Week => utc - chrono::Duration::days(7),
            Period::SinceLastCopy => self.last_copied().unwrap_or_else(|| last_working_day(now)),
        }
    }

    /// "Since 2026-11-23", with the time when the period does not start at midnight.
    fn since_text(&self, now: chrono::DateTime<chrono::Local>) -> String {
        let since = self.since(now).with_timezone(&chrono::Local);
        let day = match self.period {
            Period::Day => since.format("%Y-%m-%d %H:%M"),
            Period::SinceLastCopy if self.last_copied.is_some() => since.format("%Y-%m-%d %H:%M"),
            _ => since.format("%Y-%m-%d"),
        };
        tf("Since {day}", &[("day", &day.to_string())])
    }
}

/// Where the stand-up's "since" starts: the last working day, at midnight
/// (Friday on a Monday, else yesterday).
fn last_working_day(now: chrono::DateTime<chrono::Local>) -> chrono::DateTime<chrono::Utc> {
    use chrono::{Datelike, Weekday};
    let back = match now.weekday() {
        Weekday::Mon => 3,
        Weekday::Sun => 2,
        _ => 1,
    };
    let day = now.date_naive() - chrono::Duration::days(back);
    day.and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
        .map_or_else(
            || now.with_timezone(&chrono::Utc) - chrono::Duration::days(back),
            |t| t.with_timezone(&chrono::Utc),
        )
}

/// Which "what I did" section `mr` goes in for `me`, looking back to `since`:
/// one only, the first that applies.
fn done(mr: &TrackedMr, me: &str, since: chrono::DateTime<chrono::Utc>) -> Option<Section> {
    let after = |at: Option<&str>| {
        at.and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
            .is_some_and(|t| t.with_timezone(&chrono::Utc) >= since)
    };
    let is_me = |who: &str| matches_gitlab_username(who, me) || who == me;
    let merged = mr.state == GitlabMrState::Merged && after(mr.merged_at.as_deref());
    let by_me = |map: fn(&ReviewActivity) -> &std::collections::BTreeMap<String, String>| {
        after(
            mr.review
                .as_ref()
                .and_then(|r| map(r).get(me))
                .map(String::as_str),
        )
    };
    if is_me(&mr.author) {
        if merged {
            Some(Section::Merged)
        } else if after(mr.created_at.as_deref()) {
            Some(Section::Opened)
        } else if mr.state == GitlabMrState::Opened
            && after(mr.review.as_ref().and_then(|r| r.last_push_at.as_deref()))
        {
            Some(Section::Pushed)
        } else {
            None
        }
    } else if merged && mr.merged_by.as_deref().is_some_and(is_me) {
        // Someone else's I merged: whatever else I did on it, it shows here.
        Some(Section::ReviewedMerged)
    } else if by_me(|r| &r.last_push_by) {
        Some(Section::Pushed)
    } else if by_me(|r| &r.last_activity_by) {
        Some(Section::Reviewed)
    } else {
        None
    }
}

/// The stand-up before its format: every section's MR lines, in Markdown
/// (with their links when asked), hidden sections included for the counts.
pub struct Standup {
    date: chrono::NaiveDate,
    since: String,
    groups: Vec<(Section, Vec<String>)>,
}

impl Standup {
    fn of(&self, section: Section) -> &[String] {
        self.groups
            .iter()
            .find(|(s, _)| *s == section)
            .map_or(&[], |(_, lines)| lines.as_slice())
    }
}

/// The stand-up of every open tab, as `settings` asks, at `now`.
fn build(
    g: &GuiApp,
    items: &[Item],
    settings: &Settings,
    now: chrono::DateTime<chrono::Local>,
) -> Standup {
    let since = settings.since(now);
    let line = |id: &str, title: &str, project: Option<&str>| match project {
        Some(p) => format!("!{id} {title} ({p})"),
        None => format!("!{id} {title}"),
    };
    let several = g
        .tabs
        .iter()
        .filter(|t| matches!(t.state, SlotState::Active | SlotState::Parked(_)))
        .count()
        > 1;
    // Each tab's links: its MRs and tickets, clickable once pasted.
    let links: Vec<Option<super::board::Links>> = g
        .tabs
        .iter()
        .map(|slot| match &slot.state {
            _ if !settings.links => None,
            SlotState::Active => Some(super::board::Links::of(&g.app)),
            SlotState::Parked(tab) => Some(super::board::Links::of(&tab.app)),
            _ => None,
        })
        .collect();
    let linked = |tab: usize, text: String| match links.get(tab).and_then(Option::as_ref) {
        Some(l) => l.apply(&text),
        None => text,
    };
    let mut groups: Vec<(Section, Vec<String>)> = Section::DONE
        .iter()
        .chain(&Section::TODAY)
        .chain(&[Section::Waiting])
        .map(|s| (*s, Vec::new()))
        .collect();
    let mut push = |section: Section, text: String| {
        if let Some((_, lines)) = groups.iter_mut().find(|(s, _)| *s == section) {
            lines.push(text);
        }
    };
    for (index, slot) in g.tabs.iter().enumerate() {
        let app = match &slot.state {
            SlotState::Active => &g.app,
            SlotState::Parked(tab) => &tab.app,
            _ => continue,
        };
        let Some(me) = app.config.gitlab_username.as_deref() else {
            continue;
        };
        let project = app
            .project_name
            .clone()
            .unwrap_or_else(|| app.project_id.clone());
        let project = several.then_some(project.as_str());
        for mr in app.mrs.iter().filter(|mr| !mr.title.is_empty()) {
            let Some(section) = done(mr, me, since) else {
                continue;
            };
            let mut text = linked(index, line(&mr.id, &mr.title, project));
            let mine = matches_gitlab_username(&mr.author, me) || mr.author == me;
            if section == Section::Pushed && !mine {
                // Pushed to someone else's: say whose.
                let name = split_person(&mr.author).0;
                text = format!("{text} — {}", tf("{name}'s MR", &[("name", name)]));
            }
            push(section, text);
        }
    }
    for (section, kind) in [
        (Section::ToReview, Kind::Review),
        (Section::ToFix, Kind::Fix),
        (Section::ToMerge, Kind::Ready),
        (Section::Waiting, Kind::Waiting),
    ] {
        for i in items.iter().filter(|i| i.kind == kind) {
            let base = linked(
                i.tab,
                line(&i.id, &i.title, several.then_some(i.project.as_str())),
            );
            push(
                section,
                if i.reasons.is_empty() {
                    base
                } else {
                    format!("{base} — {}", i.reasons.join(", "))
                },
            );
        }
    }
    Standup {
        date: now.date_naive(),
        since: settings.since_text(now),
        groups,
    }
}

/// A line of the stand-up, before its format.
#[derive(Debug, Clone, PartialEq)]
enum Line {
    Title(String),
    /// "Since …", "Today", "Waiting on others".
    Part(String),
    Heading(Section),
    /// An MR, in Markdown.
    Item(String),
    /// "Nothing on GitLab.": a part with nothing to say.
    Nothing(String),
    Gap,
}

/// The lines of `standup`, the sections `settings` hides left out.
fn lines(standup: &Standup, settings: &Settings) -> Vec<Line> {
    let mut out = vec![
        Line::Title(tf(
            "Stand-up · {date}",
            &[("date", &standup.date.format("%Y-%m-%d").to_string())],
        )),
        Line::Gap,
        Line::Part(standup.since.clone()),
    ];
    let part = |out: &mut Vec<Line>, sections: &[Section], nothing: &str| {
        let before = out.len();
        for s in sections.iter().filter(|s| settings.shows(**s)) {
            let items = standup.of(*s);
            if !items.is_empty() {
                out.push(Line::Heading(*s));
                out.extend(items.iter().cloned().map(Line::Item));
            }
        }
        if out.len() == before {
            out.push(Line::Nothing(nothing.to_string()));
        }
    };
    part(&mut out, &Section::DONE, tl("Nothing on GitLab."));
    out.extend([Line::Gap, Line::Part(tl("Today").to_string())]);
    part(&mut out, &Section::TODAY, tl("Nothing waits for me."));
    let waiting = standup.of(Section::Waiting);
    if settings.shows(Section::Waiting) && !waiting.is_empty() {
        out.extend([
            Line::Gap,
            Line::Part(Section::Waiting.heading().to_string()),
        ]);
        out.extend(waiting.iter().cloned().map(Line::Item));
    }
    out
}

/// `text`'s Markdown links `[label](url)` cut out: the plain pieces, and the
/// links as (label, url).
fn pieces(text: &str) -> Vec<(&str, Option<&str>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let link = rest[open..].find("](").and_then(|mid| {
            let label = &rest[open + 1..open + mid];
            let after = &rest[open + mid + 2..];
            let close = after.find(')')?;
            let url = &after[..close];
            (!label.contains('[') && url.starts_with("http")).then_some((
                label,
                url,
                open + mid + 2 + close + 1,
            ))
        });
        match link {
            Some((label, url, end)) => {
                if open > 0 {
                    out.push((&rest[..open], None));
                }
                out.push((label, Some(url)));
                rest = &rest[end..];
            }
            None => {
                out.push((&rest[..open + 1], None));
                rest = &rest[open + 1..];
            }
        }
    }
    if !rest.is_empty() {
        out.push((rest, None));
    }
    out
}

/// `text`'s Markdown links written as `format` wants them.
fn relink(text: &str, format: Format) -> String {
    if format == Format::Markdown {
        return text.to_string();
    }
    pieces(text)
        .into_iter()
        .map(|(label, url)| match (url, format) {
            (Some(url), Format::Slack) => format!("<{url}|{label}>"),
            _ => label.to_string(),
        })
        .collect()
}

/// The text to paste.
fn render(lines: &[Line], format: Format) -> String {
    let bullet = if format == Format::Slack { "•" } else { "-" };
    let mut out = String::new();
    for line in lines {
        let text = match line {
            Line::Title(t) => match format {
                Format::Markdown => format!("**{t}**"),
                Format::Slack => format!("*{t}*"),
                Format::Plain => t.clone(),
            },
            Line::Part(t) => match format {
                Format::Plain => t.clone(),
                _ => format!("_{t}_"),
            },
            Line::Heading(s) => s.heading().to_string(),
            Line::Item(t) => format!("{bullet} {}", relink(t, format)),
            Line::Nothing(t) => format!("{bullet} {t}"),
            Line::Gap => String::new(),
        };
        out.push_str(&text);
        out.push('\n');
    }
    out
}

/// The stand-up's text, as `settings` asks, at `now`.
fn text(
    g: &GuiApp,
    items: &[Item],
    settings: &Settings,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    render(
        &lines(&build(g, items, settings, now), settings),
        settings.format,
    )
}

const PANEL_WIDTH: f32 = 780.0;
const LEFT_WIDTH: f32 = 330.0;

/// Today's "Copy my stand-up | ▾", in a right-to-left row: a click copies, the
/// ▾ opens the panel.
pub fn button(g: &mut GuiApp, ui: &mut Ui, items: &[Item]) {
    let theme = g.theme;
    let p = &theme.palette;
    let label = tl("Copy my stand-up");
    let font = fonts::medium(theme.text_size - 0.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, p.text_secondary);
    let chevron_w = 30.0;
    let main_w = 10.0 + 20.0 + galley.size().x + 12.0;
    let height = theme.spacing.control_height + 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(main_w + chevron_w, height), Sense::hover());
    let main_rect = Rect::from_min_size(rect.min, Vec2::new(main_w, height));
    let chevron_rect = Rect::from_min_max(egui::pos2(main_rect.right(), rect.top()), rect.max);
    let main = ui
        .interact(
            main_rect,
            ui.scope_id().with("standup-copy"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let chevron = ui
        .interact(
            chevron_rect,
            ui.scope_id().with("standup-menu"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&chevron));
    if ui.is_rect_visible(rect) {
        let r = theme.radii.medium;
        let painter = ui.painter();
        let half = |rect: Rect, left: bool, on: bool| {
            if on {
                let radius = if left {
                    CornerRadius {
                        nw: r,
                        sw: r,
                        ne: 0,
                        se: 0,
                    }
                } else {
                    CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: r,
                        se: r,
                    }
                };
                painter.rect_filled(rect, radius, p.raised);
            }
        };
        half(main_rect, true, main.hovered());
        half(chevron_rect, false, chevron.hovered() || open);
        painter.rect_stroke(
            rect,
            CornerRadius::same(r),
            Stroke::new(1.0, if open { p.accent } else { p.border }),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [
                egui::pos2(main_rect.right(), rect.top() + 6.0),
                egui::pos2(main_rect.right(), rect.bottom() - 6.0),
            ],
            Stroke::new(1.0, p.border),
        );
        let tint = |hovered: bool| if hovered { p.text } else { p.text_secondary };
        let icon_rect = Rect::from_center_size(
            egui::pos2(main_rect.left() + 17.0, rect.center().y),
            Vec2::splat(14.0),
        );
        icons::paint(painter, icon_rect, Icon::Copy, tint(main.hovered()));
        painter.galley(
            egui::pos2(
                main_rect.left() + 30.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            tint(main.hovered()),
        );
        let chevron_icon = Rect::from_center_size(chevron_rect.center(), Vec2::splat(12.0));
        icons::paint(
            painter,
            chevron_icon,
            if open { Icon::ArrowUp } else { Icon::ArrowDown },
            if open {
                p.accent
            } else {
                tint(chevron.hovered())
            },
        );
    }
    super::widgets::focus_ring(ui, &theme, &main, theme.radii.medium);
    let main = main.tip(tl(
        "What you did since the last working day, what is on today, and what waits on others, as text for the daily meeting",
    ));
    Tip::new()
        .text(tl("Pick the period, the sections and the format"))
        .show(chevron.clone());

    let mut settings = g.standup;
    let mut copy = main.clicked();
    let frame = egui::Frame::new()
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .shadow(egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(90),
        });
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_PAGE=standup` keeps it open.
    #[cfg(debug_assertions)]
    let forced = std::env::var("GITLAB_TRACKER_GUI_PAGE").as_deref() == Ok("standup");
    #[cfg(not(debug_assertions))]
    let forced = false;
    let now = chrono::Local::now();
    egui::Popup::menu(&chevron)
        .open_memory(if forced {
            Some(egui::SetOpenCommand::Bool(true))
        } else {
            chevron.clicked().then_some(egui::SetOpenCommand::Toggle)
        })
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let standup = build(g, items, &settings, now);
            if panel(ui, &theme, &standup, &mut settings, now) {
                copy = true;
                ui.close();
            }
        });
    if copy {
        ui.ctx().copy_text(text(g, items, &settings, now));
        settings.last_copied = Some(now.timestamp());
        g.app
            .flash(tl("Stand-up copied: paste it in the team's chat"), false);
    }
    g.standup = settings;
}

/// The panel's content; whether "Copy" was clicked.
fn panel(
    ui: &mut Ui,
    theme: &Theme,
    standup: &Standup,
    settings: &mut Settings,
    now: chrono::DateTime<chrono::Local>,
) -> bool {
    ui.set_width(PANEL_WIDTH);
    ui.spacing_mut().item_spacing.y = 0.0;
    let p = &theme.palette;
    // Header: title, the period's start, reset; the periods.
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new(tl("Stand-up")).font(fonts::semibold(theme.text_size + 2.0)),
                );
                ui.add(bevel_theme::Chip::new(&standup.since, theme.tones.blue).pill());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let changed = Settings {
                        last_copied: settings.last_copied,
                        ..Settings::default()
                    } != *settings;
                    if changed && link(ui, theme, tl("Reset")).clicked() {
                        *settings = Settings {
                            last_copied: settings.last_copied,
                            ..Settings::default()
                        };
                    }
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (period, label) in [
                    (Period::LastWorkingDay, tl("Last working day")),
                    (Period::Day, tl("24 h")),
                    (Period::Week, tl("7 days")),
                    (Period::SinceLastCopy, tl("Since the last copy")),
                ] {
                    let never = period == Period::SinceLastCopy && settings.last_copied.is_none();
                    let r = ui.add_enabled_ui(!never, |ui| {
                        super::widgets::pill_toggle(
                            ui,
                            theme,
                            label,
                            theme.tones.blue,
                            settings.period == period,
                        )
                    });
                    let r = r.inner;
                    if never {
                        Tip::new()
                            .text(tl("Once you have copied a stand-up"))
                            .show(r.clone());
                    } else if period == Period::SinceLastCopy {
                        if let Some(at) = settings.last_copied() {
                            let at = at.with_timezone(&chrono::Local);
                            Tip::new()
                                .text(tf(
                                    "Last copied {when}",
                                    &[("when", &at.format("%Y-%m-%d %H:%M").to_string())],
                                ))
                                .show(r.clone());
                        }
                    }
                    if r.clicked() {
                        settings.period = period;
                    }
                }
            });
        });
    separator(ui, theme);

    // Body: the sections and the format on the left, the preview on the right.
    let mut body_height = 0.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let left = ui
            .allocate_ui_with_layout(
                Vec2::new(LEFT_WIDTH, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(LEFT_WIDTH);
                    ui.add_space(6.0);
                    heading(ui, theme, tl("What I did"));
                    for s in Section::DONE {
                        toggle_row(ui, theme, s, standup.of(s).len(), settings);
                    }
                    heading(ui, theme, tl("Today"));
                    for s in Section::TODAY.into_iter().chain([Section::Waiting]) {
                        toggle_row(ui, theme, s, standup.of(s).len(), settings);
                    }
                    heading(ui, theme, tl("Format"));
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 16,
                            right: 12,
                            top: 2,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for (format, label, tip) in [
                                    (
                                        Format::Markdown,
                                        tl("Markdown"),
                                        tl("GitLab, Mattermost, Teams, Discord"),
                                    ),
                                    (Format::Slack, tl("Slack"), tl("Slack's own bold and links")),
                                    (Format::Plain, tl("Plain text"), tl("No markup, no links")),
                                ] {
                                    let r = super::widgets::pill_toggle(
                                        ui,
                                        theme,
                                        label,
                                        theme.tones.purple,
                                        settings.format == format,
                                    )
                                    .tip(tip);
                                    if r.clicked() {
                                        settings.format = format;
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            ui.add_enabled_ui(settings.format != Format::Plain, |ui| {
                                let on = settings.links && settings.format != Format::Plain;
                                if super::widgets::switch(
                                    ui,
                                    theme,
                                    on,
                                    tl("Links to the MRs and tickets"),
                                )
                                .clicked()
                                {
                                    settings.links = !settings.links;
                                }
                            });
                        });
                    ui.add_space(10.0);
                },
            )
            .response
            .rect;
        body_height = left.height();
        // The line between the columns.
        let x = left.right();
        ui.painter().line_segment(
            [
                egui::pos2(x, left.top()),
                egui::pos2(x, left.top() + body_height),
            ],
            Stroke::new(1.0, p.border),
        );
        ui.allocate_ui_with_layout(
            Vec2::new(PANEL_WIDTH - LEFT_WIDTH, body_height),
            Layout::top_down(Align::Min),
            |ui| {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 12,
                        bottom: 12,
                    })
                    .show(ui, |ui| {
                        preview(ui, theme, standup, settings, body_height - 24.0);
                    });
            },
        );
    });
    separator(ui, theme);

    // Footer: what is kept, and Copy.
    let mut copy = false;
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let kept = match settings.last_copied() {
                    Some(at) => format!(
                        "{} · {}",
                        tl("Kept for the next stand-ups"),
                        tf(
                            "last copied {time} ago",
                            &[(
                                "time",
                                &super::board::duration_text(
                                    super::board::days_between(at, now.with_timezone(&chrono::Utc))
                                        .max(0.0)
                                )
                            )]
                        )
                    ),
                    None => tl("Kept for the next stand-ups").to_string(),
                };
                ui.label(muted(theme, kept).size(theme.text_size - 1.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    copy = super::widgets::button(ui, theme, Some(Icon::Copy), tl("Copy"), true)
                        .clicked();
                });
            });
        });
    copy
}

/// A line across the panel: `border`, as `border_soft` hardly shows on `raised`.
fn separator(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.palette.border);
}

fn link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
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

/// A small caps heading, as the notifications' days.
fn heading(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(muted(theme, text.to_uppercase()).size(theme.text_size - 2.5));
        });
}

/// A section's row: its icon, its name, how many MRs it has, its switch.
fn toggle_row(ui: &mut Ui, theme: &Theme, section: Section, count: usize, settings: &mut Settings) {
    let p = &theme.palette;
    let on = settings.shows(section);
    let (outer, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    let rect = outer.shrink2(Vec2::new(8.0, 1.0));
    if ui.is_rect_visible(outer) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(theme.radii.medium), p.border_soft);
        }
        let tone = section.tone(theme);
        let how_on = ui.ctx().animate_bool(response.id, on);
        let center = egui::pos2(rect.left() + 20.0, rect.center().y);
        painter.circle_filled(center, 12.0, lerp(p.border_soft, tone.bg, how_on));
        icons::paint(
            painter,
            Rect::from_center_size(center, Vec2::splat(12.0)),
            section.icon(),
            lerp(p.text_muted, tone.fg, how_on),
        );
        let name = painter.layout_no_wrap(
            section.label().to_string(),
            FontId::proportional(theme.text_size),
            if on { p.text } else { p.text_muted },
        );
        painter.galley(
            egui::pos2(rect.left() + 42.0, rect.center().y - name.size().y / 2.0),
            name,
            p.text,
        );
        let track = super::widgets::switch_track(
            ui,
            theme,
            egui::pos2(rect.right() - 38.0, rect.center().y),
            how_on,
        );
        // The count, as a pill in the section's tone while it has some.
        let n = painter.layout_no_wrap(
            count.to_string(),
            fonts::semibold(theme.text_size - 2.0),
            if count > 0 && on {
                tone.fg
            } else {
                p.text_muted
            },
        );
        let pill = Rect::from_center_size(
            egui::pos2(
                track.left() - 12.0 - (n.size().x + 14.0) / 2.0,
                rect.center().y,
            ),
            Vec2::new((n.size().x + 14.0).max(22.0), 18.0),
        );
        if count > 0 && on {
            painter.rect_filled(pill, CornerRadius::same(9), tone.bg);
        }
        painter.galley(pill.center() - n.size() / 2.0, n, p.text_muted);
    }
    super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        settings.hidden ^= section.bit();
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// The stand-up as the chat will show it: bold title, the parts in small caps,
/// each heading in its section's tone, the links in the accent.
fn preview(ui: &mut Ui, theme: &Theme, standup: &Standup, settings: &Settings, height: f32) {
    let p = &theme.palette;
    let lines = lines(standup, settings);
    let mrs = lines.iter().filter(|l| matches!(l, Line::Item(_))).count();
    ui.horizontal(|ui| {
        ui.label(muted(theme, tl("Preview").to_uppercase()).size(theme.text_size - 2.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(muted(theme, tn(mrs as u64, "{n} MR", "{n} MRs")).size(theme.text_size - 2.5));
        });
    });
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, p.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height((height - 52.0).max(120.0))
                .min_scrolled_height((height - 52.0).max(120.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let wrap = ui.available_width();
                    let size = theme.text_size - 1.0;
                    for line in &lines {
                        match line {
                            Line::Title(t) => {
                                ui.label(RichText::new(t).font(fonts::semibold(size + 1.5)));
                            }
                            Line::Part(t) => {
                                ui.label(
                                    RichText::new(t)
                                        .italics()
                                        .size(size - 0.5)
                                        .color(p.text_muted),
                                );
                            }
                            Line::Heading(s) => {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    super::widgets::dot(ui, s.tone(theme).fg, 6.0);
                                    ui.label(
                                        RichText::new(s.label())
                                            .font(fonts::semibold(size))
                                            .color(s.tone(theme).fg),
                                    );
                                });
                            }
                            Line::Item(t) => {
                                let mut job = LayoutJob::default();
                                let plain = TextFormat {
                                    font_id: FontId::proportional(size),
                                    color: p.text_secondary,
                                    ..TextFormat::default()
                                };
                                for (piece, url) in pieces(t) {
                                    let linked = url.is_some() && settings.format != Format::Plain;
                                    job.append(
                                        piece,
                                        0.0,
                                        if linked {
                                            TextFormat {
                                                color: p.accent,
                                                ..plain.clone()
                                            }
                                        } else {
                                            plain.clone()
                                        },
                                    );
                                }
                                job.wrap.max_width = wrap - 34.0;
                                // The bullet hangs: the wrapped lines align
                                // with the text.
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.add_sized(
                                        Vec2::new(16.0, size + 4.0),
                                        egui::Label::new(
                                            RichText::new("•").size(size).color(p.text_muted),
                                        ),
                                    );
                                    ui.add(egui::Label::new(job).wrap());
                                });
                            }
                            Line::Nothing(t) => {
                                ui.label(
                                    RichText::new(format!("•  {t}"))
                                        .size(size)
                                        .italics()
                                        .color(p.text_muted),
                                );
                            }
                            Line::Gap => ui.add_space(8.0),
                        }
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn each_mr_goes_in_one_section() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let since = chrono::Utc::now() - chrono::Duration::days(2);
        let of = |title: &str, me: &str| {
            let mr = app.mrs.iter().find(|m| m.title.starts_with(title)).unwrap();
            done(mr, me, since)
        };
        // Alex's hotfix: I reviewed and merged it, it shows as merged only.
        assert_eq!(
            of("fix(auth): Refresh tokens raced", "thomas_db"),
            Some(Section::ReviewedMerged)
        );
        // Julien's draft: I commented and pushed to it, the push wins.
        assert_eq!(
            of("Draft: feat(notif): Mute notifications", "thomas_db"),
            Some(Section::Pushed)
        );
        // Marina's refactor: Julien answered his review with a commit.
        assert_eq!(
            of("refactor(api): One module per shipments route", "julien_m"),
            Some(Section::Pushed)
        );
        // My own MR merged: merged, never "reviewed & merged".
        assert_eq!(
            of("fix(api): 404, not 500", "thomas_db"),
            Some(Section::Merged)
        );
        // Commented only: reviewed.
        let mut mute = app
            .mrs
            .iter()
            .find(|m| m.title.starts_with("Draft: feat(notif): Mute"))
            .unwrap()
            .clone();
        mute.review
            .as_mut()
            .unwrap()
            .last_push_by
            .remove("thomas_db");
        assert_eq!(done(&mute, "thomas_db", since), Some(Section::Reviewed));
    }

    #[test]
    fn it_looks_back_to_the_last_working_day() {
        use chrono::TimeZone;
        let at = |y, m, d| {
            chrono::Local
                .with_ymd_and_hms(y, m, d, 9, 30, 0)
                .earliest()
                .unwrap()
        };
        let day = |t: chrono::DateTime<chrono::Utc>| t.with_timezone(&chrono::Local).date_naive();
        // Monday 2026-11-09 → Friday; Wednesday → Tuesday.
        assert_eq!(
            day(last_working_day(at(2026, 11, 9))),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
        );
        assert_eq!(
            day(last_working_day(at(2026, 11, 11))),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
        );
        // "Since the last copy" before any copy: the last working day too.
        let s = Settings {
            period: Period::SinceLastCopy,
            ..Settings::default()
        };
        assert_eq!(
            s.since(at(2026, 11, 11)),
            last_working_day(at(2026, 11, 11))
        );
    }

    fn sample() -> Standup {
        let mut groups: Vec<(Section, Vec<String>)> = Section::DONE
            .iter()
            .chain(&Section::TODAY)
            .chain(&[Section::Waiting])
            .map(|s| (*s, Vec::new()))
            .collect();
        groups[0]
            .1
            .push("[!12](https://gl/mr/12) ([#7](https://rm/7)) Fix it".into());
        groups[6]
            .1
            .push("[!13](https://gl/mr/13) Mine — CI failed".into());
        groups[8].1.push("[!14](https://gl/mr/14) Theirs".into());
        Standup {
            date: chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            since: "Since 2026-11-23".into(),
            groups,
        }
    }

    #[test]
    fn each_format_writes_its_own_markup() {
        let s = sample();
        let settings = Settings::default();
        let md = render(&lines(&s, &settings), Format::Markdown);
        assert!(md.starts_with("**Stand-up · 2026-11-24**\n\n_Since 2026-11-23_\nMerged:\n"));
        assert!(md.contains("- [!12](https://gl/mr/12) ([#7](https://rm/7)) Fix it\n"));
        let slack = render(&lines(&s, &settings), Format::Slack);
        assert!(slack.starts_with("*Stand-up · 2026-11-24*\n"));
        assert!(slack.contains("• <https://gl/mr/12|!12> (<https://rm/7|#7>) Fix it\n"));
        let plain = render(&lines(&s, &settings), Format::Plain);
        assert!(plain.starts_with("Stand-up · 2026-11-24\n\nSince 2026-11-23\n"));
        assert!(plain.contains("- !12 (#7) Fix it\n"));
        assert!(!plain.contains("http"));
    }

    #[test]
    fn hidden_sections_are_left_out() {
        let s = sample();
        let settings = Settings {
            hidden: Section::Merged.bit() | Section::Waiting.bit(),
            ..Settings::default()
        };
        let text = render(&lines(&s, &settings), Format::Plain);
        assert!(!text.contains("!12"));
        assert!(text.contains("Nothing on GitLab."));
        assert!(text.contains("!13"));
        assert!(!text.contains("!14"));
    }

    #[test]
    fn only_real_links_are_cut_out() {
        assert_eq!(
            pieces("[WIP] a [!1](https://x/1) b"),
            vec![
                ("[", None),
                ("WIP] a ", None),
                ("!1", Some("https://x/1")),
                (" b", None)
            ]
        );
    }
}
