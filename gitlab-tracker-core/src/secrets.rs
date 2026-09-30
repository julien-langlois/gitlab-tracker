//! Resolving an API token shared by the orchestrator and tracker plugins
//! (feature `secrets`): environment variable, then OS keyring, then a hidden
//! interactive prompt whose answer is saved to the keyring.

use zeroize::Zeroizing;

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
/// left empty or cannot be read. The value is zeroized when dropped.
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
