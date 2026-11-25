#[test]
fn mark_active_keeps_a_single_active_entry() {
    use super::{mark_active, ProjectEntry, ProjectsConfig};
    let entry = |id: &str, active: bool| ProjectEntry {
        gitlab_url: "https://g".into(),
        project_id: id.into(),
        active,
        ..ProjectEntry::default()
    };
    let mut cfg = ProjectsConfig {
        projects: vec![entry("1", true), entry("2", false)],
        ..Default::default()
    };
    assert!(mark_active(&mut cfg, "https://g/", "2"));
    let active: Vec<bool> = cfg.projects.iter().map(|p| p.active).collect();
    assert_eq!(active, [false, true]);
    assert!(
        !mark_active(&mut cfg, "https://g", "3"),
        "unknown project: untouched"
    );
    assert!(cfg.projects[1].active);
}

#[test]
fn legacy_state_file_still_parses() {
    // Older versions stored `null` for optional fields and omitted newer ones.
    let legacy = r#"{"mrs": [{
        "id": "42", "title": "Fix login", "sha": null, "found_branches": ["main"],
        "description": null, "author": null, "assignee": "None", "milestone": null,
        "web_url": null, "labels": null
    }], "branches": []}"#;
    let state: SavedState = serde_json::from_str(legacy).expect("legacy file parses");
    let mr = &state.mrs[0];
    assert_eq!(
        (mr.id.as_str(), mr.data.title.as_str()),
        ("42", "Fix login")
    );
    assert_eq!(mr.data.description, "");
    assert!(mr.data.labels.is_empty() && mr.data.pipelines.is_empty());
    assert!(mr.found_branches.contains("main"));
    // Written back flat, as before (no nested "data" object).
    let json = serde_json::to_value(&state.mrs[0]).unwrap();
    assert!(json.get("data").is_none() && json.get("title").is_some());
}

#[test]
fn saves_target_the_current_project_not_the_active_one() {
    let project = |url: &str, id: &str, active| ProjectEntry {
        gitlab_url: url.into(),
        project_id: id.into(),
        active,
        ..Default::default()
    };
    let mut cfg = ProjectsConfig {
        projects: vec![
            project("https://gitlab.a.com", "1", true),
            project("https://gitlab.b.com", "2", false),
        ],
        ..Default::default()
    };
    // Started with `--project 2`: the second entry is edited, the active one untouched.
    let entry = find_project_mut(&mut cfg, "https://gitlab.b.com/", "2").unwrap();
    entry.tracked_branches = Some(vec!["main".into()]);
    assert_eq!(cfg.projects[0].tracked_branches, None);
    assert_eq!(
        cfg.projects[1].tracked_branches.as_deref(),
        Some(&["main".to_string()][..])
    );
    // Same id on another instance, or a project only known from env vars: no match.
    assert!(find_project_mut(&mut cfg, "https://gitlab.a.com", "2").is_none());
    assert!(find_project_mut(&mut cfg, "https://env-only.example", "9").is_none());
}

#[test]
fn env_list_syntax_for_figment() {
    use figment::providers::{Env, Serialized};
    use figment::Figment;
    let parse = |value: &str| {
        // Unique prefix: tests share the process environment.
        std::env::set_var("GT_ENVTEST_DEFAULT_BRANCHES", value);
        let result = Figment::from(Serialized::defaults(AppConfig::default()))
            .merge(Env::prefixed("GT_ENVTEST_").map(|k| k.as_str().to_lowercase().into()))
            .extract::<AppConfig>()
            .map(|c| c.default_branches)
            .ok();
        std::env::remove_var("GT_ENVTEST_DEFAULT_BRANCHES");
        result
    };
    // A bare comma list is a string, not a sequence: extraction fails (and the
    // whole config used to fall back to defaults silently). The README documents
    // the bracket syntax.
    assert!(parse("main,staging").is_none());
    assert_eq!(parse("[main,staging]").unwrap(), ["main", "staging"]);

    std::env::set_var("GT_ENVTEST2_TABLE_LABEL_PREFIXES", "[deploy::,review::]");
    let prefixes = Figment::from(Serialized::defaults(AppConfig::default()))
        .merge(Env::prefixed("GT_ENVTEST2_").map(|k| k.as_str().to_lowercase().into()))
        .extract::<AppConfig>()
        .map(|c| c.table_label_prefixes)
        .ok();
    std::env::remove_var("GT_ENVTEST2_TABLE_LABEL_PREFIXES");
    assert_eq!(prefixes.unwrap(), ["deploy::", "review::"]);
}

use super::*;

#[test]
fn env_overrides_win_and_keep_runtime_fields() {
    // Unique prefix: tests share the process environment.
    std::env::set_var("GT_ENVTEST3_ACTIVITY_STALE_DAYS", "21");
    let project_config = AppConfig {
        activity_stale_days: 7, // as set from projects.toml
        gitlab_username: Some("jdoe".into()),
        // The per-project settings kept out of config.json.
        hotfix_branches: "fix/prod-*".into(),
        release_note_tickets: true,
        freeze_periods: "0 18 * * 5 -> 0 7 * * 1 Europe/Paris".into(),
        signed_commits: Some(crate::signatures::Policy::default()),
        ..AppConfig::default()
    };
    let config = apply_env_overrides_with_prefix(project_config, "GT_ENVTEST3_");
    std::env::remove_var("GT_ENVTEST3_ACTIVITY_STALE_DAYS");
    assert_eq!(config.activity_stale_days, 21);
    assert_eq!(config.gitlab_username.as_deref(), Some("jdoe"));
    assert_eq!(config.hotfix_branches, "fix/prod-*");
    assert!(config.release_note_tickets);
    assert_eq!(
        config.freeze_periods,
        "0 18 * * 5 -> 0 7 * * 1 Europe/Paris"
    );
    assert!(config.signed_commits.is_some());
}

#[tokio::test]
async fn atomic_write_and_corrupt_backup() {
    let dir = std::env::temp_dir().join(format!("gt-storage-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.json");

    write_atomic(&path, "{broken").await.unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    assert!(!dir.join("state.json.tmp").exists());

    backup_corrupt_file(&path, &"parse error").await;
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("state.json.bak")).unwrap(),
        "{broken"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Points `get_save_dir` at a fresh temporary directory for the current test
/// (current thread — `#[tokio::test]` runs on it), removed on drop.
struct TempConfigDir(PathBuf);

impl TempConfigDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TEST_SAVE_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        Self(dir)
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for TempConfigDir {
    fn drop(&mut self) {
        TEST_SAVE_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const URL: &str = "https://gitlab.example";

fn project(url: &str, id: &str, active: bool) -> ProjectEntry {
    ProjectEntry {
        gitlab_url: url.into(),
        project_id: id.into(),
        active,
        ..Default::default()
    }
}

async fn write_projects(entries: Vec<ProjectEntry>) {
    save_projects_toml(&ProjectsConfig {
        projects: entries,
        ..Default::default()
    })
    .await;
}

#[test]
fn state_file_and_keyring_account_are_scoped_per_project() {
    let name = tracker_state_file_name(URL, "42");
    assert!(name.starts_with("tracker_") && name.ends_with(".json"));
    assert_eq!(name.len(), "tracker_.json".len() + 8);
    // A trailing slash is the same instance; another id or instance is not.
    assert_eq!(
        name,
        tracker_state_file_name("https://gitlab.example/", "42")
    );
    assert_ne!(name, tracker_state_file_name(URL, "43"));
    assert_ne!(name, tracker_state_file_name("https://other.example", "42"));
    // "<url>|<id>" is hashed: moving a character across the separator changes it.
    assert_ne!(
        tracker_state_file_name("https://g/1", "2"),
        tracker_state_file_name("https://g/", "12")
    );
    assert_eq!(
        account_for("https://gitlab.example/"),
        "gitlab_token::https://gitlab.example"
    );
}

#[test]
fn tracker_section_keeps_provider_fields_apart_from_provider_and_url() {
    let cfg: ProjectsConfig = toml::from_str(
        r##"
        [[project]]
        gitlab_url = "https://gitlab.example"
        project_id = "1"

        [project.tracker]
        provider = "redmine"
        url = "https://redmine.example"
        ticket_patterns = ['RM-(\d+)']

        [project.tracker.status_transitions.gitlab_state]
        merged = "5"
        "##,
    )
    .unwrap();
    let tracker = cfg.projects[0].tracker.as_ref().unwrap();
    assert_eq!(tracker.provider, "redmine");
    assert_eq!(tracker.url, "https://redmine.example");
    // What the plugin receives: its own fields only — it must not expect `url`.
    let mut keys: Vec<&str> = tracker.extra.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["status_transitions", "ticket_patterns"]);

    // Written back flat under [project.tracker], as it was read.
    let written = toml::to_string_pretty(&cfg).unwrap();
    let reread: ProjectsConfig = toml::from_str(&written).unwrap();
    let reread = reread.projects[0].tracker.as_ref().unwrap();
    assert_eq!(reread.url, tracker.url);
    assert_eq!(reread.extra, tracker.extra);
}

#[tokio::test]
async fn without_a_config_dir_loads_are_empty_and_saves_are_no_ops() {
    assert!(get_save_dir().is_none());
    assert!(load_projects_toml().await.projects.is_empty());
    save_branches_async(&["main".into()], URL, "1").await;
    let (mrs, branches, known, discovery, dismissed) = load_state_async(URL, "1").await;
    assert!(mrs.is_empty() && branches.is_empty() && known.is_empty());
    assert!(discovery.is_none() && dismissed.is_empty());
}

#[cfg(feature = "gui")]
#[tokio::test]
async fn the_window_adds_projects_without_duplicates() {
    let _dir = TempConfigDir::new("add-project");
    write_projects(vec![project(URL, "1", true)]).await;
    // Opened in a tab: added behind, the active one kept.
    add_project(project(&format!("{URL}/"), "2", true), false).await;
    // Added again (same instance and id): replaced, not doubled.
    let renamed = ProjectEntry {
        name: Some("Two".into()),
        ..project(URL, "2", false)
    };
    add_project(renamed, false).await;
    let cfg = load_projects_toml().await;
    let ids: Vec<(&str, bool)> = cfg
        .projects
        .iter()
        .map(|p| (p.project_id.as_str(), p.active))
        .collect();
    assert_eq!(ids, [("1", true), ("2", false)]);
    assert_eq!(cfg.projects[1].name.as_deref(), Some("Two"));
    // The first-run window's project: the only active one.
    add_project(project(URL, "3", false), true).await;
    let active: Vec<bool> = load_projects_toml()
        .await
        .projects
        .iter()
        .map(|p| p.active)
        .collect();
    assert_eq!(active, [false, false, true]);
}

#[tokio::test]
async fn project_settings_are_saved_into_the_current_entry_only() {
    let dir = TempConfigDir::new("project-settings");
    write_projects(vec![project(URL, "1", true), project(URL, "2", false)]).await;

    save_branches_async(&["main".into(), "prod".into()], URL, "2").await;
    save_hide_merged_async(true, URL, "2").await;
    let mut cols = crate::config::VisibleColumns::default();
    cols.set_visible("milestone", true);
    save_visible_columns_async(&cols, URL, "2").await;
    // Not in projects.toml (e.g. selected through env vars): nothing is written.
    save_hide_merged_async(true, "https://env-only.example", "9").await;

    let cfg = load_projects_toml().await;
    assert_eq!(cfg.projects.len(), 2);
    let (first, second) = (&cfg.projects[0], &cfg.projects[1]);
    assert_eq!(first.tracked_branches, None);
    assert_eq!(first.hide_merged, None);
    assert_eq!(
        second.tracked_branches.as_deref(),
        Some(&["main".to_string(), "prod".to_string()][..])
    );
    assert_eq!(second.hide_merged, Some(true));
    assert!(second.visible_columns.is_some());

    // An empty branch list removes the key instead of storing `[]`.
    save_branches_async(&[], URL, "2").await;
    assert_eq!(
        load_projects_toml().await.projects[1].tracked_branches,
        None
    );

    set_active_project_async(URL, "2").await;
    let active: Vec<bool> = load_projects_toml()
        .await
        .projects
        .iter()
        .map(|p| p.active)
        .collect();
    assert_eq!(active, [false, true]);
    assert!(!dir.path("projects.toml.tmp").exists());
}

#[tokio::test]
async fn settings_table_is_validated_before_replacing_the_entry() {
    let _dir = TempConfigDir::new("settings-table");
    write_projects(vec![project(URL, "1", true)]).await;

    let valid: toml::Table = toml::from_str(
        "gitlab_url = \"https://gitlab.example\"\nproject_id = \"1\"\n\
         activity_stale_days = 30\ngitlab_username = \"jdoe\"\n",
    )
    .unwrap();
    let saved = save_project_settings_async(&valid, URL, "1").await.unwrap();
    assert_eq!(saved.activity_stale_days, Some(30));
    let entry = load_projects_toml().await.projects.remove(0);
    assert_eq!(entry.activity_stale_days, Some(30));
    assert_eq!(entry.gitlab_username.as_deref(), Some("jdoe"));

    // A wrong type is rejected and leaves the file as it was.
    let invalid: toml::Table =
        toml::from_str("gitlab_url = \"u\"\nproject_id = \"1\"\nactivity_stale_days = \"soon\"\n")
            .unwrap();
    assert!(save_project_settings_async(&invalid, URL, "1")
        .await
        .is_none());
    assert_eq!(
        load_projects_toml().await.projects[0].activity_stale_days,
        Some(30)
    );
    // Valid table, but the project is not listed: not saved.
    assert!(save_project_settings_async(&valid, URL, "404")
        .await
        .is_none());
}

#[tokio::test]
async fn tracker_state_round_trips() {
    let dir = TempConfigDir::new("state-round-trip");
    let mut merged = TrackedMr::placeholder("7".into(), "Fix login".into(), None);
    merged.status = MrStatus::MergedIn(["main".to_string()].into());
    merged.flagged = true;
    let loading = TrackedMr::placeholder("8".into(), "Add stats".into(), Some("v1".into()));
    let known = HashMap::from([("main".to_string(), HashSet::from(["7".to_string()]))]);
    let dismissed = HashSet::from(["3".to_string()]);

    save_state_async(
        &[merged, loading],
        &known,
        Some("2026-01-02"),
        &dismissed,
        URL,
        "1",
    )
    .await;
    assert!(dir.path(crate::state_db::DB_FILE).exists());
    assert!(!dir.path(&tracker_state_file_name(URL, "1")).exists());

    let (mrs, branches, loaded_known, discovery, loaded_dismissed) =
        load_state_async(URL, "1").await;
    assert_eq!(mrs.len(), 2);
    assert_eq!(
        (mrs[0].id.as_str(), mrs[0].data.title.as_str()),
        ("7", "Fix login")
    );
    assert!(mrs[0].flagged && mrs[0].found_branches.contains("main"));
    // Only a `MergedIn` status carries branches.
    assert!(mrs[1].found_branches.is_empty());
    assert_eq!(mrs[1].data.milestone.as_deref(), Some("v1"));
    assert!(branches.is_empty(), "branches live in projects.toml");
    assert_eq!(loaded_known, known);
    assert_eq!(discovery.as_deref(), Some("2026-01-02"));
    assert_eq!(loaded_dismissed, dismissed);
    // Another project has its own (absent) file.
    assert!(load_state_async(URL, "2").await.0.is_empty());
}

#[tokio::test]
async fn legacy_state_file_is_renamed_and_its_branches_backfilled() {
    let dir = TempConfigDir::new("state-migration");
    write_projects(vec![project(URL, "1", true), project(URL, "2", false)]).await;
    let legacy = r#"{"mrs": [], "branches": ["main", "staging"]}"#;
    std::fs::write(dir.path("tracker_state.json"), legacy).unwrap();

    // Loading project 2 claims the legacy file and backfills *its* entry.
    let (_, branches, ..) = load_state_async(URL, "2").await;
    assert_eq!(branches, ["main", "staging"]);
    assert!(!dir.path("tracker_state.json").exists());
    // Imported into tracker.db, the file is kept aside.
    let file = tracker_state_file_name(URL, "2");
    assert!(!dir.path(&file).exists());
    assert!(dir.path(&format!("{file}.imported")).exists());
    let cfg = load_projects_toml().await;
    assert_eq!(cfg.projects[0].tracked_branches, None);
    assert_eq!(
        cfg.projects[1].tracked_branches.as_deref(),
        Some(&["main".to_string(), "staging".to_string()][..])
    );

    // Branches already in projects.toml are never overwritten by the state file.
    save_branches_async(&["prod".into()], URL, "2").await;
    load_state_async(URL, "2").await;
    assert_eq!(
        load_projects_toml().await.projects[1]
            .tracked_branches
            .as_deref(),
        Some(&["prod".to_string()][..])
    );

    // A project that already has its file leaves a legacy file alone.
    std::fs::write(dir.path("tracker_state.json"), legacy).unwrap();
    load_state_async(URL, "2").await;
    assert!(dir.path("tracker_state.json").exists());
}

/// A configuration directory written by gitlab-tracker 0.6.0 itself
/// (`tests/fixtures/v0.6.0`): the first launch reads all of it, every field of
/// every MR survives the import into tracker.db, and stats.db is migrated
/// with its rows.
#[tokio::test]
async fn a_0_6_configuration_upgrades_without_losing_anything() {
    const URL_06: &str = "https://gitlab.com";
    const STATE: &str = "tracker_1baab28d.json";
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.6.0");
    let dir = TempConfigDir::new("upgrade-0.6");
    for file in ["config.json", "projects.toml", "stats.db", STATE] {
        std::fs::copy(fixture.join(file), dir.path(file)).unwrap();
    }
    assert_eq!(tracker_state_file_name(URL_06, "123456"), STATE);
    let before: SavedState =
        serde_json::from_str(&std::fs::read_to_string(dir.path(STATE)).unwrap()).unwrap();

    // projects.toml and config.json, as they are.
    let projects = load_projects_toml().await.projects;
    assert_eq!(projects.len(), 1);
    assert_eq!(
        projects[0].tracked_branches.as_deref(),
        Some(&["main".to_string(), "staging".into(), "production".into()][..])
    );
    let config = load_or_create_config_async().await;
    assert!(!config.default_branches.is_empty());

    // The state: imported once, then read back from tracker.db, unchanged.
    let first = load_state_async(URL_06, "123456").await;
    assert!(!dir.path(STATE).exists());
    assert!(dir.path(&format!("{STATE}.imported")).exists());
    let (mrs, _, known, discovery, dismissed) = load_state_async(URL_06, "123456").await;
    assert_eq!(mrs.len(), 7);
    assert_eq!(mrs.len(), first.0.len());
    // Field by field, through JSON (sets sorted: their order is not kept).
    fn canonical(mut v: serde_json::Value) -> serde_json::Value {
        match &mut v {
            serde_json::Value::Array(items) => {
                let mut sorted: Vec<_> = items.drain(..).map(canonical).collect();
                sorted.sort_by_key(|i| i.to_string());
                *items = sorted;
            }
            serde_json::Value::Object(map) => {
                for item in map.values_mut() {
                    *item = canonical(item.take());
                }
            }
            _ => {}
        }
        v
    }
    for old in &before.mrs {
        let new = mrs.iter().find(|m| m.id == old.id).expect("MR kept");
        assert_eq!(
            canonical(serde_json::to_value(new).unwrap()),
            canonical(serde_json::to_value(old).unwrap()),
            "MR {}",
            old.id
        );
    }
    assert_eq!(known, before.last_known_branches);
    assert_eq!(discovery, before.discovery_started_at);
    assert_eq!(dismissed, before.dismissed_mr_ids);

    // stats.db: migrated from 0.6's schema, its snapshots kept.
    #[cfg(feature = "stats")]
    {
        use gitlab_tracker_stats::db::SnapshotQuery;
        let db = gitlab_tracker_stats::SqliteStatsDb::open(dir.path("stats.db").to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(db.query(&SnapshotQuery::default()).await.unwrap().len(), 20);
    }
}

#[tokio::test]
async fn a_state_file_is_imported_once_and_reloaded_from_the_database() {
    let dir = TempConfigDir::new("state-import");
    let json = r#"{
        "mrs": [
            {"id": "12", "found_branches": ["main"], "flagged": true,
             "title": "Fix login", "author": "ana", "web_url": "https://g/12",
             "state": "merged", "merged_by": "bob",
             "merged_at": "2026-11-17T10:00:00Z", "approved_by": ["bob"],
             "source_branch": "fix", "target_branch": "main"},
            {"id": "13", "found_branches": [], "title": "Add stats",
             "author": "ana", "web_url": "https://g/13",
             "source_branch": "stats", "target_branch": "main"}
        ],
        "last_known_branches": {"main": ["12"]},
        "discovery_started_at": "2026-01-02",
        "dismissed_mr_ids": ["3"]
    }"#;
    let file = tracker_state_file_name(URL, "1");
    std::fs::write(dir.path(&file), json).unwrap();

    let (mrs, _, known, discovery, dismissed) = load_state_async(URL, "1").await;
    let ids: Vec<&str> = mrs.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["12", "13"]);
    assert!(!dir.path(&file).exists());
    assert!(dir.path(&format!("{file}.imported")).exists());

    // Read again: from the database, the same.
    let (again, _, known2, discovery2, dismissed2) = load_state_async(URL, "1").await;
    let ids2: Vec<&str> = again.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids2, ["12", "13"]);
    assert!(again[0].flagged && again[0].found_branches.contains("main"));
    assert_eq!(again[0].data.approved_by, ["bob"]);
    assert_eq!(
        (known2, discovery2, dismissed2),
        (known, discovery, dismissed)
    );

    // By hand: already there.
    assert!(import_state_command(URL, "1").await.is_err());
    // Its history, dated by GitLab.
    let db = crate::state_db::open_in(&dir.0).await.unwrap();
    let kinds: Vec<&str> = db
        .events_of(URL, "1", "12")
        .await
        .unwrap()
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            "tracked",
            "opened",
            "approved",
            "merged",
            "branch_reached",
            "flagged"
        ]
    );
}

#[cfg(feature = "gui")]
#[tokio::test]
async fn the_caches_and_window_prefs_are_imported_then_kept_in_the_database() {
    use crate::gitlab_detail::MergeHistory;
    use crate::owners::{History, Merge};
    let dir = TempConfigDir::new("caches");
    let hash = tracker_state_file_name(URL, "1");
    let merges_file = hash.replacen("tracker_", "merges_", 1);
    let owners_file = hash.replacen("tracker_", "owners_", 1);
    let recent = chrono::Utc::now().to_rfc3339();
    let merges = MergeHistory {
        merged: [(4, recent.clone()), (1, "2020-01-01T00:00:00Z".to_string())].into(),
        read_at: Some(recent.clone()),
    };
    std::fs::write(
        dir.path(&merges_file),
        serde_json::to_string(&merges).unwrap(),
    )
    .unwrap();
    let merge = Merge {
        merged_at: recent.clone(),
        author: "ana".into(),
        reviewers: vec!["zoe".into(), "bob".into()],
        approvers: vec!["bob".into()],
        paths: vec!["b.rs".into(), "a.rs".into()],
    };
    let owners = History {
        merges: [(4, merge.clone())].into(),
        people: [("ana".to_string(), "Ana".to_string())].into(),
        read_at: Some(recent.clone()),
    };
    std::fs::write(
        dir.path(&owners_file),
        serde_json::to_string(&owners).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path("gui.json"),
        r#"{"page": "delivery", "light": true}"#,
    )
    .unwrap();

    // The caches first (a page opened before the MRs were saved): the
    // project's state file is still imported after.
    let store = project_store(URL, "1").unwrap();
    assert_eq!(store.load_merges().await, Some(merges.clone()));
    assert_eq!(store.load_owners().await, Some(owners.clone()));
    assert!(!dir.path(&merges_file).exists() && !dir.path(&owners_file).exists());
    std::fs::write(
        dir.path(&hash),
        r#"{"mrs": [{"id": "4", "found_branches": [], "title": "T", "web_url": "https://g/4"}]}"#,
    )
    .unwrap();
    assert_eq!(load_state_async(URL, "1").await.0.len(), 1);

    // Read back from the database, orders kept; a new read adds to them.
    assert_eq!(store.load_owners().await, Some(owners.clone()));
    let mut more = owners.clone();
    more.merges.insert(
        5,
        Merge {
            author: "bob".into(),
            ..merge
        },
    );
    store.save_owners(&more, &owners).await;
    assert_eq!(store.load_owners().await.unwrap().merges.len(), 2);
    // Out of the period, still in the database: the history.
    assert!(store.load_merges().await.unwrap().merged.contains_key(&1));

    let prefs: serde_json::Value = load_setting("gui", "gui.json").await.unwrap();
    assert_eq!(prefs["page"], "delivery");
    assert!(!dir.path("gui.json").exists());
    save_setting("gui", &serde_json::json!({"page": "today"})).await;
    let prefs: serde_json::Value = load_setting("gui", "gui.json").await.unwrap();
    assert_eq!(prefs["page"], "today");
}

/// The window saves from its UI thread, blocking inside the runtime.
#[test]
fn a_save_blocking_inside_the_runtime_completes() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let _dir = TempConfigDir::new("blocking-save");
        let mr = TrackedMr::placeholder("7".into(), "Fix".into(), None);
        let file = StateFile::new(&[mr], &HashMap::new(), None, &HashSet::new(), URL, "1").unwrap();
        tokio::task::block_in_place(|| futures::executor::block_on(file.write()));
        assert_eq!(load_state_async(URL, "1").await.0.len(), 1);
    });
}

#[tokio::test]
async fn unparseable_files_are_moved_aside_not_overwritten() {
    let dir = TempConfigDir::new("corrupt");
    std::fs::write(dir.path("projects.toml"), "[[project]\nbroken").unwrap();
    assert!(load_projects_toml().await.projects.is_empty());
    assert!(!dir.path("projects.toml").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path("projects.toml.bak")).unwrap(),
        "[[project]\nbroken"
    );

    let state = tracker_state_file_name(URL, "1");
    std::fs::write(dir.path(&state), "{not json").unwrap();
    assert!(load_state_async(URL, "1").await.0.is_empty());
    assert!(!dir.path(&state).exists());
    assert!(dir.path(&format!("{state}.bak")).exists());
}

#[tokio::test]
async fn legacy_config_json_migrates_once_and_backfills_missing_fields() {
    let dir = TempConfigDir::new("config-json");
    let config_json = r#"{
        "gitlab_url": "https://gitlab.example/", "project_id": "1",
        "default_branches": ["main"], "table_label_prefixes": [],
        "refresh_interval_secs": 300, "activity_stale_days": 9
    }"#;
    std::fs::write(dir.path("config.json"), config_json).unwrap();

    // Backfill: only the fields the entry does not set yet.
    let mut entry = ProjectEntry {
        activity_stale_days: Some(3),
        ..project(URL, "1", true)
    };
    assert!(try_enrich_from_config_json(&mut entry).await);
    assert_eq!(entry.activity_stale_days, Some(3));
    assert_eq!(entry.refresh_interval_secs, Some(300));
    assert_eq!(
        entry.default_branches.as_deref(),
        Some(&["main".to_string()][..])
    );
    assert_eq!(
        entry.table_label_prefixes, None,
        "empty lists are not migrated"
    );
    assert!(
        !try_enrich_from_config_json(&mut entry).await,
        "nothing left to backfill"
    );

    // Migration: creates projects.toml from config.json, once.
    let migrated = try_migrate_from_config_json().await.unwrap();
    assert_eq!(migrated.gitlab_url, URL);
    assert!(migrated.active);
    assert_eq!(migrated.activity_stale_days, Some(9));
    let cfg = load_projects_toml().await;
    assert_eq!(cfg.projects.len(), 1);
    assert_eq!(cfg.projects[0].project_id, "1");
    assert!(try_migrate_from_config_json().await.is_none());

    // Without the project fields there is nothing to migrate.
    std::fs::remove_file(dir.path("projects.toml")).unwrap();
    std::fs::write(
        dir.path("config.json"),
        r#"{"gitlab_url": " ", "project_id": "1"}"#,
    )
    .unwrap();
    assert!(try_migrate_from_config_json().await.is_none());
    assert!(!dir.path("projects.toml").exists());
}

#[tokio::test]
async fn config_json_is_created_on_first_run_and_survives_bad_content() {
    let dir = TempConfigDir::new("config-load");
    // Whatever GITLAB_TRACKER_* the shell exports applies to both sides.
    let defaults = apply_env_overrides(AppConfig::default());

    let first = load_or_create_config_async().await;
    assert!(
        dir.path("config.json").exists(),
        "template written on first run"
    );
    assert_eq!(first.activity_stale_days, defaults.activity_stale_days);

    std::fs::write(dir.path("config.json"), r#"{"show_cockpit": "maybe"}"#).unwrap();
    let fallback = load_or_create_config_async().await;
    assert_eq!(fallback.show_cockpit, defaults.show_cockpit);
    assert_eq!(fallback.default_branches, defaults.default_branches);
}

#[tokio::test]
async fn state_writer_lands_the_newest_state_last() {
    let _dir = TempConfigDir::new("state-writer");
    let writer = StateWriter::default();
    for id in ["1", "2", "3"] {
        let dismissed = HashSet::from([id.to_string()]);
        let file = StateFile::new(&[], &HashMap::new(), None, &dismissed, URL, "1").unwrap();
        writer.save(file);
    }
    writer.flush().await;
    let (.., dismissed) = load_state_async(URL, "1").await;
    assert_eq!(dismissed, HashSet::from(["3".to_string()]));
}
