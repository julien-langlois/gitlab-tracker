use super::*;

/// Where the summary tiles go in a themed window `width` points wide.
fn tiles(width: f32) -> Vec<egui::Rect> {
    let theme = Theme::dark();
    let ctx = egui::Context::default();
    ctx.global_style_mut(|style| theme.apply_to_style(style));
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(width, 900.0),
        )),
        ..Default::default()
    };
    let mut rects = Vec::new();
    let mut output = ctx.run_ui(input, |ui| rects = summary(ui, &theme, &[], 5, &mut None));
    output.textures_delta.clear();
    rects
}

fn ticket() -> LinkedTicket {
    LinkedTicket {
        schema_version: gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION,
        id: "4271".into(),
        subject: "Expose MR metadata".into(),
        status: "In Progress".into(),
        url: "https://tracker.example/issues/4271".into(),
        author: None,
        assignee: Some("Julien Langlois".into()),
        time_estimate: Some(28_800),
        time_spent: Some(18_000),
        time_remaining: None,
        tracker_type: Some("Evolution".into()),
        priority: None,
        version: None,
        start_date: None,
        done_ratio: Some(65),
    }
}

#[test]
fn the_assignee_sits_in_the_middle_of_the_row() {
    let theme = Theme::dark();
    let ctx = egui::Context::default();
    ctx.global_style_mut(|style| theme.apply_to_style(style));
    let row = Row {
        ticket: ticket(),
        mrs: vec![RowMr {
            index: 0,
            id: "104".into(),
            title: "feat: GraphQL".into(),
        }],
    };
    // Wide (progress in its column) and narrow (progress under the subject).
    for width in [1200.0, 600.0] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 900.0),
            )),
            ..Default::default()
        };
        let mut out = None;
        let mut output = ctx.run_ui(input, |ui| out = Some(ticket_row(ui, &theme, &row, false)));
        output.textures_delta.clear();
        let (card, avatar, _) = out.unwrap();
        let avatar = avatar.expect("an assignee");
        let offset = (avatar.center().y - card.rect.center().y).abs();
        assert!(
            offset < 1.0,
            "{width}: card {:?}, avatar {avatar:?}",
            card.rect
        );
    }
}

#[test]
fn summary_tiles_line_up() {
    // Four on a line, then two lines of two on a narrow page.
    for (width, per_line) in [(1200.0, 4), (500.0, 2)] {
        let rects = tiles(width);
        for line in rects.chunks(per_line) {
            for rect in line {
                assert_eq!(rect.top(), line[0].top(), "{width}: {rects:?}");
                assert_eq!(rect.height(), line[0].height(), "{width}: {rects:?}");
                assert!(rect.right() <= width, "{width}: {rects:?}");
                // Name, value and caption stacked, not side by side.
                assert!(rect.height() > 80.0, "{width}: {rects:?}");
            }
        }
    }
}

#[tokio::test]
async fn a_ticket_linked_from_several_mrs_is_one_row() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let rows = rows_by_ticket(app.mrs.iter().enumerate());
    let mut ids: Vec<&str> = rows.iter().map(|r| r.ticket.id.as_str()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "a ticket shows once");
    // The demo's #4302 is split over a stack of three MRs.
    let split = rows.iter().find(|r| r.ticket.id == "4302").expect("#4302");
    assert_eq!(split.mrs.len(), 3);
    let linked = app.mrs.iter().filter(|m| m.linked_ticket.is_some()).count();
    assert_eq!(rows.iter().map(|r| r.mrs.len()).sum::<usize>(), linked);
}
