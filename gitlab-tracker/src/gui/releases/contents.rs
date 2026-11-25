//! What a release holds: its MRs (by merge commit, SHA or cherry-pick), tickets and direct commits, each with its kind.

use super::*;

/// What a change is, from its title: the sections of the release notes, and
/// what it does to the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Breaking,
    Feature,
    Fix,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Breaking, Kind::Feature, Kind::Fix, Kind::Other];

    /// From a title: its conventional prefix (`feat(api)!: …`), else its
    /// first word (`Add …`, `Fix …`).
    pub fn of(title: &str) -> Kind {
        let title = title.trim();
        if title.to_lowercase().contains("breaking change") {
            return Kind::Breaking;
        }
        if let Some((head, _)) = conventional(title) {
            let head = head.to_lowercase();
            if head.ends_with('!') {
                return Kind::Breaking;
            }
            let word = head.trim_end_matches('!');
            let word = word.split('(').next().unwrap_or_default();
            return match word {
                "feat" | "feature" => Kind::Feature,
                "fix" | "hotfix" | "bugfix" | "revert" => Kind::Fix,
                _ => Kind::Other,
            };
        }
        let first = title
            .split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default()
            .to_lowercase();
        match first.as_str() {
            "add" | "adds" | "added" | "new" | "implement" | "support" | "feat" | "feature" => {
                Kind::Feature
            }
            "fix" | "fixes" | "fixed" | "hotfix" | "bug" | "revert" | "resolve" => Kind::Fix,
            _ => Kind::Other,
        }
    }

    /// Its tag in the lists.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Breaking => tl("breaking"),
            Kind::Feature => tl("feat"),
            Kind::Fix => tl("fix"),
            Kind::Other => tl("other"),
        }
    }

    /// Its section in the release notes.
    pub fn heading(self) -> &'static str {
        match self {
            Kind::Breaking => tl("Breaking changes"),
            Kind::Feature => tl("Features"),
            Kind::Fix => tl("Fixes"),
            Kind::Other => tl("Other changes"),
        }
    }

    pub fn tone(self, theme: &Theme) -> Tone {
        match self {
            Kind::Breaking => theme.tones.red,
            Kind::Feature => theme.tones.green,
            Kind::Fix => theme.tones.amber,
            Kind::Other => theme.tones.gray,
        }
    }
}

/// A conventional title's head and the rest: `feat(api)!: Add x` →
/// (`feat(api)!`, `Add x`). `None` when the title has no such head.
pub(super) fn conventional(title: &str) -> Option<(&str, &str)> {
    let (head, rest) = title.split_once(':')?;
    let word = head.trim_end_matches('!');
    let word = word.split('(').next().unwrap_or_default();
    let ok = !word.is_empty()
        && word.len() <= 12
        && word.chars().all(|c| c.is_ascii_alphabetic())
        && !head.contains(' ');
    ok.then(|| (head, rest.trim()))
}

/// A title for the release notes: without its conventional head, its scope
/// kept in front (`feat(api): add x` → `api: Add x`).
pub fn clean_title(title: &str) -> String {
    let Some((head, rest)) = conventional(title.trim()) else {
        return title.trim().to_string();
    };
    let mut chars = rest.chars();
    let rest = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    let scope = head
        .split_once('(')
        .and_then(|(_, s)| s.split(')').next())
        .filter(|s| !s.is_empty());
    match scope {
        Some(scope) => format!("{scope}: {rest}"),
        None => rest,
    }
}

// ── What a range holds ───────────────────────────────────────────────────────

/// An MR of a range.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub kind: Kind,
    /// Tracked by the app: its page can open.
    pub tracked: bool,
    /// When its commit was made.
    pub at: Option<DateTime<Utc>>,
    /// Its ticket in the tracker, when it names one.
    pub ticket: Option<Ticket>,
}

/// An MR's ticket, as the release notes name it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    pub id: String,
    /// The tracker's kind of ticket (`Feature`, `Bug`), when it gives one.
    pub kind: Option<String>,
    pub url: String,
}

impl Ticket {
    /// `Feature #1234`, `#1234`, `PROJ-42`.
    pub fn label(&self) -> String {
        let reference = if self.id.bytes().all(|b| b.is_ascii_digit()) {
            format!("#{}", self.id)
        } else {
            self.id.clone()
        };
        match self
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            Some(kind) => format!("{kind} {reference}"),
            None => reference,
        }
    }
}

/// The ticket of MR `id` titled `title`: the one the app linked to it, else
/// the one its title names, when a tracker is set.
pub(super) fn ticket_of(app: &App, id: &str, title: &str) -> Option<Ticket> {
    let tracker = app.tracker.as_ref()?;
    let mr = app.mrs.iter().find(|m| m.id == id);
    if let Some(t) = mr.and_then(|m| m.linked_ticket.as_ref()) {
        return Some(Ticket {
            id: t.id.clone(),
            kind: t.tracker_type.clone(),
            url: if t.url.is_empty() {
                tracker.ticket_url(&t.id)
            } else {
                t.url.clone()
            },
        });
    }
    let description = mr.map_or("", |m| m.description.as_str());
    let found = tracker.detect_ticket_id(title, description)?;
    Some(Ticket {
        url: tracker.ticket_url(&found),
        id: found,
        kind: None,
    })
}

/// What a range of commits holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contents {
    /// Oldest first.
    pub mrs: Vec<Item>,
    /// Commits that came through no MR (pushed straight, release commits).
    pub direct: usize,
    /// More commits than were read.
    pub truncated: bool,
}

impl Contents {
    pub fn count(&self, kind: Kind) -> usize {
        self.mrs.iter().filter(|i| i.kind == kind).count()
    }
}

/// The MR a merge commit names: `See merge request group/project!123` → `123`.
pub(super) fn named_mr(message: &str) -> Option<String> {
    let line = message
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("See merge request "))?;
    let (_, number) = line.trim().rsplit_once('!')?;
    let number: String = number.chars().take_while(char::is_ascii_digit).collect();
    (!number.is_empty()).then_some(number)
}

/// The commits a cherry-pick names in its trailers.
pub(super) fn picked_from(message: &str) -> Vec<&str> {
    message
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("(cherry picked from commit ")?
                .strip_suffix(')')
        })
        .collect()
}

/// The MR's title in GitLab's merge commit: the paragraph after
/// `Merge branch 'x' into 'y'`.
pub(super) fn merge_title(message: &str) -> Option<String> {
    let mut lines = message.lines();
    let first = lines.next()?;
    if !first.starts_with("Merge branch ") {
        return Some(first.trim().to_string()).filter(|t| !t.is_empty());
    }
    lines
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("See merge request "))
        .map(str::to_string)
}

/// What `commits` (newest first, as git lists a range) hold: the MRs, by
/// their merge commit, their SHA or a cherry-pick of it; the commits of an
/// MR's branch go with it; the rest are direct commits.
pub fn contents(app: &App, commits: &[ApiCommit], truncated: bool) -> Contents {
    let by_sha: HashMap<&str, &ApiCommit> = commits.iter().map(|c| (c.id.as_str(), c)).collect();
    // The tracked MRs by any of their SHAs.
    let mut tracked: HashMap<&str, &crate::models::TrackedMr> = HashMap::new();
    for mr in app
        .mrs
        .iter()
        .filter(|m| m.state == GitlabMrState::Merged && !m.title.is_empty())
    {
        if let Some(sha) = &mr.sha {
            tracked.insert(sha.as_str(), mr);
        }
        for c in &mr.commits {
            tracked.entry(c.sha.as_str()).or_insert(mr);
        }
    }
    let mut owner: HashMap<&str, String> = HashMap::new();
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Oldest first: an MR shows where it first landed.
    for commit in commits.iter().rev() {
        let id = named_mr(&commit.message)
            .or_else(|| tracked.get(commit.id.as_str()).map(|m| m.id.clone()))
            .or_else(|| {
                picked_from(&commit.message)
                    .into_iter()
                    .find_map(|sha| tracked.get(sha).map(|m| m.id.clone()))
            });
        let Some(id) = id else { continue };
        owner.insert(commit.id.as_str(), id.clone());
        // The branch it merged: its commits are the MR's, not direct ones.
        let mut todo: Vec<&str> = commit
            .parent_ids
            .iter()
            .skip(1)
            .map(String::as_str)
            .collect();
        while let Some(sha) = todo.pop() {
            let Some(c) = by_sha.get(sha) else { continue };
            if owner.contains_key(sha) || named_mr(&c.message).is_some() {
                continue;
            }
            owner.insert(sha, id.clone());
            todo.extend(c.parent_ids.iter().map(String::as_str));
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        let mr = app.mrs.iter().find(|m| m.id == id && !m.title.is_empty());
        let title = mr
            .map(|m| m.title.clone())
            .or_else(|| merge_title(&commit.message))
            .unwrap_or_default();
        items.push(Item {
            ticket: ticket_of(app, &id, &title),
            kind: Kind::of(&title),
            tracked: mr.is_some(),
            at: board::time_of(commit.committed_date.as_deref()),
            id,
            title,
        });
    }
    let direct = commits
        .iter()
        .filter(|c| c.parent_ids.len() <= 1 && !owner.contains_key(c.id.as_str()))
        .count();
    Contents {
        mrs: items,
        direct,
        truncated,
    }
}

// ── The page's model ─────────────────────────────────────────────────────────
