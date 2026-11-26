use super::*;
use std::collections::HashMap;

mod batching;
mod pipelines;
mod reading;
mod review;

fn ctx(base_url: &str) -> FetchContext {
    FetchContext {
        base_url: base_url.to_string(),
        token: Arc::new(zeroize::Zeroizing::new("t".to_string())),
        project_id: "1".into(),
        branches: vec!["main".into()],
    }
}

/// What a route returns for a 404.
pub const NOT_FOUND: &str = "<404>";

/// A stand-in GitLab answering every request through `route(path, body)`, and
/// recording the paths it was asked for. A [`NOT_FOUND`] body answers 404.
pub async fn fake_gitlab(
    route: fn(&str, &str) -> String,
) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                    let read = socket.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                };
                let head = String::from_utf8_lossy(&raw[..head_end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map_or(0, |v| v.trim().parse::<usize>().unwrap_or(0));
                while raw.len() < head_end + length {
                    let read = socket.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                }
                let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                let path = head.split(' ').nth(1).unwrap_or_default().to_string();
                let request = String::from_utf8_lossy(&raw[head_end..]).to_string();
                let body = route(&path, &request);
                log.lock().unwrap().push(path);
                let (status, body) = if body == NOT_FOUND {
                    ("404 Not Found", "{}".to_string())
                } else {
                    ("200 OK", body)
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (url, seen)
}
