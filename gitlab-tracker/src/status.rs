//! The status an MR shows, one per GitLab state and mergeability, each with a
//! color of its own: the TUI badges, the window's badges and its status bar all
//! read them from here.

use crate::models::{GitlabMrState, MergeabilityStatus};

/// An MR's status as its badge shows it. The order is the status bar's: ready,
/// then waiting, then blocked, then the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatusKind {
    Mergeable,
    /// Open, its mergeability not known (yet).
    Open,
    CiRunning,
    Retrying,
    CiMustPass,
    NeedsRebase,
    NotApproved,
    Discussions,
    RequestedChanges,
    Conflict,
    SyncFailed,
    /// Open in the tracker, no longer open in GitLab.
    NotOpen,
    Draft,
    Merged,
    Closed,
}

impl StatusKind {
    #[cfg(test)]
    pub const ALL: [StatusKind; 15] = [
        Self::Mergeable,
        Self::Open,
        Self::CiRunning,
        Self::Retrying,
        Self::CiMustPass,
        Self::NeedsRebase,
        Self::NotApproved,
        Self::Discussions,
        Self::RequestedChanges,
        Self::Conflict,
        Self::SyncFailed,
        Self::NotOpen,
        Self::Draft,
        Self::Merged,
        Self::Closed,
    ];

    pub fn of(state: GitlabMrState, mergeability: &MergeabilityStatus) -> Self {
        match state {
            GitlabMrState::Merged => Self::Merged,
            GitlabMrState::Closed => Self::Closed,
            GitlabMrState::Opened => match mergeability {
                MergeabilityStatus::Mergeable => Self::Mergeable,
                MergeabilityStatus::Unknown => Self::Open,
                MergeabilityStatus::CiStillRunning => Self::CiRunning,
                MergeabilityStatus::Retrying => Self::Retrying,
                MergeabilityStatus::CiMustPass => Self::CiMustPass,
                MergeabilityStatus::NeedsRebase => Self::NeedsRebase,
                MergeabilityStatus::NotApproved => Self::NotApproved,
                MergeabilityStatus::DiscussionsNotResolved => Self::Discussions,
                MergeabilityStatus::RequestedChanges => Self::RequestedChanges,
                MergeabilityStatus::Conflict => Self::Conflict,
                MergeabilityStatus::SyncFailed => Self::SyncFailed,
                MergeabilityStatus::NotOpen => Self::NotOpen,
                MergeabilityStatus::Draft => Self::Draft,
            },
        }
    }

    /// The badge's text, as in the TUI.
    pub fn badge(self) -> &'static str {
        match self {
            Self::Mergeable => "MERGEABLE",
            Self::Open => "OPEN",
            Self::CiRunning => "CI STILL RUNNING",
            Self::Retrying => "RETRYING",
            Self::CiMustPass => "CI MUST PASS",
            Self::NeedsRebase => "REBASE",
            Self::NotApproved => "NOT APPROVED",
            Self::Discussions => "DISCUSSIONS",
            Self::RequestedChanges => "REQUESTED CHANGES",
            Self::Conflict => "CONFLICT",
            Self::SyncFailed => "SYNC FAILED",
            Self::NotOpen => "NOT OPEN",
            Self::Draft => "DRAFT",
            Self::Merged => "MERGED",
            Self::Closed => "CLOSED",
        }
    }

    /// The window's shorter badge text, to fit the column.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn short_badge(self) -> &'static str {
        match self {
            Self::RequestedChanges => "CHANGES REQ.",
            Self::CiRunning => "CI RUNNING",
            other => other.badge(),
        }
    }

    /// Its name in a sentence ("Conflict", "Changes requested").
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn name(self) -> &'static str {
        match self {
            Self::Mergeable => "Mergeable",
            Self::Open => "Open",
            Self::CiRunning => "CI running",
            Self::Retrying => "Retrying",
            Self::CiMustPass => "CI must pass",
            Self::NeedsRebase => "Needs rebase",
            Self::NotApproved => "Not approved",
            Self::Discussions => "Open threads",
            Self::RequestedChanges => "Changes requested",
            Self::Conflict => "Conflict",
            Self::SyncFailed => "Sync failed",
            Self::NotOpen => "Not open",
            Self::Draft => "Draft",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
        }
    }

    /// Its color on a dark background (and the TUI badge's, under black text):
    /// a hue of its own for each status.
    pub fn rgb(self) -> (u8, u8, u8) {
        let hex: u32 = match self {
            Self::Mergeable => 0x6fdc8c,        // green
            Self::Open => 0xb5d86a,             // lime
            Self::CiRunning => 0x5eb8f5,        // sky
            Self::Retrying => 0x4fd1c5,         // teal
            Self::CiMustPass => 0xe6dc5a,       // yellow
            Self::NeedsRebase => 0xf5b45c,      // amber
            Self::NotApproved => 0xe07be8,      // magenta
            Self::Discussions => 0x7f95ff,      // indigo
            Self::RequestedChanges => 0xff9447, // orange
            Self::Conflict => 0xff6b6b,         // red
            Self::SyncFailed => 0xf27fb0,       // pink
            Self::NotOpen => 0xc49a6c,          // brown
            Self::Draft => 0xa8a8a8,            // gray
            Self::Merged => 0xc9b0ff,           // purple
            Self::Closed => 0x7d8b99,           // slate
        };
        ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// Its text color on a light background: the same hue, darkened to read.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub fn rgb_on_light(self) -> (u8, u8, u8) {
        let (r, g, b) = self.rgb();
        let dark = |c: u8| (c as f32 * 0.6) as u8;
        (dark(r), dark(g), dark(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_a_color_of_its_own() {
        let colors: std::collections::HashSet<_> =
            StatusKind::ALL.iter().map(|s| s.rgb()).collect();
        assert_eq!(colors.len(), StatusKind::ALL.len());
    }

    #[test]
    fn the_order_is_the_bars() {
        let mut sorted = StatusKind::ALL;
        sorted.sort();
        assert_eq!(sorted, StatusKind::ALL);
    }

    #[test]
    fn closed_and_merged_mrs_ignore_their_mergeability() {
        assert_eq!(
            StatusKind::of(GitlabMrState::Merged, &MergeabilityStatus::Conflict),
            StatusKind::Merged
        );
        assert_eq!(
            StatusKind::of(GitlabMrState::Opened, &MergeabilityStatus::Unknown),
            StatusKind::Open
        );
    }
}
