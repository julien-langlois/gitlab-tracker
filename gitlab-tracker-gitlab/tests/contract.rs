//! Contract tests: the client against what real GitLab instances answered.
//!
//! Each directory of `tests/fixtures/` holds the requests a refresh sent to one
//! instance and its answers (`recording.json`), what the scenario asked for
//! (`meta.json`) and what the client made of the answers (`expected.json`). The
//! replay test serves the recording to the client and compares: a field GitLab
//! sends that stops being read, a request the recording never saw, an answer
//! that no longer parses — each fails here, against real payloads rather than
//! hand-written ones.
//!
//! Recording (an instance's answers change; the code that reads them did not):
//!
//! ```text
//! GT_RECORD_UPSTREAM=https://gitlab.com GT_RECORD_PROJECT=34675721 \
//! GT_RECORD_MRS=101,102 GT_RECORD_BRANCHES=main GT_RECORD_NAME=gitlab.com \
//!   [GT_RECORD_TOKEN=…] [GT_RECORD_DAYS=3] cargo test -p gitlab-tracker-gitlab --test contract \
//!   record -- --ignored --nocapture
//! ```
//!
//! People are anonymized on the way to disk (names, logins, e-mails, avatars,
//! free text), so a recording holds no one's identity: only its shapes and the
//! project's own data (titles, branches, labels, pipelines).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gitlab_tracker_gitlab::gitlab::{self, BranchScan, CachedMrData, FetchContext};
use gitlab_tracker_gitlab::GitlabEvent;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// What the scenario asked the instance for.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Meta {
    /// Which instance and version answered (for people reading the fixture).
    instance: String,
    recorded_on: String,
    project_id: String,
    mrs: Vec<String>,
    branches: Vec<String>,
    /// The start of the branch scans and of the discovery poll.
    since: String,
}

/// One request and its answer.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Exchange {
    key: String,
    status: u16,
    body: Value,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A request as the recording knows it: method, path and query, and the body of
/// a POST (GraphQL), with every date replaced by `*` — the scenario's dates and
/// the sentinel's "now" differ from one run to the next.
fn key(method: &str, path_and_query: &str, body: &str) -> String {
    let dates = regex_lite_dates(path_and_query);
    if method == "POST" {
        let body = serde_json::from_str::<Value>(body)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| body.to_string());
        format!("{method} {dates} {}", regex_lite_dates(&body))
    } else {
        format!("{method} {dates}")
    }
}

/// `text` with each date (`2026-11-23`, with or without a time, raw or
/// percent-encoded) replaced by `*`.
fn regex_lite_dates(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let is_digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    while i < bytes.len() {
        let date = (0..4).all(|k| is_digit(i + k))
            && bytes.get(i + 4) == Some(&b'-')
            && is_digit(i + 5)
            && is_digit(i + 6)
            && bytes.get(i + 7) == Some(&b'-')
            && is_digit(i + 8)
            && is_digit(i + 9);
        if date {
            // The date and whatever time follows, up to a separator.
            let mut end = i + 10;
            while end < bytes.len() && !matches!(bytes[end], b'&' | b'"' | b',' | b' ' | b'}') {
                end += 1;
            }
            out.push('*');
            i = end;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

// ── The stand-in instance ────────────────────────────────────────────────────

enum Mode {
    /// Forwards to `upstream` and keeps each exchange.
    Record { upstream: String, token: String },
    /// Answers from the recording; what it lacks is listed in `missing`.
    Replay(HashMap<String, Exchange>),
}

struct Server {
    url: String,
    exchanges: Arc<Mutex<BTreeMap<String, Exchange>>>,
    missing: Arc<Mutex<Vec<String>>>,
}

async fn serve(mode: Mode) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let exchanges = Arc::new(Mutex::new(BTreeMap::new()));
    let missing = Arc::new(Mutex::new(Vec::new()));
    let mode = Arc::new(mode);
    let (kept, lacking) = (exchanges.clone(), missing.clone());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (mode, kept, lacking) = (mode.clone(), kept.clone(), lacking.clone());
            tokio::spawn(async move {
                let Some((method, path, body)) = read_request(&mut socket).await else {
                    return;
                };
                let key = key(&method, &path, &body);
                let (status, answer) = match mode.as_ref() {
                    Mode::Replay(recorded) => match recorded.get(&key) {
                        Some(e) => (e.status, e.body.clone()),
                        None => {
                            lacking.lock().unwrap().push(key);
                            (404, json!({"message": "not in the recording"}))
                        }
                    },
                    Mode::Record { upstream, token } => {
                        let (status, answer) =
                            forward(upstream, token, &method, &path, &body).await;
                        kept.lock().unwrap().entry(key.clone()).or_insert(Exchange {
                            key,
                            status,
                            body: answer.clone(),
                        });
                        (status, answer)
                    }
                };
                let text = answer.to_string();
                let reason = if status < 400 { "OK" } else { "Error" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    Server {
        url,
        exchanges,
        missing,
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<(String, String, String)> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while raw.len() < head_end + length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let mut first = head.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    Some((
        method,
        path,
        String::from_utf8_lossy(&raw[head_end..]).to_string(),
    ))
}

async fn forward(
    upstream: &str,
    token: &str,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, Value) {
    let client = reqwest::Client::new();
    let url = format!("{}{path}", upstream.trim_end_matches('/'));
    let mut request = if method == "POST" {
        client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string())
    } else {
        client.get(url)
    };
    if !token.is_empty() {
        request = request.header("PRIVATE-TOKEN", token);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
            (status, value)
        }
        Err(e) => (599, json!({ "message": e.to_string() })),
    }
}

// ── Anonymization ────────────────────────────────────────────────────────────

/// Replaces who people are by stable stand-ins (`user-1`…), consistently across
/// the whole recording, and the free text they wrote by a placeholder.
#[derive(Default)]
struct Anonymizer {
    people: HashMap<String, usize>,
}

impl Anonymizer {
    fn person(&mut self, real: &str) -> String {
        let next = self.people.len() + 1;
        let n = *self.people.entry(real.to_lowercase()).or_insert(next);
        format!("user-{n}")
    }

    fn value(&mut self, value: &mut Value) {
        match value {
            Value::Array(items) => items.iter_mut().for_each(|v| self.value(v)),
            Value::Object(map) => {
                // A user (REST `username`, GraphQL `username`): everything that names them.
                if let Some(login) = map
                    .get("username")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                {
                    let alias = self.person(&login);
                    for (field, stand_in) in [
                        ("username", alias.clone()),
                        ("name", alias.replace("user-", "User ")),
                        ("public_email", format!("{alias}@example.com")),
                        ("email", format!("{alias}@example.com")),
                        ("avatar_url", String::new()),
                        ("avatarUrl", String::new()),
                        ("web_url", format!("https://gitlab.example/{alias}")),
                        ("webUrl", format!("https://gitlab.example/{alias}")),
                        ("bio", String::new()),
                        ("location", String::new()),
                    ] {
                        if map.get(field).is_some_and(|v| !v.is_null()) {
                            map.insert(field.into(), Value::String(stand_in));
                        }
                    }
                }
                // Logins elsewhere (`author_username` of events…).
                let logins: Vec<String> = map
                    .keys()
                    .filter(|k| k.ends_with("username") && k.as_str() != "username")
                    .cloned()
                    .collect();
                for field in logins {
                    if let Some(real) = map.get(&field).and_then(Value::as_str).map(str::to_string)
                    {
                        let alias = self.person(&real);
                        map.insert(field, Value::String(alias));
                    }
                }
                // Commits: author and committer.
                for (name, mail) in [
                    ("author_name", "author_email"),
                    ("committer_name", "committer_email"),
                    ("authorName", "authorEmail"),
                ] {
                    if let Some(real) = map.get(mail).and_then(Value::as_str).map(str::to_string) {
                        let alias = self.person(&real);
                        map.insert(mail.into(), Value::String(format!("{alias}@example.com")));
                        if map.contains_key(name) {
                            map.insert(name.into(), Value::String(alias.replace("user-", "User ")));
                        }
                    }
                }
                // Free text people wrote: kept to its first line for commit
                // messages (the title, and the cherry-pick trailers the client
                // reads), replaced elsewhere.
                if let Some(Value::String(message)) = map.get_mut("message") {
                    let kept: Vec<&str> = message
                        .lines()
                        .enumerate()
                        .filter(|(i, l)| *i == 0 || l.contains("(cherry picked from commit"))
                        .map(|(_, l)| l)
                        .collect();
                    *message = kept.join("\n\n");
                }
                for field in ["description", "body", "note", "bodyHtml", "descriptionHtml"] {
                    if let Some(Value::String(text)) = map.get_mut(field) {
                        if !text.is_empty() {
                            *text = "Text written by a person, replaced by the recorder.".into();
                        }
                    }
                }
                map.values_mut().for_each(|v| self.value(v));
            }
            Value::String(text) => {
                // `@login` mentions and e-mail addresses left in other strings.
                for (real, n) in &self.people {
                    if text.to_lowercase().contains(&format!("@{real}")) {
                        *text = text.replace(&format!("@{real}"), &format!("@user-{n}"));
                    }
                }
            }
            _ => {}
        }
    }
}

// ── The scenario: one refresh, as the app runs it ────────────────────────────

/// Runs a refresh of `meta`'s MRs and branches against `url`, plus what runs
/// beside it (labels, milestones, discovery, sentinel), and returns what the
/// client made of it, in a stable form.
async fn scenario(url: &str, token: &str, meta: &Meta) -> Value {
    let ctx = FetchContext {
        base_url: url.to_string(),
        token: Arc::new(token.to_string().into()),
        project_id: meta.project_id.clone(),
        branches: meta.branches.clone(),
    };
    let semaphore = Arc::new(Semaphore::new(3));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<GitlabEvent>();

    // The MRs, as a refresh with nothing cached reads them.
    gitlab::spawn_mr_fetches(
        ctx.clone(),
        meta.mrs
            .iter()
            .map(|id| (id.clone(), CachedMrData::default()))
            .collect(),
        semaphore.clone(),
        tx.clone(),
    );
    for branch in &meta.branches {
        gitlab::spawn_branch_commits_fetch(
            ctx.clone(),
            branch.clone(),
            BranchScan::Full {
                since: meta.since.clone(),
            },
            None,
            semaphore.clone(),
            tx.clone(),
        );
    }
    let since = chrono::DateTime::parse_from_rfc3339(&meta.since)
        .unwrap()
        .with_timezone(&chrono::Utc);
    gitlab_tracker_gitlab::sentinel::spawn_check(ctx.clone(), since, semaphore.clone(), tx.clone());
    drop(tx);

    let mut mrs = BTreeMap::new();
    let mut branches = BTreeMap::new();
    let mut sentinel = Value::Null;
    while let Some(event) = rx.recv().await {
        match event {
            GitlabEvent::MrLoaded(loaded) => {
                let mut branches: Vec<_> = loaded.branches.iter().cloned().collect();
                branches.sort();
                mrs.insert(
                    loaded.id.clone(),
                    json!({
                        "data": canonical(serde_json::to_value(&loaded.data).unwrap()),
                        "branches": branches,
                        "mergeability": format!("{:?}", loaded.mergeability),
                    }),
                );
            }
            GitlabEvent::MrFailed { id, error } => {
                mrs.insert(id, json!({ "failed": format!("{error:?}") }));
            }
            GitlabEvent::BranchCommitsLoaded {
                branch, commits, ..
            } => {
                branches.insert(
                    branch,
                    commits.map_or(Value::Null, |c| {
                        json!({
                            "commits": c.shas.len(),
                            "cherry_picked_from": c.picked_from.len(),
                            "fingerprints": c.fingerprints.len(),
                        })
                    }),
                );
            }
            GitlabEvent::SentinelChecked(report) => {
                sentinel = report.map_or(Value::Null, |r| {
                    json!({
                        "truncated": r.truncated,
                        "mrs": r.mrs.len(),
                        "pipelines": r.pipelines.len(),
                        "events": r.events.len(),
                    })
                });
            }
            _ => {}
        }
    }

    let labels: Vec<String> = gitlab::fetch_gitlab_labels(&ctx)
        .await
        .into_iter()
        .map(|l| l.name)
        .collect();
    let milestones: Vec<String> = gitlab::fetch_milestones(&ctx)
        .await
        .into_iter()
        .map(|m| m.title)
        .collect();
    let discovered = gitlab::fetch_recent_mr_ids(&ctx, &meta.since).await;
    canonical(json!({
        "mrs": mrs,
        "branches": branches,
        "sentinel": sentinel,
        "labels": labels,
        "milestones": milestones,
        "discovered": discovered.map(|ids| ids.len()),
    }))
}

/// `value` with every array sorted (sets come out in any order) and the fields
/// that depend on the clock, not on GitLab's answer, dropped.
fn canonical(mut value: Value) -> Value {
    match &mut value {
        Value::Array(items) => {
            let mut sorted: Vec<Value> = items.drain(..).map(canonical).collect();
            sorted.sort_by_key(|v| v.to_string());
            *items = sorted;
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                *item = canonical(item.take());
            }
            // When the client read the review (the machine's clock, not the
            // instance's answer): only whether it was read can be compared.
            if let Some(read_at) = map.get_mut("review_read_at") {
                if !read_at.is_null() {
                    *read_at = Value::String("<read>".into());
                }
            }
        }
        _ => {}
    }
    value
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn write(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").unwrap();
}

async fn replay(dir: &Path) -> (Value, Vec<String>) {
    let meta: Meta = read(&dir.join("meta.json"));
    let recording: Vec<Exchange> = read(&dir.join("recording.json"));
    let server = serve(Mode::Replay(
        recording.into_iter().map(|e| (e.key.clone(), e)).collect(),
    ))
    .await;
    let outcome = scenario(&server.url, "", &meta).await;
    let missing = server.missing.lock().unwrap().clone();
    (outcome, missing)
}

// ── The tests ────────────────────────────────────────────────────────────────

/// Every recorded instance: same requests, same reading of the answers.
#[tokio::test(flavor = "multi_thread")]
async fn the_client_reads_what_real_instances_answer() {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    assert!(!dirs.is_empty(), "no recording in {}", fixtures().display());
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let (outcome, missing) = replay(&dir).await;
        assert!(
            missing.is_empty(),
            "{name}: requests the recording never saw (the client asks for something \
             new: record the instance again, see the top of tests/contract.rs):\n{}",
            missing.join("\n")
        );
        let expected: Value = read(&dir.join("expected.json"));
        assert!(
            outcome == expected,
            "{name}: the client reads the recorded answers differently than when they \
             were recorded.\n--- expected\n{}\n--- now\n{}",
            serde_json::to_string_pretty(&expected).unwrap(),
            serde_json::to_string_pretty(&outcome).unwrap()
        );
        // What a refresh must get out of any instance.
        for (id, mr) in outcome["mrs"].as_object().unwrap() {
            assert!(mr.get("failed").is_none(), "{name}: MR {id} failed: {mr}");
            let data = &mr["data"];
            for field in [
                "title",
                "author",
                "web_url",
                "source_branch",
                "target_branch",
            ] {
                assert!(
                    data[field].as_str().is_some_and(|s| !s.is_empty()),
                    "{name}: MR {id} has no {field}"
                );
            }
        }
        assert!(
            outcome["branches"]
                .as_object()
                .unwrap()
                .values()
                .all(|b| !b.is_null()),
            "{name}: a branch scan failed"
        );
        assert!(
            !outcome["sentinel"].is_null(),
            "{name}: the sentinel check failed"
        );
    }
}

/// Records an instance (see the top of this file). Writes the fixture, then
/// replays it once to store what the client makes of it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to a real GitLab"]
async fn record() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"));
    let upstream = var("GT_RECORD_UPSTREAM");
    let token = std::env::var("GT_RECORD_TOKEN").unwrap_or_default();
    // A short window keeps the recording small on a busy project.
    let days: i64 = std::env::var("GT_RECORD_DAYS")
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(3);
    let since = (chrono::Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%dT00:00:00Z")
        .to_string();
    let list = |name: &str| var(name).split(',').map(|s| s.trim().to_string()).collect();
    let meta = Meta {
        instance: std::env::var("GT_RECORD_INSTANCE").unwrap_or_else(|_| upstream.clone()),
        recorded_on: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        project_id: var("GT_RECORD_PROJECT"),
        mrs: list("GT_RECORD_MRS"),
        branches: list("GT_RECORD_BRANCHES"),
        since,
    };
    let server = serve(Mode::Record {
        upstream,
        token: token.clone(),
    })
    .await;
    scenario(&server.url, &token, &meta).await;

    let mut exchanges: Vec<Exchange> = server.exchanges.lock().unwrap().values().cloned().collect();
    let mut anonymizer = Anonymizer::default();
    for exchange in &mut exchanges {
        anonymizer.value(&mut exchange.body);
    }
    let dir = fixtures().join(var("GT_RECORD_NAME"));
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir.join("meta.json"), &meta);
    write(&dir.join("recording.json"), &exchanges);
    let (outcome, missing) = replay(&dir).await;
    assert!(
        missing.is_empty(),
        "replay asks for more than was recorded: {missing:?}"
    );
    write(&dir.join("expected.json"), &outcome);
    println!("{}: {} exchanges", dir.display(), exchanges.len());
}
