//! The Releases page: what shipped, and what the next release will hold.
//!
//! The releases are the project's GitLab releases, else its tags named like a
//! version ([`ProjectOverview::releases`]). What each one holds is read from
//! git: the commits between it and the one before (`v1.2.0..v1.3.0`), each
//! tied to its MR by its merge commit (`See merge request !N`), its SHA or its
//! `cherry picked from` trailer, the MRs the app does not track too. What
//! waits is the same range from the latest release to the integration branch.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevel_theme::{Theme, Tone};
use chrono::{DateTime, Duration, Local, Utc};
use egui::{Align2, FontId, Ui};

use super::board::{self, Figure, GAP};
use super::details::Fetch;
use super::icons::Icon;
use super::shell::Sizes;
use super::widgets::{button, muted, primary_button, semibold};
use super::GuiApp;
use crate::app::App;
use crate::branch_flow::BranchRole;
use crate::changelog::ApiCommit;
use crate::gitlab_detail::{ProjectOverview, ReleaseRecord};
use crate::i18n::{tf, tl, tn};
use crate::models::GitlabMrState;

/// The releases whose content is read, newest first.
const READ_RELEASES: usize = 8;
/// The window of the cadence, in days.
const CADENCE_DAYS: i64 = 90;
/// Rows of the unreleased list before "N more".
const PENDING_ROWS: usize = 14;
/// Rows of the history.
const HISTORY_ROWS: usize = 12;

/// A range's commits, newest first, and whether there were more.
type RangeCommits = (Vec<ApiCommit>, bool);

/// The page's state: the release picked, the ranges read.
#[derive(Default)]
pub struct State {
    selected: Option<String>,
    /// The project the ranges belong to.
    project: String,
    /// The commits of each range read, and when.
    ranges: HashMap<String, (Instant, Fetch<RangeCommits>)>,
    /// Newer reads of ranges already shown; a failed one is dropped.
    next: HashMap<String, Fetch<RangeCommits>>,
}

// ── What an MR is ───────────────────────────────────────────────────────────

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
fn conventional(title: &str) -> Option<(&str, &str)> {
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
fn ticket_of(app: &App, id: &str, title: &str) -> Option<Ticket> {
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
fn named_mr(message: &str) -> Option<String> {
    let line = message
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("See merge request "))?;
    let (_, number) = line.trim().rsplit_once('!')?;
    let number: String = number.chars().take_while(char::is_ascii_digit).collect();
    (!number.is_empty()).then_some(number)
}

/// The commits a cherry-pick names in its trailers.
fn picked_from(message: &str) -> Vec<&str> {
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
fn merge_title(message: &str) -> Option<String> {
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

/// A range being read, read, or that could not be.
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    Loading,
    Failed(String),
    Ready(Contents),
}

impl Read {
    pub fn ready(&self) -> Option<&Contents> {
        match self {
            Read::Ready(c) => Some(c),
            _ => None,
        }
    }
}

/// A release on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Shipped {
    pub record: ReleaseRecord,
    /// When it was released (else when its commit was made).
    pub at: Option<DateTime<Utc>>,
    /// The environments it was deployed to, newest deployment first.
    pub deployed: Vec<String>,
    /// What it holds; `None` for the oldest read (no release before it) and
    /// those past [`READ_RELEASES`].
    pub contents: Option<Read>,
}

/// How the version moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bump {
    Major,
    Minor,
    Patch,
}

impl Bump {
    pub fn label(self) -> &'static str {
        match self {
            Bump::Major => tl("major"),
            Bump::Minor => tl("minor"),
            Bump::Patch => tl("patch"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The project's releases are known (read).
    pub loaded: bool,
    /// The token cannot read GitLab's releases: tags only.
    pub denied: bool,
    /// The branch the unreleased MRs are merged into.
    pub integration: String,
    /// The tracked branches from feature to production.
    pub ordered: Vec<String>,
    /// Newest first.
    pub releases: Vec<Shipped>,
    /// Merged into the integration branch since the latest release; `None`
    /// without a release.
    pub pending: Option<Read>,
    /// The median days between releases in the last [`CADENCE_DAYS`].
    pub cadence: Option<f64>,
    /// Releases in the last [`CADENCE_DAYS`].
    pub recent: usize,
    /// The version the unreleased MRs call for.
    pub next: Option<(String, Bump)>,
}

/// The ranges the page reads: each of the newest releases from the one
/// before, and the unreleased from the latest to `integration`.
fn ranges(releases: &[ReleaseRecord], integration: &str) -> (Vec<String>, Option<String>) {
    let history = releases
        .windows(2)
        .take(READ_RELEASES)
        .map(|w| format!("{}..{}", w[1].tag, w[0].tag))
        .collect();
    let pending = releases
        .first()
        .filter(|_| !integration.is_empty())
        .map(|r| format!("{}..{integration}", r.tag));
    (history, pending)
}

/// The branch MRs are merged into: the integration one, else the first from
/// feature to production that is not a feature branch.
fn integration_branch(app: &App) -> (String, Vec<String>) {
    let flow = app.branch_flow();
    let ordered: Vec<String> = flow.in_order().iter().map(|b| b.name.clone()).collect();
    let integration = flow
        .in_order()
        .into_iter()
        .find(|b| b.role == BranchRole::Integration)
        .or_else(|| {
            flow.in_order()
                .into_iter()
                .find(|b| b.role != BranchRole::Feature)
        })
        .map(|b| b.name.clone())
        .or_else(|| ordered.first().cloned())
        .unwrap_or_default();
    (integration, ordered)
}

/// The next version after `latest` for `bump`, its form kept (`v`, number
/// of parts); `None` when `latest` does not read as a version.
pub fn next_version(latest: &str, bump: Bump) -> Option<String> {
    if !crate::gitlab_detail::is_version_tag(latest) {
        return None;
    }
    let (prefix, bare) = match latest.strip_prefix(['v', 'V']) {
        Some(rest) => (&latest[..1], rest),
        None => ("", latest),
    };
    let core = bare.split(['-', '+']).next().unwrap_or_default();
    let mut parts: Vec<u64> = core.split('.').filter_map(|p| p.parse().ok()).collect();
    while parts.len() < 3 {
        parts.push(0);
    }
    // Before 1.0, a breaking change moves the minor (semver's habit).
    let bump = match bump {
        Bump::Major if parts[0] == 0 => Bump::Minor,
        b => b,
    };
    match bump {
        Bump::Major => {
            parts[0] += 1;
            parts[1] = 0;
            parts[2] = 0;
        }
        Bump::Minor => {
            parts[1] += 1;
            parts[2] = 0;
        }
        Bump::Patch => parts[2] += 1,
    }
    parts.truncate(3);
    let text: Vec<String> = parts.iter().map(u64::to_string).collect();
    Some(format!("{prefix}{}", text.join(".")))
}

/// The bump `items` call for: a breaking change the major, a feature the
/// minor, the rest the patch.
pub fn bump_of(items: &[Item]) -> Bump {
    if items.iter().any(|i| i.kind == Kind::Breaking) {
        Bump::Major
    } else if items.iter().any(|i| i.kind == Kind::Feature) {
        Bump::Minor
    } else {
        Bump::Patch
    }
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// The page's model, the ranges answered by `read` (`None`: not asked).
pub fn analyze(
    app: &App,
    overview: Option<&ProjectOverview>,
    read: &dyn Fn(&str) -> Option<Read>,
    now: DateTime<Utc>,
) -> Model {
    let (integration, ordered) = integration_branch(app);
    let Some(overview) = overview else {
        return Model {
            loaded: false,
            denied: false,
            integration,
            ordered,
            releases: Vec::new(),
            pending: None,
            cadence: None,
            recent: 0,
            next: None,
        };
    };
    let records = &overview.releases;
    let (history, pending_range) = ranges(records, &integration);
    let releases: Vec<Shipped> = records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut deployed: Vec<String> = Vec::new();
            for d in overview
                .deployments
                .iter()
                .filter(|d| d.status == "success" && (d.sha == r.sha || d.reference == r.tag))
            {
                if !deployed.contains(&d.environment) {
                    deployed.push(d.environment.clone());
                }
            }
            Shipped {
                at: board::time_of(r.released_at.as_deref())
                    .or_else(|| board::time_of(Some(&r.committed_at))),
                deployed,
                contents: history
                    .get(i)
                    .map(|range| read(range).unwrap_or(Read::Loading)),
                record: r.clone(),
            }
        })
        .collect();
    // What a release already holds (a fix picked to production) does not
    // wait for the next one.
    let released: HashSet<&str> = releases
        .iter()
        .filter_map(|s| s.contents.as_ref()?.ready())
        .flat_map(|c| c.mrs.iter().map(|i| i.id.as_str()))
        .collect();
    let pending = pending_range.map(|range| match read(&range).unwrap_or(Read::Loading) {
        Read::Ready(mut c) => {
            c.mrs.retain(|i| !released.contains(i.id.as_str()));
            Read::Ready(c)
        }
        other => other,
    });
    let start = now - Duration::days(CADENCE_DAYS);
    let dates: Vec<DateTime<Utc>> = releases.iter().filter_map(|s| s.at).collect();
    let gaps: Vec<f64> = dates
        .windows(2)
        .filter(|w| w[0] >= start)
        .map(|w| board::days_between(w[1], w[0]).max(0.0))
        .collect();
    let next = match (&pending, records.first()) {
        (Some(Read::Ready(c)), Some(latest)) if !c.mrs.is_empty() => {
            let bump = bump_of(&c.mrs);
            next_version(&latest.tag, bump).map(|v| (v, bump))
        }
        _ => None,
    };
    Model {
        loaded: true,
        denied: overview.releases_denied,
        integration,
        ordered,
        recent: dates.iter().filter(|d| **d >= start).count(),
        cadence: median(gaps),
        releases,
        pending,
        next,
    }
}

/// Release notes of `items`: a section per kind, newest last.
/// With `tickets`, each line starts with the MR's ticket, linked, when it
/// has one: `- [Feature #123](…) - Title (!42)`.
pub fn notes(title: &str, items: &[Item], tickets: bool) -> String {
    let mut out = format!("## {title}\n");
    for kind in Kind::ALL {
        let list: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
        if list.is_empty() {
            continue;
        }
        out.push_str(&format!("\n### {}\n\n", kind.heading()));
        for i in list {
            let line = format!("{} (!{})", clean_title(&i.title), i.id);
            match i.ticket.as_ref().filter(|_| tickets) {
                Some(t) if t.url.is_empty() => {
                    out.push_str(&format!("- {} - {line}\n", t.label()));
                }
                Some(t) => out.push_str(&format!("- [{}]({}) - {line}\n", t.label(), t.url)),
                None => out.push_str(&format!("- {line}\n")),
            }
        }
    }
    out
}

/// `text` with each Markdown link `[label](url)` as its label, for the
/// preview.
fn unlink(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else { break };
        let Some(end) = after[close + 2..].find(')') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&after[..close]);
        rest = &after[close + 2 + end + 1..];
    }
    out.push_str(rest);
    out
}

/// "3 feat · 4 fix · 2 other".
fn breakdown(c: &Contents) -> String {
    Kind::ALL
        .iter()
        .filter_map(|k| {
            let n = c.count(*k);
            (n > 0).then(|| format!("{n} {}", k.label()))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The project's page on GitLab, from a tracked MR's address.
pub(super) fn project_url(app: &App) -> Option<String> {
    app.mrs.iter().find_map(|m| {
        m.web_url
            .split_once("/-/merge_requests/")
            .map(|(base, _)| base.to_string())
    })
}

/// A release's page, else its tag's.
fn release_url(app: &App, r: &ReleaseRecord) -> Option<String> {
    if !r.web_url.is_empty() {
        return Some(r.web_url.clone());
    }
    project_url(app).map(|base| format!("{base}/-/tags/{}", urlencoding::encode(&r.tag)))
}

/// The furthest tracked branch an MR is on.
fn reached(app: &App, id: &str, ordered: &[String]) -> Option<String> {
    let mr = app.mrs.iter().find(|m| m.id == id)?;
    ordered
        .iter()
        .rev()
        .find(|b| app.branch_presence(mr, b).is_some_and(|p| !p.is_missing()))
        .cloned()
}

// ── Reading the ranges ───────────────────────────────────────────────────────

/// Asks for `range` when not read yet (a release's) or older than the
/// refresh interval (the unreleased one, `moving`); what is known of it.
fn read_range(g: &mut GuiApp, ctx: &egui::Context, range: &str, moving: bool) {
    let max_age = std::time::Duration::from_secs(g.app.refresh_interval_secs.max(60));
    let stale = match g.releases.ranges.get(range) {
        None => true,
        Some((at, Fetch::Ready(_) | Fetch::Failed(_))) => moving && at.elapsed() >= max_age,
        Some((_, Fetch::Loading(_))) => false,
    };
    if !stale {
        return;
    }
    let fetch = if g.app.read_only {
        Fetch::Ready(crate::demo::demo_range(&g.app, range))
    } else {
        let target = crate::gitlab_detail::ProjectTarget {
            base_url: g.app.base_url.clone(),
            token: g.app.token.clone(),
            project_id: g.app.project_id.clone(),
        };
        Fetch::spawn(
            ctx,
            crate::changelog::range_commits(target, range.to_string()),
        )
    };
    // A range read before keeps showing while the new read runs.
    match g.releases.ranges.get_mut(range) {
        Some((at, Fetch::Ready(_))) if matches!(fetch, Fetch::Loading(_)) => {
            *at = Instant::now();
            g.releases.next.insert(range.to_string(), fetch);
        }
        _ => {
            g.releases
                .ranges
                .insert(range.to_string(), (Instant::now(), fetch));
        }
    }
}

fn model(g: &mut GuiApp, ctx: &egui::Context, now: DateTime<Utc>) -> Model {
    if g.releases.project != g.app.project_id {
        g.releases = State {
            project: g.app.project_id.clone(),
            ..State::default()
        };
    }
    let overview = g.details.project(&g.app, ctx).cloned();
    if let Some(o) = &overview {
        let (integration, _) = integration_branch(&g.app);
        let (history, pending) = ranges(&o.releases, &integration);
        for range in &history {
            read_range(g, ctx, range, false);
        }
        if let Some(range) = &pending {
            read_range(g, ctx, range, true);
        }
    }
    for (_, fetch) in g.releases.ranges.values_mut() {
        fetch.poll();
    }
    let mut done = Vec::new();
    for (range, fetch) in g.releases.next.iter_mut() {
        fetch.poll();
        if !matches!(fetch, Fetch::Loading(_)) {
            done.push(range.clone());
        }
    }
    for range in done {
        if let Some(Fetch::Ready(value)) = g.releases.next.remove(&range) {
            g.releases
                .ranges
                .insert(range, (Instant::now(), Fetch::Ready(value)));
        }
    }
    let app = &g.app;
    let ranges = &g.releases.ranges;
    let read = |range: &str| -> Option<Read> {
        Some(match &ranges.get(range)?.1 {
            Fetch::Loading(_) => Read::Loading,
            Fetch::Failed(e) => Read::Failed(e.clone()),
            Fetch::Ready((commits, truncated)) => Read::Ready(contents(app, commits, *truncated)),
        })
    };
    analyze(app, overview.as_ref(), &read, now)
}

// ── The page ─────────────────────────────────────────────────────────────────

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let ctx = ui.ctx().clone();
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_RELEASE=v2.3.0`
    // picks that release, for a screenshot.
    #[cfg(debug_assertions)]
    if g.releases.selected.is_none() {
        if let Ok(tag) = std::env::var("GITLAB_TRACKER_GUI_RELEASE") {
            g.releases.selected = Some(tag);
        }
    }
    let m = model(g, &ctx, now);
    board::screen(
        g,
        ui,
        sizes,
        "releases",
        |g, ui| nav(g, ui, &m, now),
        |g, ui| inspector(g, ui, &m, now),
        |g, ui| main(g, ui, &m, now),
    );
}

fn local_date(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

fn nav(g: &mut GuiApp, ui: &mut Ui, m: &Model, now: DateTime<Utc>) {
    let theme = g.theme;
    board::nav_header(
        ui,
        &theme,
        tl("Releases"),
        &tn(
            m.recent as u64,
            "{n} release in 90 days",
            "{n} releases in 90 days",
        ),
    );
    if !m.releases.is_empty() {
        let mut picked = None;
        board::nav_group(ui, &theme, tl("Releases"), |ui| {
            for s in m.releases.iter().take(HISTORY_ROWS) {
                let when =
                    s.at.map(|at| board::ago(at, now))
                        .unwrap_or_else(|| board::dash().to_string());
                let selected = g.releases.selected.as_deref() == Some(s.record.tag.as_str());
                if board::nav_item(ui, &theme, &s.record.tag, &when, None, selected).clicked() {
                    picked = Some(s.record.tag.clone());
                }
            }
        });
        if let Some(tag) = picked {
            g.releases.selected =
                (g.releases.selected.as_deref() != Some(tag.as_str())).then_some(tag);
        }
    }
    board::nav_note(
        ui,
        &theme,
        &tf(
            "What a release holds is read from git: the commits since the release before, each tied to its MR. Unreleased: merged into {branch} since the latest release.",
            &[("branch", &m.integration)],
        ),
    );
    if m.denied {
        board::nav_note(
            ui,
            &theme,
            tl("The token cannot read the GitLab releases: the version tags stand in. Give it Release › Read for the releases' names and notes."),
        );
    }
}

fn main(g: &mut GuiApp, ui: &mut Ui, m: &Model, now: DateTime<Utc>) {
    let theme = g.theme;
    let pending: Option<&Contents> = m.pending.as_ref().and_then(Read::ready);
    let draft_title = m
        .next
        .as_ref()
        .map(|(v, _)| v.clone())
        .unwrap_or_else(|| tl("Unreleased").to_string());
    let draft = pending
        .filter(|c| !c.mrs.is_empty())
        .map(|c| notes(&draft_title, &c.mrs, g.app.config.release_note_tickets));
    let cliff = super::changelog::config(g, ui.ctx());
    let mut copy = false;
    let mut changelog = false;
    board::heading(
        ui,
        &theme,
        tl("Releases"),
        tl("What shipped, and what the next release will hold"),
        |ui| {
            if draft.is_some() {
                copy = primary_button(ui, &theme, tl("Copy the release notes")).clicked();
            }
            if cliff.is_some() {
                changelog = button(
                    ui,
                    &theme,
                    Some(Icon::List),
                    tl("Generate changelog"),
                    false,
                )
                .on_hover_text(tl(
                    "The changelog since the last release tag, with the project's git-cliff config",
                ))
                .clicked();
            }
        },
    );
    if let (true, Some(text)) = (copy, &draft) {
        ui.ctx().copy_text(board::Links::of(&g.app).apply(text));
        g.app.flash(tl("Release notes copied"), false);
    }
    if let (true, Some(config)) = (changelog, cliff) {
        let version = m.next.as_ref().map(|(v, _)| v.as_str()).unwrap_or_default();
        super::changelog::open(g, ui.ctx(), config, version);
    }
    ui.add_space(14.0);
    if !m.loaded {
        board::card(ui, &theme, "", "", 0.0, |ui| {
            board::empty(ui, &theme, tl("Reading the releases…"));
        });
        return;
    }
    if m.releases.is_empty() {
        board::card(ui, &theme, tl("No release yet"), "", 0.0, |ui| {
            board::empty(
                ui,
                &theme,
                tl("No GitLab release, and no tag named like a version (v1.2.0, 3.8.0). The page fills in with the first one."),
            );
        });
        return;
    }
    figures(ui, &theme, m, pending, now);

    let mut open = None;
    let mut show = None;
    let mut picked = None;
    let mut copied = None;
    let app = &g.app;
    board::split(
        ui,
        0.58,
        360.0,
        280.0,
        |ui| unreleased_card(ui, &theme, app, m, now, &mut open, &mut show),
        |ui| copied = notes_card(ui, &theme, m, draft.as_deref()),
    );
    if let Some(text) = copied {
        ui.ctx().copy_text(board::Links::of(&g.app).apply(&text));
        g.app.flash(tl("Release notes copied"), false);
    }
    ui.add_space(GAP);
    history_card(
        ui,
        &theme,
        m,
        g.releases.selected.as_deref(),
        now,
        &mut picked,
    );
    if let Some(tag) = picked {
        g.releases.selected = (g.releases.selected.as_deref() != Some(tag.as_str())).then_some(tag);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
    if let Some(ids) = show {
        g.focus_on_ids(tl("Unreleased"), ids);
    }
}

fn figures(ui: &mut Ui, theme: &Theme, m: &Model, pending: Option<&Contents>, now: DateTime<Utc>) {
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
fn item_row(
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
    response.on_hover_text(format!("{}\n{hint}", item.title))
}

fn unreleased_card(
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
            ui.add(egui::Label::new(muted(theme, &line).size(theme.text_size - 1.5)).truncate())
                .on_hover_text(line);
        });
    });
}

/// The draft notes and their copy button; the text to copy when clicked.
fn notes_card(ui: &mut Ui, theme: &Theme, m: &Model, draft: Option<&str>) -> Option<String> {
    let mut copied = None;
    let note = m.next.as_ref().map(|(v, _)| v.clone()).unwrap_or_default();
    board::card(ui, theme, tl("Draft release notes"), &note, 0.0, |ui| {
        let Some(text) = draft else {
            board::empty(
                ui,
                theme,
                tl("The notes write themselves once an MR is merged."),
            );
            return;
        };
        egui::Frame::new()
            .fill(theme.palette.panel)
            .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
            .corner_radius(egui::CornerRadius::same(theme.radii.medium))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // A preview: the headings as headings, the items as a list.
                ui.spacing_mut().item_spacing.y = 3.0;
                for line in text.lines().filter(|l| !l.trim().is_empty()) {
                    if let Some(title) = line.strip_prefix("### ") {
                        ui.add_space(6.0);
                        ui.label(
                            semibold(title.to_uppercase(), theme.text_size - 2.5)
                                .color(theme.palette.text_muted),
                        );
                    } else if let Some(title) = line.strip_prefix("## ") {
                        ui.label(semibold(title, theme.text_size + 1.0));
                    } else {
                        let item = unlink(line.strip_prefix("- ").unwrap_or(line));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("•  {item}"))
                                    .size(theme.text_size - 1.0)
                                    .color(theme.palette.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                }
            });
        ui.add_space(8.0);
        // Right to left: the button keeps its room, the hint is cut short.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, theme, Some(Icon::Copy), tl("Copy Markdown"), false).clicked() {
                copied = Some(text.to_string());
            }
            ui.add(
                egui::Label::new(
                    muted(theme, tl("Paste it in GitLab when you tag."))
                        .size(theme.text_size - 1.5),
                )
                .truncate(),
            );
        });
    });
    copied
}

fn history_card(
    ui: &mut Ui,
    theme: &Theme,
    m: &Model,
    selected: Option<&str>,
    now: DateTime<Utc>,
    picked: &mut Option<String>,
) {
    let note = match m.cadence {
        Some(c) => tf("one every {time}", &[("time", &board::duration_text(c))]),
        None => String::new(),
    };
    board::card(ui, theme, tl("History"), &note, 0.0, |ui| {
        let font = FontId::proportional(theme.text_size - 1.0);
        let small = FontId::proportional(theme.text_size - 2.0);
        for s in m.releases.iter().take(HISTORY_ROWS) {
            let (rect, response) =
                board::row(ui, theme, 34.0, selected == Some(s.record.tag.as_str()));
            let y = rect.center().y;
            let tone = if s.record.is_release {
                theme.tones.blue
            } else {
                board::id_tone(theme)
            };
            board::paint_tag_within(
                ui,
                theme,
                egui::pos2(rect.left() + 6.0, y),
                &s.record.tag,
                tone,
                true,
                110.0,
            );
            let when =
                s.at.map(|at| format!("{} · {}", local_date(at), board::ago(at, now)))
                    .unwrap_or_else(|| board::dash().to_string());
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 130.0, y),
                Align2::LEFT_CENTER,
                &when,
                font.clone(),
                theme.palette.text_secondary,
                170.0,
            );
            let right = rect.right() - 8.0;
            let envs = if s.deployed.is_empty() {
                board::dash().to_string()
            } else {
                s.deployed.join(", ")
            };
            board::paint_text(
                ui,
                egui::pos2(right, y),
                Align2::RIGHT_CENTER,
                &envs,
                small.clone(),
                if s.deployed.is_empty() {
                    theme.palette.text_muted
                } else {
                    theme.tones.green.fg
                },
                150.0,
            );
            let (count, detail) = match &s.contents {
                Some(Read::Ready(c)) if c.mrs.is_empty() => {
                    (tl("no MR").to_string(), String::new())
                }
                Some(Read::Ready(c)) => (tn(c.mrs.len() as u64, "{n} MR", "{n} MRs"), breakdown(c)),
                Some(Read::Loading) => ("…".to_string(), String::new()),
                Some(Read::Failed(_)) => (board::dash().to_string(), tl("not read").to_string()),
                None => (board::dash().to_string(), String::new()),
            };
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 310.0, y),
                Align2::LEFT_CENTER,
                &count,
                font.clone(),
                theme.palette.text,
                80.0,
            );
            board::paint_text(
                ui,
                egui::pos2(rect.left() + 395.0, y),
                Align2::LEFT_CENTER,
                &detail,
                small.clone(),
                theme.palette.text_muted,
                (right - 160.0 - rect.left() - 395.0).max(0.0),
            );
            if response.clicked() {
                *picked = Some(s.record.tag.clone());
            }
        }
        if m.releases.len() > HISTORY_ROWS {
            board::caption(
                ui,
                theme,
                &tn(
                    (m.releases.len() - HISTORY_ROWS) as u64,
                    "and {n} older",
                    "and {n} older",
                ),
            );
        }
    });
}

fn inspector(g: &mut GuiApp, ui: &mut Ui, m: &Model, now: DateTime<Utc>) {
    let theme = g.theme;
    let release = g
        .releases
        .selected
        .as_deref()
        .and_then(|tag| m.releases.iter().find(|s| s.record.tag == tag));
    let Some(s) = release else {
        board::section(ui, &theme, tl("Releases"), "", |ui| {
            ui.add(
                egui::Label::new(muted(
                    &theme,
                    tl("Pick a release to see what it holds, where it went and its notes."),
                ))
                .wrap(),
            );
        });
        return;
    };
    if board::inspector_title(ui, &theme, &s.record.tag) {
        g.releases.selected = None;
        return;
    }
    let url = release_url(&g.app, &s.record);
    let mut open_url = false;
    let mut copy = None;
    let mut open = None;
    board::section(ui, &theme, "", "", |ui| {
        if let Some(name) = &s.record.name {
            ui.add(egui::Label::new(semibold(name, theme.text_size)).wrap());
            ui.add_space(6.0);
        }
        let mut rows = Vec::new();
        if let Some(at) = s.at {
            rows.push((
                tl("Released"),
                format!("{} · {}", local_date(at), board::ago(at, now)),
            ));
        }
        rows.push((tl("Commit"), s.record.sha.chars().take(8).collect()));
        rows.push((
            tl("Deployed to"),
            if s.deployed.is_empty() {
                board::dash().to_string()
            } else {
                s.deployed.join(", ")
            },
        ));
        rows.push((
            tl("Kind"),
            if s.record.is_release {
                tl("GitLab release").to_string()
            } else {
                tl("Tag").to_string()
            },
        ));
        board::facts(ui, &theme, &rows);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if url.is_some() {
                open_url = button(
                    ui,
                    &theme,
                    Some(Icon::ExternalLink),
                    tl("Open in GitLab"),
                    false,
                )
                .clicked();
            }
            if let Some(Read::Ready(c)) = &s.contents {
                if !c.mrs.is_empty()
                    && button(ui, &theme, Some(Icon::Copy), tl("Copy its notes"), false).clicked()
                {
                    copy = Some(notes(
                        &s.record.tag,
                        &c.mrs,
                        g.app.config.release_note_tickets,
                    ));
                }
            }
        });
    });
    board::section(ui, &theme, tl("What it holds"), "", |ui| {
        match &s.contents {
            Some(Read::Ready(c)) => {
                if c.mrs.is_empty() {
                    board::empty(ui, &theme, tl("No MR: commits only."));
                }
                let mut sorted: Vec<&Item> = c.mrs.iter().collect();
                sorted.sort_by_key(|i| i.kind);
                for item in sorted {
                    if item_row(ui, &theme, item, None).clicked() && item.tracked {
                        open = Some(item.id.clone());
                    }
                }
                if c.direct > 0 {
                    board::caption(
                        ui,
                        &theme,
                        &tn(
                            c.direct as u64,
                            "{n} commit outside an MR",
                            "{n} commits outside an MR",
                        ),
                    );
                }
            }
            Some(Read::Loading) => board::empty(ui, &theme, tl("Reading the commits…")),
            Some(Read::Failed(e)) => board::empty(
                ui,
                &theme,
                &tf("The commits could not be read: {error}", &[("error", e)]),
            ),
            None => board::empty(
                ui,
                &theme,
                tl("The oldest release read: what came before it is not known."),
            ),
        }
    });
    if let Some(text) = &s.record.notes {
        board::section(ui, &theme, tl("Notes in GitLab"), "", |ui| {
            ui.add(egui::Label::new(muted(&theme, text.as_str())).wrap());
        });
    }
    if open_url {
        if let Some(url) = &url {
            board::open_url(ui, url);
        }
    }
    if let Some(text) = copy {
        ui.ctx().copy_text(board::Links::of(&g.app).apply(&text));
        g.app.flash(tl("Release notes copied"), false);
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(id: &str, message: &str, parents: &[&str]) -> ApiCommit {
        ApiCommit {
            id: id.into(),
            message: message.into(),
            parent_ids: parents.iter().map(|p| p.to_string()).collect(),
            committed_date: Some("2026-11-10T10:00:00Z".into()),
            ..ApiCommit::default()
        }
    }

    #[test]
    fn kinds_come_from_the_title() {
        assert_eq!(Kind::of("feat(api): Add x"), Kind::Feature);
        assert_eq!(Kind::of("feat(api)!: Drop v1"), Kind::Breaking);
        assert_eq!(Kind::of("fix: the footer"), Kind::Fix);
        assert_eq!(Kind::of("chore(deps): Bump tokio"), Kind::Other);
        assert_eq!(Kind::of("Add the dark mode"), Kind::Feature);
        assert_eq!(Kind::of("Revert \"feat: x\""), Kind::Fix);
        assert_eq!(Kind::of("Mise en page"), Kind::Other);
        assert_eq!(
            Kind::of("refactor: x\n\nBREAKING CHANGE: y"),
            Kind::Breaking
        );
        // A colon later in a sentence is no conventional head.
        assert_eq!(Kind::of("Fix the docs: typo"), Kind::Fix);
    }

    #[test]
    fn titles_lose_their_head_in_the_notes() {
        assert_eq!(clean_title("feat(api): add x"), "api: Add x");
        assert_eq!(clean_title("fix: the footer"), "The footer");
        assert_eq!(clean_title("Add the dark mode"), "Add the dark mode");
    }

    #[test]
    fn versions_move_by_what_waits() {
        assert_eq!(
            next_version("v2.3.3", Bump::Minor).as_deref(),
            Some("v2.4.0")
        );
        assert_eq!(next_version("3.7.0", Bump::Patch).as_deref(), Some("3.7.1"));
        assert_eq!(
            next_version("v1.9.2", Bump::Major).as_deref(),
            Some("v2.0.0")
        );
        // Before 1.0 a breaking change moves the minor.
        assert_eq!(next_version("0.4.1", Bump::Major).as_deref(), Some("0.5.0"));
        assert_eq!(next_version("v2.0", Bump::Patch).as_deref(), Some("v2.0.1"));
        assert_eq!(
            next_version("v3.0.0-rc.2", Bump::Patch).as_deref(),
            Some("v3.0.1")
        );
        assert_eq!(next_version("deploy-2026", Bump::Patch), None);
    }

    #[test]
    fn merge_commits_name_their_mr_and_take_its_commits() {
        let app = crate::app::tests::test_app();
        // Newest first: a merge of !41 (with its two commits), a commit
        // pushed straight, a merge of !40.
        let commits = vec![
            commit(
                "m41",
                "Merge branch 'pay' into 'main'\n\nfeat(pay): Apple Pay\n\nSee merge request orbit/app!41",
                &["d1", "c2"],
            ),
            commit("c2", "wip", &["c1"]),
            commit("c1", "start", &["m40"]),
            commit("d1", "chore(release): 3.8.0", &["m40"]),
            commit(
                "m40",
                "Merge branch 'x' into 'main'\n\nfix: Crash on start\n\nSee merge request orbit/app!40",
                &["base", "x1"],
            ),
        ];
        let c = contents(&app, &commits, false);
        let ids: Vec<&str> = c.mrs.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["40", "41"]);
        assert_eq!(c.mrs[1].title, "feat(pay): Apple Pay");
        assert_eq!(c.mrs[1].kind, Kind::Feature);
        assert!(!c.mrs[1].tracked);
        // c1 and c2 are !41's; d1 alone came through no MR.
        assert_eq!(c.direct, 1);
    }

    #[test]
    fn notes_have_a_section_per_kind() {
        let item = |id: &str, title: &str| Item {
            id: id.into(),
            title: title.into(),
            kind: Kind::of(title),
            tracked: true,
            at: None,
            ticket: None,
        };
        let out = notes(
            "v2.4.0",
            &[
                item("1", "fix: Crash"),
                item("2", "feat(api): Add x"),
                item("3", "docs: Readme"),
            ],
            true,
        );
        assert_eq!(
            out,
            "## v2.4.0\n\n### Features\n\n- api: Add x (!2)\n\n### Fixes\n\n- Crash (!1)\n\n### Other changes\n\n- Readme (!3)\n"
        );
    }

    #[test]
    fn the_preview_shows_the_labels_of_links() {
        assert_eq!(
            unlink("[Feature #12](https://x/12) - Title (!4)"),
            "Feature #12 - Title (!4)"
        );
        assert_eq!(unlink("no link [here"), "no link [here");
    }

    #[test]
    fn tickets_lead_the_lines_when_asked() {
        let mut item = Item {
            id: "4220".into(),
            title: "feat: New Greece website".into(),
            kind: Kind::Feature,
            tracked: true,
            at: None,
            ticket: Some(Ticket {
                id: "1406385".into(),
                kind: Some("Feature".into()),
                url: "https://redmine.example/issues/1406385".into(),
            }),
        };
        let with = notes("3.5.0", std::slice::from_ref(&item), true);
        assert!(
            with.contains("- [Feature #1406385](https://redmine.example/issues/1406385) - New Greece website (!4220)\n"),
            "{with}"
        );
        let without = notes("3.5.0", std::slice::from_ref(&item), false);
        assert!(
            without.contains("- New Greece website (!4220)\n"),
            "{without}"
        );
        // A Jira-like key keeps its form, with no kind.
        item.ticket = Some(Ticket {
            id: "PROJ-42".into(),
            kind: None,
            url: String::new(),
        });
        let jira = notes("3.5.0", &[item], true);
        assert!(
            jira.contains("- PROJ-42 - New Greece website (!4220)\n"),
            "{jira}"
        );
    }

    #[tokio::test]
    async fn the_demo_shows_its_releases_and_what_waits() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let overview = crate::demo::demo_project_overview(&app);
        assert!(overview.releases.len() >= 10);
        assert_eq!(overview.releases[0].tag, "v2.3.3");
        let read = |range: &str| {
            let (commits, truncated) = crate::demo::demo_range(&app, range);
            Some(Read::Ready(contents(&app, &commits, truncated)))
        };
        let m = analyze(&app, Some(&overview), &read, Utc::now());
        assert_eq!(m.integration, "main");
        // The latest release went to production.
        assert!(m.releases[0].deployed.iter().any(|e| e == "production"));
        let held = m.releases[0]
            .contents
            .as_ref()
            .and_then(Read::ready)
            .unwrap();
        assert!(!held.mrs.is_empty(), "the hotfixes are in v2.3.3");
        let pending = m.pending.as_ref().and_then(Read::ready).unwrap();
        assert!(!pending.mrs.is_empty());
        // A fix picked into the release does not wait for the next one.
        for item in &held.mrs {
            assert!(!pending.mrs.iter().any(|p| p.id == item.id), "!{}", item.id);
        }
        assert!(m.next.is_some());
        assert!(m.cadence.is_some());
    }
}
