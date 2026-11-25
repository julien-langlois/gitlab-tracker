use super::*;

#[test]
fn a_valid_form_is_cleaned() {
    let checked = check(
        " https://gitlab.example/ ",
        " group/app/ ",
        "  My   App ",
        "main, staging,,prod ",
    )
    .unwrap();
    assert_eq!(
        checked,
        Checked {
            url: "https://gitlab.example".into(),
            project_id: "group/app".into(),
            name: Some("My App".into()),
            branches: vec!["main".into(), "staging".into(), "prod".into()],
        }
    );
    // No branch: the project's default branch, read from GitLab.
    assert!(check("https://g.io", "42", "", " ")
        .unwrap()
        .branches
        .is_empty());
}

#[test]
fn mistakes_are_named() {
    assert_eq!(check("gitlab.com", "42", "", "main").unwrap_err().len(), 1);
    assert_eq!(check("https://", "42", "", "main").unwrap_err().len(), 1);
    assert_eq!(
        check("https://g.io", "my project", "", "main")
            .unwrap_err()
            .len(),
        1
    );
    assert_eq!(
        check("https://g.io", "", "", "release 1")
            .unwrap_err()
            .len(),
        2
    );
}

#[test]
fn gitlab_fills_what_was_left_empty() {
    let info = crate::gitlab::ProjectInfo {
        id: 42,
        name: "App".into(),
        path_with_namespace: "group/app".into(),
        default_branch: Some("develop".into()),
    };
    let checked = check("https://g.io", "group/app", "", "").unwrap();
    let entry = entry_for(checked, &info, Some("https://redmine.io".into()));
    assert_eq!(entry.name.as_deref(), Some("App"));
    assert_eq!(entry.tracked_branches, Some(vec!["develop".to_string()]));
    assert_eq!(entry.tracker.unwrap().url, "https://redmine.io");
    assert!(!entry.active);
}
