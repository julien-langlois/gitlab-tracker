//! The GitLab token: the interactive prompt, the OS keyring, legacy keyring entries.

use super::*;

/// Prompts the user interactively for a required config value (read from stdin).
pub(super) fn prompt_required(label: &str) -> String {
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

/// Service name used consistently for all keyring read/write operations.
pub(super) const KEYRING_SERVICE: &str = "gitlab-tracker";
/// Legacy account name — flat, instance-agnostic key used before multi-tenant support.
/// Only used during the one-time migrations in `migrate_legacy_keyring_entry`.
pub(super) const KEYRING_ACCOUNT_LEGACY: &str = "gitlab_token";
/// Legacy service name used before the naming was unified (underscore variant).
/// Only used during the one-time migration in `migrate_legacy_keyring_entry`.
pub(super) const KEYRING_SERVICE_LEGACY: &str = "gitlab_tracker";

/// Derives a stable, per-instance keyring account name from the GitLab instance URL.
///
/// Using the URL as the account key enables multi-tenant setups: each GitLab
/// instance stores its token independently so switching projects never clobbers
/// another instance's credentials.
///
/// Example: `"https://gitlab.example.com"` → `"gitlab_token::https://gitlab.example.com"`
pub(super) fn account_for(gitlab_url: &str) -> String {
    format!("gitlab_token::{}", gitlab_url.trim_end_matches('/'))
}

/// Migrates tokens stored under legacy keyring keys to the current per-instance key.
///
/// Two migration steps are performed silently in order:
///   1. `gitlab_tracker` / `gitlab_token`  →  `gitlab-tracker` / `gitlab_token`  (service rename)
///   2. `gitlab-tracker` / `gitlab_token`  →  `gitlab-tracker` / `gitlab_token::<url>` (multi-tenant)
///
/// Each step is skipped if the target already contains a token, or if the source is empty.
/// This is a one-time, non-destructive operation safe to run on every startup.
pub fn migrate_legacy_keyring_entry(gitlab_url: &str) {
    let canonical_account = account_for(gitlab_url);

    // ── Step 1: service name rename (gitlab_tracker → gitlab-tracker) ────────
    // Skip if the intermediate flat canonical entry already exists.
    let flat_token = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) if !pwd.trim().is_empty() => Some(Zeroizing::new(pwd.trim().to_string())),
            _ => None,
        },
        Err(_) => None,
    };

    if flat_token.is_none() {
        // Try to pull from the very legacy entry (underscore service name).
        if let Ok(legacy_entry) =
            keyring::Entry::new(KEYRING_SERVICE_LEGACY, KEYRING_ACCOUNT_LEGACY)
        {
            if let Ok(pwd) = legacy_entry.get_password() {
                let pwd = Zeroizing::new(pwd.trim().to_string());
                if !pwd.is_empty() {
                    tracing::info!(
                        from = KEYRING_SERVICE_LEGACY,
                        to = KEYRING_SERVICE,
                        "Migrating token from legacy service name to canonical service name"
                    );
                    if let Ok(canonical_flat) =
                        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY)
                    {
                        if let Err(e) = canonical_flat.set_password(&pwd) {
                            tracing::error!(error = %e, "Failed to write to canonical flat entry during step-1 migration");
                        } else {
                            // Clean up the legacy service entry.
                            if let Err(e) = legacy_entry.delete_credential() {
                                tracing::warn!(error = %e, "Step-1 migration succeeded but failed to delete legacy entry");
                            } else {
                                tracing::info!(
                                    "Legacy keyring entry (step 1) deleted successfully"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Step 2: flat account → per-instance URL account ──────────────────────
    // Skip if the per-instance entry already holds a token.
    if let Ok(per_instance) = keyring::Entry::new(KEYRING_SERVICE, &canonical_account) {
        if let Ok(existing) = per_instance.get_password() {
            if !existing.trim().is_empty() {
                tracing::debug!(
                    account = %canonical_account,
                    "Per-instance keyring entry already populated — skipping step-2 migration"
                );
                return;
            }
        }
    }

    // Read the flat legacy token (written by step 1 or already present).
    let flat_token = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) if !pwd.trim().is_empty() => Zeroizing::new(pwd.trim().to_string()),
            _ => return,
        },
        Err(_) => return,
    };

    tracing::info!(
        url = %gitlab_url,
        from = KEYRING_ACCOUNT_LEGACY,
        to = %canonical_account,
        "Migrating token from flat account key to per-instance account key"
    );

    // Write to the per-instance entry.
    match keyring::Entry::new(KEYRING_SERVICE, &canonical_account) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(&flat_token) {
                tracing::error!(error = %e, "Failed to write to per-instance keyring entry during step-2 migration");
                return;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to open per-instance keyring entry during step-2 migration");
            return;
        }
    }

    // Delete the flat legacy entry now that the token is safely copied.
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY) {
        Ok(entry) => {
            if let Err(e) = entry.delete_credential() {
                tracing::warn!(error = %e, "Step-2 migration succeeded but failed to delete flat legacy entry");
            } else {
                tracing::info!("Flat legacy keyring entry (step 2) deleted successfully");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Step-2 migration succeeded but could not open flat legacy entry for deletion: {e}");
        }
    }
}

/// Resolves the GitLab PAT for a specific GitLab instance: `GITLAB_TOKEN`, then the
/// OS keyring entry for that URL, then a hidden prompt saved to the keyring
/// (see [`gitlab_tracker_core::secrets::resolve_secret`]). `None` when no token is
/// given — the caller stops, as nothing works without one.
pub fn get_or_prompt_token(gitlab_url: &str) -> Option<Zeroizing<String>> {
    gitlab_tracker_core::secrets::resolve_secret(&gitlab_tracker_core::secrets::SecretSource {
        env: gitlab_tracker_core::secrets::EnvToken {
            var: "GITLAB_TOKEN",
            url_var: "GITLAB_URL",
            instance_url: gitlab_url,
        },
        keyring_service: KEYRING_SERVICE,
        keyring_account: &account_for(gitlab_url),
        label: "GitLab Personal Access Token",
        prompt_hint: None,
    })
}
