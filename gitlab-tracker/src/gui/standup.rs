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

mod gather;
mod output;
mod picker;

pub use gather::*;
use output::*;
pub use picker::*;

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

#[cfg(test)]
mod tests;
