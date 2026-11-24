//! Whether a newer release of the app is out: the latest GitHub release, read
//! at most once a day (the answer is kept in `tracker.db`), without a token.
//! The window only says so; nothing is downloaded or installed.

use serde::{Deserialize, Serialize};

/// The latest published release (drafts and pre-releases are left out by GitHub).
const LATEST: &str = "https://api.github.com/repos/julien-langlois/gitlab-tracker/releases/latest";
/// Seconds between two reads of GitHub.
const EVERY: i64 = 24 * 3600;
/// The `tracker.db` setting that keeps the last answer.
const SETTING: &str = "update_check";

/// A release newer than the running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// `0.7.0`, without the tag's `v`.
    pub version: String,
    /// Its page on GitHub.
    pub url: String,
}

/// The last read of GitHub: when, and the latest release then.
#[derive(Default, Serialize, Deserialize)]
struct Checked {
    at: i64,
    latest: Option<Release>,
}

/// The running app's version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `major.minor.patch` of a version or tag (`v1.2.3`, `1.2.3-rc.1`), and
/// whether it is a pre-release.
fn parse(version: &str) -> Option<([u64; 3], bool)> {
    let version = version.trim().trim_start_matches('v');
    // Build metadata (`+…`) does not order versions.
    let version = version.split_once('+').map_or(version, |(v, _)| v);
    let (core, pre) = match version.split_once('-') {
        Some((core, _)) => (core, true),
        None => (version, false),
    };
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let numbers = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((numbers, pre))
}

/// Whether `candidate` is a later version than `current`. A pre-release
/// comes before its release; one that does not parse is never newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some((a, a_pre)), Some((b, b_pre))) => a > b || (a == b && b_pre && !a_pre),
        _ => false,
    }
}

/// The release newer than this app, if there is one: from the last answer
/// when it is less than a day old, else read from GitHub. A failed read says
/// nothing and is tried again at the next start.
pub async fn newer_release() -> Option<Release> {
    let now = chrono::Utc::now().timestamp();
    let kept: Option<Checked> = crate::storage::load_setting(SETTING, "update_check.json").await;
    let latest = match kept {
        Some(checked) if (0..EVERY).contains(&(now - checked.at)) => checked.latest,
        _ => match fetch().await {
            Ok(latest) => {
                let checked = Checked {
                    at: now,
                    latest: latest.clone(),
                };
                crate::storage::save_setting(SETTING, &checked).await;
                latest
            }
            Err(e) => {
                tracing::info!(error = %e, "Could not read the latest release");
                None
            }
        },
    };
    latest.filter(|release| is_newer(&release.version, current()))
}

/// The latest release on GitHub; `None` when there is none yet.
async fn fetch() -> anyhow::Result<Option<Release>> {
    #[derive(Deserialize)]
    struct Answer {
        tag_name: String,
        html_url: String,
    }
    let response = reqwest::Client::builder()
        .user_agent(concat!("gitlab-tracker/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let answer: Answer = response.error_for_status()?.json().await?;
    Ok(Some(Release {
        version: answer.tag_name.trim_start_matches('v').to_string(),
        url: answer.html_url,
    }))
}

/// What the demo shows: the next minor version, as if it were out.
pub fn demo() -> Release {
    let ([major, minor, _], _) = parse(current()).unwrap_or(([0, 0, 0], false));
    let version = format!("{major}.{}.0", minor + 1);
    Release {
        url: format!("https://github.com/julien-langlois/gitlab-tracker/releases/tag/v{version}"),
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_version_is_newer() {
        assert!(is_newer("0.7.0", "0.6.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.6.1", "0.6.0"));
        assert!(is_newer("0.10.0", "0.9.0"), "numbers, not text");
    }

    #[test]
    fn the_same_or_an_older_version_is_not() {
        assert!(!is_newer("0.6.0", "0.6.0"));
        assert!(!is_newer("v0.5.9", "0.6.0"));
    }

    #[test]
    fn a_release_comes_after_its_pre_releases() {
        assert!(is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_never_newer() {
        assert!(!is_newer("nightly", "0.6.0"));
        assert!(!is_newer("1.2", "0.6.0"));
        assert!(!is_newer("1.2.3.4", "0.6.0"));
    }

    #[test]
    fn the_demo_shows_a_newer_version() {
        assert!(is_newer(&demo().version, current()));
        assert!(demo().url.ends_with(&format!("v{}", demo().version)));
    }
}
