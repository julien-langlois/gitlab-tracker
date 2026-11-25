//! Reads made when a view asks for them: a file's diff, a job's log.

use super::*;

/// One file of an MR's diff, read on demand by the window's inspector.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    #[serde(default)]
    pub new_file: bool,
    #[serde(default)]
    pub renamed_file: bool,
    #[serde(default)]
    pub deleted_file: bool,
    /// The unified patch; empty when GitLab left it out (too large, binary).
    #[serde(default, deserialize_with = "crate::models::null_as_default")]
    pub diff: String,
}

/// The files an MR changes, with their patches (`GET …/changes`). The error is
/// the window's to show.
pub async fn fetch_file_diffs(
    base_url: &str,
    token: &str,
    project_id: &str,
    mr_id: &str,
) -> Result<Vec<FileDiff>, String> {
    #[derive(serde::Deserialize)]
    struct Body {
        changes: Option<Vec<FileDiff>>,
    }
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/changes",
        base_url.trim_end_matches('/'),
        urlencoding::encode(project_id),
        mr_id
    );
    let res = http_client()
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .send_paced()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !res.status().is_success() {
        return Err(format!("GitLab answered {}", res.status()));
    }
    let body: Body = res.json().await.map_err(|e| e.to_string())?;
    body.changes
        .ok_or_else(|| "GitLab did not list the changes: the diff is too large".to_string())
}

/// The end of a job's log (`GET /jobs/:id/trace`): its last `lines` lines, the
/// terminal colours and GitLab's section markers removed.
pub async fn fetch_job_log(
    base_url: &str,
    token: &str,
    project_id: &str,
    job_id: u64,
    lines: usize,
) -> Result<String, String> {
    let url = format!(
        "{}/api/v4/projects/{}/jobs/{}/trace",
        base_url.trim_end_matches('/'),
        urlencoding::encode(project_id),
        job_id
    );
    let res = http_client()
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .send_paced()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !res.status().is_success() {
        return Err(format!("GitLab answered {}", res.status()));
    }
    let text = res.text().await.map_err(|e| e.to_string())?;
    Ok(log_tail(&text, lines))
}

/// The last `lines` lines of a job log, cleaned for reading: ANSI escapes and
/// `section_start` / `section_end` markers dropped, carriage-return progress
/// lines reduced to their last state.
pub fn log_tail(text: &str, lines: usize) -> String {
    let cleaned: Vec<String> = text
        .lines()
        .map(|line| {
            let line = line
                .rsplit('\r')
                .find(|part| !part.is_empty())
                .unwrap_or("");
            strip_ansi(line)
        })
        .map(|line| {
            // `section_start:1700000000:name\r\x1b[0K` leaves `section_start:…:name`.
            if line.starts_with("section_start:") || line.starts_with("section_end:") {
                String::new()
            } else {
                line
            }
        })
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = cleaned.len().saturating_sub(lines);
    cleaned[start..].join("\n")
}

/// `line` without its ANSI escape sequences (`ESC [ … letter`).
pub(super) fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}
