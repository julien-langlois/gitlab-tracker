//! Checking and saving a project: token lookups, the GitLab and tracker checks, the keyring, missing tokens.

use super::*;

/// Looks up whether `url` has a token of `kind`, off the runtime (the keyring blocks).
pub(super) async fn lookup(
    known: &Arc<Mutex<Known>>,
    kind: Kind,
    url: String,
    ctx: &egui::Context,
) {
    let probe = url.clone();
    let found = tokio::task::spawn_blocking(move || match kind {
        Kind::Gitlab => crate::storage::has_gitlab_token(&probe),
        #[cfg(feature = "redmine")]
        Kind::Redmine => gitlab_tracker_redmine::keyring::has_token(&probe),
        #[cfg(not(feature = "redmine"))]
        Kind::Redmine => false,
    })
    .await
    .unwrap_or(false);
    if let Ok(mut known) = known.lock() {
        known.pending.remove(&(kind, url.clone()));
        known.found.insert((kind, url), found);
    }
    ctx.request_repaint();
}

/// The URL as the keyring keys it: trimmed, without its trailing `/`.
pub(super) fn normalize(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

#[cfg(feature = "redmine")]
pub(super) fn is_redmine(provider: &str) -> bool {
    provider.eq_ignore_ascii_case("redmine")
}

/// A checked new project and its tokens.
pub(super) struct NewProject {
    pub(super) checked: Checked,
    pub(super) token: Zeroizing<String>,
    pub(super) redmine_url: String,
    pub(super) redmine_token: Zeroizing<String>,
    pub(super) make_active: bool,
}

/// The token typed, else the one stored for `url`.
pub(super) async fn gitlab_token(url: &str, typed: Zeroizing<String>) -> Option<Zeroizing<String>> {
    if !typed.trim().is_empty() {
        return Some(Zeroizing::new(typed.trim().to_string()));
    }
    let url = url.to_string();
    tokio::task::spawn_blocking(move || crate::storage::stored_gitlab_token(&url))
        .await
        .ok()
        .flatten()
}

/// A project and its tokens GitLab (and Redmine) accepted.
pub(super) struct Verified {
    pub(super) info: crate::gitlab::ProjectInfo,
    pub(super) token: Zeroizing<String>,
    /// The token was typed (else it is the saved one).
    pub(super) typed: bool,
    /// Redmine's URL, and its key when typed.
    pub(super) tracker: Option<(String, Option<Zeroizing<String>>)>,
    pub(super) username: Option<String>,
}

/// Asks GitLab whether the token sees the project, and Redmine whether it takes
/// the key; saves nothing. The problems are the form's to show.
pub(super) async fn verify(
    checked: &Checked,
    token: Zeroizing<String>,
    redmine_url: String,
    redmine_token: Zeroizing<String>,
) -> Result<Verified, Vec<Problem>> {
    let typed = !token.trim().is_empty();
    let Some(token) = gitlab_token(&checked.url, token).await else {
        return Err(vec![Problem::remote(
            Some(Slot::Token),
            tl("No token is saved for this GitLab instance yet: type one."),
        )]);
    };
    let info = crate::gitlab::check_project(&checked.url, &token, &checked.project_id)
        .await
        .map_err(|e| vec![check_error(&e)])?;

    #[cfg(feature = "redmine")]
    let tracker = if redmine_url.is_empty() {
        None
    } else {
        let url = redmine_url.clone();
        let typed_redmine = !redmine_token.trim().is_empty();
        let key = if typed_redmine {
            Some(Zeroizing::new(redmine_token.trim().to_string()))
        } else {
            let url = url.clone();
            tokio::task::spawn_blocking(move || {
                gitlab_tracker_redmine::keyring::has_token(&url)
                    .then(|| gitlab_tracker_redmine::get_or_prompt_token(&url))
                    .flatten()
            })
            .await
            .ok()
            .flatten()
        };
        let Some(key) = key else {
            return Err(vec![Problem::remote(Some(Slot::RedmineToken), tl("No API key is saved for this Redmine instance yet: type one, or leave the Redmine URL empty."))]);
        };
        gitlab_tracker_redmine::check_token(&url, &key)
            .await
            .map_err(|e| vec![redmine_error(&e)])?;
        Some((url, typed_redmine.then_some(key)))
    };
    #[cfg(not(feature = "redmine"))]
    let tracker: Option<(String, Option<Zeroizing<String>>)> = {
        let _ = (redmine_url, redmine_token);
        None
    };
    let username = crate::gitlab::current_username(&checked.url, &token).await;
    Ok(Verified {
        info,
        token,
        typed,
        tracker,
        username,
    })
}

/// Checks the project and the tokens, without saving: what was found.
pub(super) async fn check_new(request: NewProject) -> Result<Report, Vec<Problem>> {
    let NewProject {
        checked,
        token,
        redmine_url,
        redmine_token,
        ..
    } = request;
    let verified = verify(&checked, token, redmine_url, redmine_token).await?;
    Ok(Report {
        name: verified.info.name,
        path: verified.info.path_with_namespace,
        default_branch: verified.info.default_branch,
        username: verified.username,
        saved_token: !verified.typed,
        redmine: verified.tracker.map(|(url, _)| url),
    })
}

/// Checks the project and the tokens, saves the tokens typed to the keyring and
/// the project to `projects.toml`. The problems are the form's to show.
pub(super) async fn add_new(request: NewProject) -> Result<ProjectEntry, Vec<Problem>> {
    let NewProject {
        checked,
        token,
        redmine_url,
        redmine_token,
        make_active,
    } = request;
    let Verified {
        info,
        token,
        typed,
        tracker,
        username,
    } = verify(&checked, token, redmine_url, redmine_token).await?;

    // Saved once both are known good: a refused token never reaches the keyring.
    let url = checked.url.clone();
    let save = typed.then(|| token.clone());
    let redmine_save = tracker
        .as_ref()
        .and_then(|(u, k)| k.clone().map(|k| (u.clone(), k)));
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(token) = save {
            crate::storage::save_gitlab_token(&url, &token)?;
        }
        #[cfg(feature = "redmine")]
        if let Some((url, key)) = redmine_save {
            gitlab_tracker_redmine::keyring::save_token(&url, &key)?;
        }
        #[cfg(not(feature = "redmine"))]
        let _ = redmine_save;
        Ok(())
    })
    .await
    .map_err(|e| vec![Problem::remote(None, e.to_string())])?
    .map_err(|e| vec![keyring_error(&e)])?;

    let mut entry = entry_for(checked, &info, tracker.map(|(url, _)| url));
    entry.gitlab_username = username;
    // Already configured (same instance and project): open it as it is.
    let projects = crate::storage::load_projects_toml().await.projects;
    if let Some(existing) = projects.into_iter().find(|p| {
        p.gitlab_url.trim_end_matches('/') == entry.gitlab_url
            && (p.project_id == entry.project_id
                || p.project_id == info.path_with_namespace
                || p.project_id == info.id.to_string())
    }) {
        return Ok(existing);
    }
    crate::storage::add_project(entry.clone(), make_active).await;
    Ok(entry)
}

/// The keyring refused to save a token.
pub(super) fn keyring_error(e: &str) -> Problem {
    Problem::remote(
        None,
        tf(
            "The token could not be saved in the keyring: {error}",
            &[("error", e)],
        ),
    )
}

/// Why GitLab refused, in the window's language, on the field it is about.
pub(super) fn check_error(e: &crate::gitlab::ProjectCheckError) -> Problem {
    use crate::gitlab::ProjectCheckError as E;
    match e {
        E::Token => Problem::remote(
            Some(Slot::Token),
            tl("GitLab refused the token: it may be expired, or lack the read_api scope."),
        ),
        E::NotFound => Problem::remote(
            Some(Slot::Project),
            tl("No such project on this instance, or the token cannot see it."),
        ),
        E::Status(status) => Problem::remote(
            Some(Slot::Url),
            tf(
                "GitLab answered {status}.",
                &[("status", &status.to_string())],
            ),
        ),
        E::Network(why) => Problem::remote(
            Some(Slot::Url),
            tf("Could not reach GitLab: {error}", &[("error", why)]),
        ),
    }
}

/// Why Redmine refused, in the window's language.
#[cfg(feature = "redmine")]
pub(super) fn redmine_error(e: &gitlab_tracker_core::TrackerError) -> Problem {
    match e {
        gitlab_tracker_core::TrackerError::Auth(_) => {
            Problem::remote(Some(Slot::RedmineToken), tl("Redmine refused the API key."))
        }
        other => Problem::remote(
            Some(Slot::RedmineUrl),
            tf("Redmine: {error}", &[("error", &other.to_string())]),
        ),
    }
}

/// The `projects.toml` entry of a checked project: GitLab's name and default
/// branch fill what was left empty.
pub(super) fn entry_for(
    checked: Checked,
    info: &crate::gitlab::ProjectInfo,
    redmine: Option<String>,
) -> ProjectEntry {
    let branches = if checked.branches.is_empty() {
        vec![info.default_branch.clone().unwrap_or_else(|| "main".into())]
    } else {
        checked.branches
    };
    ProjectEntry {
        name: checked.name.or_else(|| Some(info.name.clone())),
        gitlab_url: checked.url,
        project_id: checked.project_id,
        tracked_branches: Some(branches),
        tracker: redmine.map(|url| crate::storage::TrackerConfig {
            provider: "redmine".into(),
            url,
            extra: toml::Table::new(),
        }),
        ..ProjectEntry::default()
    }
}

/// Checks and saves the tokens typed for `entry` (an empty Redmine key leaves the
/// tracker out for this session).
pub(super) async fn save_tokens(
    entry: ProjectEntry,
    token: Zeroizing<String>,
    redmine_token: Zeroizing<String>,
) -> Result<ProjectEntry, Vec<Problem>> {
    if !token.trim().is_empty() {
        crate::gitlab::check_project(&entry.gitlab_url, token.trim(), &entry.project_id)
            .await
            .map_err(|e| vec![check_error(&e)])?;
        let (url, token) = (entry.gitlab_url.clone(), token.clone());
        tokio::task::spawn_blocking(move || crate::storage::save_gitlab_token(&url, &token))
            .await
            .map_err(|e| vec![Problem::remote(None, e.to_string())])?
            .map_err(|e| vec![keyring_error(&e)])?;
    }
    #[cfg(feature = "redmine")]
    if let Some(tracker) = entry
        .tracker
        .as_ref()
        .filter(|_| !redmine_token.trim().is_empty())
    {
        gitlab_tracker_redmine::check_token(&tracker.url, redmine_token.trim())
            .await
            .map_err(|e| vec![redmine_error(&e)])?;
        let (url, key) = (tracker.url.clone(), redmine_token.clone());
        tokio::task::spawn_blocking(move || {
            gitlab_tracker_redmine::keyring::save_token(&url, &key)
        })
        .await
        .map_err(|e| vec![Problem::remote(None, e.to_string())])?
        .map_err(|e| vec![keyring_error(&e)])?;
    }
    #[cfg(not(feature = "redmine"))]
    let _ = redmine_token;
    Ok(entry)
}

/// The tokens `project` lacks, as the form's [`Mode::Tokens`]; `None` when it has
/// them all. Blocks on the keyring: off the UI thread.
pub fn missing_tokens(project: &ProjectEntry) -> Option<Mode> {
    let gitlab = !crate::storage::has_gitlab_token(&project.gitlab_url);
    #[cfg(feature = "redmine")]
    let redmine = project
        .tracker
        .as_ref()
        .filter(|t| is_redmine(&t.provider) && !t.url.trim().is_empty())
        .is_some_and(|t| {
            std::env::var("REDMINE_URL").is_err()
                && !gitlab_tracker_redmine::keyring::has_token(&t.url)
        });
    #[cfg(not(feature = "redmine"))]
    let redmine = false;
    (gitlab || redmine).then(|| Mode::Tokens {
        entry: Box::new(project.clone()),
        gitlab,
        redmine,
    })
}

/// A checked form, ready to save.
#[derive(Debug, PartialEq)]
pub(super) struct Checked {
    pub(super) url: String,
    pub(super) project_id: String,
    pub(super) name: Option<String>,
    /// Empty: the project's default branch.
    pub(super) branches: Vec<String>,
}

/// Checks the form: an https URL (http only to this machine) without spaces (its trailing `/` dropped), a
/// project id or path without spaces, branch names without spaces, and the name
/// cleaned like the settings do.
pub(super) fn check(
    url: &str,
    project_id: &str,
    name: &str,
    branches: &str,
) -> Result<Checked, Vec<(Slot, &'static str)>> {
    let mut errors = Vec::new();
    let url = url.trim().trim_end_matches('/').to_string();
    let url_ok = gitlab_tracker_core::secrets::is_secure_url(&url)
        && url.len() > "https://".len()
        && !url.contains(char::is_whitespace);
    if !url_ok {
        errors.push((
            Slot::Url,
            "The GitLab URL starts with https:// (http:// only for this machine), without spaces.",
        ));
    }
    let project_id = project_id.trim().trim_matches('/').to_string();
    if project_id.is_empty() || project_id.contains(char::is_whitespace) {
        errors.push((
            Slot::Project,
            "The project is its numeric id or its path (group/project), without spaces.",
        ));
    }
    let mut list: Vec<String> = branches
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(str::to_string)
        .collect();
    if list.iter().any(|b| b.contains(char::is_whitespace)) {
        errors.push((
            Slot::Branches,
            "Branch names have no spaces; separate them with commas.",
        ));
    }
    list.dedup();
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Checked {
        url,
        project_id,
        name: crate::settings_core::clean_project_name(name),
        branches: list,
    })
}
