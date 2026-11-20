//! Who owns and who knows the files an MR changes, to suggest its reviewers.
//!
//! Two sources, compared file by file:
//! * **Declared**: the project's `CODEOWNERS` (the first of `CODEOWNERS`,
//!   `docs/CODEOWNERS`, `.gitlab/CODEOWNERS`, read on the MR's target branch,
//!   as GitLab does). Applied whatever GitLab's tier: on Free, GitLab enforces
//!   nothing, but the file still says who answers for what.
//! * **Known**: who authored, reviewed or approved the project's MRs merged in
//!   the last [`HISTORY_DAYS`] days that touched each file ([`History`]).
//!
//! Without a `CODEOWNERS`, owners are deduced from the history (who did most
//! of the work in the file's folder), said so wherever they show, and given as
//! lines to start a `CODEOWNERS` from. With one, each suggested person says
//! how the two agree: an owner who knows the files, an owner who has not been
//! near them in the period (the file may be out of date), or someone who knows
//! them and is not in it.
//!
//! Matching follows GitLab's: a pattern without a leading `/` matches at any
//! depth (`/**/` before it), one ending in `/` everything under it (`**/*`
//! after it), then `fnmatch` with `FNM_PATHNAME | FNM_DOTMATCH`: `*` and `?`
//! stop at `/`, `**/` spans folders, a trailing `/**` is one level. In each
//! section the last matching entry wins; every section applies.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};

/// Where GitLab looks for the file, in its order: the first found is used.
pub const LOCATIONS: [&str; 3] = ["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"];

/// How far back the merged MRs are read.
pub const HISTORY_DAYS: i64 = 90;

// ── CODEOWNERS ───────────────────────────────────────────────────────────────

/// A parsed `CODEOWNERS`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodeOwners {
    /// Where it was found (`CODEOWNERS`, `docs/CODEOWNERS`, …).
    pub path: String,
    sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq)]
struct Section {
    /// Lower case (GitLab compares names so); `None`: entries before any header.
    name: Option<String>,
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
struct Rule {
    /// Normalised as GitLab does, ready for [`fnmatch`].
    glob: String,
    /// `!pattern`: the file needs no owner from this section.
    exclude: bool,
    /// `@user`, `@group/sub`, `@@role` or an email, as written.
    owners: Vec<String>,
}

/// An owner as written in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    /// `@login`: a person (or a group: the file does not tell them apart).
    Handle,
    /// `@group/subgroup`: a group, members unknown here.
    Group,
    /// `@@maintainer`: the project members with that role.
    Role,
    Email,
}

/// What `owner` names.
pub fn owner_kind(owner: &str) -> OwnerKind {
    if owner.starts_with("@@") {
        OwnerKind::Role
    } else if let Some(handle) = owner.strip_prefix('@') {
        if handle.contains('/') {
            OwnerKind::Group
        } else {
            OwnerKind::Handle
        }
    } else {
        OwnerKind::Email
    }
}

/// The login an owner names, when it may be a person.
pub fn owner_login(owner: &str) -> Option<&str> {
    (owner_kind(owner) == OwnerKind::Handle).then(|| &owner[1..])
}

impl CodeOwners {
    /// Reads `text`, found at `path`. Lines it cannot read are skipped, as
    /// GitLab does.
    pub fn parse(path: &str, text: &str) -> Self {
        let mut sections: Vec<Section> = vec![Section {
            name: None,
            rules: Vec::new(),
        }];
        let mut defaults: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((name, owners)) = section_header(line) {
                defaults = owners;
                let name = name.to_lowercase();
                // Sections of the same name are one.
                if !sections.iter().any(|s| s.name.as_deref() == Some(&name)) {
                    sections.push(Section {
                        name: Some(name.clone()),
                        rules: Vec::new(),
                    });
                }
                // Entries go to the section last named: move it last.
                let at = sections
                    .iter()
                    .position(|s| s.name.as_deref() == Some(&name))
                    .unwrap_or(0);
                let section = sections.remove(at);
                sections.push(section);
                continue;
            }
            let mut tokens = split_unescaped(line).into_iter();
            let Some(pattern) = tokens.next() else {
                continue;
            };
            let owners: Vec<String> = tokens
                .take_while(|t| !t.starts_with('#'))
                .filter(|t| t.contains('@'))
                .collect();
            let (exclude, pattern) = match pattern.strip_prefix('!') {
                Some(rest) => (true, rest.to_string()),
                None => (false, pattern),
            };
            let owners = if owners.is_empty() && !exclude {
                defaults.clone()
            } else {
                owners
            };
            if owners.is_empty() && !exclude {
                continue;
            }
            let section = sections.last_mut().expect("the unnamed section is there");
            section.rules.push(Rule {
                glob: normalise(&pattern),
                exclude,
                owners,
            });
        }
        sections.retain(|s| !s.rules.is_empty());
        Self {
            path: path.to_string(),
            sections,
        }
    }

    /// The owners of `file` (a path from the repository's root), each once,
    /// section by section: in each, the last entry matching it, none when an
    /// exclusion matched it first.
    pub fn owners_of(&self, file: &str) -> Vec<String> {
        let path = format!("/{}", file.trim_start_matches('/'));
        let mut out: Vec<String> = Vec::new();
        for section in &self.sections {
            let mut excluded = false;
            let mut last: Option<&Rule> = None;
            for rule in &section.rules {
                if !fnmatch(rule.glob.as_bytes(), path.as_bytes()) {
                    continue;
                }
                if rule.exclude {
                    excluded = true;
                } else if !excluded {
                    last = Some(rule);
                }
            }
            if excluded {
                continue;
            }
            for owner in last.map(|r| r.owners.as_slice()).unwrap_or_default() {
                if !out.contains(owner) {
                    out.push(owner.clone());
                }
            }
        }
        out
    }
}

/// `[Name]`, `^[Name]`, `[Name][2]`, each with default owners after it: the
/// name and the owners.
fn section_header(line: &str) -> Option<(String, Vec<String>)> {
    let rest = line.strip_prefix('^').unwrap_or(line);
    let rest = rest.strip_prefix('[')?;
    let end = rest.find(']')?;
    let name = rest[..end].trim();
    if name.is_empty() {
        return None;
    }
    let mut rest = rest[end + 1..].trim_start();
    // The approvals count, `[2]`.
    if let Some(count) = rest.strip_prefix('[') {
        let close = count.find(']')?;
        if !count[..close].trim().chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        rest = &count[close + 1..];
    }
    let owners = rest
        .split_whitespace()
        .take_while(|t| !t.starts_with('#'))
        .filter(|t| t.contains('@'))
        .map(str::to_string)
        .collect();
    Some((name.to_string(), owners))
}

/// The words of a line, a `\ ` or `\#` kept in its word without its backslash.
fn split_unescaped(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(next @ (' ' | '#' | '\t')) => word.push(next),
                Some(next) => {
                    // Kept for the glob: `\*` stays a literal star.
                    word.push('\\');
                    word.push(next);
                }
                None => word.push('\\'),
            },
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// A pattern as GitLab matches it: anchored with `/`, `/**/` before a pattern
/// that is not, `**/*` after a folder.
fn normalise(pattern: &str) -> String {
    let mut glob = if pattern.starts_with('/') {
        pattern.to_string()
    } else {
        format!("/**/{pattern}")
    };
    if glob.ends_with('/') {
        glob.push_str("**/*");
    }
    glob
}

/// `fnmatch` with `FNM_PATHNAME | FNM_DOTMATCH`: `*`, `?` and classes never
/// match a `/`; `**/` matches any number of folders; `\` quotes.
fn fnmatch(pattern: &[u8], text: &[u8]) -> bool {
    match pattern {
        [] => text.is_empty(),
        [b'*', b'*', b'/', rest @ ..] => {
            fnmatch(rest, text)
                || text
                    .iter()
                    .enumerate()
                    .any(|(i, &c)| c == b'/' && fnmatch(rest, &text[i + 1..]))
        }
        [b'*', ..] => {
            let rest = {
                let mut r = pattern;
                while let [b'*', tail @ ..] = r {
                    r = tail;
                }
                r
            };
            for i in 0..=text.len() {
                if fnmatch(rest, &text[i..]) {
                    return true;
                }
                if i < text.len() && text[i] == b'/' {
                    break;
                }
            }
            false
        }
        [b'?', rest @ ..] => {
            matches!(text.first(), Some(&c) if c != b'/') && fnmatch(rest, &text[1..])
        }
        [b'[', class @ ..] => match (text.first(), class_end(class)) {
            (Some(&c), Some(end)) if c != b'/' => {
                in_class(&class[..end], c) && fnmatch(&class[end + 1..], &text[1..])
            }
            // No closing `]`: a plain `[`.
            (Some(&b'['), None) => fnmatch(class, &text[1..]),
            _ => false,
        },
        [b'\\', c, rest @ ..] => text.first() == Some(c) && fnmatch(rest, &text[1..]),
        [c, rest @ ..] => text.first() == Some(c) && fnmatch(rest, &text[1..]),
    }
}

/// Where a class's `]` is (a `]` first in it is a member).
fn class_end(class: &[u8]) -> Option<usize> {
    let start = usize::from(matches!(class.first(), Some(b'!' | b'^')));
    let start = start + usize::from(class.get(start) == Some(&b']'));
    class[start..]
        .iter()
        .position(|&c| c == b']')
        .map(|p| p + start)
}

fn in_class(class: &[u8], c: u8) -> bool {
    let (negated, class) = match class {
        [b'!' | b'^', rest @ ..] => (true, rest),
        _ => (false, class),
    };
    let mut found = false;
    let mut i = 0;
    while i < class.len() {
        if i + 2 < class.len() && class[i + 1] == b'-' {
            if (class[i]..=class[i + 2]).contains(&c) {
                found = true;
            }
            i += 3;
        } else {
            if class[i] == c {
                found = true;
            }
            i += 1;
        }
    }
    found != negated
}

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

/// One merged MR, as far as ownership goes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Merge {
    /// RFC 3339.
    pub merged_at: String,
    /// Logins.
    pub author: String,
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub approvers: Vec<String>,
    /// The files it changed.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The project's merged MRs of the period, kept on disk between launches and
/// read again from where they stopped.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct History {
    /// By MR iid.
    #[serde(default)]
    pub merges: BTreeMap<u64, Merge>,
    /// Names by login.
    #[serde(default)]
    pub people: BTreeMap<String, String>,
    /// Up to when it is complete (RFC 3339): every MR merged before is in.
    #[serde(default)]
    pub read_at: Option<String>,
}

impl History {
    pub fn read_at(&self) -> Option<DateTime<Utc>> {
        time(self.read_at.as_deref()?)
    }

    /// Merges older than the period dropped.
    pub fn trim(&mut self, now: DateTime<Utc>) {
        let start = now - chrono::Duration::days(HISTORY_DAYS);
        self.merges
            .retain(|_, m| time(&m.merged_at).is_some_and(|t| t >= start));
    }
}

fn time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// The folder a file is in (`""` at the root).
fn folder(path: &str) -> &str {
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
type Knowers = BTreeMap<String, BTreeSet<u64>>;

/// How far up from a file's folder owners are looked for, when its folder
/// has too few merges.
const LEVELS_UP: usize = 2;

/// Whether `path` is in `folder` or under it (the root: only its own files).
fn under(path: &str, folder: &str) -> bool {
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
fn deduce_up(
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
fn deduce(history: &History, folder: &str, skip: &dyn Fn(&str) -> bool) -> Vec<String> {
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
fn pair(files: &[FileRow], people: &[Candidate]) -> Option<(String, String, usize)> {
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

// ── Reading from GitLab ──────────────────────────────────────────────────────

use crate::gitlab::http_client;
use crate::gitlab_detail::ProjectTarget;
use gitlab_tracker_core::pacing::SendPaced;

/// MRs per GraphQL page, until the instance finds it too complex.
const PAGE: usize = 50;
/// GraphQL pages read in one go: a first read of a busy project goes on at
/// the next one.
const PAGES: usize = 10;
/// MRs whose files are read one by one over REST in one go, when GraphQL
/// cannot be used.
const REST_MRS: usize = 40;
/// Read again from this far before the last read: an MR merged while it ran.
const OVERLAP_HOURS: i64 = 24;

fn api_url(target: &ProjectTarget, path: &str) -> String {
    format!(
        "{}/api/v4/projects/{}/{}",
        target.base_url.trim_end_matches('/'),
        urlencoding::encode(&target.project_id),
        path
    )
}

async fn get(target: &ProjectTarget, path: &str) -> Result<reqwest::Response, String> {
    http_client()
        .get(api_url(target, path))
        .header("PRIVATE-TOKEN", target.token.as_str())
        .send_paced()
        .await
        .map_err(|e| e.without_url().to_string())
}

/// The `CODEOWNERS` GitLab would use for an MR into `branch`.
pub async fn read_declared(target: &ProjectTarget, branch: &str) -> Result<Declared, String> {
    for location in LOCATIONS {
        let path = format!(
            "repository/files/{}/raw?ref={}",
            urlencoding::encode(location),
            urlencoding::encode(branch)
        );
        let res = get(target, &path).await?;
        match res.status() {
            s if s.is_success() => {
                let text = res.text().await.map_err(|e| e.to_string())?;
                return Ok(Declared::Found(CodeOwners::parse(location, &text)));
            }
            reqwest::StatusCode::NOT_FOUND => continue,
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::UNAUTHORIZED => {
                return Ok(Declared::Denied)
            }
            s => return Err(format!("GitLab answered {s}")),
        }
    }
    Ok(Declared::Absent)
}

#[derive(serde::Deserialize)]
struct Person {
    #[serde(default)]
    username: String,
    #[serde(default)]
    name: String,
}

/// What one read brought: the merges, the names, and up to when it is complete.
struct Read {
    merges: Vec<(u64, Merge)>,
    people: Vec<Person>,
    complete_to: DateTime<Utc>,
}

/// The project's merged MRs since the last read (the whole period the first
/// time), added to `known`; `known` as it was when GitLab cannot be read. The
/// result is saved at `cache` when it changed; a first read starts from what
/// `cache` holds.
pub async fn read_history(
    target: ProjectTarget,
    known: History,
    cache: Option<crate::storage::ProjectStore>,
) -> History {
    let now = Utc::now();
    let known = match (&known.read_at, &cache) {
        (None, Some(store)) => store
            .load_owners()
            .await
            .map(|mut h| {
                // The database keeps them all: the period's only.
                h.trim(now);
                h
            })
            .unwrap_or(known),
        _ => known,
    };
    let start = now - chrono::Duration::days(HISTORY_DAYS);
    let since = known
        .read_at()
        .map(|at| at - chrono::Duration::hours(OVERLAP_HOURS))
        .filter(|at| *at > start)
        .unwrap_or(start);
    let read = match crate::gitlab_graphql::support(&target.base_url) {
        crate::gitlab_graphql::Support::Batch(_) => match graphql(&target, since, now).await {
            Ok(read) => Ok(read),
            Err(e) => {
                tracing::warn!(error = %e, "Merged MRs over GraphQL failed — REST");
                rest(&target, since, start, now).await
            }
        },
        crate::gitlab_graphql::Support::Off => rest(&target, since, start, now).await,
    };
    let read = match read {
        Ok(read) => read,
        Err(e) => {
            tracing::warn!(error = %e, "Could not read the merged MRs");
            return known;
        }
    };
    let mut history = known.clone();
    history.merges.extend(read.merges);
    for p in read.people.into_iter().filter(|p| !p.username.is_empty()) {
        if !p.name.is_empty() {
            history.people.insert(p.username, p.name);
        }
    }
    history.read_at = Some(
        read.complete_to
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    history.trim(now);
    if let Some(store) = cache.filter(|_| history != known) {
        store.save_owners(&history, &known).await;
    }
    history
}

/// The query of a page of merged MRs, oldest merge first.
fn query(project_id: &str, since: &str, after: Option<&str>, first: usize) -> serde_json::Value {
    let mrs = "mergeRequests(state: merged, mergedAfter: $since, sort: MERGED_AT_ASC, \
               first: $first, after: $after) { pageInfo { hasNextPage endCursor } \
               nodes { iid mergedAt author { username name } \
               reviewers { nodes { username name } } \
               approvedBy { nodes { username name } } diffStats { path } } }";
    let (head, body, project) = match crate::gitlab_graphql::project_ref(project_id) {
        crate::gitlab_graphql::ProjectRef::Path(path) => (
            "$project: ID!",
            format!("project(fullPath: $project) {{ {mrs} }}"),
            serde_json::json!({ "project": path }),
        ),
        crate::gitlab_graphql::ProjectRef::Id(gid) => (
            "$ids: [ID!]",
            format!("projects(ids: $ids) {{ nodes {{ {mrs} }} }}"),
            serde_json::json!({ "ids": [gid] }),
        ),
    };
    let mut variables = serde_json::json!({ "since": since, "after": after, "first": first });
    if let (Some(v), Some(p)) = (variables.as_object_mut(), project.as_object()) {
        v.extend(p.clone());
    }
    serde_json::json!({
        "query": format!("query({head}, $since: Time, $after: String, $first: Int) {{ {body} }}"),
        "variables": variables,
    })
}

/// A page of the answer: its MRs, their people, and the cursor of the next one.
type Page = (Vec<(u64, Merge)>, Vec<Person>, Option<String>);

/// A page of the answer: its MRs, and the cursor of the next one.
fn page_of(body: &serde_json::Value) -> Result<Page, String> {
    if let Some(errors) = body.get("errors").and_then(|e| e.as_array()) {
        if let Some(first) = errors.first() {
            let message = first.get("message").and_then(|m| m.as_str()).unwrap_or("");
            return Err(message.to_string());
        }
    }
    let data = body.get("data").ok_or("no data")?;
    let project = data
        .get("project")
        .or_else(|| data.pointer("/projects/nodes/0"))
        .ok_or("project not found or not visible")?;
    let mrs = project.get("mergeRequests").ok_or("no mergeRequests")?;
    let people_of = |node: &serde_json::Value, key: &str| -> Vec<Person> {
        node.pointer(&format!("/{key}/nodes"))
            .and_then(|n| serde_json::from_value(n.clone()).ok())
            .unwrap_or_default()
    };
    let mut merges = Vec::new();
    let mut people = Vec::new();
    for node in mrs
        .get("nodes")
        .and_then(|n| n.as_array())
        .into_iter()
        .flatten()
    {
        let Some(iid) = node
            .get("iid")
            .and_then(|i| i.as_str())
            .and_then(|i| i.parse().ok())
        else {
            continue;
        };
        let Some(merged_at) = node.get("mergedAt").and_then(|m| m.as_str()) else {
            continue;
        };
        let author: Option<Person> = node
            .get("author")
            .and_then(|a| serde_json::from_value(a.clone()).ok());
        let reviewers = people_of(node, "reviewers");
        let approvers = people_of(node, "approvedBy");
        let paths = node
            .get("diffStats")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|d| d.get("path")?.as_str().map(str::to_string))
            .collect();
        merges.push((
            iid,
            Merge {
                merged_at: merged_at.to_string(),
                author: author
                    .as_ref()
                    .map(|a| a.username.clone())
                    .unwrap_or_default(),
                reviewers: reviewers.iter().map(|p| p.username.clone()).collect(),
                approvers: approvers.iter().map(|p| p.username.clone()).collect(),
                paths,
            },
        ));
        people.extend(author);
        people.extend(reviewers);
        people.extend(approvers);
    }
    let next = mrs
        .pointer("/pageInfo/hasNextPage")
        .and_then(|h| h.as_bool())
        .filter(|h| *h)
        .and_then(|_| {
            mrs.pointer("/pageInfo/endCursor")?
                .as_str()
                .map(str::to_string)
        });
    Ok((merges, people, next))
}

async fn graphql(
    target: &ProjectTarget,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Read, String> {
    let url = format!("{}/api/graphql", target.base_url.trim_end_matches('/'));
    let since_text = since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut read = Read {
        merges: Vec::new(),
        people: Vec::new(),
        complete_to: now,
    };
    let mut after: Option<String> = None;
    let mut size = PAGE;
    let mut pages = 0;
    while pages < PAGES {
        let res = http_client()
            .post(&url)
            .bearer_auth(target.token.as_str())
            .json(&query(
                &target.project_id,
                &since_text,
                after.as_deref(),
                size,
            ))
            .send_paced()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if !res.status().is_success() {
            return Err(format!("HTTP {}", res.status()));
        }
        let body: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        let (merges, people, next) = match page_of(&body) {
            Ok(page) => page,
            Err(message) => {
                let lower = message.to_lowercase();
                let too_big = ["complexity", "depth", "too many", "max_", "maximum"]
                    .iter()
                    .any(|w| lower.contains(w));
                if too_big && size > 5 {
                    size /= 2;
                    continue;
                }
                return Err(message);
            }
        };
        pages += 1;
        read.merges.extend(merges);
        read.people.extend(people);
        match next {
            Some(cursor) => after = Some(cursor),
            None => return Ok(read),
        }
    }
    // More than the pages hold: complete up to the last merge read.
    if let Some(at) = read
        .merges
        .iter()
        .filter_map(|(_, m)| time(&m.merged_at))
        .max()
    {
        read.complete_to = at;
    }
    Ok(read)
}

/// Without GraphQL: the merged MRs over REST, then each one's files
/// ([`REST_MRS`] at most in one go). Approvals are not read this way.
async fn rest(
    target: &ProjectTarget,
    since: DateTime<Utc>,
    start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Read, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        iid: u64,
        merged_at: Option<String>,
        updated_at: Option<String>,
        author: Option<Person>,
        #[serde(default)]
        reviewers: Vec<Person>,
    }
    #[derive(serde::Deserialize)]
    struct Changes {
        #[serde(default)]
        changes: Vec<Change>,
    }
    #[derive(serde::Deserialize)]
    struct Change {
        #[serde(default)]
        new_path: String,
    }
    let since_text = since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let path = format!(
        "merge_requests?state=merged&updated_after={}&order_by=updated_at&sort=asc&per_page=100",
        urlencoding::encode(&since_text)
    );
    let res = get(target, &path).await?;
    if !res.status().is_success() {
        return Err(format!("GitLab answered {}", res.status()));
    }
    let list: Vec<Raw> = res.json().await.map_err(|e| e.to_string())?;
    let full = list.len() == 100;
    let mut read = Read {
        merges: Vec::new(),
        people: Vec::new(),
        complete_to: now,
    };
    let mut fetched = 0;
    let mut last_updated: Option<String> = None;
    for raw in list {
        let Some(merged_at) = raw.merged_at.clone() else {
            continue;
        };
        if time(&merged_at).is_none_or(|t| t < start) {
            continue;
        }
        if fetched == REST_MRS {
            // The rest at the next read.
            if let Some(at) = last_updated.as_deref().and_then(time) {
                read.complete_to = at;
            }
            return Ok(read);
        }
        fetched += 1;
        let res = get(target, &format!("merge_requests/{}/changes", raw.iid)).await?;
        if !res.status().is_success() {
            return Err(format!("GitLab answered {}", res.status()));
        }
        let changes: Changes = res.json().await.map_err(|e| e.to_string())?;
        read.merges.push((
            raw.iid,
            Merge {
                merged_at,
                author: raw
                    .author
                    .as_ref()
                    .map(|a| a.username.clone())
                    .unwrap_or_default(),
                reviewers: raw.reviewers.iter().map(|p| p.username.clone()).collect(),
                approvers: Vec::new(),
                paths: changes.changes.into_iter().map(|c| c.new_path).collect(),
            },
        ));
        read.people.extend(raw.author);
        read.people.extend(raw.reviewers);
        last_updated = raw.updated_at;
    }
    if full {
        if let Some(at) = last_updated.as_deref().and_then(time) {
            read.complete_to = at;
        }
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(text: &str, file: &str) -> Vec<String> {
        CodeOwners::parse("CODEOWNERS", text).owners_of(file)
    }

    #[test]
    fn patterns_match_as_on_gitlab() {
        // No leading slash: at any depth.
        assert_eq!(owners("README.md @a", "docs/x/README.md"), ["@a"]);
        assert_eq!(
            owners("internal/README.md @a", "docs/internal/README.md"),
            ["@a"]
        );
        // Anchored.
        assert!(owners("/README.md @a", "docs/README.md").is_empty());
        assert_eq!(owners("/README.md @a", "README.md"), ["@a"]);
        // A folder: everything under it.
        assert_eq!(owners("/docs/ @a", "docs/api/v4/index.md"), ["@a"]);
        assert_eq!(owners("docs/ @a", "src/docs/x.md"), ["@a"]);
        // `*` stays in its folder, `**/` spans them, a trailing `/**` is one level.
        assert_eq!(owners("/docs/*.md @a", "docs/x.md"), ["@a"]);
        assert!(owners("/docs/*.md @a", "docs/api/x.md").is_empty());
        assert_eq!(owners("/docs/**/index.md @a", "docs/a/b/index.md"), ["@a"]);
        assert_eq!(owners("/docs/**/index.md @a", "docs/index.md"), ["@a"]);
        assert_eq!(owners("/docs/** @a", "docs/x.md"), ["@a"]);
        assert!(owners("/docs/** @a", "docs/api/x.md").is_empty());
        assert_eq!(owners("*.rs @a", "src/a/b.rs"), ["@a"]);
        assert_eq!(owners("* @a", ".gitlab-ci.yml"), ["@a"]);
        assert_eq!(owners("/src/[ab]*.rs @a", "src/beta.rs"), ["@a"]);
        assert!(owners("/src/[!ab]*.rs @a", "src/beta.rs").is_empty());
        // Escaped spaces and hashes.
        assert_eq!(owners("/my\\ file.txt @a", "my file.txt"), ["@a"]);
        assert_eq!(owners("/\\#notes.md @a", "#notes.md"), ["@a"]);
    }

    #[test]
    fn the_last_match_of_each_section_wins_and_every_section_applies() {
        let text = "\
# a comment @nobody
* @admin
/src/ @dev
/src/pricing/ @sarah @nadia   # pricing

[Database] @dba
/migrations/
/src/db/ @alex

^[Docs][2] @docs-team
*.md
";
        assert_eq!(owners(text, "src/pricing/zones.rs"), ["@sarah", "@nadia"]);
        assert_eq!(owners(text, "src/api/x.rs"), ["@dev"]);
        assert_eq!(owners(text, "migrations/0042.sql"), ["@admin", "@dba"]);
        assert_eq!(owners(text, "src/db/x.rs"), ["@dev", "@alex"]);
        assert_eq!(owners(text, "src/README.md"), ["@dev", "@docs-team"]);
    }

    #[test]
    fn exclusions_hold_in_their_section() {
        let text = "\
[Backend] @dev
/src/
!/src/generated/
/src/generated/keep.rs @dev
";
        assert_eq!(owners(text, "src/a.rs"), ["@dev"]);
        // Excluded: it cannot be included again in the same section.
        assert!(owners(text, "src/generated/keep.rs").is_empty());
    }

    #[test]
    fn sections_of_the_same_name_are_one() {
        let text = "[API] @a\n/src/api/\n[Other] @o\n/x/\n[api]\n/src/api/v2/ @b\n";
        assert_eq!(owners(text, "src/api/v2/x.rs"), ["@b"]);
        assert_eq!(owners(text, "src/api/x.rs"), ["@a"]);
    }

    #[test]
    fn owners_are_told_apart() {
        assert_eq!(owner_kind("@sarah"), OwnerKind::Handle);
        assert_eq!(owner_kind("@org/payments"), OwnerKind::Group);
        assert_eq!(owner_kind("@@maintainer"), OwnerKind::Role);
        assert_eq!(owner_kind("jane@example.com"), OwnerKind::Email);
        assert_eq!(owner_login("@sarah"), Some("sarah"));
        assert_eq!(owner_login("@@developer"), None);
    }

    fn merge(at: &str, author: &str, reviewers: &[&str], paths: &[&str]) -> Merge {
        Merge {
            merged_at: format!("2026-11-{at}T10:00:00Z"),
            author: author.into(),
            reviewers: reviewers.iter().map(|s| s.to_string()).collect(),
            approvers: Vec::new(),
            paths: paths.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn history() -> History {
        let mut h = History::default();
        h.merges.insert(
            1,
            merge(
                "01",
                "sarah",
                &["nadia"],
                &["src/pricing/zones.rs", "src/pricing/quote.rs"],
            ),
        );
        h.merges.insert(
            2,
            merge("05", "sarah", &["alex"], &["src/pricing/zones.rs"]),
        );
        h.merges.insert(
            3,
            merge(
                "08",
                "nadia",
                &["sarah"],
                &["src/db/scans.rs", "src/pricing/quote.rs"],
            ),
        );
        h.merges.insert(
            4,
            merge("10", "alex", &["renovate-bot"], &["src/db/scans.rs"]),
        );
        h.merges
            .insert(5, merge("12", "marina", &[], &["src/booking/hold.rs"]));
        h.people.insert("sarah".into(), "Sarah Codewyn".into());
        h
    }

    fn run(declared: &Declared, files: &[&str], author: &str) -> Suggestions {
        let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        let history = history();
        let waiting = HashMap::from([("nadia".to_string(), 4)]);
        suggest(&Input {
            files: &files,
            author,
            reviewers: &[],
            history: &history,
            declared,
            waiting: &waiting,
            names: &HashMap::new(),
            is_bot: &|l: &str| l.ends_with("-bot"),
        })
    }

    #[test]
    fn without_a_file_owners_are_deduced_by_folder() {
        let s = run(
            &Declared::Absent,
            &[
                "src/pricing/zones.rs",
                "src/pricing/quote.rs",
                "src/db/scans.rs",
                "src/booking/hold.rs",
                "src/new.rs",
            ],
            "marina",
        );
        assert_eq!(s.source, Source::Deduced);
        let sarah = s.people.iter().find(|p| p.login == "sarah").unwrap();
        assert_eq!(sarah.standing, Standing::Deduced);
        assert_eq!(sarah.name, "Sarah Codewyn");
        assert_eq!(sarah.known, 3);
        assert_eq!(s.people[0].login, "sarah");
        // Bots never suggested.
        assert!(s.people.iter().all(|p| p.login != "renovate-bot"));
        assert!(
            s.lines.contains(&"/src/pricing/ @sarah @nadia".to_string()),
            "{:?}",
            s.lines
        );
        let hold = s
            .files
            .iter()
            .find(|f| f.path == "src/booking/hold.rs")
            .unwrap();
        assert!(hold.only_author && !hold.unknown);
        let new = s.files.iter().find(|f| f.path == "src/new.rs").unwrap();
        assert!(new.unknown);
        let nadia = s.people.iter().find(|p| p.login == "nadia").unwrap();
        assert_eq!(nadia.waiting, 4);
    }

    #[test]
    fn with_a_file_each_person_says_how_it_agrees() {
        let file = CodeOwners::parse(
            ".gitlab/CODEOWNERS",
            "/src/pricing/ @sarah\n/src/db/ @dba-lead\n",
        );
        let s = run(
            &Declared::Found(file),
            &["src/pricing/zones.rs", "src/db/scans.rs"],
            "marina",
        );
        assert_eq!(s.source, Source::File(".gitlab/CODEOWNERS".into()));
        let standing = |login: &str| s.people.iter().find(|p| p.login == login).unwrap().standing;
        assert_eq!(standing("sarah"), Standing::Owner);
        assert_eq!(standing("dba-lead"), Standing::StaleOwner);
        assert_eq!(standing("alex"), Standing::NotDeclared);
        assert!(s.lines.is_empty());
        // The stale owner goes last.
        assert_eq!(s.people.last().unwrap().login, "dba-lead");
    }

    #[test]
    fn a_pair_is_offered_when_one_alone_falls_short() {
        let mut history = History::default();
        history
            .merges
            .insert(1, merge("01", "ann", &[], &["a.rs", "b.rs"]));
        history.merges.insert(2, merge("02", "bob", &[], &["c.rs"]));
        let files: Vec<String> = ["a.rs", "b.rs", "c.rs"].map(String::from).to_vec();
        let input = |files: &[String]| {
            suggest(&Input {
                files,
                author: "me",
                reviewers: &[],
                history: &history,
                declared: &Declared::Absent,
                waiting: &HashMap::new(),
                names: &HashMap::new(),
                is_bot: &|_| false,
            })
        };
        assert_eq!(input(&files).pair, Some(("ann".into(), "bob".into(), 3)));
        // One knows them all: no pair.
        assert_eq!(input(&files[..2]).pair, None);
    }

    #[test]
    fn the_author_and_current_reviewers_are_left_out() {
        let files = vec!["src/pricing/zones.rs".to_string()];
        let history = history();
        let s = suggest(&Input {
            files: &files,
            author: "sarah",
            reviewers: &["nadia".to_string()],
            history: &history,
            declared: &Declared::Absent,
            waiting: &HashMap::new(),
            names: &HashMap::new(),
            is_bot: &|_| false,
        });
        let logins: Vec<&str> = s.people.iter().map(|p| p.login.as_str()).collect();
        assert_eq!(logins, ["alex"]);
    }

    #[test]
    fn people_who_worked_in_the_folder_count_after_those_who_know_the_file() {
        let mut history = History::default();
        history
            .merges
            .insert(1, merge("01", "ann", &[], &["app/x/One.kt"]));
        history
            .merges
            .insert(2, merge("02", "bob", &[], &["app/x/Two.kt"]));
        let files = vec!["app/x/Two.kt".to_string(), "app/x/New.kt".to_string()];
        let s = suggest(&Input {
            files: &files,
            author: "me",
            reviewers: &[],
            history: &history,
            declared: &Declared::Absent,
            waiting: &HashMap::new(),
            names: &HashMap::new(),
            is_bot: &|_| false,
        });
        let people: Vec<(&str, usize, usize)> = s
            .people
            .iter()
            .map(|p| (p.login.as_str(), p.known, p.near))
            .collect();
        assert_eq!(people, [("bob", 1, 1), ("ann", 0, 2)]);
        // A new file in a known folder is not "known by nobody".
        assert!(s.files.iter().all(|f| !f.unknown && !f.only_author));
    }

    #[test]
    fn owners_are_looked_for_higher_up_but_never_a_top_folder() {
        let mut history = History::default();
        for (i, path) in ["app/src/a/A.kt", "app/src/b/B.kt", "app/src/c/C.kt"]
            .iter()
            .enumerate()
        {
            history
                .merges
                .insert(i as u64, merge("01", "lea", &["omar"], &[path]));
        }
        let skip = |_: &str| false;
        // One merge in `app/src/a`: its parent `app/src` holds three.
        assert_eq!(
            deduce_up(&history, "app/src/a", &skip),
            (
                "app/src".to_string(),
                vec!["lea".to_string(), "omar".to_string()]
            )
        );
        // `app` is a top-level folder: never owned.
        let mut thin = History::default();
        thin.merges
            .insert(1, merge("01", "lea", &[], &["app/x/A.kt"]));
        thin.merges
            .insert(2, merge("02", "lea", &[], &["app/y/B.kt"]));
        assert_eq!(deduce_up(&thin, "app/x", &skip).1, Vec::<String>::new());
    }

    #[test]
    fn an_author_who_owns_the_files_is_no_stale_owner() {
        let file = CodeOwners::parse(
            "CODEOWNERS",
            "/src/pricing/ @sarah\n/src/db/ @dba-lead\n/x/ @org/team\n",
        );
        let s = run(
            &Declared::Found(file),
            &[
                "src/pricing/zones.rs",
                "src/db/scans.rs",
                "x/a.rs",
                "y/b.rs",
            ],
            "sarah",
        );
        // Sarah wrote the pricing MRs; nobody named dba-lead was ever seen; a
        // group counts as knowing; y/ has no owner.
        assert_eq!(agreement(&s), (2, 1, 1));
    }

    #[test]
    fn the_history_keeps_the_period() {
        let mut h = history();
        h.merges.insert(9, merge("01", "x", &[], &[]));
        h.merges.get_mut(&9).unwrap().merged_at = "2026-01-01T00:00:00Z".into();
        h.trim(time("2026-11-20T00:00:00Z").unwrap());
        assert!(!h.merges.contains_key(&9));
        assert!(h.merges.contains_key(&1));
    }
}
