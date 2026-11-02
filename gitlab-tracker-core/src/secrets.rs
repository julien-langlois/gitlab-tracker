//! Resolving an API token shared by the orchestrator and tracker plugins
//! (feature `secrets`): environment variable, then OS keyring, then a hidden
//! interactive prompt whose answer is saved to the keyring.

use std::sync::atomic::{AtomicBool, Ordering};

use zeroize::Zeroizing;

/// Whether [`resolve_secret`] may ask in the terminal. The desktop window turns it
/// off: it has no terminal to prompt in, and asks for missing tokens itself.
static INTERACTIVE: AtomicBool = AtomicBool::new(true);

/// Lets [`resolve_secret`] prompt in the terminal (`true`, the default) or not.
pub fn set_interactive(on: bool) {
    INTERACTIVE.store(on, Ordering::Relaxed);
}

/// Whether a token is available without asking: in `env_var` or in the OS
/// keyring entry. Blocks on the keyring (D-Bus): call it off the UI thread.
pub fn has_secret(env_var: &str, keyring_service: &str, keyring_account: &str) -> bool {
    std::env::var(env_var).is_ok_and(|v| !v.trim().is_empty())
        || keyring::Entry::new(keyring_service, keyring_account)
            .and_then(|e| e.get_password())
            .is_ok_and(|v| !v.trim().is_empty())
}

/// Stores `token` (trimmed) in the OS keyring entry; the error says why not.
pub fn store_secret(
    keyring_service: &str,
    keyring_account: &str,
    token: &str,
) -> Result<(), String> {
    keyring::Entry::new(keyring_service, keyring_account)
        .and_then(|e| e.set_password(token.trim()))
        .map_err(|e| e.to_string())
}

/// Where a token comes from, and how to ask for it.
pub struct SecretSource<'a> {
    /// Environment variable checked first (e.g. `GITLAB_TOKEN`), for CI / dotenv.
    pub env_var: &'a str,
    /// OS keyring service name.
    pub keyring_service: &'a str,
    /// OS keyring account — per instance URL, so several instances coexist.
    pub keyring_account: &'a str,
    /// Human name used in messages and in the prompt (e.g. "GitLab Personal Access Token").
    pub label: &'a str,
    /// Extra line printed before the prompt (e.g. how to skip it).
    pub prompt_hint: Option<&'a str>,
}

/// Returns the token from, in order: `env_var`, the OS keyring entry, then a hidden
/// prompt (no echo; the answer is saved to the keyring). `None` when the prompt is
/// left empty or cannot be read, or prompting is off ([`set_interactive`]). The
/// value is zeroized when dropped.
pub fn resolve_secret(source: &SecretSource<'_>) -> Option<Zeroizing<String>> {
    let SecretSource {
        env_var,
        keyring_service,
        keyring_account,
        label,
        prompt_hint,
    } = source;

    if let Ok(raw) = std::env::var(env_var) {
        let token = Zeroizing::new(raw.trim().to_string());
        if !token.is_empty() {
            tracing::info!(env_var, "{label} loaded from environment variable");
            return Some(token);
        }
    }

    match keyring::Entry::new(keyring_service, keyring_account).and_then(|e| e.get_password()) {
        Ok(raw) => {
            let token = Zeroizing::new(raw.trim().to_string());
            if !token.is_empty() {
                tracing::info!(account = keyring_account, "{label} loaded from OS keyring");
                return Some(token);
            }
            tracing::debug!(account = keyring_account, "Empty {label} in OS keyring");
        }
        Err(e) => {
            tracing::debug!(error = %e, account = keyring_account, "No {label} in OS keyring")
        }
    }

    if !INTERACTIVE.load(Ordering::Relaxed) {
        tracing::info!(
            account = keyring_account,
            "No {label}, and no terminal to ask in"
        );
        return None;
    }
    println!("🔑 No {label} found in {env_var} or the OS keyring.");
    if let Some(hint) = prompt_hint {
        println!("   {hint}");
    }
    let raw = match rpassword::prompt_password(format!("{label}: ")) {
        Ok(raw) => Zeroizing::new(raw),
        Err(e) => {
            tracing::error!(error = %e, "Failed to read {label} from prompt");
            return None;
        }
    };
    let token = Zeroizing::new(raw.trim().to_string());
    if token.is_empty() {
        return None;
    }
    match keyring::Entry::new(keyring_service, keyring_account).and_then(|e| e.set_password(&token))
    {
        Ok(()) => {
            tracing::info!(account = keyring_account, "{label} saved to OS keyring");
            println!("✅ {label} securely saved to the OS keyring.\n");
        }
        Err(e) => tracing::error!(error = %e, "Failed to save {label} to OS keyring"),
    }
    Some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_variable_wins_and_is_trimmed() {
        // Unique name: tests share the process environment. Only the environment
        // branch is tested — the next ones need the OS keyring and a terminal.
        const VAR: &str = "GT_SECRETS_TEST_TOKEN";
        std::env::set_var(VAR, "  glpat-123\n");
        let token = resolve_secret(&SecretSource {
            env_var: VAR,
            keyring_service: "gitlab-tracker-test",
            keyring_account: "never-read",
            label: "Test token",
            prompt_hint: None,
        });
        std::env::remove_var(VAR);
        assert_eq!(token.as_deref().map(String::as_str), Some("glpat-123"));
    }
}
