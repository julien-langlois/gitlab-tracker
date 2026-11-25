//! Reviewer suggestions: who owns each changed file (declared or deduced from history) and who knows it.

use super::*;

/// What reading the file gave, for a branch.
#[derive(Debug, Clone, PartialEq)]
pub enum Declared {
    Found(CodeOwners),
    /// None of the three places holds one.
    Absent,
    /// The token may not read the repository's files.
    Denied,
}

// ── The merged MRs ───────────────────────────────────────────────────────────

pub(super) fn time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// The folder a file is in (`""` at the root).
pub(super) fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(f, _)| f)
}

// ── The suggestions ──────────────────────────────────────────────────────────

/// Where the owners come from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The project's `CODEOWNERS`, at this path.
    File(String),
    /// None: deduced from the merged MRs.
    Deduced,
    /// The token may not read the files: deduced, and said why.
    DeducedDenied,
}

/// How a suggested person stands against the owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// In `CODEOWNERS` for some of the files, and knows them.
    Owner,
    /// In `CODEOWNERS` for some of the files, and not near them in the period.
    StaleOwner,
    /// Knows files `CODEOWNERS` gives to others.
    NotDeclared,
    /// Deduced owner of some of the files (no `CODEOWNERS`).
    Deduced,
    /// Knows some of the files, nobody owns them.
    Knows,
}

/// A person who could review.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub login: String,
    pub name: String,
    pub standing: Standing,
    /// The MR's files they changed, reviewed or approved in the period.
    pub known: usize,
    /// The MR's other files whose folder they worked in.
    pub near: usize,
    /// The merged MRs that made them know those files.
    pub mrs: usize,
    pub last: Option<DateTime<Utc>>,
    /// The folders (or files) they own, declared or deduced.
    pub owns: Vec<String>,
    /// Open MRs waiting on their review.
    pub waiting: usize,
}

/// One of the MR's files.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub path: String,
    /// Owners from `CODEOWNERS`, as written (people, groups, roles, emails).
    pub declared: Vec<String>,
    /// Owners deduced (logins), when there is no `CODEOWNERS`.
    pub deduced: Vec<String>,
    /// Who knows it (logins), the MR's author and bots aside.
    pub knowers: Vec<String>,
    /// Who knows its folder but not the file itself.
    pub near: Vec<String>,
    /// Whether the MR's author is the only one who touched it (or its folder).
    pub only_author: bool,
    /// Neither it nor its folder touched by a merged MR of the period.
    pub unknown: bool,
    /// Whether the MR's author touched it or its folder in the period.
    pub author_knows: bool,
}

/// What the MR page shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestions {
    pub source: Source,
    /// Best first.
    pub people: Vec<Candidate>,
    pub files: Vec<FileRow>,
    /// Two people who together know the most files, when one alone falls short.
    pub pair: Option<(String, String, usize)>,
    /// Without `CODEOWNERS`: lines to start one from.
    pub lines: Vec<String>,
    /// The merged MRs read.
    pub merges: usize,
    /// Names by login, of everyone shown.
    pub names: HashMap<String, String>,
    /// The MR's author (login): an owner too, at times.
    pub author: String,
}

/// What the suggestions are worked out from.
pub struct Input<'a> {
    pub files: &'a [String],
    /// Logins.
    pub author: &'a str,
    /// Logins already asked: left out.
    pub reviewers: &'a [String],
    pub history: &'a History,
    pub declared: &'a Declared,
    /// Open MRs waiting on each login's review.
    pub waiting: &'a HashMap<String, usize>,
    /// Names known besides the history's, by login.
    pub names: &'a HashMap<String, String>,
    pub is_bot: &'a dyn Fn(&str) -> bool,
}

/// Who knows a file: by login, the merges (iids) that touched it with them.
pub(super) type Knowers = BTreeMap<String, BTreeSet<u64>>;

/// How far up from a file's folder owners are looked for, when its folder
/// has too few merges.
pub(super) const LEVELS_UP: usize = 2;

/// Whether `path` is in `folder` or under it (the root: only its own files).
pub(super) fn under(path: &str, folder: &str) -> bool {
    if folder.is_empty() {
        self::folder(path).is_empty()
    } else {
        path.strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// The owners of the files in `folder`: those of the folder itself, else of
/// its parent, [`LEVELS_UP`] levels at most and never a top-level folder
/// (`src/` owns nothing in particular). The folder they own, and them.
pub(super) fn deduce_up(
    history: &History,
    folder: &str,
    skip: &dyn Fn(&str) -> bool,
) -> (String, Vec<String>) {
    let mut at = folder;
    for level in 0..=LEVELS_UP {
        let owners = deduce(history, at, skip);
        if !owners.is_empty() {
            return (at.to_string(), owners);
        }
        match at.rsplit_once('/') {
            // The parent, unless it is a top-level folder (or the root).
            Some((parent, _)) if parent.contains('/') && level < LEVELS_UP => at = parent,
            _ => break,
        }
    }
    (folder.to_string(), Vec::new())
}

/// Who did the most in `folder` and under it (author twice, reviewer or
/// approver once): the first two, when they did enough to own it.
pub(super) fn deduce(history: &History, folder: &str, skip: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut weight: HashMap<&str, u32> = HashMap::new();
    let mut merges = 0;
    for merge in history.merges.values() {
        let inside = merge.paths.iter().any(|p| under(p, folder));
        if !inside {
            continue;
        }
        merges += 1;
        *weight.entry(&merge.author).or_default() += 2;
        let reviewers: BTreeSet<&String> = merge.reviewers.iter().chain(&merge.approvers).collect();
        for r in reviewers {
            if *r != merge.author {
                *weight.entry(r).or_default() += 1;
            }
        }
    }
    // Two merges at least: one MR owns nothing.
    if merges < 2 {
        return Vec::new();
    }
    let mut ranked: Vec<(&str, u32)> = weight.into_iter().filter(|(l, _)| !skip(l)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let Some(&(_, top)) = ranked.first() else {
        return Vec::new();
    };
    ranked
        .into_iter()
        .take(2)
        .filter(|(_, w)| *w >= 3 && *w * 2 >= top)
        .map(|(l, _)| l.to_string())
        .collect()
}

/// The suggestions for an MR changing `input.files`.
pub fn suggest(input: &Input) -> Suggestions {
    let skip = |login: &str| login == input.author || (input.is_bot)(login);
    let file = match input.declared {
        Declared::Found(file) => Some(file),
        _ => None,
    };
    let mut knowers: BTreeMap<&str, Knowers> = BTreeMap::new();
    // Who worked in each of the MR's folders.
    let mut folks: BTreeMap<&str, Knowers> = BTreeMap::new();
    let mut touched_by_author: HashSet<&str> = HashSet::new();
    let folders: BTreeSet<&str> = input.files.iter().map(|f| folder(f)).collect();
    for (iid, merge) in &input.history.merges {
        let people: BTreeSet<&String> = std::iter::once(&merge.author)
            .chain(&merge.reviewers)
            .chain(&merge.approvers)
            .collect();
        for path in &merge.paths {
            if let Some(wanted) = input.files.iter().find(|f| *f == path) {
                let entry = knowers.entry(wanted.as_str()).or_default();
                for login in &people {
                    if *login == input.author {
                        touched_by_author.insert(wanted.as_str());
                    }
                    if !skip(login) {
                        entry.entry((*login).clone()).or_default().insert(*iid);
                    }
                }
            }
            if let Some(f) = folders.get(folder(path)) {
                let entry = folks.entry(f).or_default();
                for login in &people {
                    if *login == input.author {
                        touched_by_author.insert(f);
                    }
                    if !skip(login) {
                        entry.entry((*login).clone()).or_default().insert(*iid);
                    }
                }
            }
        }
    }
    // Deduced owners, by folder, when there is no file.
    let mut deduced_by_folder: BTreeMap<&str, (String, Vec<String>)> = BTreeMap::new();
    if file.is_none() {
        for f in input.files {
            let folder = folder(f);
            deduced_by_folder
                .entry(folder)
                .or_insert_with(|| deduce_up(input.history, folder, &skip));
        }
    }
    let files: Vec<FileRow> = input
        .files
        .iter()
        .map(|path| {
            let known: Vec<String> = knowers
                .get(path.as_str())
                .map(|k| k.keys().cloned().collect())
                .unwrap_or_default();
            let near: Vec<String> = folks
                .get(folder(path))
                .map(|k| k.keys().filter(|l| !known.contains(l)).cloned().collect())
                .unwrap_or_default();
            let touched = touched_by_author.contains(path.as_str())
                || touched_by_author.contains(folder(path));
            let alone = known.is_empty() && near.is_empty();
            FileRow {
                path: path.clone(),
                declared: file.map(|f| f.owners_of(path)).unwrap_or_default(),
                deduced: deduced_by_folder
                    .get(folder(path))
                    .map(|(_, owners)| owners.clone())
                    .unwrap_or_default(),
                only_author: alone && touched,
                unknown: alone && !touched,
                author_knows: touched,
                knowers: known,
                near,
            }
        })
        .collect();

    // Everyone who knows a file, or owns one.
    let mut logins: BTreeSet<String> = files
        .iter()
        .flat_map(|f| f.knowers.iter().chain(&f.near).cloned())
        .collect();
    for row in &files {
        logins.extend(row.deduced.iter().cloned());
        logins.extend(
            row.declared
                .iter()
                .filter_map(|o| owner_login(o))
                .filter(|l| !skip(l))
                .map(str::to_string),
        );
    }
    logins.retain(|l| !input.reviewers.contains(l));

    let mut people: Vec<Candidate> = logins
        .into_iter()
        .map(|login| {
            let mut mrs: BTreeSet<u64> = BTreeSet::new();
            let mut known = 0;
            let mut near = 0;
            let mut declared = false;
            let mut deduced = false;
            let mut others_own = false;
            let mut owns: BTreeSet<String> = BTreeSet::new();
            for row in &files {
                let knows = row.knowers.contains(&login);
                let owner = row
                    .declared
                    .iter()
                    .any(|o| owner_login(o) == Some(login.as_str()));
                if row.near.contains(&login) {
                    near += 1;
                    if let Some(set) = folks.get(folder(&row.path)).and_then(|k| k.get(&login)) {
                        mrs.extend(set);
                    }
                    if !owner && !row.declared.is_empty() {
                        others_own = true;
                    }
                }
                if knows {
                    known += 1;
                    if let Some(set) = knowers.get(row.path.as_str()).and_then(|k| k.get(&login)) {
                        mrs.extend(set);
                    }
                    if !owner && !row.declared.is_empty() {
                        others_own = true;
                    }
                }
                if owner {
                    declared = true;
                    owns.insert(row.path.clone());
                }
                if row.deduced.contains(&login) {
                    deduced = true;
                    if let Some((owned, _)) = deduced_by_folder.get(folder(&row.path)) {
                        owns.insert(format!("{owned}/"));
                    }
                }
            }
            let standing = match (file.is_some(), declared, deduced) {
                (true, true, _) if known + near > 0 => Standing::Owner,
                (true, true, _) => Standing::StaleOwner,
                (true, false, _) if others_own => Standing::NotDeclared,
                (false, _, true) => Standing::Deduced,
                _ => Standing::Knows,
            };
            let last = mrs
                .iter()
                .filter_map(|iid| time(&input.history.merges.get(iid)?.merged_at))
                .max();
            let name = input
                .history
                .people
                .get(&login)
                .or_else(|| input.names.get(&login))
                .cloned()
                .unwrap_or_else(|| login.clone());
            Candidate {
                waiting: input.waiting.get(&login).copied().unwrap_or(0),
                name,
                standing,
                known,
                near,
                mrs: mrs.len(),
                last,
                owns: owns.into_iter().collect(),
                login,
            }
        })
        .collect();
    let rank = |s: Standing| match s {
        Standing::Owner | Standing::Deduced => 0,
        Standing::NotDeclared | Standing::Knows => 1,
        Standing::StaleOwner => 2,
    };
    people.sort_by(|a, b| {
        rank(a.standing)
            .cmp(&rank(b.standing))
            .then(b.known.cmp(&a.known))
            .then(b.near.cmp(&a.near))
            .then(b.mrs.cmp(&a.mrs))
            .then(a.waiting.cmp(&b.waiting))
            .then(a.name.cmp(&b.name))
    });

    let pair = pair(&files, &people);
    let lines = if file.is_none() {
        let mut lines: Vec<String> = deduced_by_folder
            .values()
            .filter(|(_, owners)| !owners.is_empty())
            .map(|(folder, owners)| {
                let pattern = if folder.is_empty() {
                    "/*".to_string()
                } else {
                    format!("/{folder}/")
                };
                let owners: Vec<String> = owners.iter().map(|o| format!("@{o}")).collect();
                format!("{pattern} {}", owners.join(" "))
            })
            .collect();
        lines.sort();
        lines.dedup();
        lines
    } else {
        Vec::new()
    };
    let source = match input.declared {
        Declared::Found(f) => Source::File(f.path.clone()),
        Declared::Absent => Source::Deduced,
        Declared::Denied => Source::DeducedDenied,
    };
    let mut names: HashMap<String, String> = input.names.clone();
    names.extend(
        input
            .history
            .people
            .iter()
            .map(|(l, n)| (l.clone(), n.clone())),
    );
    Suggestions {
        source,
        people,
        files,
        pair,
        lines,
        merges: input.history.merges.len(),
        names,
        author: input.author.to_string(),
    }
}

/// How `CODEOWNERS` and the history agree on the MR's files: (files with an
/// owner who knows them, files whose owners did not touch them in the period,
/// files without an owner). An owner knows a file when they changed,
/// reviewed or approved it or worked in its folder, the MR's author
/// included; groups, roles and emails count as knowing (their members are
/// not known here).
pub fn agreement(s: &Suggestions) -> (usize, usize, usize) {
    let (mut known, mut stale, mut none) = (0, 0, 0);
    for f in &s.files {
        let logins: Vec<&str> = f.declared.iter().filter_map(|o| owner_login(o)).collect();
        let knows = |login: &&str| {
            f.knowers.iter().chain(&f.near).any(|k| k == login)
                || (*login == s.author && f.author_knows)
        };
        if f.declared.is_empty() {
            none += 1;
        } else if logins.len() < f.declared.len() || logins.iter().any(knows) {
            known += 1;
        } else {
            stale += 1;
        }
    }
    (known, stale, none)
}

/// The two people who together know the most files, when that is more than
/// the first one knows alone.
pub(super) fn pair(files: &[FileRow], people: &[Candidate]) -> Option<(String, String, usize)> {
    let known = |login: &str| -> HashSet<usize> {
        files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.knowers.iter().any(|k| k == login))
            .map(|(i, _)| i)
            .collect()
    };
    let first = people.iter().max_by(|a, b| {
        a.known
            .cmp(&b.known)
            .then(b.waiting.cmp(&a.waiting))
            .then(b.name.cmp(&a.name))
    })?;
    let mine = known(&first.login);
    let (second, together) = people
        .iter()
        .filter(|p| p.login != first.login)
        .map(|p| (p, mine.union(&known(&p.login)).count()))
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.waiting.cmp(&a.0.waiting)))?;
    (together > mine.len()).then(|| (first.login.clone(), second.login.clone(), together))
}
