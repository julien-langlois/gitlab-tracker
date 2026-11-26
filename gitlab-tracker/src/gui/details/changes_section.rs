//! The Changes section: the files, each unfolding to its patch, or a tree with the selected patch beside it.

use super::*;

/// The "Changes" section's body: a button until loaded, then the files, each
/// unfolding to its patch; or, when `tree`, as in GitLab, the files as a tree of
/// folders on the left and the patch of the one selected on the right.
pub fn changes(
    details: &mut Details,
    app: &App,
    ui: &mut Ui,
    theme: &Theme,
    mr_id: &str,
    web_url: &str,
    tree: bool,
) {
    let key = mr_id.to_string();
    if let Some(fetch) = details.diffs.get_mut(&key) {
        fetch.poll();
    }
    match details.diffs.get(&key) {
        None => {
            if button(ui, theme, None, tl("Show the changed files"), false).clicked() {
                details.load_diff(app, mr_id, ui.ctx());
            }
        }
        Some(Fetch::Loading(_)) => {
            ui.horizontal(|ui| {
                spinner(ui, 12.0, theme.palette.accent);
                ui.label(muted(theme, tl("Reading the diff…")));
            });
        }
        Some(Fetch::Failed(error)) => {
            ui.label(RichText::new(format!("⚠ {error}")).color(theme.tones.red.fg));
            if button(ui, theme, None, tl("Retry"), false).clicked() {
                details.load_diff(app, mr_id, ui.ctx());
            }
        }
        Some(Fetch::Ready(files)) => {
            if files.is_empty() {
                ui.label(muted(theme, tl("No file changed.")));
                return;
            }
            let mut toggle = None;
            let mut target = None;
            let scroll_to = details
                .scroll_to
                .take_if(|(id, _)| id == mr_id)
                .map(|(_, path)| path);
            if tree {
                let by_path: HashMap<&str, &FileDiff> =
                    files.iter().map(|f| (f.new_path.as_str(), f)).collect();
                let root = Folder::of(files.iter().map(|f| f.new_path.as_str()));
                // The file selected, else the tree's first.
                let selected = details
                    .selected
                    .get(mr_id)
                    .map(String::as_str)
                    .filter(|path| by_path.contains_key(path))
                    .or_else(|| first_file(&root));
                let shown = selected.and_then(|path| by_path.get(path).copied());
                let top = ui.cursor().min;
                let open: HashSet<(String, String)> = selected
                    .map(|path| (mr_id.to_string(), path.to_string()))
                    .into_iter()
                    .collect();
                let mut list = FileList {
                    mr_id,
                    web_url,
                    open_files: &open,
                    scroll_to: None,
                    toggle: &mut toggle,
                    target: &mut target,
                    columns: true,
                };
                let width = ui.available_width();
                // As wide as the widest row (long paths, as Drupal's), within
                // reason: the patch keeps most of the width.
                let left = tree_width(ui, theme, &root, &by_path, 0).clamp(
                    TREE_MIN_WIDTH,
                    (width * 0.42).clamp(TREE_MIN_WIDTH, TREE_MAX_WIDTH),
                );
                if let Some(side_by_side) = changes_header(ui, theme, files, details.side_by_side) {
                    details.side_by_side = side_by_side;
                }
                let side_by_side = details.side_by_side;
                let origin = ui.cursor().min;
                let mut tree_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(
                            origin,
                            Vec2::new(left, f32::INFINITY),
                        ))
                        .layout(Layout::top_down(Align::Min)),
                );
                tree_ui.set_width(left);
                tree_ui.spacing_mut().item_spacing.y = 2.0;
                list.folder(&mut tree_ui, theme, &root, &by_path, "", 0);
                let tree_height = tree_ui.min_rect().height();
                // The patch sticks to the top of the view while the tree scrolls
                // by, as far as the tree goes: a file picked low in a long tree
                // shows at once. Placed with its height of the last frame.
                let height_id = egui::Id::unique(("changes patch height", mr_id));
                let last_height = ui.data(|d| d.get_temp::<f32>(height_id)).unwrap_or(0.0);
                let offset = (ui.clip_rect().top() + STICKY_GAP - origin.y)
                    .clamp(0.0, (tree_height - last_height).max(0.0));
                let patch_left = origin.x + left + COLUMN_GAP;
                let mut patch_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(
                            egui::pos2(patch_left, origin.y + offset),
                            Vec2::new(width - left - COLUMN_GAP, f32::INFINITY),
                        ))
                        .layout(Layout::top_down(Align::Min)),
                );
                patch_ui.set_width(width - left - COLUMN_GAP);
                match shown {
                    Some(file) if side_by_side => {
                        split_patch(&mut patch_ui, theme, &file.diff, web_url)
                    }
                    Some(file) => patch(&mut patch_ui, theme, &file.diff, web_url),
                    None => {}
                }
                let patch_height = patch_ui.min_rect().height();
                if (patch_height - last_height).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(height_id, patch_height));
                    ui.ctx().request_repaint();
                }
                ui.allocate_rect(
                    egui::Rect::from_min_size(
                        origin,
                        Vec2::new(width, tree_height.max(offset + patch_height)),
                    ),
                    Sense::hover(),
                );
                // A file asked from elsewhere: the two columns at the top of the page.
                if scroll_to.is_some() {
                    ui.scroll_to_rect(
                        egui::Rect::from_min_max(top, egui::pos2(top.x + width, top.y + 40.0)),
                        Some(Align::TOP),
                    );
                }
                if let Some((_, path)) = toggle {
                    details.selected.insert(mr_id.to_string(), path);
                }
                return;
            }
            let mut list = FileList {
                mr_id,
                web_url,
                open_files: &details.open_files,
                scroll_to: scroll_to.as_deref(),
                toggle: &mut toggle,
                target: &mut target,
                columns: false,
            };
            for file in files {
                list.file(ui, theme, file, 0, false);
            }
            // The file's row at the top of the page (as far as the page goes).
            if let Some(row) = target {
                ui.scroll_to_rect(row, Some(Align::TOP));
            }
            if let Some(key) = toggle {
                if !details.open_files.remove(&key) {
                    details.open_files.insert(key);
                }
            }
        }
    }
}

/// Over the tree and its patch: how many files and lines changed, then the
/// choice between the unified patch and the side-by-side one (`Some` when
/// another is clicked).
pub(super) fn changes_header(
    ui: &mut Ui,
    theme: &Theme,
    files: &[FileDiff],
    side_by_side: bool,
) -> Option<bool> {
    let (added, deleted) = files.iter().fold((0, 0), |(a, d), file| {
        let (fa, fd) = count(&file.diff);
        (a + fa, d + fd)
    });
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(muted(
            theme,
            tn(files.len() as u64, "{n} file", "{n} files"),
        ));
        ui.label(muted(theme, "·"));
        ui.label(mono(format!("+{added}"), theme.text_size - 2.0).color(theme.tones.green.fg));
        ui.label(mono(format!("−{deleted}"), theme.text_size - 2.0).color(theme.tones.red.fg));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Right to left: the last first.
            for (mode, label) in [(true, tl("Side by side")), (false, tl("Unified"))] {
                let on = mode == side_by_side;
                let text = if on {
                    RichText::new(label)
                        .font(fonts::medium(theme.text_size - 1.0))
                        .color(theme.palette.text)
                } else {
                    RichText::new(label)
                        .size(theme.text_size - 1.0)
                        .color(theme.palette.text_muted)
                };
                let response = ui.add(egui::Label::new(text).sense(Sense::click()));
                if !on {
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        chosen = Some(mode);
                    }
                }
                if mode {
                    ui.label(muted(theme, "·"));
                }
            }
        });
    });
    ui.add_space(6.0);
    chosen
}
