//! HTTP client settings shared by every API client that sends a token.

/// Redirect policy for clients that authenticate with a custom header
/// (`PRIVATE-TOKEN`, `X-Redmine-API-Key`).
///
/// reqwest strips `Authorization` and cookies when a redirect leaves the host, but
/// keeps every other header: a redirect to another server would hand it the token.
/// Redirects are followed only while the scheme, host and port stay the same
/// (at most 5 hops); any other redirect fails the request instead.
pub fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let leaves_origin = attempt.previous().last().is_some_and(|prev| {
            let next = attempt.url();
            prev.scheme() != next.scheme()
                || prev.host_str() != next.host_str()
                || prev.port_or_known_default() != next.port_or_known_default()
        });
        if leaves_origin {
            let to = attempt.url().origin().ascii_serialization();
            attempt.error(format!(
                "redirect to another server ({to}) refused: it would receive the API token"
            ))
        } else if attempt.previous().len() > 5 {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves one request: answers with a redirect to `location`, and returns the
    /// request it received.
    async fn redirecting_server(location: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            sock.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (format!("http://{addr}"), task)
    }

    #[tokio::test]
    async fn redirect_to_another_server_is_refused() {
        // 127.0.0.2 is another host than 127.0.0.1: the token must not follow.
        let (url, server) = redirecting_server("http://127.0.0.2:9/steal".into()).await;
        let client = reqwest::Client::builder()
            .redirect(same_origin_redirects())
            .build()
            .unwrap();
        let err = client
            .get(&url)
            .header("PRIVATE-TOKEN", "secret")
            .send()
            .await
            .unwrap_err();
        assert!(err.is_redirect(), "{err}");
        assert!(server.await.unwrap().contains("private-token: secret"));
    }
}
