use crate::config::AppConfig;
use crate::models::{MrStatus, SavedMr, SavedState, TrackedMr};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;

pub fn get_or_prompt_token() -> String {
    if let Ok(tok) = std::env::var("GITLAB_TOKEN") {
        if !tok.trim().is_empty() {
            return tok.trim().to_string();
        }
    }

    if let Ok(entry) = keyring::Entry::new("gitlab_tracker", "gitlab_token") {
        if let Ok(password) = entry.get_password() {
            if !password.trim().is_empty() {
                return password.trim().to_string();
            }
        }
    }

    println!("🔑 No GITLAB_TOKEN found in environment or system Keyring.");
    print!("Please enter your GitLab Personal Access Token: ");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() {
        let token = input.trim().to_string();
        if !token.is_empty() {
            if let Ok(entry) = keyring::Entry::new("gitlab-tracker", "gitlab_token") {
                let _ = entry.set_password(&token);
                println!("✅ Token securely saved to OS Keyring!\n");
            }
            return token;
        }
    }
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
                milestone: Some(m.milestone.clone()),
                web_url: Some(m.web_url.clone()),
                labels: Some(m.labels.clone()),
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
