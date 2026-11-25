//! What goes in the stand-up: which section each MR belongs to, for every open tab.

use super::*;

/// Where the stand-up's "since" starts: the last working day, at midnight
/// (Friday on a Monday, else yesterday).
pub(super) fn last_working_day(
    now: chrono::DateTime<chrono::Local>,
) -> chrono::DateTime<chrono::Utc> {
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
pub(super) fn done(
    mr: &TrackedMr,
    me: &str,
    since: chrono::DateTime<chrono::Utc>,
) -> Option<Section> {
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
    pub(super) date: chrono::NaiveDate,
    pub(super) since: String,
    pub(super) groups: Vec<(Section, Vec<String>)>,
}

impl Standup {
    pub(super) fn of(&self, section: Section) -> &[String] {
        self.groups
            .iter()
            .find(|(s, _)| *s == section)
            .map_or(&[], |(_, lines)| lines.as_slice())
    }
}

/// The stand-up of every open tab, as `settings` asks, at `now`.
pub(super) fn build(
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
    let links: Vec<Option<super::super::board::Links>> = g
        .tabs
        .iter()
        .map(|slot| match &slot.state {
            _ if !settings.links => None,
            SlotState::Active => Some(super::super::board::Links::of(&g.app)),
            SlotState::Parked(tab) => Some(super::super::board::Links::of(&tab.app)),
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
