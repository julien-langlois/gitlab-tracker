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

/// A token given in the environment, and the instance it belongs to.
///
/// The token is only used for the instance its URL variable names (e.g.
/// `GITLAB_TOKEN` with `GITLAB_URL`): sent to every configured instance, a token
/// for one server would reach the others. Without the URL variable, or with
/// another URL, the environment token is ignored.
#[derive(Clone, Copy)]
pub struct EnvToken<'a> {
    /// Variable holding the token (e.g. `GITLAB_TOKEN`).
    pub var: &'a str,
    /// Variable naming the instance the token is for (e.g. `GITLAB_URL`).
    pub url_var: &'a str,
    /// The instance a token is wanted for.
    pub instance_url: &'a str,
}

impl EnvToken<'_> {
    /// The trimmed token, when set, non-empty and meant for `instance_url`.
    pub fn read(&self) -> Option<Zeroizing<String>> {
        let token = Zeroizing::new(std::env::var(self.var).ok()?.trim().to_string());
        if token.is_empty() {
            return None;
        }
        let scoped_url = std::env::var(self.url_var).unwrap_or_default();
        if same_instance(&scoped_url, self.instance_url) {
            Some(token)
        } else {
            tracing::warn!(
                var = self.var,
                url_var = self.url_var,
                instance = self.instance_url,
                "Token in the environment ignored: {} does not name this instance",
                self.url_var
            );
            None
        }
    }
}

/// Whether a token can be sent to `url` without crossing the network in clear:
/// `https://`, or `http://` to this machine (`localhost`, `127.0.0.0/8`, `[::1]`).
pub fn is_secure_url(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    if url.starts_with("https://") {
        return true;
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split_inclusive(']').next().unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    host == "localhost"
        || host == "[::1]"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether two instance URLs are the same, ignoring surrounding spaces, a trailing
/// `/` and case.
fn same_instance(a: &str, b: &str) -> bool {
    let norm = |u: &str| u.trim().trim_end_matches('/').to_ascii_lowercase();
    !norm(a).is_empty() && norm(a) == norm(b)
}

/// Whether a token is available without asking: in the environment (see
/// [`EnvToken`]) or in the OS keyring entry. Blocks on the keyring (D-Bus): call
/// it off the UI thread.
pub fn has_secret(env: EnvToken<'_>, keyring_service: &str, keyring_account: &str) -> bool {
    env.read().is_some()
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
    /// Token in the environment, checked first (e.g. `GITLAB_TOKEN`), for CI / dotenv.
    pub env: EnvToken<'a>,
    /// OS keyring service name.
    pub keyring_service: &'a str,
    /// OS keyring account — per instance URL, so several instances coexist.
    pub keyring_account: &'a str,
    /// Human name used in messages and in the prompt (e.g. "GitLab Personal Access Token").
    pub label: &'a str,
    /// Extra line printed before the prompt (e.g. how to skip it).
    pub prompt_hint: Option<&'a str>,
}

/// Returns the token from, in order: the environment, the OS keyring entry, then a hidden
/// prompt (no echo; the answer is saved to the keyring). `None` when the prompt is
/// left empty or cannot be read, or prompting is off ([`set_interactive`]). The
/// value is zeroized when dropped.
pub fn resolve_secret(source: &SecretSource<'_>) -> Option<Zeroizing<String>> {
    let SecretSource {
        env,
        keyring_service,
        keyring_account,
        label,
        prompt_hint,
    } = source;

    let insecure = !is_secure_url(env.instance_url);
    if insecure {
        tracing::warn!(
            instance = env.instance_url,
            "{label} would be sent over plain http: use https://"
        );
    }

    if let Some(token) = env.read() {
        tracing::info!(
            env_var = env.var,
            "{label} loaded from environment variable"
        );
        return Some(token);
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
    println!("🔑 No {label} found in {} or the OS keyring.", env.var);
    if insecure {
        println!(
            "⚠️  {} uses http://: the token would cross the network unencrypted. Prefer https://.",
            env.instance_url
        );
    }
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
    fn only_https_or_this_machine_is_secure() {
        for url in [
            "https://git.a.io",
            " HTTPS://git.a.io/x",
            "http://localhost:8080",
            "http://127.0.0.1/gitlab",
            "http://[::1]:3000",
        ] {
            assert!(is_secure_url(url), "{url}");
        }
        for url in [
            "http://git.a.io",
            "http://localhost.evil.io",
            "http://127.0.0.1.evil.io",
            "http://localhost@evil.io",
            "ftp://git.a.io",
            "git.a.io",
        ] {
            assert!(!is_secure_url(url), "{url}");
        }
    }

    #[test]
    fn environment_token_only_serves_the_instance_its_url_names() {
        // Unique names: tests share the process environment. Only the environment
        // branch is tested — the next ones need the OS keyring and a terminal.
        const VAR: &str = "GT_SECRETS_TEST_TOKEN";
        const URL_VAR: &str = "GT_SECRETS_TEST_URL";
        let env = |instance_url| EnvToken {
            var: VAR,
            url_var: URL_VAR,
            instance_url,
        };
        std::env::set_var(VAR, "  glpat-123\n");

        // No URL named: the token is for no instance in particular, so none.
        std::env::remove_var(URL_VAR);
        assert!(env("https://git.a.io").read().is_none());

        std::env::set_var(URL_VAR, "https://Git.A.io/");
        assert_eq!(
            env("https://git.a.io")
                .read()
                .as_deref()
                .map(String::as_str),
            Some("glpat-123")
        );
        assert!(env("https://git.b.io").read().is_none());

        set_interactive(false);
        let token = resolve_secret(&SecretSource {
            env: env("https://git.a.io"),
            keyring_service: "gitlab-tracker-test",
            keyring_account: "never-read",
            label: "Test token",
            prompt_hint: None,
        });
        std::env::remove_var(VAR);
        std::env::remove_var(URL_VAR);
        assert_eq!(token.as_deref().map(String::as_str), Some("glpat-123"));
    }
}
