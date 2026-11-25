//! The files as a tree of folders, their widths, rows and totals.

use super::*;

/// Narrowest and widest the Changes tree goes beside its patch.
pub(super) const TREE_MIN_WIDTH: f32 = 240.0;
pub(super) const TREE_MAX_WIDTH: f32 = 520.0;
/// Between the tree and its patch.
pub(super) const COLUMN_GAP: f32 = 14.0;
/// Between the top of the view and the patch stuck to it.
pub(super) const STICKY_GAP: f32 = 8.0;

/// The width the rows of `folder` (`depth` deep) take untruncated, as
/// [`FileList::folder`] draws them.
pub(super) fn tree_width(
    ui: &Ui,
    theme: &Theme,
    folder: &Folder,
    files: &HashMap<&str, &FileDiff>,
    depth: usize,
) -> f32 {
    let text = |text: String, font: egui::FontId| {
        ui.fonts_mut(|f| f.layout_no_wrap(text, font, Color32::WHITE).size().x)
    };
    // The row's margins and the spaces between its parts.
    let indent = depth as f32 * TREE_INDENT + 8.0;
    let mut widest: f32 = 0.0;
    for (name, mut sub) in &folder.folders {
        let mut label = (*name).to_string();
        while sub.files.is_empty() && sub.folders.len() == 1 {
            let (next, deeper) = sub.folders.iter().next().expect("one folder");
            label = format!("{label}/{next}");
            sub = deeper;
        }
        let chevron = text("▾".into(), egui::FontId::proportional(theme.text_size)) + 6.0;
        let row =
            indent + chevron + text(format!("{label}/"), fonts::medium(theme.text_size - 1.0));
        widest = widest
            .max(row)
            .max(tree_width(ui, theme, sub, files, depth + 1));
    }
    for path in &folder.files {
        let Some(file) = files.get(path) else {
            continue;
        };
        let (added, deleted) = count(&file.diff);
        let name = path.rsplit('/').next().unwrap_or(path);
        let counts = egui::FontId::monospace(theme.text_size - 2.0);
        let row = indent
            + text("M".into(), egui::FontId::monospace(theme.text_size - 1.5))
            + 6.0
            + text(name.to_string(), fonts::medium(theme.text_size - 1.0))
            + 10.0
            + text(format!("+{added}"), counts.clone())
            + 4.0
            + text(format!("−{deleted}"), counts);
        widest = widest.max(row);
    }
    widest
}

/// The tree's first file, in the order it is drawn.
pub(super) fn first_file<'a>(folder: &Folder<'a>) -> Option<&'a str> {
    folder
        .folders
        .values()
        .find_map(first_file)
        .or_else(|| folder.files.first().copied())
}

/// Indentation of one level of the Changes tree.
pub(super) const TREE_INDENT: f32 = 18.0;

/// What drawing the Changes files needs and gives back: the files unfolded, the
/// one to scroll to; the one clicked and where the scrolled-to one is.
pub(super) struct FileList<'a> {
    pub(super) mr_id: &'a str,
    pub(super) web_url: &'a str,
    pub(super) open_files: &'a HashSet<(String, String)>,
    pub(super) scroll_to: Option<&'a str>,
    pub(super) toggle: &'a mut Option<(String, String)>,
    pub(super) target: &'a mut Option<egui::Rect>,
    /// The tree beside the patch: a file is selected (`open_files`), not unfolded.
    pub(super) columns: bool,
}

impl FileList<'_> {
    /// A file's row, `depth` deep, and its patch when unfolded (beside the tree,
    /// in columns).
    pub(super) fn file(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        file: &FileDiff,
        depth: usize,
        name_only: bool,
    ) {
        let key = (self.mr_id.to_string(), file.new_path.clone());
        let open = self.open_files.contains(&key);
        let top = ui.cursor().min;
        if file_row(ui, theme, file, open, depth, name_only, self.columns) {
            *self.toggle = Some(key);
        }
        if self.scroll_to == Some(file.new_path.as_str()) {
            *self.target = Some(egui::Rect::from_min_max(
                top,
                egui::pos2(ui.max_rect().right(), ui.cursor().min.y),
            ));
        }
        if open && !self.columns {
            let indent = depth as f32 * TREE_INDENT;
            let top = ui.cursor().min.y;
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                ui.vertical(|ui| patch(ui, theme, &file.diff, self.web_url));
            });
            guides(ui, theme, depth, top, ui.cursor().min.y);
        }
    }

    /// The rows of `folder` (at `path`, `depth` deep): each sub-folder (a chain
    /// of folders holding only one folder on one row, as `src/api/graphql`),
    /// foldable, its content under it; then its files.
    pub(super) fn folder(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        folder: &Folder,
        files: &HashMap<&str, &FileDiff>,
        path: &str,
        depth: usize,
    ) {
        for (name, mut sub) in &folder.folders {
            let mut label = (*name).to_string();
            while sub.files.is_empty() && sub.folders.len() == 1 {
                let (next, deeper) = sub.folders.iter().next().expect("one folder");
                label = format!("{label}/{next}");
                sub = deeper;
            }
            let full = format!("{path}{label}/");
            let id = egui::Id::unique(("changes folder", self.mr_id, &full));
            let folded = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
            if folder_row(ui, theme, &label, folded, depth, sub, files) {
                ui.data_mut(|d| d.insert_temp(id, !folded));
            }
            if !folded {
                self.folder(ui, theme, sub, files, &full, depth + 1);
            }
        }
        for path in &folder.files {
            if let Some(file) = files.get(path) {
                self.file(ui, theme, file, depth, true);
            }
        }
    }
}

/// The tree's guides, one per level above `depth`, from `top` to `bottom`.
pub(super) fn guides(ui: &Ui, theme: &Theme, depth: usize, top: f32, bottom: f32) {
    let left = ui.max_rect().left();
    for level in 0..depth {
        let x = left + level as f32 * TREE_INDENT + 10.0;
        ui.painter().vline(
            x,
            egui::Rangef::new(top, bottom),
            egui::Stroke::new(1.0, theme.palette.text_muted.gamma_multiply(0.45)),
        );
    }
}

/// A folder of the tree: its chevron, its name, what changed under it; `true`
/// when clicked.
pub(super) fn folder_row(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    folded: bool,
    depth: usize,
    folder: &Folder,
    files: &HashMap<&str, &FileDiff>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
    }
    guides(ui, theme, depth, rect.top() - 1.0, rect.bottom() + 1.0);
    let indent = depth as f32 * TREE_INDENT;
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(
                rect.shrink2(Vec2::new(4.0, 0.0))
                    .with_min_x(rect.left() + 4.0 + indent),
            )
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The row takes the click: its texts, selectable, would take it over them.
    row.style_mut().interaction.selectable_labels = false;
    row.spacing_mut().item_spacing.x = 6.0;
    row.label(muted(theme, if folded { "▸" } else { "▾" }));
    let (inside, added, deleted) = folder_totals(folder, files);
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if folded {
            ui.label(mono(format!("−{deleted}"), theme.text_size - 2.0).color(theme.tones.red.fg));
            ui.label(mono(format!("+{added}"), theme.text_size - 2.0).color(theme.tones.green.fg));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{label}/"))
                        .font(fonts::medium(theme.text_size - 1.0))
                        .color(theme.palette.text_secondary),
                )
                .truncate(),
            );
            if folded {
                ui.label(muted(theme, tn(inside as u64, "{n} file", "{n} files")));
            }
        });
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tl(if folded { "Unfold" } else { "Fold" }))
        .clicked()
}

/// Files under `folder`, at any depth, and the lines they add and remove.
pub(super) fn folder_totals(
    folder: &Folder,
    files: &HashMap<&str, &FileDiff>,
) -> (usize, usize, usize) {
    let mut totals = (0, 0, 0);
    for path in &folder.files {
        if let Some(file) = files.get(path) {
            let (a, d) = count(&file.diff);
            totals = (totals.0 + 1, totals.1 + a, totals.2 + d);
        }
    }
    for sub in folder.folders.values() {
        let (n, a, d) = folder_totals(sub, files);
        totals = (totals.0 + n, totals.1 + a, totals.2 + d);
    }
    totals
}

/// A file: its state, its path (its name alone in the tree) and its counts;
/// `true` when clicked. In `columns`, `open` is the file selected (no chevron).
pub(super) fn file_row(
    ui: &mut Ui,
    theme: &Theme,
    file: &FileDiff,
    open: bool,
    depth: usize,
    name_only: bool,
    columns: bool,
) -> bool {
    let (added, deleted) = count(&file.diff);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    if columns && open {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.palette.raised);
    } else if response.hovered() {
        let fill = if columns {
            theme.palette.raised.gamma_multiply(0.6)
        } else {
            theme.palette.raised
        };
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    guides(ui, theme, depth, rect.top() - 1.0, rect.bottom() + 1.0);
    let indent = depth as f32 * TREE_INDENT;
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(
                rect.shrink2(Vec2::new(4.0, 0.0))
                    .with_min_x(rect.left() + 4.0 + indent),
            )
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The row takes the click: its texts, selectable, would take it over them.
    row.style_mut().interaction.selectable_labels = false;
    row.spacing_mut().item_spacing.x = 6.0;
    if !columns {
        row.label(muted(theme, if open { "▾" } else { "▸" }));
    }
    let (mark, color) = if file.new_file {
        ("A", theme.tones.green.fg)
    } else if file.deleted_file {
        ("D", theme.tones.red.fg)
    } else if file.renamed_file {
        ("R", theme.tones.blue.fg)
    } else {
        ("M", theme.tones.amber.fg)
    };
    row.label(mono(mark, theme.text_size - 1.5).color(color));
    let path = row
        .with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(mono(format!("−{deleted}"), theme.text_size - 2.0).color(theme.tones.red.fg));
            ui.label(mono(format!("+{added}"), theme.text_size - 2.0).color(theme.tones.green.fg));
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let path = if file.renamed_file {
                    format!("{} → {}", file.old_path, file.new_path)
                } else {
                    file.new_path.clone()
                };
                // The file name stands out from its folder (given by the tree).
                let (dir, name) = match path.rsplit_once('/') {
                    Some(_) if name_only => {
                        let name = file.new_path.rsplit('/').next().unwrap_or(&file.new_path);
                        (String::new(), name.to_string())
                    }
                    Some((dir, name)) => (format!("{dir}/"), name.to_string()),
                    None => (String::new(), path.clone()),
                };
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &dir,
                    0.0,
                    egui::TextFormat::simple(
                        egui::FontId::proportional(theme.text_size - 1.5),
                        theme.palette.text_secondary,
                    ),
                );
                job.append(
                    &name,
                    0.0,
                    egui::TextFormat::simple(
                        fonts::medium(theme.text_size - 1.0),
                        theme.palette.text,
                    ),
                );
                ui.add(egui::Label::new(job).truncate());
                path
            })
            .inner
        })
        .inner;
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(Tip::new().mono(path))
        .clicked()
}
