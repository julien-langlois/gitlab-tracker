//! Reading a CODEOWNERS file: sections, rules, owners, GitLab's path patterns.

/// A parsed `CODEOWNERS`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodeOwners {
    /// Where it was found (`CODEOWNERS`, `docs/CODEOWNERS`, …).
    pub path: String,
    sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Section {
    /// Lower case (GitLab compares names so); `None`: entries before any header.
    pub(super) name: Option<String>,
    pub(super) rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Rule {
    /// Normalised as GitLab does, ready for [`fnmatch`].
    pub(super) glob: String,
    /// `!pattern`: the file needs no owner from this section.
    pub(super) exclude: bool,
    /// `@user`, `@group/sub`, `@@role` or an email, as written.
    pub(super) owners: Vec<String>,
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
pub(super) fn section_header(line: &str) -> Option<(String, Vec<String>)> {
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
pub(super) fn split_unescaped(line: &str) -> Vec<String> {
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
pub(super) fn normalise(pattern: &str) -> String {
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
pub(super) fn fnmatch(pattern: &[u8], text: &[u8]) -> bool {
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
pub(super) fn class_end(class: &[u8]) -> Option<usize> {
    let start = usize::from(matches!(class.first(), Some(b'!' | b'^')));
    let start = start + usize::from(class.get(start) == Some(&b']'));
    class[start..]
        .iter()
        .position(|&c| c == b']')
        .map(|p| p + start)
}

pub(super) fn in_class(class: &[u8], c: u8) -> bool {
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
