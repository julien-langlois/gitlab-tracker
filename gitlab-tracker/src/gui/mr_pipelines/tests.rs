use super::*;

#[test]
fn the_project_is_read_off_the_mr_url() {
    assert_eq!(
        project_url("https://gitlab.com/shop/api/-/merge_requests/104").as_deref(),
        Some("https://gitlab.com/shop/api")
    );
    assert_eq!(project_url("not a url"), None);
}

#[test]
fn durations_read_as_in_gitlab() {
    assert_eq!(long_duration(42.4), "42s");
    assert_eq!(long_duration(185.0), "3m 05s");
    assert_eq!(long_duration(3720.0), "1h 2m");
}
