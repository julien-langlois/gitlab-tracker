//! What a demo project is written as: its people, its branches and their
//! promotions, and each MR as the list of what happened to it (opened, pushed,
//! reviewed, merged…), the way GitLab records it. Everything the app shows is
//! then worked out from these by [`super::engine`], so that the pages agree.

use super::clock::When;
use crate::models::MergeabilityStatus;

/// Someone of the team.
#[derive(Debug, PartialEq, Eq)]
pub struct Person {
    pub name: &'static str,
    pub login: &'static str,
}

impl Person {
    /// As GitLab names people in the app: `Name (@login)`.
    pub fn display(&self) -> String {
        format!("{} (@{})", self.name, self.login)
    }

    pub fn email(&self) -> String {
        format!("{}@orbit-logistics.dev", self.login.replace('_', "."))
    }
}

/// What the pipeline of a push does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ci {
    Pass,
    /// These jobs fail; the stages after theirs are skipped.
    Fail(&'static [&'static str]),
    /// This job fails, and the same commit passes when run again.
    Flaky(&'static str),
    /// Still running now.
    Running,
    /// Waiting for a runner.
    Pending,
    /// Cancelled once the first stage passed (a newer push came, or by hand).
    Canceled,
}

/// One thing that happened to an MR. The author pushes; the others review.
#[derive(Debug, Clone, Copy)]
pub enum Ev {
    /// Opened by its author.
    Open,
    /// Commits pushed by the author, and the pipeline they started.
    Push(&'static [&'static str], Ci),
    /// The source branch rebased on its target (a force push).
    Rebase(Ci),
    /// A review asked of someone.
    Ask(&'static Person),
    /// A new thread: who, its key in this MR, the file and line (or none), the note.
    Note(
        &'static Person,
        &'static str,
        Option<(&'static str, u32)>,
        &'static str,
    ),
    /// An answer in a thread.
    Reply(&'static Person, &'static str, &'static str),
    Resolve(&'static Person, &'static str),
    Approve(&'static Person),
    /// A review submitted as "Request changes".
    Changes(&'static Person),
    Merge(&'static Person),
    Close(&'static Person),
}

/// How the last refresh of an MR went.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetch {
    Ok,
    /// GitLab answered with this error: the MR keeps what was read before.
    Failed(&'static str),
    /// Just added: the first read is not back yet.
    Loading,
}

/// A file an MR changes, with its patch.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub new_file: bool,
    pub deleted_file: bool,
    /// The unified patch, hunk headers included.
    pub diff: String,
}

/// A file modified by `hunks`: (first line of the hunk in the old file, its
/// lines, each starting with ' ', '+' or '-'; an empty line is an empty
/// context line).
pub fn patch(path: &str, hunks: &[(u32, &str)]) -> FileChange {
    let mut diff = String::new();
    let mut shift: i64 = 0;
    for (start, body) in hunks {
        let lines: Vec<&str> = body.trim_matches('\n').lines().collect();
        let old = lines.iter().filter(|l| !l.starts_with('+')).count() as i64;
        let new = lines.iter().filter(|l| !l.starts_with('-')).count() as i64;
        let new_start = i64::from(*start) + shift;
        diff.push_str(&format!("@@ -{start},{old} +{new_start},{new} @@\n"));
        for line in &lines {
            if line.is_empty() {
                diff.push_str(" \n");
            } else {
                diff.push_str(line);
                diff.push('\n');
            }
        }
        shift += new - old;
    }
    FileChange {
        path: path.into(),
        old_path: None,
        new_file: false,
        deleted_file: false,
        diff: diff.trim_end().to_string(),
    }
}

/// A new file with these lines.
pub fn added(path: &str, body: &str) -> FileChange {
    let lines: Vec<&str> = body.trim_matches('\n').lines().collect();
    let mut diff = format!("@@ -0,0 +1,{} @@\n", lines.len());
    for line in &lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    FileChange {
        path: path.into(),
        old_path: None,
        new_file: true,
        deleted_file: false,
        diff: diff.trim_end().to_string(),
    }
}

/// A file GitLab does not show a patch of (binary, or too large).
pub fn opaque(path: &str, new_file: bool) -> FileChange {
    FileChange {
        path: path.into(),
        old_path: None,
        new_file,
        deleted_file: false,
        diff: String::new(),
    }
}

/// A `Cargo.lock` patch bumping these crates: (name, from, to).
pub fn cargo_lock(start: u32, bumps: &[(&str, &str, &str)]) -> FileChange {
    let mut body = String::new();
    let mut line = start;
    let mut hunks = Vec::new();
    for (name, from, to) in bumps {
        let old_sum = super::clock::sha(&[name, from, "crate"]);
        let new_sum = super::clock::sha(&[name, to, "crate"]);
        body.clear();
        body.push_str(" [[package]]\n");
        body.push_str(&format!(" name = \"{name}\"\n"));
        body.push_str(&format!("-version = \"{from}\"\n+version = \"{to}\"\n"));
        body.push_str(" source = \"registry+https://github.com/rust-lang/crates.io-index\"\n");
        body.push_str(&format!(
            "-checksum = \"{old_sum}{}\"\n+checksum = \"{new_sum}{}\"\n",
            &old_sum[..24],
            &new_sum[..24]
        ));
        hunks.push((line, body.clone()));
        line += 31 + (super::clock::hash(&[name]) % 140) as u32;
    }
    let refs: Vec<(u32, &str)> = hunks.iter().map(|(l, b)| (*l, b.as_str())).collect();
    patch("Cargo.lock", &refs)
}

/// One MR as it happened.
#[derive(Debug, Clone)]
pub struct MrSpec {
    /// How the rest of the demo names it.
    pub key: &'static str,
    pub title: &'static str,
    pub author: &'static Person,
    pub assignee: Option<&'static Person>,
    pub source: &'static str,
    pub target: &'static str,
    pub milestone: Option<&'static str>,
    pub labels: Vec<&'static str>,
    pub ticket: Option<u32>,
    pub description: String,
    pub files: Vec<FileChange>,
    pub log: Vec<(When, Ev)>,
    /// GitLab's verdict on an open MR (the others are `NotOpen`).
    pub mergeability: MergeabilityStatus,
    pub flagged: bool,
    pub fetch: Fetch,
    /// What a job of its pipelines prints when it fails (job, output).
    pub failure: Option<(&'static str, &'static str)>,
}

impl MrSpec {
    pub fn new(key: &'static str, title: &'static str, author: &'static Person) -> Self {
        Self {
            key,
            title,
            author,
            assignee: None,
            source: key,
            target: "main",
            milestone: None,
            labels: Vec::new(),
            ticket: None,
            description: String::new(),
            files: Vec::new(),
            log: Vec::new(),
            mergeability: MergeabilityStatus::NotApproved,
            flagged: false,
            fetch: Fetch::Ok,
            failure: None,
        }
    }

    pub fn branch(mut self, source: &'static str, target: &'static str) -> Self {
        self.source = source;
        self.target = target;
        self
    }

    pub fn assignee(mut self, person: &'static Person) -> Self {
        self.assignee = Some(person);
        self
    }

    pub fn milestone(mut self, title: &'static str) -> Self {
        self.milestone = Some(title);
        self
    }

    pub fn labels(mut self, labels: &[&'static str]) -> Self {
        self.labels = labels.to_vec();
        self
    }

    pub fn ticket(mut self, id: u32) -> Self {
        self.ticket = Some(id);
        self
    }

    pub fn about(mut self, text: &str) -> Self {
        self.description = text.trim().to_string();
        self
    }

    pub fn file(mut self, file: FileChange) -> Self {
        self.files.push(file);
        self
    }

    pub fn at(mut self, when: When, ev: Ev) -> Self {
        self.log.push((when, ev));
        self
    }

    pub fn status(mut self, mergeability: MergeabilityStatus) -> Self {
        self.mergeability = mergeability;
        self
    }

    pub fn flagged(mut self) -> Self {
        self.flagged = true;
        self
    }

    pub fn failure(mut self, job: &'static str, output: &'static str) -> Self {
        self.failure = Some((job, output));
        self
    }

    pub fn fetch(mut self, fetch: Fetch) -> Self {
        self.fetch = fetch;
        self
    }

    /// The usual life of a merged MR: opened and pushed, reviews asked, one
    /// thread from the first reviewer (`review`: the remark, the answer),
    /// answered with a fix push when `fix` names one, approved by every
    /// reviewer, merged by the first.
    #[allow(clippy::too_many_arguments)]
    pub fn shipped(
        self,
        reviewers: &[&'static Person],
        opened: When,
        reviewed: When,
        approved: When,
        merged: When,
        review: (&'static str, &'static str),
        fix: Option<&'static str>,
    ) -> Self {
        let title = self.title;
        let author = self.author;
        let first = reviewers[0];
        let mut spec = self
            .at(opened, Ev::Open)
            .at(opened.plus(1), Ev::Push(leak_one(title), Ci::Pass));
        for reviewer in reviewers {
            spec = spec.at(opened.plus(4), Ev::Ask(reviewer));
        }
        spec = spec.at(reviewed, Ev::Note(first, "review", None, review.0));
        if let Some(fix) = fix {
            spec = spec
                .at(reviewed.plus(70), Ev::Push(leak_one(fix), Ci::Pass))
                .at(reviewed.plus(72), Ev::Reply(author, "review", review.1));
        } else {
            spec = spec.at(reviewed.plus(35), Ev::Reply(author, "review", review.1));
        }
        spec = spec.at(approved.plus(-1), Ev::Resolve(first, "review"));
        for (k, reviewer) in reviewers.iter().enumerate() {
            spec = spec.at(approved.plus(-(k as i64) * 23), Ev::Approve(reviewer));
        }
        spec.at(merged, Ev::Merge(first))
            .status(MergeabilityStatus::NotOpen)
    }
}

/// `title` as a one-commit list, for [`Ev::Push`].
pub fn leak_one(title: &'static str) -> &'static [&'static str] {
    Box::leak(Box::new([title]))
}

/// What a deployment after a promotion does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Deploy {
    Ok,
    Failed,
    /// Failed, then deployed again this many minutes later.
    FailedThenOk(i64),
}

/// `from` merged into `to` (a promotion, or a back-merge).
#[derive(Debug, Clone)]
pub struct Sync {
    pub at: When,
    pub from: &'static str,
    pub to: &'static str,
    pub by: &'static Person,
    pub deploy: Deploy,
}

/// How an MR was copied to a branch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CopyHow {
    /// `git cherry-pick -x` of its merge commit: a trailer names it.
    CherryPick,
    /// Copied without trailer (a plain cherry-pick, a rebase).
    Plain,
    /// Only its first commits, copied without trailer.
    Partial(usize),
}

#[derive(Debug, Clone)]
pub struct CopySpec {
    pub at: When,
    pub mr: &'static str,
    pub to: &'static str,
    pub how: CopyHow,
    pub by: &'static Person,
}

/// A commit pushed straight to a branch.
#[derive(Debug, Clone)]
pub struct Direct {
    pub at: When,
    pub branch: &'static str,
    pub title: &'static str,
    pub by: &'static Person,
}

#[derive(Debug, Clone)]
pub struct MilestoneSpec {
    pub title: &'static str,
    pub due: Option<When>,
    pub description: &'static str,
}

/// A ticket of the tracker and the time logged on it.
#[derive(Debug, Clone)]
pub struct TicketSpec {
    pub id: u32,
    pub subject: &'static str,
    pub tracker: &'static str,
    pub status: &'static str,
    pub priority: &'static str,
    pub version: Option<&'static str>,
    pub author: &'static Person,
    pub assignee: &'static Person,
    pub estimate_hours: Option<f32>,
    pub done: u8,
    pub start: When,
    /// (day, hours, activity: 0 development · 1 review · 2 testing, comment, who).
    pub entries: Vec<(When, f32, usize, &'static str, &'static Person)>,
}

/// An approval rule of the project.
#[derive(Debug, Clone)]
pub struct RuleSpec {
    pub name: &'static str,
    pub required: u32,
    pub eligible: Vec<&'static Person>,
    /// The rule applies to MRs changing a path starting with one of these
    /// (none: every MR).
    pub paths: Vec<&'static str>,
}

/// A job of the project's pipelines: name, stage, usual seconds.
pub type Job = (&'static str, &'static str, f64);

/// Everything a demo project is made of.
pub struct ProjectSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// `group/project`, for the URLs.
    pub path: &'static str,
    pub branches: Vec<&'static str>,
    pub flow: Option<std::collections::BTreeMap<String, crate::branch_flow::BranchSetting>>,
    /// The environment each branch deploys to.
    pub environments: Vec<(&'static str, &'static str)>,
    /// The jobs of an MR's pipeline; a branch's adds `deploy-<environment>`.
    pub jobs: Vec<Job>,
    pub first_iid: u32,
    pub mrs: Vec<MrSpec>,
    pub syncs: Vec<Sync>,
    pub copies: Vec<CopySpec>,
    pub directs: Vec<Direct>,
    /// The latest pipeline of these branches, when it does not pass.
    pub branch_ci: Vec<(&'static str, Ci)>,
    pub milestones: Vec<MilestoneSpec>,
    pub tickets: Vec<TicketSpec>,
    pub rules: Vec<RuleSpec>,
    /// When the app was last closed: what changed since shows at startup.
    pub last_session: When,
    pub profile: crate::models::DifficultyProfile,
}
