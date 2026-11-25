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
