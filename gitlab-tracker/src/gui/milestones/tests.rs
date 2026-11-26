use super::*;

#[tokio::test]
async fn the_demo_milestones_have_their_mrs() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let list = milestones(&app);
    let v240 = list.iter().find(|m| m.title == "v2.4.0").expect("v2.4.0");
    assert!(!v240.items.is_empty());
    // The MR on every branch is delivered.
    let delivered: Vec<&str> = v240
        .items
        .iter()
        .filter(|i| i.stand == Stand::Delivered)
        .map(|i| i.id.as_str())
        .collect();
    let eta = crate::demo::iid("eta-timezone");
    assert!(delivered.contains(&eta.as_str()), "{delivered:?}");
    // The closed MR is left out.
    let closed = crate::demo::iid("juniper-prototype");
    assert!(list.iter().all(|m| m.items.iter().all(|i| i.id != closed)));
    // The security audit is past its date with an MR still open.
    let audit = list.iter().find(|m| m.title == "Security audit").unwrap();
    assert!(audit.items.iter().any(|i| i.stand != Stand::Delivered));
}

#[test]
fn projection_follows_the_pace() {
    let now = board::parse_time("2026-11-05T12:00:00Z").unwrap();
    let m = Milestone {
        title: "v1".into(),
        due: None,
        description: None,
        items: Vec::new(),
    };
    // Nothing left: done today.
    assert_eq!(projection(&m, &[0; WEEKS], now), Some(now.date_naive()));
}

fn item(id: &str, stand: Stand) -> Item {
    Item {
        id: id.into(),
        title: String::new(),
        author: String::new(),
        stand,
        step: NextStep {
            actor: Actor::Reviewers,
            people: Vec::new(),
            mine: false,
            todos: Vec::new(),
            since: None,
            then: Vec::new(),
            reached: None,
            closed: false,
        },
        created: None,
        delivered: None,
        branches: Vec::new(),
        ticket: None,
    }
}

#[test]
fn the_weeks_before_the_first_merge_are_left_out() {
    let now = board::parse_time("2026-11-05T12:00:00Z").unwrap();
    let mut app = crate::app::tests::test_app();
    let merged = |id: &str, days: i64| {
        let mut mr = TrackedMr::placeholder(id.into(), id.into(), None);
        mr.data.state = GitlabMrState::Merged;
        mr.data.merged_at = Some((now - Duration::days(days)).to_rfc3339());
        mr
    };
    // Tracked for three weeks: merges 2 and 16 days ago, none in between.
    app.mrs = vec![merged("1", 2), merged("2", 16), merged("3", 16)];
    // Three weeks, oldest first, the quiet week kept as a real zero.
    assert_eq!(weekly_merges(&app, None, now), vec![2, 0, 1]);
    // The project's merges, when read, win over the tracked MRs'.
    let overview = crate::gitlab_detail::ProjectOverview {
        merges: crate::gitlab_detail::MergeHistory {
            merged: [3, 10, 10, 40]
                .iter()
                .enumerate()
                .map(|(iid, d)| (iid as u64, (now - Duration::days(*d)).to_rfc3339()))
                .collect(),
            read_at: Some(now.to_rfc3339()),
        },
        ..Default::default()
    };
    assert_eq!(
        weekly_merges(&app, Some(&overview), now),
        vec![1, 0, 0, 0, 2, 1]
    );
    app.mrs.clear();
    assert!(weekly_merges(&app, None, now).is_empty());
}

#[test]
fn the_forecast_draws_from_the_recent_weeks() {
    let now = board::parse_time("2026-11-05T12:00:00Z").unwrap();
    let today = now.date_naive();
    let m = Milestone {
        title: "v1".into(),
        due: Some(today + Duration::days(14)),
        description: None,
        items: (0..6)
            .map(|i| item(&i.to_string(), Stand::InReview))
            .collect(),
    };
    // Three a week, every week: six left take two weeks, in every run.
    let f = forecast(&m, &[3; WEEKS], now);
    assert_eq!(f.p50, Some(today + Duration::days(14)));
    assert_eq!(f.p85, f.p50);
    assert_eq!(f.chance, Some(1.0));
    // Nothing delivered lately: no forecast.
    assert_eq!(forecast(&m, &[0; WEEKS], now), Forecast::default());
    // Uneven weeks: the 85 % date is no earlier than the median, the same
    // on every call (seeded).
    // Two weeks of history: too short to draw from.
    assert_eq!(forecast(&m, &[2, 4], now), Forecast::default());
    let uneven = [0, 1, 4, 0, 2, 6, 1, 3];
    let f = forecast(&m, &uneven, now);
    assert!(f.p50.unwrap() <= f.p85.unwrap());
    assert!(f.chance.unwrap() < 1.0);
    assert_eq!(f, forecast(&m, &uneven, now));
}

#[test]
fn a_milestone_past_its_date_has_dates_but_no_chance() {
    let now = board::parse_time("2026-11-26T12:00:00Z").unwrap();
    let today = now.date_naive();
    let m = Milestone {
        title: "Security audit".into(),
        due: Some(today - Duration::days(7)),
        description: None,
        items: vec![item("203", Stand::ToMerge)],
    };
    let f = forecast(&m, &[3; WEEKS], now);
    assert!(f.p85.is_some());
    assert_eq!(
        f.chance, None,
        "a 0 % chance of a date already past says nothing"
    );
}
