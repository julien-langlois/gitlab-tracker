//! Which open MRs touch the same files and lines: pairs, shared files, hot spots.

use super::*;

/// How two MRs meet in a file, from the worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    SameLines,
    Near,
    SameFile,
}

impl Level {
    pub(super) fn label(self) -> &'static str {
        tl(match self {
            Level::SameLines => "Same lines",
            Level::Near => "Close lines",
            Level::SameFile => "Same file",
        })
    }

    pub(super) fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Level::SameLines => t.red,
            Level::Near => t.orange,
            Level::SameFile => t.amber,
        }
    }
}

/// The side panel's views: which list the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Files,
    Pairs,
    Mine,
}

/// The overlaps page's choices.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub view: View,
    pub folder: Option<String>,
    pub lockfiles: bool,
    pub drafts: bool,
    pub pair: Option<(String, String)>,
    /// Hotspots by file rather than by folder.
    pub spots_by_file: bool,
    /// The hotspot unfolded.
    pub spot: Option<String>,
}

/// An open MR the page looks at.
#[derive(Debug, Clone)]
pub struct Mr {
    pub id: String,
    pub title: String,
    pub author: String,
    pub effort: Option<Effort>,
    pub lines: u32,
    pub approved: bool,
    pub mergeable: bool,
    pub files: Vec<ChangedFile>,
}

/// A file changed by several open MRs.
#[derive(Debug, Clone)]
pub struct Shared {
    pub path: String,
    /// The MRs changing it, by id, with their hunks.
    pub mrs: Vec<(String, Vec<(u32, u32)>)>,
    pub level: Level,
    pub added: u32,
    pub deleted: u32,
}

/// Two MRs and the files they share, the worst level first.
#[derive(Debug, Clone)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub files: Vec<(String, Level)>,
}

impl Pair {
    pub(super) fn level(&self) -> Level {
        self.files
            .iter()
            .map(|f| f.1)
            .min()
            .unwrap_or(Level::SameFile)
    }
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Overlaps {
    pub mrs: Vec<Mr>,
    pub files: Vec<Shared>,
    pub pairs: Vec<Pair>,
    /// All the files the open MRs change.
    pub total_files: usize,
    /// Open MRs whose files are not known yet.
    pub unknown: usize,
    /// Where each MR, pair (lower id first) and file is in the lists above: the
    /// page looks them up on every frame, thousands of pairs on a busy project.
    pub(super) mr_at: HashMap<String, usize>,
    pub(super) pair_at: HashMap<(String, String), usize>,
    pub(super) file_at: HashMap<String, usize>,
    /// The matrix's MRs: the ten lowest ids among the pairs.
    pub(super) matrix: Vec<String>,
    /// The pairs with one of my MRs.
    pub(super) my_pairs: usize,
}

pub(in crate::gui) fn is_lock_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    LOCK_FILES.contains(&name)
}

/// How two MRs' hunks in the same file meet.
pub(super) fn meet(a: &ChangedFile, b: &ChangedFile) -> Level {
    // Both add the file, or one deletes it: git stops.
    if (a.new_file && b.new_file) || a.deleted_file || b.deleted_file {
        return Level::SameLines;
    }
    let mut level = Level::SameFile;
    for (a0, a1) in &a.hunks {
        for (b0, b1) in &b.hunks {
            if a0 <= b1 && b0 <= a1 {
                return Level::SameLines;
            }
            let gap = if a1 < b0 {
                b0 - a1
            } else {
                a0.saturating_sub(*b1)
            };
            if gap <= NEAR_LINES {
                level = Level::Near;
            }
        }
    }
    level
}

/// The overlaps between `app`'s open MRs.
pub fn analyze(app: &App, lockfiles: bool, drafts: bool) -> Overlaps {
    let profile = &app.config.complexity_profile;
    let mut out = Overlaps::default();
    for mr in &app.mrs {
        if mr.title.is_empty() || mr.state != GitlabMrState::Opened {
            continue;
        }
        if !drafts && mr.mergeability == MergeabilityStatus::Draft {
            continue;
        }
        let Some(stats) = mr.diff_stats.as_ref().filter(|s| s.files_listed) else {
            out.unknown += 1;
            continue;
        };
        out.mrs.push(Mr {
            id: mr.id.clone(),
            title: mr.title.clone(),
            author: mr.author.clone(),
            effort: Some(Effort::from_score(stats.difficulty(profile))),
            lines: stats.additions + stats.deletions,
            approved: !mr.approved_by.is_empty(),
            mergeable: mr.mergeability == MergeabilityStatus::Mergeable,
            files: stats
                .files
                .iter()
                .filter(|f| lockfiles || !is_lock_file(&f.path))
                .cloned()
                .collect(),
        });
    }
    // By the file's path on the target.
    let mut by_path: BTreeMap<String, Vec<(usize, &ChangedFile)>> = BTreeMap::new();
    for (index, mr) in out.mrs.iter().enumerate() {
        for f in &mr.files {
            let key = f.old_path.clone().unwrap_or_else(|| f.path.clone());
            by_path.entry(key).or_default().push((index, f));
        }
    }
    out.total_files = by_path.len();
    let mut pairs: BTreeMap<(usize, usize), Vec<(String, Level)>> = BTreeMap::new();
    for (path, list) in &by_path {
        if list.len() < 2 {
            continue;
        }
        let mut worst = Level::SameFile;
        for (i, (ia, fa)) in list.iter().enumerate() {
            for (ib, fb) in &list[i + 1..] {
                let level = meet(fa, fb);
                worst = worst.min(level);
                let key = ((*ia).min(*ib), (*ia).max(*ib));
                pairs.entry(key).or_default().push((path.clone(), level));
            }
        }
        out.files.push(Shared {
            path: path.clone(),
            mrs: list
                .iter()
                .map(|(i, f)| (out.mrs[*i].id.clone(), f.hunks.clone()))
                .collect(),
            level: worst,
            added: list.iter().map(|(_, f)| f.added).sum(),
            deleted: list.iter().map(|(_, f)| f.deleted).sum(),
        });
    }
    out.files.sort_by(|a, b| {
        a.level
            .cmp(&b.level)
            .then(b.mrs.len().cmp(&a.mrs.len()))
            .then(a.path.cmp(&b.path))
    });
    out.pairs = pairs
        .into_iter()
        .map(|((a, b), mut files)| {
            files.sort_by(|x, y| x.1.cmp(&y.1).then(x.0.cmp(&y.0)));
            Pair {
                a: out.mrs[a].id.clone(),
                b: out.mrs[b].id.clone(),
                files,
            }
        })
        .collect();
    out.pairs.sort_by(|x, y| {
        x.level()
            .cmp(&y.level())
            .then(y.files.len().cmp(&x.files.len()))
            .then(x.a.cmp(&y.a))
    });
    out.index();
    out.my_pairs = out
        .pairs
        .iter()
        .filter(|p| super::mine(app, &out, p))
        .count();
    out
}

/// The key of the pair of `a` and `b`, in either order.
fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

impl Overlaps {
    /// Fills the lookups from the lists.
    pub(super) fn index(&mut self) {
        self.mr_at = self
            .mrs
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), i))
            .collect();
        self.pair_at = self
            .pairs
            .iter()
            .enumerate()
            .map(|(i, p)| (pair_key(&p.a, &p.b), i))
            .collect();
        self.file_at = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.clone(), i))
            .collect();
        let ids: std::collections::BTreeSet<&str> = self
            .pairs
            .iter()
            .flat_map(|p| [p.a.as_str(), p.b.as_str()])
            .collect();
        let mut ids: Vec<&str> = ids.into_iter().collect();
        ids.sort_by_key(|id| id.parse::<u64>().unwrap_or(u64::MAX));
        self.matrix = ids.into_iter().take(10).map(str::to_string).collect();
    }

    pub(super) fn mr(&self, id: &str) -> Option<&Mr> {
        self.mr_at.get(id).map(|&i| &self.mrs[i])
    }

    pub(super) fn pair(&self, a: &str, b: &str) -> Option<&Pair> {
        self.pair_at.get(&pair_key(a, b)).map(|&i| &self.pairs[i])
    }

    pub(super) fn file(&self, path: &str) -> Option<&Shared> {
        self.file_at.get(path).map(|&i| &self.files[i])
    }
}

/// The files changed by the most tracked MRs created in the last 30 days.
pub(super) fn hot_spots(app: &App, now: DateTime<Utc>) -> Vec<(String, usize)> {
    let since = now - Duration::days(30);
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for mr in &app.mrs {
        let recent = board::time_of(mr.created_at.as_deref())
            .or_else(|| board::time_of(mr.updated_at.as_deref()))
            .is_some_and(|t| t >= since);
        let Some(stats) = mr.diff_stats.as_ref().filter(|_| recent) else {
            continue;
        };
        for f in stats.files.iter().filter(|f| !is_lock_file(&f.path)) {
            *counts.entry(f.path.as_str()).or_default() += 1;
        }
    }
    let mut list: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .map(|(p, n)| (p.to_string(), n))
        .collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    list.truncate(5);
    list
}
