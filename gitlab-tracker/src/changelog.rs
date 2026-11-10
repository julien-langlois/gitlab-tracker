//! The Plan page's changelog (`gui` feature): the tracked project's own
//! git-cliff config, read from its repository through the GitLab API, applied
//! to the commits of a branch since its last release tag, also read through the
//! API — no clone, no `git-cliff` on the machine.
//!
//! The config is looked for where `git cliff` looks for it: `cliff.toml`,
//! `.cliff.toml`, `.config/cliff.toml`, then a `git-cliff` table in
//! `Cargo.toml` or `pyproject.toml`. What the CLI does on top of
//! `git-cliff-core` is done here the same way: the release boundary
//! (`tag_pattern`, `ignore_tags`), `limit_commits`, `sort_commits`. The header
//! is left out, as `--strip header` does: the result is the section to paste.

use git_cliff_core::commit::{Commit, Signature};
use git_cliff_core::config::Config;
use git_cliff_core::release::Release;

use crate::gitlab::http_client;
use crate::gitlab_detail::ProjectTarget;
use gitlab_tracker_core::pacing::SendPaced;

/// git-cliff's own config files, in its order.
const CONFIG_FILES: &[&str] = &["cliff.toml", ".cliff.toml", ".config/cliff.toml"];
/// Manifests that can hold the config, with the prefix its tables have there.
const MANIFESTS: &[(&str, &[&str])] = &[
    (
        "Cargo.toml",
        &[
            "[workspace.metadata.git-cliff.",
            "[package.metadata.git-cliff.",
        ],
    ),
    ("pyproject.toml", &["[tool.git-cliff."]),
];
/// Pages of 100 commits read at most while looking for the last release.
const MAX_COMMIT_PAGES: usize = 20;
/// Pages of 100 tags read at most.
const MAX_TAG_PAGES: usize = 5;

/// The project's git-cliff config, as found on its default branch.
#[derive(Debug, Clone)]
pub struct CliffConfig {
    /// Where it was found (`cliff.toml`, `Cargo.toml`…).
    pub path: String,
    pub default_branch: String,
}

/// A generated changelog.
#[derive(Debug, Clone)]
pub struct Generated {
    pub markdown: String,
    /// The release tag it starts after; `None`: none was found, it covers the
    /// commits read.
    pub since: Option<String>,
    /// Commits read since that tag (before git-cliff skips any).
    pub commits: usize,
    /// The tag was not found in the commits read: the history was cut.
    pub truncated: bool,
}

/// A commit as the API lists it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ApiCommit {
    pub id: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_email: Option<String>,
    #[serde(default)]
    pub authored_date: Option<String>,
    #[serde(default)]
    pub committer_name: Option<String>,
    #[serde(default)]
    pub committer_email: Option<String>,
    #[serde(default)]
    pub committed_date: Option<String>,
    #[serde(default)]
    pub parent_ids: Vec<String>,
}

/// A tag as the API lists it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApiTag {
    pub name: String,
    #[serde(default)]
    pub message: Option<String>,
    pub commit: ApiTagCommit,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApiTagCommit {
    pub id: String,
    #[serde(default)]
    pub committed_date: Option<String>,
}

struct Api<'a>(&'a ProjectTarget);

impl Api<'_> {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/api/v4/projects/{}{}",
            self.0.base_url.trim_end_matches('/'),
            urlencoding::encode(&self.0.project_id),
            path
        )
    }

    async fn send(&self, path: &str) -> Result<Option<reqwest::Response>, String> {
        let res = http_client()
            .get(self.url(path))
            .header("PRIVATE-TOKEN", self.0.token.as_str())
            .send_paced()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !res.status().is_success() {
            return Err(format!("GitLab answered {}", res.status()));
        }
        Ok(Some(res))
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        match self.send(path).await? {
            Some(res) => res.json().await.map_err(|e| e.to_string()),
            None => Err("GitLab answered 404 Not Found".into()),
        }
    }

    /// A file of the repository; `None` when it is not there.
    async fn file(&self, path: &str, reference: &str) -> Result<Option<String>, String> {
        let path = format!(
            "/repository/files/{}/raw?ref={}",
            urlencoding::encode(path),
            urlencoding::encode(reference)
        );
        match self.send(&path).await? {
            Some(res) => res.text().await.map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    async fn default_branch(&self) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct Project {
            #[serde(default)]
            default_branch: Option<String>,
        }
        let project: Project = self.json("").await?;
        project
            .default_branch
            .ok_or_else(|| "the project has no default branch".to_string())
    }

    /// The config's text at `reference`: the file, or the manifest's table
    /// with its prefix taken off. `None` when `path` holds none.
    async fn config_text(&self, path: &str, reference: &str) -> Result<Option<String>, String> {
        let Some(text) = self.file(path, reference).await? else {
            return Ok(None);
        };
        match MANIFESTS.iter().find(|(name, _)| *name == path) {
            Some((_, prefixes)) => Ok(from_manifest(&text, prefixes)),
            None => Ok(Some(text)),
        }
    }
}

/// The git-cliff tables of a manifest, as a `cliff.toml` would hold them;
/// `None` when it has none.
fn from_manifest(text: &str, prefixes: &[&str]) -> Option<String> {
    let mut found = false;
    let out: Vec<String> = text
        .lines()
        .map(
            |line| match prefixes.iter().find_map(|p| line.strip_prefix(p)) {
                Some(rest) => {
                    found = true;
                    format!("[{rest}")
                }
                None => line.to_string(),
            },
        )
        .collect();
    found.then(|| out.join("\n"))
}

/// Looks for the project's git-cliff config on its default branch.
pub async fn detect(target: ProjectTarget) -> Result<Option<CliffConfig>, String> {
    let api = Api(&target);
    let default_branch = api.default_branch().await?;
    let candidates = CONFIG_FILES
        .iter()
        .copied()
        .chain(MANIFESTS.iter().map(|(name, _)| *name));
    for path in candidates {
        if api.config_text(path, &default_branch).await?.is_some() {
            return Ok(Some(CliffConfig {
                path: path.to_string(),
                default_branch,
            }));
        }
    }
    Ok(None)
}

/// The changelog of `branch` since its last release tag, with the config at
/// `config.path` on that branch (the default branch's when it has none), titled
/// `version` (unreleased when `None`).
pub async fn generate(
    target: ProjectTarget,
    config: CliffConfig,
    branch: String,
    version: Option<String>,
) -> Result<Generated, String> {
    let api = Api(&target);
    let text = match api.config_text(&config.path, &branch).await? {
        Some(text) => text,
        None => api
            .config_text(&config.path, &config.default_branch)
            .await?
            .ok_or_else(|| format!("{} no longer holds a git-cliff config", config.path))?,
    };
    let cliff = parse_config(&text)?;

    let mut tags: Vec<ApiTag> = Vec::new();
    for page in 1..=MAX_TAG_PAGES {
        let batch: Vec<ApiTag> = api
            .json(&format!("/repository/tags?per_page=100&page={page}"))
            .await?;
        let last = batch.len() < 100;
        tags.extend(batch);
        if last {
            break;
        }
    }
    let boundaries: Vec<ApiTag> = tags
        .into_iter()
        .filter(|t| is_release_tag(&cliff, &t.name))
        .collect();

    let mut commits: Vec<ApiCommit> = Vec::new();
    let mut previous: Option<ApiTag> = None;
    'pages: for page in 1..=MAX_COMMIT_PAGES {
        let batch: Vec<ApiCommit> = api
            .json(&format!(
                "/repository/commits?ref_name={}&per_page=100&page={page}",
                urlencoding::encode(&branch)
            ))
            .await?;
        let last = batch.len() < 100;
        for commit in batch {
            if let Some(tag) = boundaries.iter().find(|t| t.commit.id == commit.id) {
                previous = Some(tag.clone());
                break 'pages;
            }
            commits.push(commit);
        }
        if last {
            break;
        }
    }
    let truncated = previous.is_none() && commits.len() >= MAX_COMMIT_PAGES * 100;
    let count = commits.len();
    let markdown = render(
        cliff,
        &commits,
        previous.as_ref(),
        version.as_deref(),
        chrono::Utc::now().timestamp(),
    )?;
    Ok(Generated {
        markdown,
        since: previous.map(|t| t.name),
        commits: count,
        truncated,
    })
}

/// The config, offline (no remote is asked from here) and without its header.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let mut config: Config = text
        .parse()
        .map_err(|e: git_cliff_core::error::Error| format!("git-cliff config: {e}"))?;
    config.remote.offline = true;
    config.changelog.header = None;
    Ok(config)
}

/// A tag that closes a release, as the CLI sees it: it matches `tag_pattern`
/// (any tag when there is none) and not `ignore_tags`.
fn is_release_tag(config: &Config, name: &str) -> bool {
    config
        .git
        .tag_pattern
        .as_ref()
        .is_none_or(|p| p.is_match(name))
        && !config
            .git
            .ignore_tags
            .as_ref()
            .is_some_and(|p| p.is_match(name))
}

fn timestamp(date: Option<&str>) -> i64 {
    date.and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|d| d.timestamp())
        .unwrap_or_default()
}

/// Renders the release made of `newest_first` (the commits since `previous`,
/// newest first, as the API lists them) with `config`.
pub fn render(
    config: Config,
    newest_first: &[ApiCommit],
    previous: Option<&ApiTag>,
    version: Option<&str>,
    now: i64,
) -> Result<String, String> {
    let limit = config.git.limit_commits.unwrap_or(usize::MAX);
    let mut commits: Vec<Commit<'static>> = newest_first
        .iter()
        .take(limit)
        .map(|c| {
            let mut commit = Commit::new(c.id.clone(), c.message.clone());
            commit.author = Signature {
                name: c.author_name.clone(),
                email: c.author_email.clone(),
                timestamp: timestamp(c.authored_date.as_deref()),
            };
            commit.committer = Signature {
                name: c.committer_name.clone(),
                email: c.committer_email.clone(),
                timestamp: timestamp(c.committed_date.as_deref()),
            };
            commit.merge_commit = c.parent_ids.len() > 1;
            commit
        })
        .collect();
    if commits.is_empty() {
        return Ok(String::new());
    }
    if config.git.sort_commits == "oldest" {
        commits.reverse();
    }
    let previous = previous.map(|tag| Release {
        version: Some(tag.name.clone()),
        message: tag.message.clone().filter(|m| !m.is_empty()),
        commit_id: Some(tag.commit.id.clone()),
        timestamp: Some(timestamp(tag.commit.committed_date.as_deref())),
        ..Release::default()
    });
    let release = Release {
        version: version.map(str::to_string),
        commit_id: newest_first.first().map(|c| c.id.clone()),
        timestamp: Some(if version.is_some() { now } else { 0 }),
        commits,
        previous: previous.clone().map(Box::new),
        ..Release::default()
    };
    let releases: Vec<Release> = previous.into_iter().chain([release]).collect();
    let changelog = git_cliff_core::changelog::Changelog::new(releases, config, None)
        .map_err(|e| format!("git-cliff: {e}"))?;
    let mut out = Vec::new();
    changelog
        .generate(&mut out)
        .map_err(|e| format!("git-cliff: {e}"))?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// The read-only demo's config: the button shows, as on a project that has one.
const DEMO_CONFIG: &str = r##"
[changelog]
body = """
## {% if version %}[{{ version | trim_start_matches(pat="v") }}] - {{ timestamp | date(format="%Y-%m-%d") }}{% else %}[Unreleased]{% endif %}
{% for group, commits in commits | sort(attribute="group") | group_by(attribute="group") %}
### {{ group | striptags | trim }}
{% for commit in commits %}
- {% if commit.scope %}**{{ commit.scope }}**: {% endif %}{{ commit.message | split(pat="\n") | first | trim | upper_first }}\
{% endfor %}
{% endfor %}
"""
trim = true

[git]
conventional_commits = true
filter_unconventional = false
commit_parsers = [
  { message = '^(?i)(feat|add|new|implement|support)', group = "<!-- 00 -->🚀 Features" },
  { message = '^(?i)(fix|bug|hotfix|resolve)', group = "<!-- 01 -->🐛 Bug Fixes" },
  { message = '^(?i)(perf|improve|optimi|update)', group = "<!-- 02 -->✨ Improvements" },
  { message = '^(?i)(refactor|clean|rename|move)', group = "<!-- 03 -->🔨 Refactor" },
  { message = '.*', group = "<!-- 99 -->📦 Other" },
]
"##;

/// The demo's config, found in a `cliff.toml` on `main`.
pub fn demo_config() -> CliffConfig {
    CliffConfig {
        path: "cliff.toml".into(),
        default_branch: "main".into(),
    }
}

/// The demo's changelog: its MR titles as the commits since the `previous` tag.
pub fn demo(titles: &[String], previous: &str, version: Option<&str>) -> Result<Generated, String> {
    let commits: Vec<ApiCommit> = titles
        .iter()
        .enumerate()
        .map(|(i, title)| ApiCommit {
            id: format!("demo{i}"),
            message: title.clone(),
            ..ApiCommit::default()
        })
        .collect();
    let previous = ApiTag {
        name: previous.into(),
        message: None,
        commit: ApiTagCommit {
            id: "demo-tag".into(),
            committed_date: None,
        },
    };
    let markdown = render(
        parse_config(DEMO_CONFIG)?,
        &commits,
        Some(&previous),
        version,
        chrono::Utc::now().timestamp(),
    )?;
    Ok(Generated {
        markdown,
        since: Some(previous.name),
        commits: commits.len(),
        truncated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r##"
[changelog]
header = "# Changelog\n"
body = """
## {% if version %}{{ version }}{% else %}Unreleased{% endif %}\
{% if previous.version %} (since {{ previous.version }}){% endif %}
{% for group, commits in commits | group_by(attribute="group") %}
### {{ group }}
{% for commit in commits %}- {{ commit.message | split(pat="\n") | first }}
{% endfor %}{% endfor %}
"""
trim = true

[git]
conventional_commits = true
filter_unconventional = false
commit_preprocessors = [
  { pattern = '^(?i)(feat|fix)\s*(\([^)]*\))?(?:\s*[-:]\s*|\s+)', replace = '$1$2: ' },
]
commit_parsers = [
  { message = '^(?i)fix', group = "Fixes" },
  { message = '^(?i)(ajout|feat)', group = "Features" },
  { message = '^Merge', skip = true },
  { message = '.*', group = "Other" },
]
tag_pattern = '^v?[0-9]+\.[0-9]+\.[0-9]+$'
sort_commits = "oldest"
"##;

    fn commit(id: &str, message: &str) -> ApiCommit {
        ApiCommit {
            id: id.into(),
            message: message.into(),
            ..ApiCommit::default()
        }
    }

    fn tag(name: &str, id: &str) -> ApiTag {
        ApiTag {
            name: name.into(),
            message: None,
            commit: ApiTagCommit {
                id: id.into(),
                committed_date: Some("2026-10-01T10:00:00Z".into()),
            },
        }
    }

    #[test]
    fn renders_the_release_without_its_header_and_sorts_unconventional_commits() {
        let commits = [
            commit("c4", "fix the footer"),
            commit("c3", "Ajoute la page contact"),
            commit("c2", "Fix: the menu on mobile"),
            commit("c1", "Merge branch 'x' into 'main'"),
            commit("c0", "Mise en page"),
        ];
        let config = parse_config(CONFIG).unwrap();
        let out = render(
            config,
            &commits,
            Some(&tag("1.2.0", "b0")),
            Some("1.3.0"),
            0,
        )
        .unwrap();
        assert!(!out.contains("# Changelog"), "{out}");
        assert!(out.starts_with("## 1.3.0 (since 1.2.0)"), "{out}");
        assert!(
            out.contains("### Features\n- Ajoute la page contact"),
            "{out}"
        );
        // A sloppy prefix ("fix the footer", no colon) still lands in its group.
        assert!(
            out.contains("### Fixes\n- the menu on mobile\n- the footer"),
            "{out}"
        );
        assert!(out.contains("### Other\n- Mise en page"), "{out}");
        assert!(!out.contains("Merge branch"), "{out}");
    }

    #[test]
    fn an_unreleased_changelog_has_no_version() {
        let config = parse_config(CONFIG).unwrap();
        let out = render(config, &[commit("c1", "fix: a")], None, None, 0).unwrap();
        assert!(out.starts_with("## Unreleased"), "{out}");
    }

    #[test]
    fn the_demo_renders_with_its_config() {
        let titles = vec![
            "Add dark mode".to_string(),
            "Fix login redirect".to_string(),
        ];
        let out = demo(&titles, "1.3.0", Some("1.4.0")).unwrap().markdown;
        assert!(out.starts_with("## [1.4.0] - "), "{out}");
        assert!(out.contains("### 🚀 Features\n\n- Add dark mode"), "{out}");
        assert!(
            out.contains("### 🐛 Bug Fixes\n\n- Fix login redirect"),
            "{out}"
        );
    }

    #[test]
    fn nothing_since_the_tag_renders_nothing() {
        let config = parse_config(CONFIG).unwrap();
        assert_eq!(render(config, &[], None, None, 0).unwrap(), "");
    }

    #[test]
    fn only_tags_matching_the_pattern_close_a_release() {
        let config = parse_config(CONFIG).unwrap();
        assert!(is_release_tag(&config, "v1.2.3"));
        assert!(is_release_tag(&config, "1.2.3"));
        assert!(!is_release_tag(&config, "v1.2.3-rc1"));
        assert!(!is_release_tag(&config, "deploy-2026"));
    }

    #[test]
    fn reads_the_config_out_of_a_manifest() {
        let cargo = "[package]\nname = \"x\"\n\n[package.metadata.git-cliff.changelog]\nbody = \"b\"\n[package.metadata.git-cliff.git]\ntag_pattern = 'v.*'\n";
        let text = from_manifest(cargo, MANIFESTS[0].1).unwrap();
        assert!(text.contains("\n[changelog]\nbody"), "{text}");
        assert!(text.contains("\n[git]\ntag_pattern"), "{text}");
        assert!(from_manifest("[package]\nname = \"x\"\n", MANIFESTS[0].1).is_none());
    }
}
