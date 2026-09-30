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

/// Retrieves the Redmine API token for a specific Redmine instance URL:
/// `REDMINE_TOKEN`, then the OS keyring entry for that URL, then a hidden prompt
/// (see [`gitlab_tracker_core::secrets::resolve_secret`]).
///
/// Returns `None` when the user leaves the prompt empty, which keeps the Redmine
/// feature inactive for this session.
pub fn get_or_prompt_token(redmine_url: &str) -> Option<Zeroizing<String>> {
    let hint = format!("For {redmine_url} — leave empty to disable Redmine for this project.");
    gitlab_tracker_core::secrets::resolve_secret(&gitlab_tracker_core::secrets::SecretSource {
        env_var: "REDMINE_TOKEN",
        keyring_service: KEYRING_SERVICE,
        keyring_account: &account_for(redmine_url),
        label: "Redmine API token",
        prompt_hint: Some(&hint),
    })
}

/// Removes the stored Redmine token for a specific Redmine instance from the OS keyring.
///
/// Returns `true` if the entry was deleted successfully, `false` otherwise.
pub fn delete_token(redmine_url: &str) -> bool {
    keyring::Entry::new(KEYRING_SERVICE, &account_for(redmine_url))
        .and_then(|e| e.delete_credential())
        .is_ok()
}
