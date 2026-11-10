//! Request pacing shared by every project open in the process, per server instance.
//!
//! Several tracked projects can live on the same GitLab (or Redmine) instance: their
//! limits must add up to one budget for that server, not one budget each. An
//! instance is identified by its scheme, host and port ([`instance_key`]), so
//! `https://gitlab.example.com/group/a` and `https://gitlab.example.com/b` share:
//!
//! * one concurrency limit ([`permits`]), held across a whole MR fetch;
//! * one pause ([`SendPaced::send_paced`]): when the server answers `429` (or `503`),
//!   or says its rate-limit budget is spent (`RateLimit-Remaining: 0`), every request
//!   to it waits until `Retry-After` / `RateLimit-Reset`, whichever project sent it.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use tokio::sync::Semaphore;

/// Pause when the server throttles without saying for how long.
pub const DEFAULT_PAUSE: Duration = Duration::from_secs(30);
/// Longest pause taken on the server's word (a bogus header must not freeze the app).
pub const MAX_PAUSE: Duration = Duration::from_secs(300);

/// `scheme://host[:port]` of `url`, lowercased, default port omitted: the identity of
/// the server instance. An unparsable URL is its own key.
pub fn instance_key(url: &str) -> String {
    match reqwest::Url::parse(url.trim()) {
        Ok(parsed) => {
            let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
            match parsed.port() {
                Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
                None => format!("{}://{host}", parsed.scheme()),
            }
        }
        Err(_) => url.trim().trim_end_matches('/').to_ascii_lowercase(),
    }
}

struct Instance {
    permits: Arc<Semaphore>,
    paused_until: Mutex<Option<Instant>>,
}

fn instance(key: &str, permits: usize) -> Arc<Instance> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Instance>>>> = OnceLock::new();
    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    registry
        .entry(key.to_string())
        .or_insert_with(|| {
            Arc::new(Instance {
                permits: Arc::new(Semaphore::new(permits.max(1))),
                paused_until: Mutex::new(None),
            })
        })
        .clone()
}

/// The concurrency limit of `url`'s instance, shared by every caller naming the same
/// instance. `limit` only applies to the first caller (the instance's creation).
pub fn permits(url: &str, limit: usize) -> Arc<Semaphore> {
    instance(&instance_key(url), limit).permits.clone()
}

/// Makes every request to `url`'s instance wait `wait` from now (a longer pause
/// already in force is kept).
pub fn pause(url: &str, wait: Duration) {
    let target = Instant::now() + wait.min(MAX_PAUSE);
    let inst = instance(&instance_key(url), 1);
    let mut until = inst.paused_until.lock().unwrap_or_else(|e| e.into_inner());
    if until.is_none_or(|current| current < target) {
        *until = Some(target);
    }
}

/// How long requests to `url`'s instance must still wait (`None`: not paused).
pub fn pause_left(url: &str) -> Option<Duration> {
    let inst = instance(&instance_key(url), 1);
    let until = *inst.paused_until.lock().unwrap_or_else(|e| e.into_inner());
    until
        .and_then(|at| at.checked_duration_since(Instant::now()))
        .filter(|left| !left.is_zero())
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok().map(str::trim)
}

/// Seconds until the Unix time in `RateLimit-Reset`, from `now`.
fn until_reset(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let reset: u64 = header(headers, "ratelimit-reset")?.parse().ok()?;
    let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Duration::from_secs(reset.saturating_sub(now)))
}

/// How long the instance must be left alone after answering `status` with `headers`
/// (`None`: no reason to wait). Clamped to `1 s ..= MAX_PAUSE`.
pub fn pause_after(status: u16, headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let wait = if status == 429 || status == 503 {
        let retry_after = header(headers, "retry-after")
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_secs);
        // A 503 without any hint is an ordinary outage, not throttling.
        match retry_after.or_else(|| until_reset(headers, now)) {
            Some(wait) => wait,
            None if status == 429 => DEFAULT_PAUSE,
            None => return None,
        }
    } else if header(headers, "ratelimit-remaining") == Some("0") {
        until_reset(headers, now).unwrap_or(DEFAULT_PAUSE)
    } else {
        return None;
    };
    Some(wait.clamp(Duration::from_secs(1), MAX_PAUSE))
}

/// `send()` that honours the instance's pause and records a new one from the answer.
pub trait SendPaced {
    fn send_paced(self) -> impl Future<Output = reqwest::Result<reqwest::Response>> + Send;
}

impl SendPaced for reqwest::RequestBuilder {
    async fn send_paced(self) -> reqwest::Result<reqwest::Response> {
        let (client, request) = self.build_split();
        let request = request?;
        let url = request.url().to_string();
        // Loop: the pause may have been extended while this request slept.
        while let Some(left) = pause_left(&url) {
            tokio::time::sleep(left).await;
        }
        let response = client.execute(request).await?;
        if let Some(wait) = pause_after(
            response.status().as_u16(),
            response.headers(),
            SystemTime::now(),
        ) {
            tracing::warn!(
                instance = %instance_key(&url),
                status = %response.status(),
                wait_secs = wait.as_secs(),
                "Server asked to slow down — pausing every request to this instance",
            );
            pause(&url, wait);
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    HeaderName::from_bytes(k.as_bytes()).unwrap(),
                    HeaderValue::from_str(v).unwrap(),
                )
            })
            .collect()
    }

    const NOW: u64 = 1_700_000_000;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(NOW)
    }

    #[test]
    fn projects_of_one_server_share_an_instance() {
        let key = instance_key("https://GitLab.Example.com/group/project");
        assert_eq!(key, "https://gitlab.example.com");
        assert_eq!(instance_key("https://gitlab.example.com:443/"), key);
        assert_eq!(instance_key("https://user@gitlab.example.com/x"), key);
        assert_ne!(instance_key("https://gitlab.example.com:8443"), key);
        assert_ne!(instance_key("http://gitlab.example.com"), key);
        assert_ne!(instance_key("https://other.example.com"), key);
    }

    #[test]
    fn permits_are_shared_per_instance_whatever_the_project() {
        let a = permits("https://shared-permits.test/group/a", 3);
        let b = permits("https://shared-permits.test/b", 7); // limit set by the first caller
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(b.available_permits(), 3);
        let other = permits("https://other-permits.test", 3);
        assert!(!Arc::ptr_eq(&a, &other));
    }

    #[test]
    fn a_pause_holds_every_project_of_the_instance_only() {
        assert!(pause_left("https://paused.test/a").is_none());
        pause("https://paused.test/a", Duration::from_secs(60));
        assert!(pause_left("https://paused.test/b").is_some());
        assert!(pause_left("https://not-paused.test").is_none());
        // A shorter pause does not cut a longer one short.
        pause("https://paused.test", Duration::from_secs(1));
        assert!(pause_left("https://paused.test").unwrap() > Duration::from_secs(30));
    }

    #[test]
    fn throttling_answers_set_the_pause() {
        let h = headers(&[("retry-after", "12")]);
        assert_eq!(pause_after(429, &h, now()), Some(Duration::from_secs(12)));
        // No hint at all: default pause on a 429, none on a plain 503.
        assert_eq!(
            pause_after(429, &HeaderMap::new(), now()),
            Some(DEFAULT_PAUSE)
        );
        assert_eq!(pause_after(503, &HeaderMap::new(), now()), None);
        let h = headers(&[("retry-after", "5")]);
        assert_eq!(pause_after(503, &h, now()), Some(Duration::from_secs(5)));
        // GitLab's reset time when Retry-After is missing.
        let h = headers(&[("ratelimit-reset", &(NOW + 40).to_string())]);
        assert_eq!(pause_after(429, &h, now()), Some(Duration::from_secs(40)));
    }

    #[test]
    fn a_spent_budget_pauses_before_the_server_refuses() {
        let h = headers(&[
            ("ratelimit-remaining", "0"),
            ("ratelimit-reset", &(NOW + 20).to_string()),
        ]);
        assert_eq!(pause_after(200, &h, now()), Some(Duration::from_secs(20)));
        let h = headers(&[("ratelimit-remaining", "15")]);
        assert_eq!(pause_after(200, &h, now()), None);
        assert_eq!(pause_after(200, &HeaderMap::new(), now()), None);
        assert_eq!(pause_after(404, &HeaderMap::new(), now()), None);
    }

    #[test]
    fn pauses_are_clamped() {
        let h = headers(&[("retry-after", "86400")]);
        assert_eq!(pause_after(429, &h, now()), Some(MAX_PAUSE));
        let h = headers(&[("retry-after", "0")]);
        assert_eq!(pause_after(429, &h, now()), Some(Duration::from_secs(1)));
    }

    #[tokio::test]
    async fn a_throttled_answer_pauses_the_next_request() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(
                    b"HTTP/1.1 429 Too Many Requests\r\nretry-after: 2\r\n\
                      content-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await;
        });
        let res = reqwest::Client::new().get(&url).send_paced().await.unwrap();
        assert_eq!(res.status().as_u16(), 429);
        let left = pause_left(&url).expect("paused after a 429");
        assert!(left <= Duration::from_secs(2) && left > Duration::from_secs(1));
    }
}
