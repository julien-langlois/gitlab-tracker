use zeroize::Zeroizing;

/// Keyring service name for the Redmine token — distinct from the GitLab one.
const KEYRING_SERVICE: &str = "gitlab-tracker-redmine";

/// Derives a stable, per-instance keyring account name from the Redmine URL.
///
/// Using the URL as the account key enables multi-tenant setups: each Redmine
/// instance stores its token independently so switching projects never clobbers
/// another instance's credentials.
///
/// Example: `"https://redmine.example.com"` → `"redmine_token::https://redmine.example.com"`
fn account_for(redmine_url: &str) -> String {
    format!("redmine_token::{}", redmine_url.trim_end_matches('/'))
}

/// Retrieves the Redmine API token for a specific Redmine instance URL using
/// the following priority chain:
///
/// 1. `REDMINE_TOKEN` environment variable (shared across all instances; useful for CI).
/// 2. OS keyring entry keyed by `redmine_url` (per-instance, multi-tenant safe).
/// 3. Interactive hidden prompt (`rpassword`), then persisted to the keyring.
///
/// Returns `None` when the user explicitly skips the prompt (empty input),
/// which causes the Redmine feature to stay inactive for this session.
/// The token is wrapped in [`Zeroizing`] to erase it from memory on drop.
pub fn get_or_prompt_token(redmine_url: &str) -> Option<Zeroizing<String>> {
    // 1. Environment variable — highest priority (CI / dotenv workflows).
    if let Ok(tok) = std::env::var("REDMINE_TOKEN") {
        let tok = Zeroizing::new(tok.trim().to_string());
        if !tok.is_empty() {
            tracing::info!("REDMINE_TOKEN loaded from environment variable");
            return Some(tok);
        }
    }

    let account = account_for(redmine_url);

    // 2. OS keyring — keyed per Redmine instance URL.
    match keyring::Entry::new(KEYRING_SERVICE, &account) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) => {
                let pwd = Zeroizing::new(pwd.trim().to_string());
                if !pwd.is_empty() {
                    tracing::info!(url = %redmine_url, "REDMINE_TOKEN loaded from OS keyring");
                    return Some(pwd);
                }
                tracing::debug!("Redmine keyring entry found but token is empty");
            }
            Err(e) => {
                tracing::debug!(error = %e, "No Redmine token in OS keyring");
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "Failed to open Redmine keyring entry");
        }
    }

    // 3. Interactive prompt — the user may leave it empty to skip.
    println!("🔑 No REDMINE_TOKEN found for {redmine_url}.");
    println!("   Leave empty to disable Redmine integration for this project.");
    match rpassword::prompt_password("Redmine API token: ") {
        Ok(raw) => {
            let token = Zeroizing::new(raw.trim().to_string());
            if token.is_empty() {
                tracing::info!("Redmine integration disabled — no token provided");
                return None;
            }
            // Persist to keyring keyed by this Redmine instance URL.
            match keyring::Entry::new(KEYRING_SERVICE, &account) {
                Ok(entry) => match entry.set_password(&token) {
                    Ok(_) => {
                        tracing::info!(url = %redmine_url, "Redmine token saved to OS keyring");
                        println!("✅ Redmine token securely saved to OS Keyring!\n");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to save Redmine token to OS keyring");
                    }
                },
                Err(e) => {
                    tracing::error!(error = %e, "Failed to open Redmine keyring entry for writing");
                }
            }
            Some(token)
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to read Redmine token from prompt");
            None
        }
    }
}

/// Removes the stored Redmine token for a specific Redmine instance from the OS keyring.
///
/// Returns `true` if the entry was deleted successfully, `false` otherwise.
pub fn delete_token(redmine_url: &str) -> bool {
    keyring::Entry::new(KEYRING_SERVICE, &account_for(redmine_url))
        .and_then(|e| e.delete_credential())
        .is_ok()
}
