//! Export: the MRs of the table as a CSV file, a Markdown table or a list to
//! paste in a chat — the rows picked, those of the view, or every tracked MR,
//! with the columns shown (or all of them). Copied to the clipboard, or saved
//! through the system's file dialog (`rfd`), off the window's thread.

use std::sync::mpsc::{channel, Receiver};

use bevel_theme::{fonts, Theme};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};
use serde::{Deserialize, Serialize};

use super::icons::{self, Icon};
use super::widgets::{button, muted, pill_toggle, switch};
use super::GuiApp;
use crate::i18n::{tf, tl, tn};
use crate::models::TrackedMr;

/// What the export writes.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Csv,
    #[default]
    Markdown,
    /// One line per MR, to paste in a chat or a comment.
    Chat,
}

impl Format {
    const ALL: [Format; 3] = [Format::Csv, Format::Markdown, Format::Chat];

    fn label(self) -> &'static str {
        match self {
            Format::Csv => tl("CSV"),
            Format::Markdown => tl("Markdown table"),
            Format::Chat => tl("Chat list"),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Markdown | Format::Chat => "md",
        }
    }
}

/// Which MRs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rows {
    /// Those picked with Ctrl / Shift + click or in bulk mode.
    Picked,
    /// Those the table shows now.
    #[default]
    View,
    /// Every tracked MR.
    All,
}

/// The export's choices, kept with the window's preferences: the shortcut
/// exports again the same way.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub format: Format,
    /// Every column, not only those shown.
    pub all_columns: bool,
    /// The MR numbers link to GitLab (a URL column in CSV).
    pub links: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: Format::Markdown,
            all_columns: false,
            links: true,
        }
    }
}

/// The MRs to export, as text: the headings after the MR column, and a row per MR.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub url: String,
    /// One per heading; the first is the title.
    pub cells: Vec<String>,
}

/// `table` written as `format`.
pub fn render(table: &Table, format: Format, links: bool) -> String {
    match format {
        Format::Csv => csv(table, links),
        Format::Markdown => markdown(table, links),
        Format::Chat => chat(table, links),
    }
}

fn csv(table: &Table, links: bool) -> String {
    fn field(text: &str) -> String {
        if text.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.to_string()
        }
    }
    let mut lines = Vec::with_capacity(table.rows.len() + 1);
    let mut head = vec![field(tl("MR"))];
    head.extend(table.headers.iter().map(|h| field(h)));
    if links {
        head.push(field(tl("URL")));
    }
    lines.push(head.join(","));
    for row in &table.rows {
        let mut cells = vec![field(&format!("!{}", row.id))];
        cells.extend(row.cells.iter().map(|c| field(c)));
        if links {
            cells.push(field(&row.url));
        }
        lines.push(cells.join(","));
    }
    // CRLF, as RFC 4180 and spreadsheets expect.
    lines.join("\r\n") + "\r\n"
}

fn markdown(table: &Table, links: bool) -> String {
    fn cell(text: &str) -> String {
        text.replace('|', "\\|").replace(['\n', '\r'], " ")
    }
    let mut out = String::new();
    let mut head = vec![tl("MR").to_string()];
    head.extend(table.headers.iter().map(|h| cell(h)));
    out.push_str(&format!("| {} |\n", head.join(" | ")));
    out.push_str(&format!("|{}\n", "---|".repeat(head.len())));
    for row in &table.rows {
        let id = if links && !row.url.is_empty() {
            format!("[!{}]({})", row.id, row.url)
        } else {
            format!("!{}", row.id)
        };
        let mut cells = vec![id];
        cells.extend(row.cells.iter().map(|c| cell(c)));
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

fn chat(table: &Table, links: bool) -> String {
    let mut out = String::new();
    for row in &table.rows {
        let title = row
            .cells
            .first()
            .map(|t| t.replace(['\n', '\r'], " "))
            .unwrap_or_default();
        let rest: Vec<&str> = row
            .cells
            .iter()
            .skip(1)
            .map(String::as_str)
            .filter(|c| !c.is_empty())
            .collect();
        out.push_str(&format!("- !{} {title}", row.id));
        if !rest.is_empty() {
            out.push_str(&format!(" — {}", rest.join(" · ")));
        }
        if links && !row.url.is_empty() {
            out.push_str(&format!(" {}", row.url));
        }
        out.push('\n');
    }
    out
}

/// The MRs `rows` takes, in the table's order (then the others, as tracked).
fn pick(g: &GuiApp, rows: Rows) -> Vec<&TrackedMr> {
    let app = &g.app;
    let shown: Vec<&TrackedMr> = app.visible_mrs().collect();
    match rows {
        Rows::View => shown,
        Rows::All => {
            let mut all = shown.clone();
            all.extend(
                app.mrs
                    .iter()
                    .filter(|m| !shown.iter().any(|s| s.id == m.id)),
            );
            all
        }
        Rows::Picked => {
            let mut picked: Vec<&TrackedMr> = shown
                .iter()
                .copied()
                .filter(|m| g.marked.contains(&m.id))
                .collect();
            // Picked, then hidden by a filter: still exported, after the others.
            let hidden: Vec<&TrackedMr> = app
                .mrs
                .iter()
                .filter(|m| g.marked.contains(&m.id) && !picked.iter().any(|p| p.id == m.id))
                .collect();
            picked.extend(hidden);
            picked
        }
    }
}

/// The export, as text, with the window's current choices.
fn text(g: &GuiApp, rows: Rows, settings: Settings) -> (String, usize) {
    let mrs = pick(g, rows);
    let table = super::table::export_table(g, &mrs, settings.all_columns);
    (
        render(&table, settings.format, settings.links),
        table.rows.len(),
    )
}

/// What the file dialog's thread reports.
pub enum Saved {
    Done(std::path::PathBuf),
    Failed(String),
    Cancelled,
}

/// The export's state in the window: its choices and a save under way.
#[derive(Default)]
pub struct State {
    pub settings: Settings,
    pub rows: Rows,
    saving: Option<Receiver<Saved>>,
}

/// The rows to offer first: the picked ones when there are some.
fn default_rows(g: &GuiApp) -> Rows {
    if g.marked.is_empty() {
        Rows::View
    } else {
        Rows::Picked
    }
}

/// Copies the table shown in the last format used (`Ctrl+Shift+C`).
pub fn copy_view(g: &mut GuiApp, ctx: &egui::Context) {
    let rows = default_rows(g);
    let (text, n) = text(g, rows, g.export.settings);
    ctx.copy_text(text);
    let format = g.export.settings.format;
    g.app.flash(copied_text(n, format), false);
}

/// "3 MRs copied as Markdown table".
fn copied_text(n: usize, format: Format) -> String {
    tn(
        n as u64,
        "{n} MR copied as {format}",
        "{n} MRs copied as {format}",
    )
    .replace("{format}", format.label())
}

/// Reports a save that ended.
pub fn poll(g: &mut GuiApp) {
    let Some(rx) = &g.export.saving else {
        return;
    };
    let Ok(saved) = rx.try_recv() else {
        return;
    };
    g.export.saving = None;
    match saved {
        Saved::Done(path) => g.app.flash(
            tf("Saved to {path}", &[("path", &path.display().to_string())]),
            false,
        ),
        Saved::Failed(error) => g.app.flash(
            tf("Could not save the export: {error}", &[("error", &error)]),
            true,
        ),
        Saved::Cancelled => {}
    }
}

/// Asks where to save `text`, off the window's thread, and writes it there.
fn save(g: &mut GuiApp, ctx: &egui::Context, text: String, format: Format) {
    let (tx, rx) = channel();
    g.export.saving = Some(rx);
    let ctx = ctx.clone();
    let name = format!(
        "merge-requests-{}.{}",
        chrono::Local::now().format("%Y-%m-%d"),
        format.extension()
    );
    let (filter, extensions): (&str, &[&str]) = match format {
        Format::Csv => (tl("CSV"), &["csv"]),
        Format::Markdown | Format::Chat => (tl("Markdown"), &["md", "txt"]),
    };
    let filter = filter.to_string();
    std::thread::spawn(move || {
        let picked = futures::executor::block_on(
            rfd::AsyncFileDialog::new()
                .set_title(tl("Export the merge requests"))
                .set_file_name(&name)
                .add_filter(&filter, extensions)
                .save_file(),
        );
        let saved = match picked {
            None => Saved::Cancelled,
            Some(file) => {
                let path = file.path().to_path_buf();
                match std::fs::write(&path, text) {
                    Ok(()) => Saved::Done(path),
                    Err(e) => Saved::Failed(e.to_string()),
                }
            }
        };
        let _ = tx.send(saved);
        ctx.request_repaint();
    });
}

/// Width of the export panel.
const WIDTH: f32 = 500.0;
/// Lines of the preview.
const PREVIEW_LINES: usize = 6;

/// The toolbar's Export button and its panel, drawn like the notifications'.
pub fn button_and_panel(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let response = button(ui, &theme, Some(Icon::Download), tl("Export"), false)
        .on_hover_text(tl("Copy or save the MRs as CSV, Markdown or a chat list"));
    if response.clicked() {
        // Each opening starts from the picked MRs when there are some.
        g.export.rows = default_rows(g);
    }
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_PAGE=export` keeps it open.
    #[cfg(debug_assertions)]
    let forced = std::env::var("GITLAB_TRACKER_GUI_PAGE").as_deref() == Ok("export");
    #[cfg(not(debug_assertions))]
    let forced = false;
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .shadow(egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(90),
        });
    let picked_count = pick(g, Rows::Picked).len();
    let view_count = g.app.visible_mrs().count();
    let all_count = g.app.mrs.len();
    let mut settings = g.export.settings;
    let mut rows = g.export.rows;
    if rows == Rows::Picked && picked_count == 0 {
        rows = Rows::View;
    }
    let (preview, n) = text(g, rows, settings);
    let saving = g.export.saving.is_some();
    let demo = g.demo;
    let mut copy = false;
    let mut save_as = false;
    egui::Popup::menu(&response)
        .open_memory(if forced {
            Some(egui::SetOpenCommand::Bool(true))
        } else {
            response.clicked().then_some(egui::SetOpenCommand::Toggle)
        })
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            header(ui, &theme, n);
            super::widgets::rule(ui, &theme);
            egui::Frame::new()
                .inner_margin(Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    caption(ui, &theme, tl("Rows"));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (choice, label, count) in [
                            (Rows::Picked, tl("Selected"), picked_count),
                            (Rows::View, tl("This view"), view_count),
                            (Rows::All, tl("All tracked"), all_count),
                        ] {
                            let text = format!("{label} {count}");
                            let on = rows == choice;
                            let r = ui.add_enabled_ui(count > 0, |ui| {
                                pill_toggle(ui, &theme, &text, theme.tones.blue, on)
                            });
                            let r = if choice == Rows::Picked && count == 0 {
                                r.inner.on_disabled_hover_text(tl(
                                    "Ctrl+click or Select to pick MRs in the table",
                                ))
                            } else {
                                r.inner
                            };
                            if r.clicked() {
                                rows = choice;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    caption(ui, &theme, tl("Format"));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for format in Format::ALL {
                            if pill_toggle(
                                ui,
                                &theme,
                                format.label(),
                                theme.tones.purple,
                                settings.format == format,
                            )
                            .clicked()
                            {
                                settings.format = format;
                            }
                        }
                    });
                    ui.add_space(10.0);
                    if switch(
                        ui,
                        &theme,
                        !settings.all_columns,
                        tl("Only the columns shown"),
                    )
                    .on_hover_text(tl("Off: every column, hidden ones included"))
                    .clicked()
                    {
                        settings.all_columns = !settings.all_columns;
                    }
                    let link_label = match settings.format {
                        Format::Csv => tl("A URL column"),
                        _ => tl("Links to GitLab"),
                    };
                    if switch(ui, &theme, settings.links, link_label).clicked() {
                        settings.links = !settings.links;
                    }
                    ui.add_space(10.0);
                    caption(ui, &theme, tl("Preview"));
                    let shown = if settings.format == Format::Markdown && settings.links {
                        elide_links(&preview)
                    } else {
                        preview.clone()
                    };
                    preview_box(ui, &theme, &shown);
                });
            super::widgets::rule(ui, &theme);
            egui::Frame::new()
                .inner_margin(Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            muted(&theme, tl("Ctrl+Shift+C copies the view this way"))
                                .size(theme.text_size - 1.5),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if button(ui, &theme, Some(Icon::Copy), tl("Copy"), true).clicked() {
                                copy = true;
                            }
                            let r = ui.add_enabled_ui(!demo && !saving && n > 0, |ui| {
                                button(ui, &theme, None, tl("Save as…"), false)
                            });
                            let r = if demo {
                                r.inner
                                    .on_disabled_hover_text(tl("The demo never writes to disk"))
                            } else {
                                r.inner
                            };
                            if r.clicked() {
                                save_as = true;
                            }
                        });
                    });
                });
            if copy || save_as {
                ui.close();
            }
        });
    g.export.settings = settings;
    g.export.rows = rows;
    if copy {
        ui.ctx().copy_text(preview);
        g.app.flash(copied_text(n, settings.format), false);
    } else if save_as {
        save(g, ui.ctx(), preview, settings.format);
    }
}

/// `[!12](https://…)` read as `[!12](…)`: the preview shows the table, not the
/// URLs (the export keeps them).
fn elide_links(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find("](http") {
        out.push_str(&rest[..start]);
        out.push_str("](…)");
        let after = &rest[start + 2..];
        rest = after.find(')').map_or("", |end| &after[end + 1..]);
    }
    out.push_str(rest);
    out
}

/// The panel's top: the icon in its circle, the title, how many MRs.
fn header(ui: &mut Ui, theme: &Theme, n: usize) {
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let tone = theme.tones.blue;
                let (circle, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                ui.painter().circle_filled(circle.center(), 16.0, tone.bg);
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(circle.center(), Vec2::splat(16.0)),
                    Icon::Download,
                    tone.fg,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new(tl("Export")).font(fonts::semibold(theme.text_size + 1.0)),
                    );
                    ui.label(
                        RichText::new(tn(n as u64, "{n} merge request", "{n} merge requests"))
                            .size(theme.text_size - 1.5)
                            .color(theme.palette.text_secondary),
                    );
                });
            });
        });
}

/// A small heading in capitals, as the notifications' days.
fn caption(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(muted(theme, text.to_uppercase()).size(theme.text_size - 2.5));
}

/// The first lines of the export, in a box, as they will be pasted.
fn preview_box(ui: &mut Ui, theme: &Theme, text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    egui::Frame::new()
        .fill(theme.palette.input)
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            if lines.is_empty() {
                ui.label(muted(theme, tl("Nothing to export")));
                return;
            }
            let font = egui::FontId::monospace(theme.text_size - 2.5);
            // Lines in full, scrolled sideways: a link would hide the rest.
            egui::ScrollArea::horizontal()
                .id_salt("export preview")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for line in lines.iter().take(PREVIEW_LINES) {
                        ui.add(
                            egui::Label::new(
                                RichText::new(*line)
                                    .font(font.clone())
                                    .color(theme.palette.text_secondary),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    }
                });
            if lines.len() > PREVIEW_LINES {
                ui.label(
                    muted(
                        theme,
                        tn(
                            (lines.len() - PREVIEW_LINES) as u64,
                            "+ {n} more line",
                            "+ {n} more lines",
                        ),
                    )
                    .size(theme.text_size - 2.5),
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        Table {
            headers: vec!["Title".into(), "Author".into(), "Status".into()],
            rows: vec![
                Row {
                    id: "12".into(),
                    url: "https://g.io/p/-/merge_requests/12".into(),
                    cells: vec![
                        "fix: a | pipe, \"quoted\"".into(),
                        "Alex".into(),
                        "Mergeable".into(),
                    ],
                },
                Row {
                    id: "13".into(),
                    url: "https://g.io/p/-/merge_requests/13".into(),
                    cells: vec!["feat: two\nlines".into(), "Sam".into(), String::new()],
                },
            ],
        }
    }

    #[test]
    fn csv_quotes_what_needs_it_and_puts_the_url_last() {
        let text = render(&sample(), Format::Csv, true);
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines[0], "MR,Title,Author,Status,URL");
        assert_eq!(
            lines[1],
            "!12,\"fix: a | pipe, \"\"quoted\"\"\",Alex,Mergeable,https://g.io/p/-/merge_requests/12"
        );
        assert!(lines[2].starts_with("!13,\"feat: two\nlines\",Sam,,"));
        assert!(!render(&sample(), Format::Csv, false).contains("URL"));
    }

    #[test]
    fn markdown_escapes_pipes_and_links_the_numbers() {
        let text = render(&sample(), Format::Markdown, true);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "| MR | Title | Author | Status |");
        assert_eq!(lines[1], "|---|---|---|---|");
        assert_eq!(
            lines[2],
            "| [!12](https://g.io/p/-/merge_requests/12) | fix: a \\| pipe, \"quoted\" | Alex | Mergeable |"
        );
        assert_eq!(
            lines[3],
            "| [!13](https://g.io/p/-/merge_requests/13) | feat: two lines | Sam |  |"
        );
        assert!(render(&sample(), Format::Markdown, false).contains("| !12 |"));
    }

    #[test]
    fn the_chat_list_skips_empty_cells() {
        let text = render(&sample(), Format::Chat, false);
        assert_eq!(
            text,
            "- !12 fix: a | pipe, \"quoted\" — Alex · Mergeable\n- !13 feat: two lines — Sam\n"
        );
        assert!(render(&sample(), Format::Chat, true)
            .starts_with("- !12 fix: a | pipe, \"quoted\" — Alex · Mergeable https://g.io/"));
    }

    #[test]
    fn the_preview_elides_the_urls_only() {
        let text = render(&sample(), Format::Markdown, true);
        let shown = elide_links(&text);
        assert!(shown.contains("| [!12](…) | fix: a \\| pipe"));
        assert!(!shown.contains("https://"));
        assert_eq!(elide_links("no link"), "no link");
    }

    #[test]
    fn settings_tolerate_an_older_preferences_file() {
        let s: Settings = serde_json::from_str(r#"{"format":"csv"}"#).unwrap();
        assert_eq!(s.format, Format::Csv);
        assert!(s.links && !s.all_columns);
    }
}
