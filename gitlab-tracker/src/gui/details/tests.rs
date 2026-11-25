use super::*;

#[test]
fn patches_count_their_lines_not_their_headers() {
    let diff = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-old\n+new\n+more\n same";
    assert_eq!(count(diff), (2, 1));
}

#[test]
fn failures_stand_out() {
    assert!(is_failure("error[E0308]: mismatched types"));
    assert!(is_failure("test result: FAILED. 1 passed; 2 failed"));
    assert!(!is_failure("Compiling gitlab-tracker v0.1.0"));
}

mod snippet_tests {
    use super::snippet;

    #[test]
    fn a_thread_finds_its_line_in_the_patch() {
        let diff = "@@ -10,3 +10,4 @@ fn a()\n ctx\n-old\n+new\n+more\n tail";
        // New lines: 10 ctx, 11 new, 12 more, 13 tail.
        assert_eq!(snippet(diff, 12, 2), ["-old", "+new", "+more"]);
        assert_eq!(snippet(diff, 10, 3), ["@@ -10,3 +10,4 @@ fn a()", " ctx"]);
        assert!(snippet(diff, 40, 2).is_empty());
    }
}

mod split_tests {
    use super::*;

    /// Whether a click at `x` from the left, mid-height, reaches a file's row.
    fn row_clicked_at(x: f32) -> bool {
        let theme = Theme::dark();
        let ctx = egui::Context::default();
        ctx.global_style_mut(|style| theme.apply_to_style(style));
        let file = FileDiff {
            old_path: "src/db/pool.rs".into(),
            new_path: "src/db/pool.rs".into(),
            new_file: false,
            renamed_file: false,
            deleted_file: false,
            diff: "@@ -1 +1 @@\n-a\n+b\n".into(),
        };
        let mut row = egui::Rect::NOTHING;
        let mut clicked = false;
        let at = std::cell::Cell::new(egui::Pos2::ZERO);
        for frame in 0..4 {
            let pos = at.get();
            let events = match frame {
                1 => vec![egui::Event::PointerMoved(pos)],
                2 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                3 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                _ => vec![],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(280.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let top = ui.cursor().min;
                clicked |= file_row(ui, &theme, &file, false, 1, true, true);
                row = egui::Rect::from_min_max(top, egui::pos2(top.x + 280.0, ui.cursor().min.y));
            });
            output.textures_delta.clear();
            at.set(egui::pos2(row.left() + x, row.center().y));
        }
        clicked
    }

    #[test]
    fn a_click_on_a_files_name_selects_it() {
        // On the name, on the counts, and on the blank between them.
        for x in [60.0, 255.0, 160.0] {
            assert!(row_clicked_at(x), "click at {x} missed");
        }
    }

    #[test]
    fn split_rows_face_removed_lines_with_added_ones() {
        let diff = "@@ -10,3 +10,4 @@ fn f\n a\n-b\n+B\n+C\n c\n";
        let rows: Vec<_> = split_rows(diff)
            .into_iter()
            .map(|row| match row {
                SplitRow::Hunk(_) => None,
                SplitRow::Lines(old, new) => Some((old, new)),
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                None,
                Some((Some((10, "a")), Some((10, "a")))),
                Some((Some((11, "b")), Some((11, "B")))),
                Some((None, Some((12, "C")))),
                Some((Some((12, "c")), Some((13, "c")))),
            ]
        );
    }
}
