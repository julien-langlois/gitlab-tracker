//! Pagination of list endpoints.

use super::*;

/// Items per page requested from list endpoints (GitLab's maximum).
pub(super) const PAGE_SIZE: usize = 100;
/// Safety cap on the pages read by [`get_all_pages`] (5 000 items).
pub(super) const MAX_PAGES: u32 = 50;

/// Page to request after reading `page` with `len` items, or `None` when done:
/// a short page is the last one, and [`MAX_PAGES`] bounds a runaway listing.
pub(super) fn next_page(page: u32, len: usize) -> Option<u32> {
    (len >= PAGE_SIZE && page < MAX_PAGES).then_some(page + 1)
}

/// GETs every page of a GitLab list endpoint. `url` already carries its query
/// string (`…?state=active`); `per_page` and `page` are appended.
///
/// Returns `None` when any page fails: a partial list must not be mistaken for a
/// complete one (the discovery poll relies on it).
pub async fn get_all_pages<T: serde::de::DeserializeOwned>(
    ctx: &FetchContext,
    url: &str,
) -> Option<Vec<T>> {
    let client = http_client();
    let mut items = Vec::new();
    let mut page = 1;
    loop {
        let res = client
            .get(format!("{url}&per_page={PAGE_SIZE}&page={page}"))
            .header("PRIVATE-TOKEN", ctx.token.as_str())
            .send_paced()
            .await
            .ok()
            .filter(|r| r.status().is_success())?;
        let batch: Vec<T> = res.json().await.ok()?;
        let len = batch.len();
        items.extend(batch);
        match next_page(page, len) {
            Some(next) => page = next,
            None => {
                if len >= PAGE_SIZE {
                    tracing::warn!(url, "List truncated at the pagination safety cap");
                }
                return Some(items);
            }
        }
    }
}
