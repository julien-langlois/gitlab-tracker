//! Whether every commit of an MR is signed, GitLab's own rebases forgiven.
//!
//! A rebase done from GitLab's UI rewrites each commit without signing it (or
//! signs it with the instance's key): the author's signature is lost. What
//! survives is the MR's history of versions: the commits before the rebase are
//! still readable, signatures included. A commit that lost its signature is
//! taken as signed when the version before it held a copy of it (same
//! [`CommitFingerprint`], same patch) whose own signature is accepted, and the
//! step between the two versions has the shape of a rebase (the base moved, no
//! commit added).
//!
//! [`classify`] is pure: what it lacks (a signature, a patch-id) it lists in
//! [`Report::missing`], and the commits that need it stay [`Verdict::Pending`].
//! The caller reads those and classifies again, until nothing is missing.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::CommitFingerprint;

/// GitLab's verdict on a signature (`verificationStatus` in GraphQL,
/// `verification_status` in REST).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Verified,
    /// Signed by the GitLab instance itself (commits made in its UI, when the
    /// instance has a signing key).
    VerifiedSystem,
    SameUserDifferentEmail,
    OtherUser,
    UnverifiedKey,
    UnknownKey,
    MultipleSignatures,
    RevokedKey,
    Unverified,
}

impl Status {
    /// From GitLab's value, in either API's case. A value this version does
    /// not know counts as unverified.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "verified" | "verified_ca" => Self::Verified,
            "verified_system" => Self::VerifiedSystem,
            "same_user_different_email" => Self::SameUserDifferentEmail,
            "other_user" => Self::OtherUser,
            "unverified_key" => Self::UnverifiedKey,
            "unknown_key" => Self::UnknownKey,
            "multiple_signatures" => Self::MultipleSignatures,
            "revoked_key" => Self::RevokedKey,
            _ => Self::Unverified,
        }
    }
}

/// A commit's signature as GitLab reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signature {
    Unsigned,
    Signed(Status),
}

impl Signature {
    /// What a rebase from GitLab's UI leaves: no signature, or the instance's.
    pub fn left_by_rebase(self) -> bool {
        matches!(self, Self::Unsigned | Self::Signed(Status::VerifiedSystem))
    }
}

/// A bot whose MRs need no signature: its GitLab account opened the MR, and
/// the commit names it as author. Either alone can be borrowed (any e-mail can
/// be written in a commit), so both must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotAccount {
    /// GitLab username, with or without `@`.
    pub username: String,
    /// The e-mail its commits are authored with.
    pub email: String,
}

impl BotAccount {
    /// The bots of the setting's text: `username email` pairs, separated by
    /// commas (`renovate-bot bot@renovate.example, dependabot dep@example`).
    /// A pair without its e-mail is left out: it would exempt nothing.
    pub fn parse_list(text: &str) -> Vec<Self> {
        text.split([',', ';', '\n'])
            .filter_map(|pair| {
                let mut words = pair.split_whitespace();
                let (username, email) = (words.next()?, words.next()?);
                Some(Self {
                    username: username.trim_start_matches('@').to_string(),
                    email: email.trim_matches(['<', '>']).to_lowercase(),
                })
            })
            .collect()
    }
}

/// The GitLab username in an MR's author as the tracker shows it
/// (`Jane Doe (@jdoe)`), or the text itself when it holds none.
pub fn author_username(author: &str) -> &str {
    author
        .rsplit_once("(@")
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap_or(author)
}

/// What the project requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// A commit that lost its signature in a rebase from GitLab's UI is taken
    /// as signed when the version before held a signed copy of it.
    pub tolerate_gitlab_rebase: bool,
    /// …and only when both have the same patch (see [`patch_id`]): without it,
    /// a commit borrowing the author, date and title of a signed one passes.
    pub require_same_patch: bool,
    /// A signature GitLab cannot verify (unknown key, other user…) is an anomaly.
    pub require_verified: bool,
    /// A commit signed by the GitLab instance itself is accepted.
    pub accept_system_signed: bool,
    /// Bots whose MRs are exempted (empty: none).
    pub exempt_bots: Vec<BotAccount>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            tolerate_gitlab_rebase: true,
            require_same_patch: true,
            require_verified: true,
            accept_system_signed: false,
            exempt_bots: Vec::new(),
        }
    }
}

/// One commit of an MR version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionCommit {
    pub sha: String,
    pub fingerprint: CommitFingerprint,
}

/// One version of an MR (`GET /merge_requests/:iid/versions/:id`): a push, a
/// rebase…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Version {
    pub base_sha: String,
    pub commits: Vec<VersionCommit>,
}

/// What is known so far, by SHA.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Facts {
    pub signatures: HashMap<String, Signature>,
    /// `None`: read, but no patch-id could be made (a diff too large): such a
    /// commit is never matched by its patch.
    pub patch_ids: HashMap<String, Option<String>>,
}

/// What was read from GitLab for one MR, kept with it: the signatures and
/// patch-ids by SHA never change, so they are read once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SignatureData {
    /// The MR's versions, oldest first. Until they are read
    /// (`versions_read`), a single version: the MR's current commits.
    pub versions: Vec<Version>,
    #[serde(default)]
    pub versions_read: bool,
    #[serde(default)]
    pub facts: Facts,
}

impl SignatureData {
    /// Whether the last version holds exactly `shas` (the MR's commits now).
    pub fn is_current<'a>(&self, shas: impl IntoIterator<Item = &'a str>) -> bool {
        let Some(last) = self.versions.last() else {
            return false;
        };
        let now: HashSet<&str> = shas.into_iter().collect();
        last.commits.len() == now.len() && last.commits.iter().all(|c| now.contains(c.sha.as_str()))
    }
}

/// What [`classify`] still needs, by SHA.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Missing {
    pub signatures: BTreeSet<String>,
    pub patch_ids: BTreeSet<String>,
}

impl Missing {
    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty() && self.patch_ids.is_empty()
    }
}

/// What a commit is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Not known yet: see [`Report::missing`].
    Pending,
    Verified,
    /// Signed by the GitLab instance.
    SystemSigned,
    /// Signed, but GitLab cannot verify the signature.
    Unverified(Status),
    Unsigned,
    /// Lost its signature in a rebase from GitLab's UI: `original` is the
    /// signed copy, found in version `version` (1 for the first).
    SignedBeforeRebase {
        original: String,
        version: usize,
    },
    /// From an exempted bot.
    BotExempt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitCheck {
    pub sha: String,
    pub verdict: Verdict,
    /// Whether the project's policy accepts it.
    pub accepted: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// The commits of the last version, in its order.
    pub commits: Vec<CommitCheck>,
    pub missing: Missing,
}

impl Report {
    /// The commits by what they are, for a summary line.
    pub fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for check in &self.commits {
            let sha = check.sha.clone();
            match (&check.verdict, check.accepted) {
                (Verdict::Pending, _) => summary.pending += 1,
                (Verdict::Unsigned, _) => summary.unsigned.push(sha),
                (Verdict::SystemSigned, false) => summary.system.push(sha),
                (Verdict::Unverified(_), false) => summary.unverified.push(sha),
                (Verdict::SignedBeforeRebase { .. }, _) => summary.rebased += 1,
                (Verdict::BotExempt, _) => summary.exempt += 1,
                _ => {}
            }
        }
        summary
    }

    pub fn complete(&self) -> bool {
        self.missing.is_empty()
    }
}

/// A [`Report`] counted: the SHAs of the commits rejected, by why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub unsigned: Vec<String>,
    /// Signed by the instance only, which the policy does not accept.
    pub system: Vec<String>,
    pub unverified: Vec<String>,
    /// Accepted: lost their signature in a rebase from GitLab's UI.
    pub rebased: usize,
    pub exempt: usize,
    pub pending: usize,
}

impl Summary {
    /// How many commits the policy rejects.
    pub fn anomalies(&self) -> usize {
        self.unsigned.len() + self.system.len() + self.unverified.len()
    }
}

/// The commits of the last of `versions` (oldest first), checked against
/// `policy`. `mr_author` is the GitLab username that opened the MR.
pub fn classify(versions: &[Version], mr_author: &str, facts: &Facts, policy: &Policy) -> Report {
    let Some(current) = versions.last() else {
        return Report::default();
    };
    let mut run = Run {
        versions,
        facts,
        policy,
        missing: Missing::default(),
    };
    let last = versions.len() - 1;
    let commits = current
        .commits
        .iter()
        .map(|commit| {
            let (verdict, accepted) = run.commit(last, commit, mr_author);
            CommitCheck {
                sha: commit.sha.clone(),
                verdict,
                accepted,
            }
        })
        .collect();
    Report {
        commits,
        missing: run.missing,
    }
}

/// `mr`'s commits checked against `policy`: `None` when there is no policy
/// (signed commits not enforced), the MR is not open, or nothing was read yet.
pub fn report_for(mr: &crate::models::TrackedMr, policy: Option<&Policy>) -> Option<Report> {
    let policy = policy?;
    if mr.state != crate::models::GitlabMrState::Opened {
        return None;
    }
    let data = mr.signatures.as_ref()?;
    Some(classify(
        &data.versions,
        author_username(&mr.author),
        &data.facts,
        policy,
    ))
}

/// Where a lost signature was traced to.
enum Trace {
    Found { original: String, version: usize },
    Pending,
    NotFound,
}

struct Run<'a> {
    versions: &'a [Version],
    facts: &'a Facts,
    policy: &'a Policy,
    missing: Missing,
}

impl Run<'_> {
    fn commit(&mut self, last: usize, commit: &VersionCommit, mr_author: &str) -> (Verdict, bool) {
        let Some(&signature) = self.facts.signatures.get(&commit.sha) else {
            self.missing.signatures.insert(commit.sha.clone());
            return (Verdict::Pending, false);
        };
        if let Some(verdict) = self.accepted(signature) {
            return (verdict, true);
        }
        if self.policy.tolerate_gitlab_rebase && signature.left_by_rebase() {
            match self.trace(last, commit) {
                Trace::Found { original, version } => {
                    return (Verdict::SignedBeforeRebase { original, version }, true);
                }
                Trace::Pending => return (Verdict::Pending, false),
                Trace::NotFound => {}
            }
        }
        if self.bot_exempt(commit, mr_author) {
            return (Verdict::BotExempt, true);
        }
        let verdict = match signature {
            Signature::Unsigned => Verdict::Unsigned,
            Signature::Signed(Status::VerifiedSystem) => Verdict::SystemSigned,
            Signature::Signed(status) => Verdict::Unverified(status),
        };
        (verdict, false)
    }

    /// The verdict of a signature the policy accepts as it is.
    fn accepted(&self, signature: Signature) -> Option<Verdict> {
        match signature {
            Signature::Signed(Status::Verified) => Some(Verdict::Verified),
            Signature::Signed(Status::VerifiedSystem) => self
                .policy
                .accept_system_signed
                .then_some(Verdict::SystemSigned),
            Signature::Signed(status) => {
                (!self.policy.require_verified).then_some(Verdict::Unverified(status))
            }
            Signature::Unsigned => None,
        }
    }

    /// The signed copy `commit` (in version `at`) was rewritten from.
    fn trace(&mut self, at: usize, commit: &VersionCommit) -> Trace {
        // The version that brought this SHA in, and the one before it.
        let Some(first) = self.versions[..=at]
            .iter()
            .position(|v| v.commits.iter().any(|c| c.sha == commit.sha))
        else {
            return Trace::NotFound;
        };
        if first == 0 {
            return Trace::NotFound;
        }
        let (before, here) = (&self.versions[first - 1], &self.versions[first]);
        // A rebase moves the base and adds no commit (an empty one may drop).
        let had: HashSet<&CommitFingerprint> =
            before.commits.iter().map(|c| &c.fingerprint).collect();
        if before.base_sha == here.base_sha
            || !here.commits.iter().all(|c| had.contains(&c.fingerprint))
        {
            return Trace::NotFound;
        }
        let mut pending = false;
        for copy in before
            .commits
            .iter()
            .filter(|c| c.fingerprint == commit.fingerprint && c.sha != commit.sha)
        {
            match self.trace_copy(first - 1, commit, copy) {
                found @ Trace::Found { .. } => return found,
                Trace::Pending => pending = true,
                Trace::NotFound => {}
            }
        }
        if pending {
            Trace::Pending
        } else {
            Trace::NotFound
        }
    }

    /// Whether `copy`, in version `at`, is a signed original of `commit`.
    fn trace_copy(&mut self, at: usize, commit: &VersionCommit, copy: &VersionCommit) -> Trace {
        let mut pending = false;
        if self.policy.require_same_patch {
            match (
                self.facts.patch_ids.get(&commit.sha),
                self.facts.patch_ids.get(&copy.sha),
            ) {
                (Some(Some(a)), Some(Some(b))) if a != b => return Trace::NotFound,
                (Some(None), _) | (_, Some(None)) => return Trace::NotFound,
                (a, b) => {
                    for (known, sha) in [(a, &commit.sha), (b, &copy.sha)] {
                        if known.is_none() {
                            self.missing.patch_ids.insert(sha.clone());
                            pending = true;
                        }
                    }
                }
            }
        }
        let Some(&signature) = self.facts.signatures.get(&copy.sha) else {
            self.missing.signatures.insert(copy.sha.clone());
            return Trace::Pending;
        };
        let trace = if self.accepted(signature).is_some() {
            Trace::Found {
                original: copy.sha.clone(),
                version: at + 1,
            }
        } else if signature.left_by_rebase() {
            // Rebased more than once: follow it further back.
            self.trace(at, copy)
        } else {
            Trace::NotFound
        };
        match trace {
            Trace::Found { .. } if pending => Trace::Pending,
            other => other,
        }
    }

    fn bot_exempt(&self, commit: &VersionCommit, mr_author: &str) -> bool {
        let author = mr_author.trim().trim_start_matches('@');
        self.policy.exempt_bots.iter().any(|bot| {
            bot.username
                .trim()
                .trim_start_matches('@')
                .eq_ignore_ascii_case(author)
                && bot
                    .email
                    .trim()
                    .eq_ignore_ascii_case(&commit.fingerprint.author_email)
        })
    }
}

/// One file of a commit's diff (`GET /repository/commits/:sha/diff`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct DiffFile {
    pub old_path: String,
    pub new_path: String,
    #[serde(default)]
    pub b_mode: String,
    #[serde(default)]
    pub new_file: bool,
    #[serde(default)]
    pub renamed_file: bool,
    #[serde(default)]
    pub deleted_file: bool,
    #[serde(default)]
    pub diff: String,
    /// GitLab left the diff out: too large to show, or folded.
    #[serde(default)]
    pub too_large: bool,
    #[serde(default)]
    pub collapsed: bool,
}

/// What a commit changes, as a hash that survives a rebase: the files, and the
/// lines added and removed, without the line numbers or the context around
/// them (which a rebase can move or change). Whitespace is kept: it matters in
/// some files. `None` when GitLab left a diff out: two such commits would match
/// on their paths alone.
pub fn patch_id(files: &[DiffFile]) -> Option<String> {
    if files.iter().any(|f| f.too_large || f.collapsed) {
        return None;
    }
    let mut files: Vec<&DiffFile> = files.iter().collect();
    files.sort_by(|a, b| (&a.new_path, &a.old_path).cmp(&(&b.new_path, &b.old_path)));
    let mut hash = Sha256::new();
    for file in files {
        hash.update(format!(
            "file {}\0{}\0{}\0{}{}{}\n",
            file.old_path,
            file.new_path,
            file.b_mode,
            u8::from(file.new_file),
            u8::from(file.renamed_file),
            u8::from(file.deleted_file),
        ));
        for line in file.diff.lines() {
            if (line.starts_with('+') || line.starts_with('-')) && !line.starts_with("@@") {
                hash.update(line);
                hash.update("\n");
            }
        }
    }
    Some(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests;
