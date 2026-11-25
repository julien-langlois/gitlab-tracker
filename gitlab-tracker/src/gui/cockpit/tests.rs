use super::*;
use crate::ui::cockpit::CockpitRow;
use ratatui::style::Color;

fn metric(label: &str) -> CockpitRow {
    CockpitRow::Metric {
        label: label.into(),
        value: "3".into(),
        color: Color::Reset,
        nested: false,
        key: None,
        history: None,
    }
}

fn heading(text: &str) -> CockpitRow {
    CockpitRow::Heading {
        text: text.into(),
        color: Color::Reset,
        alert: false,
    }
}

/// The heights of the cards on each of `frames` frames, in a themed window
/// `width` points wide.
fn heights(cards: &[(&str, Vec<CockpitRow>)], width: f32, frames: usize) -> Vec<Vec<f32>> {
    let theme = Theme::dark();
    let ctx = egui::Context::default();
    ctx.global_style_mut(|style| theme.apply_to_style(style));
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(width, 900.0),
        )),
        ..Default::default()
    };
    let members = HashMap::new();
    let counts = Counts {
        app: None,
        members: &members,
        baseline: None,
        since: None,
        hide_zeros: false,
    };
    (0..frames)
        .map(|_| {
            let mut out = Vec::new();
            let output = ctx.run_ui(input(), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| out = grid(ui, &theme, cards, &[], &counts).1);
            });
            let mut output = output;
            output.textures_delta.clear();
            out
        })
        .collect()
}

#[test]
fn every_card_takes_the_height_of_the_tallest() {
    let cards = vec![
        ("Flow", vec![metric("a"), metric("b"), metric("c")]),
        ("Attention", vec![metric("a"), metric("b")]),
        (
            "Delivery health",
            vec![metric("a"), metric("b"), metric("c")],
        ),
        ("Quality", vec![metric("a")]),
        (
            "Releases",
            vec![heading("v1"), metric("a"), heading("v2"), metric("b")],
        ),
    ];
    for width in [1400.0, 900.0, 400.0] {
        let frames = heights(&cards, width, 4);
        let last = frames.last().unwrap();
        let tallest = last.iter().cloned().fold(0.0, f32::max);
        for h in last {
            assert!((h - tallest).abs() < 0.5, "{width}: {last:?}");
        }
        // Stable: the same heights once settled.
        assert_eq!(frames[2], frames[3], "{width}: {frames:?}");
    }
}

#[test]
fn every_counter_with_a_key_is_explained() {
    let members = crate::ui::cockpit::metric_members(&crate::app::tests::test_app());
    assert!(members.is_empty());
    for key in [
        "Blocked",
        "Conflicts",
        "Stale",
        "Due soon",
        "To review (me)",
        "No ticket",
    ] {
        assert!(explain(key).is_some(), "{key}");
    }
    for (key, _) in URGENCY {
        assert!(explain(key).is_some(), "{key}");
    }
}
