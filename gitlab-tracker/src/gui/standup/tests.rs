use super::*;

#[tokio::test]
async fn each_mr_goes_in_one_section() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let since = chrono::Utc::now() - chrono::Duration::days(2);
    let of = |title: &str, me: &str| {
        let mr = app.mrs.iter().find(|m| m.title.starts_with(title)).unwrap();
        done(mr, me, since)
    };
    // Alex's hotfix: I reviewed and merged it, it shows as merged only.
    assert_eq!(
        of("fix(auth): Refresh tokens raced", "thomas_db"),
        Some(Section::ReviewedMerged)
    );
    // Julien's draft: I commented and pushed to it, the push wins.
    assert_eq!(
        of("Draft: feat(notif): Mute notifications", "thomas_db"),
        Some(Section::Pushed)
    );
    // Marina's refactor: Julien answered his review with a commit.
    assert_eq!(
        of("refactor(api): One module per shipments route", "julien_m"),
        Some(Section::Pushed)
    );
    // My own MR merged: merged, never "reviewed & merged".
    assert_eq!(
        of("fix(api): 404, not 500", "thomas_db"),
        Some(Section::Merged)
    );
    // Commented only: reviewed.
    let mut mute = app
        .mrs
        .iter()
        .find(|m| m.title.starts_with("Draft: feat(notif): Mute"))
        .unwrap()
        .clone();
    mute.review
        .as_mut()
        .unwrap()
        .last_push_by
        .remove("thomas_db");
    assert_eq!(done(&mute, "thomas_db", since), Some(Section::Reviewed));
}

#[test]
fn it_looks_back_to_the_last_working_day() {
    use chrono::TimeZone;
    let at = |y, m, d| {
        chrono::Local
            .with_ymd_and_hms(y, m, d, 9, 30, 0)
            .earliest()
            .unwrap()
    };
    let day = |t: chrono::DateTime<chrono::Utc>| t.with_timezone(&chrono::Local).date_naive();
    // Monday 2026-11-09 → Friday; Wednesday → Tuesday.
    assert_eq!(
        day(last_working_day(at(2026, 11, 9))),
        chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
    );
    assert_eq!(
        day(last_working_day(at(2026, 11, 11))),
        chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
    );
    // "Since the last copy" before any copy: the last working day too.
    let s = Settings {
        period: Period::SinceLastCopy,
        ..Settings::default()
    };
    assert_eq!(
        s.since(at(2026, 11, 11)),
        last_working_day(at(2026, 11, 11))
    );
}

fn sample() -> Standup {
    let mut groups: Vec<(Section, Vec<String>)> = Section::DONE
        .iter()
        .chain(&Section::TODAY)
        .chain(&[Section::Waiting])
        .map(|s| (*s, Vec::new()))
        .collect();
    groups[0]
        .1
        .push("[!12](https://gl/mr/12) ([#7](https://rm/7)) Fix it".into());
    groups[6]
        .1
        .push("[!13](https://gl/mr/13) Mine — CI failed".into());
    groups[8].1.push("[!14](https://gl/mr/14) Theirs".into());
    Standup {
        date: chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
        since: "Since 2026-11-23".into(),
        groups,
    }
}

#[test]
fn each_format_writes_its_own_markup() {
    let s = sample();
    let settings = Settings::default();
    let md = render(&lines(&s, &settings), Format::Markdown);
    assert!(md.starts_with("**Stand-up · 2026-11-24**\n\n_Since 2026-11-23_\nMerged:\n"));
    assert!(md.contains("- [!12](https://gl/mr/12) ([#7](https://rm/7)) Fix it\n"));
    let slack = render(&lines(&s, &settings), Format::Slack);
    assert!(slack.starts_with("*Stand-up · 2026-11-24*\n"));
    assert!(slack.contains("• <https://gl/mr/12|!12> (<https://rm/7|#7>) Fix it\n"));
    let plain = render(&lines(&s, &settings), Format::Plain);
    assert!(plain.starts_with("Stand-up · 2026-11-24\n\nSince 2026-11-23\n"));
    assert!(plain.contains("- !12 (#7) Fix it\n"));
    assert!(!plain.contains("http"));
}

#[test]
fn hidden_sections_are_left_out() {
    let s = sample();
    let settings = Settings {
        hidden: Section::Merged.bit() | Section::Waiting.bit(),
        ..Settings::default()
    };
    let text = render(&lines(&s, &settings), Format::Plain);
    assert!(!text.contains("!12"));
    assert!(text.contains("Nothing on GitLab."));
    assert!(text.contains("!13"));
    assert!(!text.contains("!14"));
}

#[test]
fn only_real_links_are_cut_out() {
    assert_eq!(
        pieces("[WIP] a [!1](https://x/1) b"),
        vec![
            ("[", None),
            ("WIP] a ", None),
            ("!1", Some("https://x/1")),
            (" b", None)
        ]
    );
}
