use crate::config::AppConfig;
use crate::models::{MrStatus, SavedMr, SavedState, TrackedMr};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use zeroize::Zeroizing;

/// Persists only the `AppConfig` (e.g. after toggling column visibility).
///
/// Reads the existing `config.json`, merges the new value, and writes it back
/// atomically so no other config fields are lost.
pub async fn save_config_async(config: &AppConfig) {
    if let Some(config_dir) = get_save_dir() {
        let path = config_dir.join("config.json");
        if let Ok(json) = serde_json::to_string_pretty(config) {
            if let Err(e) = tokio::fs::create_dir_all(&config_dir).await {
                tracing::error!(error = %e, path = ?config_dir, "Failed to create config directory");
                return;
            }
            if let Err(e) = tokio::fs::write(&path, json).await {
                tracing::error!(error = %e, path = ?path, "Failed to write config.json");
            }
        }
    }
}

/// Prompts the user interactively for a required config value (read from stdin).
fn prompt_required(label: &str) -> String {
    loop {
        print!("{}: ", label);
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let value = input.trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
        println!("  ⚠️  This field is required, please enter a value.");
    }
}

/// Ensures `gitlab_url` and `project_id` are present, either from env vars,
/// the loaded config, or an interactive prompt.
///
/// Env vars (`GITLAB_URL`, `GITLAB_PROJECT_ID`) are always checked first so
/// that CI / dotenv workflows are never interrupted by prompts.
/// Any value obtained via prompt is persisted back to `config.json` so the
/// user is only asked once.
pub async fn ensure_gitlab_config(config: &mut crate::config::AppConfig) {
    let mut changed = false;

    // --- gitlab_url ---
    let url_from_env = std::env::var("GITLAB_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let url_in_config = config.gitlab_url.as_deref().unwrap_or("").trim().is_empty();

    if url_from_env.is_none() && url_in_config {
        println!("🌐 No GitLab URL found in config or environment.");
        println!("   Leave empty to use the default (https://gitlab.com)");
        print!("GitLab URL [https://gitlab.com]: ");
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let value = input.trim().to_string();
            config.gitlab_url = if value.is_empty() {
                Some("https://gitlab.com".to_string())
            } else {
                Some(value)
            };
            changed = true;
        }
    }

    // --- project_id ---
    let id_from_env = std::env::var("GITLAB_PROJECT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let id_in_config = config.project_id.as_deref().unwrap_or("").trim().is_empty();

    if id_from_env.is_none() && id_in_config {
        println!("🔢 No GitLab Project ID found in config or environment.");
        let value = prompt_required("Please enter your GitLab Project ID");
        config.project_id = Some(value);
        changed = true;
    }

    if changed {
        save_config_async(config).await;
        println!("✅ Config saved to config.json!\n");
    }
}

/// Service name used consistently for all keyring read/write operations.
const KEYRING_SERVICE: &str = "gitlab-tracker";
/// Account name used consistently for all keyring read/write operations.
const KEYRING_ACCOUNT: &str = "gitlab_token";
/// Legacy service name used before the naming was unified (underscore variant).
/// Only used during the one-time migration in `migrate_legacy_keyring_entry`.
const KEYRING_SERVICE_LEGACY: &str = "gitlab_tracker";

/// Migrates a token stored under the legacy keyring service name (`gitlab_tracker`)
/// to the canonical one (`gitlab-tracker`), then deletes the legacy entry.
///
/// This is a silent, one-time operation: if the canonical entry already exists,
/// or if no legacy entry is found, nothing happens.
pub fn migrate_legacy_keyring_entry() {
    // Skip migration if the canonical entry already holds a token.
    if let Ok(canonical) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        if let Ok(existing) = canonical.get_password() {
            if !existing.trim().is_empty() {
                tracing::debug!(
                    "Canonical keyring entry already populated — skipping legacy migration"
                );
                return;
            }
        }
    }

    // Attempt to read from the legacy entry.
    let legacy_token = match keyring::Entry::new(KEYRING_SERVICE_LEGACY, KEYRING_ACCOUNT) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) if !pwd.trim().is_empty() => Zeroizing::new(pwd.trim().to_string()),
            _ => return,
        },
        Err(_) => return,
    };

    tracing::info!(
        from = KEYRING_SERVICE_LEGACY,
        to = KEYRING_SERVICE,
        "Migrating token from legacy keyring entry to canonical entry"
    );

    // Write to the canonical entry.
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(&legacy_token) {
                tracing::error!(error = %e, "Failed to write token to canonical keyring entry during migration");
                return;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to open canonical keyring entry during migration");
            return;
        }
    }

    // Delete the legacy entry now that the token is safely copied.
    match keyring::Entry::new(KEYRING_SERVICE_LEGACY, KEYRING_ACCOUNT) {
        Ok(entry) => {
            if let Err(e) = entry.delete_password() {
                tracing::warn!(error = %e, "Token migrated but failed to delete legacy keyring entry");
            } else {
                tracing::info!("Legacy keyring entry deleted successfully");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Token migrated but could not open legacy keyring entry for deletion");
        }
    }
}

/// Resolves the GitLab PAT using the following priority chain:
///   1. `GITLAB_TOKEN` environment variable
///   2. OS keyring (via the `keyring` crate)
///   3. Interactive prompt (hidden input via `rpassword`, no terminal echo)
///
/// The returned value is wrapped in `Zeroizing<String>` so the secret bytes
/// are overwritten in memory as soon as the caller drops the value.
///
/// # Panics
/// Panics if no token is provided — the program cannot function without one.
pub fn get_or_prompt_token() -> Zeroizing<String> {
    // 1. Environment variable takes priority (CI / dotenv workflows).
    if let Ok(tok) = std::env::var("GITLAB_TOKEN") {
        let tok = Zeroizing::new(tok);
        if !tok.trim().is_empty() {
            tracing::info!("GITLAB_TOKEN loaded from environment variable");
            return Zeroizing::new(tok.trim().to_string());
        }
    }

    // 2. Try the OS keyring.
    tracing::debug!(
        service = KEYRING_SERVICE,
        account = KEYRING_ACCOUNT,
        "Attempting to read token from OS keyring"
    );
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        Ok(entry) => match entry.get_password() {
            Ok(password) => {
                let password = Zeroizing::new(password);
                if !password.trim().is_empty() {
                    tracing::info!("GITLAB_TOKEN loaded from OS keyring");
                    return Zeroizing::new(password.trim().to_string());
                }
                tracing::warn!("Keyring entry found but token is empty — falling back to prompt");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read token from OS keyring — falling back to prompt");
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "Failed to open keyring entry — falling back to prompt");
        }
    }

    // 3. Interactive prompt as a last resort.
    // `rpassword` disables terminal echo so the PAT never appears on screen
    // and cannot end up in shell history, screen recordings or logs.
    println!("🔑 No GITLAB_TOKEN found in environment or system Keyring.");
    match rpassword::prompt_password("Please enter your GitLab Personal Access Token: ") {
        Ok(raw) => {
            let token = Zeroizing::new(raw);
            if !token.trim().is_empty() {
                let token = Zeroizing::new(token.trim().to_string());
                tracing::debug!(
                    service = KEYRING_SERVICE,
                    account = KEYRING_ACCOUNT,
                    "Saving token to OS keyring"
                );
                match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
                    Ok(entry) => match entry.set_password(&token) {
                        Ok(_) => {
                            tracing::info!("Token successfully saved to OS keyring");
                            println!("✅ Token securely saved to OS Keyring!\n");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to save token to OS keyring");
                        }
                    },
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to open keyring entry for writing");
                    }
                }
                return token;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to read token from prompt");
        }
    }

    // Panic is intentional here: without a token the program cannot function.
    // In a future refactor this should become a Result<String, TokenError>.
    panic!("Error: Personal Access Token is required to run gitlab_tracker.");
}

pub fn get_save_dir() -> Option<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "gitlab-tracker", "gitlab-tracker")?;
    Some(project_dirs.config_dir().to_path_buf())
}

pub async fn load_or_create_config_async() -> AppConfig {
    if let Some(config_dir) = get_save_dir() {
        let config_path = config_dir.join("config.json");
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                if let Ok(env_branches) = std::env::var("DEFAULT_BRANCHES") {
                    config.default_branches = env_branches
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Ok(env_prefixes) = std::env::var("TABLE_LABEL_PREFIXES") {
                    config.table_label_prefixes = env_prefixes
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Ok(val) = std::env::var("ACTIVITY_RECENT_DAYS") {
                    if let Ok(days) = val.trim().parse::<u64>() {
                        config.activity_recent_days = days;
                    }
                }
                if let Ok(val) = std::env::var("ACTIVITY_STALE_DAYS") {
                    if let Ok(days) = val.trim().parse::<u64>() {
                        config.activity_stale_days = days;
                    }
                }
                return config;
            }
        }

        let default_config = AppConfig::default();
        let _ = tokio::fs::create_dir_all(&config_dir).await;
        if let Ok(json) = serde_json::to_string_pretty(&default_config) {
            let _ = tokio::fs::write(config_path, json).await;
        }
        return default_config;
    }
    AppConfig::default()
}

pub async fn load_state_async() -> (Vec<SavedMr>, Vec<String>, HashMap<String, HashSet<String>>) {
    if let Some(config_dir) = get_save_dir() {
        let path = config_dir.join("tracker_state.json");
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(state) = serde_json::from_str::<SavedState>(&content) {
                return (state.mrs, state.branches, state.last_known_branches);
            }
        }
    }
    (vec![], vec![], HashMap::new())
}

pub async fn save_state_async(
    mrs: &[TrackedMr],
    branches: &[String],
    last_known_branches: &HashMap<String, HashSet<String>>,
) {
    let state = SavedState {
        mrs: mrs
            .iter()
            .map(|m| SavedMr {
                id: m.id.clone(),
                title: m.title.clone(),
                sha: m.sha.clone(),
                found_branches: match &m.status {
                    MrStatus::MergedIn(set) => set.clone(),
                    _ => HashSet::new(),
                },
                description: Some(m.description.clone()),
                author: Some(m.author.clone()),
                assignee: Some(m.assignee.clone()),
                reviewers: m.reviewers.clone(),
                milestone: Some(m.milestone.clone()),
                milestone_due_date: m.milestone_due_date.clone(),
                web_url: Some(m.web_url.clone()),
                labels: Some(m.labels.clone()),
                updated_at: m.updated_at.clone(),
                source_branch: Some(m.source_branch.clone()),
                target_branch: Some(m.target_branch.clone()),
                state: m.state.clone(),
                merged_by: m.merged_by.clone(),
                merged_at: m.merged_at.clone(),
                pipelines: m.pipelines.clone(),
                user_notes_count: m.user_notes_count,
                flagged: m.flagged,
                #[cfg(feature = "redmine")]
                linked_ticket: m.linked_ticket.clone(),
            })
            .collect(),
        branches: branches.to_vec(),
        last_known_branches: last_known_branches.clone(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        if let Some(config_dir) = get_save_dir() {
            let path = config_dir.join("tracker_state.json");
            let _ = tokio::fs::create_dir_all(&config_dir).await;
            let _ = tokio::fs::write(path, json).await;
        }
    }
}
