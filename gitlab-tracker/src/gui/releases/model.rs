//! The page's model: release ranges read from git, what shipped, the next version and its bump.

use super::*;

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
pub(super) fn ranges(
    releases: &[ReleaseRecord],
    integration: &str,
) -> (Vec<String>, Option<String>) {
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
pub(super) fn integration_branch(app: &App) -> (String, Vec<String>) {
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

pub(super) fn median(mut values: Vec<f64>) -> Option<f64> {
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
pub(super) fn unlink(text: &str) -> String {
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
pub(super) fn breakdown(c: &Contents) -> String {
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
pub(in crate::gui) fn project_url(app: &App) -> Option<String> {
    app.mrs.iter().find_map(|m| {
        m.web_url
            .split_once("/-/merge_requests/")
            .map(|(base, _)| base.to_string())
    })
}

/// A release's page, else its tag's.
pub(super) fn release_url(app: &App, r: &ReleaseRecord) -> Option<String> {
    if !r.web_url.is_empty() {
        return Some(r.web_url.clone());
    }
    project_url(app).map(|base| format!("{base}/-/tags/{}", urlencoding::encode(&r.tag)))
}

/// The furthest tracked branch an MR is on.
pub(super) fn reached(app: &App, id: &str, ordered: &[String]) -> Option<String> {
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
pub(super) fn read_range(g: &mut GuiApp, ctx: &egui::Context, range: &str, moving: bool) {
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

pub(super) fn model(g: &mut GuiApp, ctx: &egui::Context, now: DateTime<Utc>) -> Model {
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
