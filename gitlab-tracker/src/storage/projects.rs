//! Which project is active, adding one, and saving each per-project setting.

use super::*;

/// Resolves the active `ProjectEntry` using the following priority:
///
/// 1. `GITLAB_URL` + `GITLAB_PROJECT_ID` environment variables (both required).
///    When `projects.toml` has an entry for that pair, its settings are used;
///    otherwise the project runs on defaults.
/// 2. First entry with `active = true` in `projects.toml`
/// 3. First entry in `projects.toml`
/// 4. One-time silent migration from legacy `config.json`
/// 5. Interactive prompt → saved to `projects.toml`
pub async fn resolve_active_project() -> ProjectEntry {
    // 1. Environment variables — highest priority.
    let env_url = std::env::var("GITLAB_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let env_id = std::env::var("GITLAB_PROJECT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty());

    if let (Some(url), Some(id)) = (env_url, env_id) {
        let gitlab_url = url.trim_end_matches('/').to_string();
        let mut projects_cfg = load_projects_toml().await;
        let entry = find_project_mut(&mut projects_cfg, &gitlab_url, &id)
            .map(|entry| entry.clone())
            .unwrap_or_default();
        return ProjectEntry {
            gitlab_url,
            project_id: id,
            active: true,
            ..entry
        };
    }

    // 2 & 3. projects.toml — active entry or first entry.
    let mut projects_cfg = load_projects_toml().await;

    if !projects_cfg.projects.is_empty() {
        let idx = projects_cfg
            .projects
            .iter()
            .position(|p| p.active)
            .unwrap_or(0);
        let entry = &mut projects_cfg.projects[idx];
        entry.gitlab_url = entry.gitlab_url.trim_end_matches('/').to_string();

        // Backfill project-scoped fields that were absent when projects.toml
        // was first created (one-time enrichment from config.json).
        let needs_save_json = try_enrich_from_config_json(entry).await;
        if needs_save_json {
            tracing::info!(
                "Backfilled project-scoped settings into projects.toml from config.json"
            );
        }

        if needs_save_json {
            save_projects_toml(&projects_cfg).await;
        }

        return projects_cfg.projects[idx].clone();
    }

    // 4. One-time migration from legacy config.json.
    if let Some(migrated) = try_migrate_from_config_json().await {
        return migrated;
    }

    // 5. Interactive prompt — first run with no projects configured yet. Reading
    //    stdin blocks, so it runs off the async runtime.
    let entry = tokio::task::spawn_blocking(prompt_new_project)
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
    projects_cfg.projects.push(entry.clone());
    save_projects_toml(&projects_cfg).await;
    println!("✅ Project saved to projects.toml!\n");

    entry
}

/// Whether no project can be found: no `GITLAB_URL` + `GITLAB_PROJECT_ID`, an
/// empty `projects.toml`, and no legacy `config.json` to migrate (migrated here
/// when there is one). The window then asks for the first project itself.
#[cfg(feature = "gui")]
pub async fn needs_first_project() -> bool {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if env("GITLAB_URL").is_some() && env("GITLAB_PROJECT_ID").is_some() {
        return false;
    }
    if !load_projects_toml().await.projects.is_empty() {
        return false;
    }
    try_migrate_from_config_json().await.is_none()
}

/// Adds `entry` to `projects.toml` (the window's project form), as the only
/// `active` entry when `active`. An entry for the same project is replaced.
#[cfg(feature = "gui")]
pub async fn add_project(entry: ProjectEntry, active: bool) {
    let mut cfg = load_projects_toml().await;
    if active {
        for p in &mut cfg.projects {
            p.active = false;
        }
    }
    let entry = ProjectEntry { active, ..entry };
    match cfg
        .projects
        .iter_mut()
        .find(|p| is_same_project(p, &entry.gitlab_url, &entry.project_id))
    {
        Some(existing) => *existing = entry,
        None => cfg.projects.push(entry),
    }
    save_projects_toml(&cfg).await;
}

/// Whether a GitLab token is available for `gitlab_url` without asking
/// (`GITLAB_TOKEN` or the keyring). Blocks on the keyring: off the UI thread.
#[cfg(feature = "gui")]
pub fn has_gitlab_token(gitlab_url: &str) -> bool {
    migrate_legacy_keyring_entry(gitlab_url);
    gitlab_tracker_core::secrets::has_secret(
        gitlab_tracker_core::secrets::EnvToken {
            var: "GITLAB_TOKEN",
            url_var: "GITLAB_URL",
            instance_url: gitlab_url,
        },
        KEYRING_SERVICE,
        &account_for(gitlab_url),
    )
}

/// The GitLab token for `gitlab_url` without asking, `None` when there is none.
/// Blocks on the keyring.
#[cfg(feature = "gui")]
pub fn stored_gitlab_token(gitlab_url: &str) -> Option<Zeroizing<String>> {
    if !has_gitlab_token(gitlab_url) {
        return None;
    }
    get_or_prompt_token(gitlab_url)
}

/// Stores the GitLab token for `gitlab_url` in the OS keyring. Blocks.
#[cfg(feature = "gui")]
pub fn save_gitlab_token(gitlab_url: &str, token: &str) -> Result<(), String> {
    gitlab_tracker_core::secrets::store_secret(KEYRING_SERVICE, &account_for(gitlab_url), token)
}

/// First-run onboarding: asks for the GitLab URL, project ID and an optional name.
pub(super) fn prompt_new_project() -> ProjectEntry {
    println!("⚙️  No project configured yet. Let's set one up.\n");

    print!("GitLab URL [https://gitlab.com]: ");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    let gitlab_url = if std::io::stdin().read_line(&mut input).is_ok() {
        let v = input.trim().to_string();
        if v.is_empty() {
            "https://gitlab.com".to_string()
        } else {
            v
        }
    } else {
        "https://gitlab.com".to_string()
    };

    let project_id = prompt_required("GitLab Project ID");

    print!("Project name (optional label): ");
    let _ = std::io::stdout().flush();
    let mut name_input = String::new();
    let name = if std::io::stdin().read_line(&mut name_input).is_ok() {
        let v = name_input.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    } else {
        None
    };

    ProjectEntry {
        name,
        gitlab_url: gitlab_url.trim_end_matches('/').to_string(),
        project_id,
        active: true,
        ..Default::default()
    }
}

/// Finds the `projects.toml` entry of the project the app is running on.
///
/// Matched by `(gitlab_url, project_id)` — never by position or by the `active`
/// flag: `--project`, `GITLAB_URL` / `GITLAB_PROJECT_ID` can select a project that is
/// not the active one, and saving into the active entry would corrupt it.
pub(super) fn find_project_mut<'c>(
    cfg: &'c mut ProjectsConfig,
    gitlab_url: &str,
    project_id: &str,
) -> Option<&'c mut ProjectEntry> {
    cfg.projects
        .iter_mut()
        .find(|p| is_same_project(p, gitlab_url, project_id))
}

/// Whether `entry` is the project `(gitlab_url, project_id)` (trailing `/` ignored).
pub fn is_same_project(entry: &ProjectEntry, gitlab_url: &str, project_id: &str) -> bool {
    entry.gitlab_url.trim_end_matches('/') == gitlab_url.trim_end_matches('/')
        && entry.project_id == project_id
}

/// Marks `(gitlab_url, project_id)` as the only `active` entry of `projects.toml`,
/// so the next launch opens it. No-op when the project is not listed.
pub async fn set_active_project_async(gitlab_url: &str, project_id: &str) {
    let mut cfg = load_projects_toml().await;
    if !mark_active(&mut cfg, gitlab_url, project_id) {
        return;
    }
    save_projects_toml(&cfg).await;
}

/// Saves the window's language (`None`: follow the system) in `projects.toml`.
#[cfg(feature = "gui")]
pub async fn set_language_async(language: Option<String>) {
    let mut cfg = load_projects_toml().await;
    cfg.language = language;
    save_projects_toml(&cfg).await;
}

/// Sets `active` on the matching entry only; `false` when none matches.
pub(super) fn mark_active(cfg: &mut ProjectsConfig, gitlab_url: &str, project_id: &str) -> bool {
    if !cfg
        .projects
        .iter()
        .any(|p| is_same_project(p, gitlab_url, project_id))
    {
        return false;
    }
    for p in &mut cfg.projects {
        p.active = is_same_project(p, gitlab_url, project_id);
    }
    true
}

/// Loads `projects.toml`, applies `update` to the current project's entry and saves.
///
/// Returns `false` (and writes nothing) when the project is not in `projects.toml`,
/// e.g. when it was only given through environment variables.
pub(super) async fn update_project_entry(
    gitlab_url: &str,
    project_id: &str,
    update: impl FnOnce(&mut ProjectEntry),
) -> bool {
    let mut cfg = load_projects_toml().await;
    let Some(entry) = find_project_mut(&mut cfg, gitlab_url, project_id) else {
        tracing::warn!(
            gitlab_url,
            project_id,
            "Current project is not in projects.toml — setting not saved"
        );
        return false;
    };
    update(entry);
    save_projects_toml(&cfg).await;
    true
}

/// Persists the "hide merged MRs" toggle of the current project into `projects.toml`.
pub async fn save_hide_merged_async(hide: bool, gitlab_url: &str, project_id: &str) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.hide_merged = Some(hide);
    })
    .await;
}

/// Saves the GitLab username of the project's token owner, read from GitLab by
/// the window, so the "me" views work in the terminal too.
#[cfg(feature = "gui")]
pub async fn save_gitlab_username_async(username: &str, gitlab_url: &str, project_id: &str) {
    let username = username.to_string();
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.gitlab_username = Some(username);
    })
    .await;
}

/// Persists the column visibility settings of the current project into `projects.toml`.
///
/// Called whenever the user closes the column picker popup so that the column
/// selection survives restarts without writing to the legacy `config.json`.
pub async fn save_visible_columns_async(
    cols: &crate::config::VisibleColumns,
    gitlab_url: &str,
    project_id: &str,
) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.visible_columns = Some(cols.clone());
    })
    .await;
}

/// Persists the tracked branch list of the current project into `projects.toml`.
///
/// Called whenever the user adds or removes a branch in the TUI so that the
/// branch list survives restarts without touching `tracker_state.json`.
pub async fn save_branches_async(branches: &[String], gitlab_url: &str, project_id: &str) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.tracked_branches = (!branches.is_empty()).then(|| branches.to_vec());
    })
    .await;
}

/// Persists the branch roles and parents of the current project into `projects.toml`.
pub async fn save_branch_flow_async(
    flow: Option<&std::collections::BTreeMap<String, crate::branch_flow::BranchSetting>>,
    gitlab_url: &str,
    project_id: &str,
) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.branch_flow = flow.cloned();
    })
    .await;
}

/// Persists a TOML-edited project settings table into `projects.toml`.
///
/// The settings dashboard edits a generic TOML table so plugin-provided settings
/// can be saved without hardcoding their fields in the main crate. The table is
/// deserialised back into `ProjectEntry` before saving to preserve validation and
/// the canonical typed storage model. It replaces the entry of the **current**
/// project (`gitlab_url`, `project_id`), whatever entry is marked active.
pub async fn save_project_settings_async(
    project_table: &toml::Table,
    gitlab_url: &str,
    project_id: &str,
) -> Option<ProjectEntry> {
    let project: ProjectEntry = toml::Value::Table(project_table.clone()).try_into().ok()?;
    let saved = project.clone();
    update_project_entry(gitlab_url, project_id, |entry| *entry = project)
        .await
        .then_some(saved)
}
