use super::*;

#[tokio::test]
async fn the_demo_sorts_into_the_days_lists() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let today = chrono::Local::now().date_naive();
    let items = classify(&app, "thomas_db", 0, today);
    // Nothing is listed twice, and nothing of someone else's that does not
    // concern me shows.
    let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before);
    for item in &items {
        let mr = app.mrs.iter().find(|m| m.id == item.id).unwrap();
        let concerns_me = mr.author.contains("@thomas_db")
            || mr.author == "thomas_db"
            || mr
                .assignee
                .as_deref()
                .is_some_and(|a| a.contains("@thomas_db"))
            || mr.reviewers.iter().any(|r| r.contains("@thomas_db"))
            || mr
                .merged_by
                .as_deref()
                .is_some_and(|b| b.contains("@thomas_db"));
        assert!(concerns_me, "!{} is not about thomas_db", item.id);
    }
    // Someone with nothing to do gets nothing.
    assert!(classify(&app, "nobody_here", 0, today).is_empty());
}

#[tokio::test]
async fn merged_today_counts_what_i_merged_whoever_wrote_it() {
    let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let today = chrono::Local::now().date_naive();
    let now = chrono::Local::now().to_rfc3339();
    let mut merge = |others: bool, by: &str| {
        let mr = app
            .mrs
            .iter_mut()
            .find(|m| {
                !m.title.is_empty()
                    && m.state == GitlabMrState::Opened
                    && m.author.contains("@thomas_db") != others
            })
            .unwrap();
        mr.data.state = GitlabMrState::Merged;
        mr.data.merged_at = Some(now.clone());
        mr.data.merged_by = Some(by.into());
        mr.id.clone()
    };
    // Someone else's, merged by me; mine, merged by someone else.
    let theirs = merge(true, "Thomas (@thomas_db)");
    let mine = merge(false, "Sam (@sam_c)");
    let merged: Vec<String> = classify(&app, "thomas_db", 0, today)
        .into_iter()
        .filter(|i| i.kind == Kind::Merged)
        .map(|i| i.id)
        .collect();
    assert!(merged.contains(&theirs));
    assert!(!merged.contains(&mine));
    // Every one of them was merged by me, today.
    for id in &merged {
        let mr = app.mrs.iter().find(|m| &m.id == id).unwrap();
        assert!(mr
            .merged_by
            .as_deref()
            .is_some_and(|b| b.contains("@thomas_db")));
    }
}
