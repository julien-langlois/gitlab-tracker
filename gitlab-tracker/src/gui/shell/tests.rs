use super::*;

#[test]
fn last_update_reads_in_hours_then_days() {
    assert_eq!(updated_ago(0), "Updated just now");
    assert_eq!(updated_ago(59), "Updated 59 min ago");
    assert_eq!(updated_ago(60), "Updated 1h ago");
    assert_eq!(updated_ago(23 * 60 + 59), "Updated 23h ago");
    assert_eq!(updated_ago(24 * 60), "Updated 1 day ago");
    // A window left open over a weekend.
    assert_eq!(updated_ago(3785), "Updated 2 days ago");
}

#[test]
fn side_panels_follow_the_window_width() {
    // A small laptop window: only the table.
    let small = Sizes::for_width(1024.0, None, None);
    assert_eq!((small.views, small.inspector), (None, None));
    assert!(small.compact);
    let close = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 0.01);
    // The mockup's 1440 points: both panels, near their design widths.
    let laptop = Sizes::for_width(1440.0, None, None);
    assert!(close(laptop.views, 230.4), "{laptop:?}");
    assert!(close(laptop.inspector, 316.8), "{laptop:?}");
    // 1080p, or 4K at 200 %: wider panels.
    let hd = Sizes::for_width(1920.0, None, None);
    assert!(close(hd.views, 280.0), "{hd:?}");
    assert!(close(hd.inspector, 422.4), "{hd:?}");
    // Ultrawide 4K at 100 %: the panels stop growing, the table takes the room.
    let ultrawide = Sizes::for_width(5120.0, None, None);
    assert_eq!(
        (ultrawide.views, ultrawide.inspector),
        (Some(280.0), Some(460.0))
    );
}

#[test]
fn the_tracker_page_widens_the_inspector() {
    let widened = |width: f32| {
        let mut sizes = Sizes::for_width(width, None, Some(true));
        sizes.widen_inspector(width);
        sizes.inspector.unwrap()
    };
    // 1080p: much wider, the list keeps over 1200 points.
    assert!((widened(1920.0) - 576.0).abs() < 0.01);
    // A laptop: wider, the list keeps its minimum.
    assert!(1440.0 - RAIL_WIDTH - widened(1440.0) >= TRACKER_LIST_MIN_WIDTH);
    assert!(widened(1440.0) > 400.0);
    // A small window: a little wider, the list still keeps its minimum.
    assert!((widened(1024.0) - 332.0).abs() < 0.01);
    // Too narrow to widen: the usual inspector.
    assert!((widened(900.0) - 300.0).abs() < 0.01);
    assert_eq!(widened(5120.0), 580.0);
}

#[test]
fn toggles_override_the_width() {
    let forced = Sizes::for_width(1024.0, Some(true), Some(true));
    assert_eq!((forced.views, forced.inspector), (Some(220.0), Some(300.0)));
    let hidden = Sizes::for_width(2560.0, Some(false), Some(false));
    assert_eq!((hidden.views, hidden.inspector), (None, None));
}
