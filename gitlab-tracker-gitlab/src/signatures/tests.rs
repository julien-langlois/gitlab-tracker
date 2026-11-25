use super::*;

const ALICE: &str = "alice@example.com";

fn fp(title: &str, at: i64) -> CommitFingerprint {
    CommitFingerprint {
        author_email: ALICE.into(),
        authored_at: at,
        title: title.into(),
    }
}

fn c(sha: &str, title: &str, at: i64) -> VersionCommit {
    VersionCommit {
        sha: sha.into(),
        fingerprint: fp(title, at),
    }
}

fn v(base: &str, commits: Vec<VersionCommit>) -> Version {
    Version {
        base_sha: base.into(),
        commits,
    }
}

const VERIFIED: Signature = Signature::Signed(Status::Verified);
const SYSTEM: Signature = Signature::Signed(Status::VerifiedSystem);
const UNSIGNED: Signature = Signature::Unsigned;

/// Facts from `(sha, signature)` and `(sha, patch-id)`.
fn facts(signatures: &[(&str, Signature)], patches: &[(&str, &str)]) -> Facts {
    Facts {
        signatures: signatures
            .iter()
            .map(|(s, g)| (s.to_string(), *g))
            .collect(),
        patch_ids: patches
            .iter()
            .map(|(s, p)| (s.to_string(), Some(p.to_string())))
            .collect(),
    }
}

fn verdicts(report: &Report) -> Vec<(&str, &Verdict, bool)> {
    report
        .commits
        .iter()
        .map(|c| (c.sha.as_str(), &c.verdict, c.accepted))
        .collect()
}

/// v1: `a`, `b` signed on base `x`; v2: rebased from GitLab's UI on `y`.
fn rebased() -> Vec<Version> {
    vec![
        v("x", vec![c("a", "one", 1), c("b", "two", 2)]),
        v("y", vec![c("a2", "one", 1), c("b2", "two", 2)]),
    ]
}

fn rebased_facts() -> Facts {
    facts(
        &[
            ("a", VERIFIED),
            ("b", VERIFIED),
            ("a2", UNSIGNED),
            ("b2", UNSIGNED),
        ],
        &[("a", "p1"), ("a2", "p1"), ("b", "p2"), ("b2", "p2")],
    )
}

#[test]
fn every_commit_verified() {
    let versions = vec![v("x", vec![c("a", "one", 1), c("b", "two", 2)])];
    let report = classify(
        &versions,
        "alice",
        &facts(&[("a", VERIFIED), ("b", VERIFIED)], &[]),
        &Policy::default(),
    );
    assert_eq!(
        verdicts(&report),
        [
            ("a", &Verdict::Verified, true),
            ("b", &Verdict::Verified, true)
        ]
    );
    assert!(report.complete());
    assert_eq!(report.summary().anomalies(), 0);
}

#[test]
fn no_versions_no_commits() {
    let report = classify(&[], "alice", &Facts::default(), &Policy::default());
    assert!(report.commits.is_empty() && report.complete());
}

#[test]
fn an_unsigned_commit_is_an_anomaly() {
    let versions = vec![v("x", vec![c("a", "one", 1), c("b", "two", 2)])];
    let report = classify(
        &versions,
        "alice",
        &facts(&[("a", VERIFIED), ("b", UNSIGNED)], &[]),
        &Policy::default(),
    );
    assert_eq!(report.commits[1].verdict, Verdict::Unsigned);
    assert_eq!(report.summary().unsigned, ["b"]);
}

#[test]
fn an_unknown_signature_is_pending_and_asked_for() {
    let versions = vec![v("x", vec![c("a", "one", 1)])];
    let report = classify(&versions, "alice", &Facts::default(), &Policy::default());
    assert_eq!(report.commits[0].verdict, Verdict::Pending);
    assert!(!report.commits[0].accepted);
    assert_eq!(
        report.summary().anomalies(),
        0,
        "pending is not an anomaly yet"
    );
    assert!(report.missing.signatures.contains("a"));
}

#[test]
fn a_rebase_from_gitlab_keeps_the_signatures() {
    let report = classify(&rebased(), "alice", &rebased_facts(), &Policy::default());
    assert_eq!(
        verdicts(&report),
        [
            (
                "a2",
                &Verdict::SignedBeforeRebase {
                    original: "a".into(),
                    version: 1
                },
                true
            ),
            (
                "b2",
                &Verdict::SignedBeforeRebase {
                    original: "b".into(),
                    version: 1
                },
                true
            ),
        ]
    );
    assert!(report.complete());
}

#[test]
fn the_rebase_is_traced_step_by_step() {
    // Only what the current commits lack is asked for: first their own
    // signatures, then the copies' signatures and both patch-ids.
    let policy = Policy::default();
    let mut known = facts(&[("a2", UNSIGNED), ("b2", UNSIGNED)], &[]);
    let report = classify(&rebased(), "alice", &known, &policy);
    assert!(report.commits.iter().all(|c| c.verdict == Verdict::Pending));
    assert_eq!(
        report.missing.signatures,
        BTreeSet::from(["a".to_string(), "b".to_string()])
    );
    assert_eq!(report.missing.patch_ids.len(), 4);

    known = rebased_facts();
    let report = classify(&rebased(), "alice", &known, &policy);
    assert!(report.complete());
    assert!(report.commits.iter().all(|c| c.accepted));
}

#[test]
fn a_copy_with_another_patch_is_not_forgiven() {
    // Same author, date and title as a signed commit, other content.
    let mut known = rebased_facts();
    known.patch_ids.insert("b2".into(), Some("forged".into()));
    let report = classify(&rebased(), "alice", &known, &Policy::default());
    assert!(report.commits[0].accepted);
    assert_eq!(report.commits[1].verdict, Verdict::Unsigned);
    assert!(!report.commits[1].accepted);
}

#[test]
fn a_patch_that_cannot_be_read_is_not_forgiven() {
    let mut known = rebased_facts();
    known.patch_ids.insert("a".into(), None);
    let report = classify(&rebased(), "alice", &known, &Policy::default());
    assert_eq!(report.commits[0].verdict, Verdict::Unsigned);
    assert!(report.complete(), "not asked for again");
}

#[test]
fn without_the_patch_check_the_fingerprint_is_enough() {
    let policy = Policy {
        require_same_patch: false,
        ..Policy::default()
    };
    let known = facts(
        &[
            ("a", VERIFIED),
            ("b", VERIFIED),
            ("a2", UNSIGNED),
            ("b2", UNSIGNED),
        ],
        &[],
    );
    let report = classify(&rebased(), "alice", &known, &policy);
    assert!(report.commits.iter().all(|c| c.accepted));
    assert!(report.missing.patch_ids.is_empty());
}

#[test]
fn a_rewrite_on_the_same_base_is_not_a_rebase() {
    let versions = vec![
        v("x", vec![c("a", "one", 1)]),
        v("x", vec![c("a2", "one", 1)]),
    ];
    let known = facts(
        &[("a", VERIFIED), ("a2", UNSIGNED)],
        &[("a", "p"), ("a2", "p")],
    );
    let report = classify(&versions, "alice", &known, &Policy::default());
    assert_eq!(report.commits[0].verdict, Verdict::Unsigned);
}

#[test]
fn a_version_that_adds_a_commit_is_not_a_rebase() {
    let versions = vec![
        v("x", vec![c("a", "one", 1)]),
        v("y", vec![c("a2", "one", 1), c("n", "new", 5)]),
    ];
    let known = facts(
        &[("a", VERIFIED), ("a2", UNSIGNED), ("n", UNSIGNED)],
        &[("a", "p"), ("a2", "p")],
    );
    let report = classify(&versions, "alice", &known, &Policy::default());
    assert_eq!(report.commits[0].verdict, Verdict::Unsigned);
    assert_eq!(report.commits[1].verdict, Verdict::Unsigned);
}

#[test]
fn a_commit_dropped_by_the_rebase_keeps_it_a_rebase() {
    // `b` was already upstream: the rebase left it out.
    let versions = vec![
        v("x", vec![c("a", "one", 1), c("b", "two", 2)]),
        v("y", vec![c("a2", "one", 1)]),
    ];
    let known = facts(
        &[("a", VERIFIED), ("a2", UNSIGNED)],
        &[("a", "p"), ("a2", "p")],
    );
    let report = classify(&versions, "alice", &known, &Policy::default());
    assert!(report.commits[0].accepted);
}

#[test]
fn rebased_twice_points_at_the_first_signature() {
    let mut versions = rebased();
    versions.push(v("z", vec![c("a3", "one", 1), c("b3", "two", 2)]));
    let mut known = rebased_facts();
    for (sha, patch) in [("a3", "p1"), ("b3", "p2")] {
        known.signatures.insert(sha.into(), UNSIGNED);
        known.patch_ids.insert(sha.into(), Some(patch.into()));
    }
    let report = classify(&versions, "alice", &known, &Policy::default());
    assert_eq!(
        report.commits[0].verdict,
        Verdict::SignedBeforeRebase {
            original: "a".into(),
            version: 1
        }
    );
    assert!(report.complete());
}

#[test]
fn a_commit_pushed_after_the_rebase_must_be_signed() {
    let mut versions = rebased();
    versions.push(v(
        "y",
        vec![c("a2", "one", 1), c("b2", "two", 2), c("n", "three", 3)],
    ));
    let mut known = rebased_facts();
    known.signatures.insert("n".into(), UNSIGNED);
    let report = classify(&versions, "alice", &known, &Policy::default());
    assert!(report.commits[0].accepted && report.commits[1].accepted);
    assert_eq!(report.commits[2].verdict, Verdict::Unsigned);
    assert!(!report.commits[2].accepted);
}

#[test]
fn the_tolerance_can_be_turned_off() {
    let policy = Policy {
        tolerate_gitlab_rebase: false,
        ..Policy::default()
    };
    let report = classify(&rebased(), "alice", &rebased_facts(), &policy);
    assert!(report
        .commits
        .iter()
        .all(|c| c.verdict == Verdict::Unsigned));
}

#[test]
fn an_original_without_a_valid_signature_forgives_nothing() {
    let mut known = rebased_facts();
    known
        .signatures
        .insert("a".into(), Signature::Signed(Status::UnknownKey));
    known.signatures.insert("b".into(), UNSIGNED);
    let report = classify(&rebased(), "alice", &known, &Policy::default());
    assert!(report
        .commits
        .iter()
        .all(|c| c.verdict == Verdict::Unsigned));
}

#[test]
fn signed_by_the_instance() {
    let versions = vec![v("x", vec![c("a", "one", 1)])];
    let known = facts(&[("a", SYSTEM)], &[]);
    let refused = classify(&versions, "alice", &known, &Policy::default());
    assert_eq!(refused.commits[0].verdict, Verdict::SystemSigned);
    assert!(!refused.commits[0].accepted);

    let policy = Policy {
        accept_system_signed: true,
        ..Policy::default()
    };
    let accepted = classify(&versions, "alice", &known, &policy);
    assert_eq!(accepted.commits[0].verdict, Verdict::SystemSigned);
    assert!(accepted.commits[0].accepted);
}

#[test]
fn a_rebase_signed_by_the_instance_keeps_the_signatures() {
    // An instance with a signing key signs its rebases with it.
    let mut known = rebased_facts();
    known.signatures.insert("a2".into(), SYSTEM);
    known.signatures.insert("b2".into(), SYSTEM);
    let report = classify(&rebased(), "alice", &known, &Policy::default());
    assert!(report
        .commits
        .iter()
        .all(|c| matches!(c.verdict, Verdict::SignedBeforeRebase { .. })));
}

#[test]
fn an_unverified_signature() {
    let versions = vec![v("x", vec![c("a", "one", 1)])];
    let known = facts(&[("a", Signature::Signed(Status::UnknownKey))], &[]);
    let strict = classify(&versions, "alice", &known, &Policy::default());
    assert_eq!(
        strict.commits[0].verdict,
        Verdict::Unverified(Status::UnknownKey)
    );
    assert!(!strict.commits[0].accepted);

    let lenient = Policy {
        require_verified: false,
        ..Policy::default()
    };
    assert!(classify(&versions, "alice", &known, &lenient).commits[0].accepted);
}

#[test]
fn an_unverified_signature_is_not_a_rebase() {
    // A rebase leaves no signature: a bad one is the author's own doing.
    let mut known = rebased_facts();
    known
        .signatures
        .insert("a2".into(), Signature::Signed(Status::OtherUser));
    let report = classify(&rebased(), "alice", &known, &Policy::default());
    assert_eq!(
        report.commits[0].verdict,
        Verdict::Unverified(Status::OtherUser)
    );
}

fn bot_policy() -> Policy {
    Policy {
        exempt_bots: vec![BotAccount {
            username: "@renovate-bot".into(),
            email: "Bot@Renovate.example".into(),
        }],
        ..Policy::default()
    }
}

fn bot_commit(sha: &str, email: &str) -> VersionCommit {
    VersionCommit {
        sha: sha.into(),
        fingerprint: CommitFingerprint {
            author_email: email.into(),
            authored_at: 1,
            title: "chore(deps): bump".into(),
        },
    }
}

#[test]
fn an_exempted_bot_needs_no_signature() {
    let versions = vec![v(
        "x",
        vec![
            bot_commit("r", "bot@renovate.example"),
            bot_commit("h", ALICE),
        ],
    )];
    let known = facts(&[("r", UNSIGNED), ("h", UNSIGNED)], &[]);
    let report = classify(&versions, "renovate-bot", &known, &bot_policy());
    assert_eq!(
        verdicts(&report),
        [
            ("r", &Verdict::BotExempt, true),
            // A human's commit in the bot's MR still needs a signature.
            ("h", &Verdict::Unsigned, false),
        ]
    );
}

#[test]
fn the_bot_email_alone_exempts_nothing() {
    let versions = vec![v("x", vec![bot_commit("r", "bot@renovate.example")])];
    let known = facts(&[("r", UNSIGNED)], &[]);
    let report = classify(&versions, "mallory", &known, &bot_policy());
    assert_eq!(report.commits[0].verdict, Verdict::Unsigned);
    // No bot listed by default.
    let report = classify(&versions, "renovate-bot", &known, &Policy::default());
    assert_eq!(report.commits[0].verdict, Verdict::Unsigned);
}

#[test]
fn statuses_from_either_api() {
    assert_eq!(Status::parse("VERIFIED"), Status::Verified);
    assert_eq!(Status::parse("verified"), Status::Verified);
    assert_eq!(Status::parse("VERIFIED_CA"), Status::Verified);
    assert_eq!(Status::parse("VERIFIED_SYSTEM"), Status::VerifiedSystem);
    assert_eq!(Status::parse("unknown_key"), Status::UnknownKey);
    assert_eq!(
        Status::parse("SAME_USER_DIFFERENT_EMAIL"),
        Status::SameUserDifferentEmail
    );
    assert_eq!(Status::parse("REVOKED_KEY"), Status::RevokedKey);
    assert_eq!(Status::parse("something_new"), Status::Unverified);
}

fn file(path: &str, diff: &str) -> DiffFile {
    DiffFile {
        old_path: path.into(),
        new_path: path.into(),
        b_mode: "100644".into(),
        diff: diff.into(),
        ..DiffFile::default()
    }
}

#[test]
fn a_patch_id_survives_a_rebase() {
    let before = [file(
        "src/a.rs",
        "@@ -10,3 +10,3 @@ fn a\n context\n-old\n+new\n",
    )];
    // Moved down by upstream changes, with other context around it.
    let after = [file(
        "src/a.rs",
        "@@ -42,3 +42,3 @@ fn b\n other\n-old\n+new\n",
    )];
    assert_eq!(patch_id(&before), patch_id(&after));
    assert!(patch_id(&before).is_some());
}

#[test]
fn a_patch_id_tells_changes_apart() {
    let base = patch_id(&[file("a.rs", "@@ -1 +1 @@\n-old\n+new\n")]);
    assert_ne!(
        base,
        patch_id(&[file("a.rs", "@@ -1 +1 @@\n-old\n+other\n")])
    );
    assert_ne!(base, patch_id(&[file("b.rs", "@@ -1 +1 @@\n-old\n+new\n")]));
    // Whitespace counts (YAML, Python…).
    assert_ne!(
        base,
        patch_id(&[file("a.rs", "@@ -1 +1 @@\n-old\n+ new\n")])
    );
}

#[test]
fn a_patch_id_ignores_the_file_order() {
    let a = file("a.rs", "+1\n");
    let b = file("b.rs", "+2\n");
    assert_eq!(patch_id(&[a.clone(), b.clone()]), patch_id(&[b, a]));
}

#[test]
fn no_patch_id_for_a_diff_left_out() {
    let mut big = file("big.json", "");
    big.too_large = true;
    assert_eq!(patch_id(&[file("a.rs", "+1\n"), big]), None);
}

#[test]
fn diff_files_from_gitlab() {
    let files: Vec<DiffFile> = serde_json::from_str(
        r#"[{"old_path":"a.rs","new_path":"a.rs","a_mode":"100644","b_mode":"100644",
             "new_file":false,"renamed_file":false,"deleted_file":false,"diff":"@@ -1 +1 @@\n-a\n+b\n"}]"#,
    )
    .unwrap();
    assert_eq!(files[0].diff, "@@ -1 +1 @@\n-a\n+b\n");
    assert!(!files[0].too_large);
}

#[test]
fn bots_from_the_setting() {
    assert_eq!(
        BotAccount::parse_list("@renovate-bot <Bot@Renovate.example>, dependabot; lonely"),
        [BotAccount {
            username: "renovate-bot".into(),
            email: "bot@renovate.example".into(),
        }]
    );
    assert!(BotAccount::parse_list("").is_empty());
}

#[test]
fn the_username_in_an_author() {
    assert_eq!(author_username("Jane Doe (@jdoe)"), "jdoe");
    assert_eq!(
        author_username("Bot (beta) (@renovate-bot)"),
        "renovate-bot"
    );
    assert_eq!(author_username("jdoe"), "jdoe");
}

#[test]
fn data_for_the_current_commits() {
    let data = SignatureData {
        versions: rebased(),
        ..SignatureData::default()
    };
    assert!(data.is_current(["b2", "a2"]));
    assert!(!data.is_current(["a2"]));
    assert!(!data.is_current(["a2", "b2", "c"]));
    assert!(!SignatureData::default().is_current([]));
}
