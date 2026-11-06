//! The Plan page's "Generate changelog": offered when the tracked project's
//! repository holds a git-cliff config ([`crate::changelog`]), it shows the
//! changelog of a branch since its last release, to copy.

use egui::{Align, Layout, Margin, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use super::details::Fetch;
use super::icons::Icon;
use super::widgets::{button, mono, muted, primary_button, spinner};
use super::GuiApp;
use crate::changelog::{CliffConfig, Generated};
use crate::gitlab_detail::ProjectTarget;
use crate::i18n::{tf, tl, tn};

/// How long a project's answer (config or not) is kept before asking again.
const DETECT_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Default)]
pub struct State {
    /// The config found, by project, and when it was asked.
    detected: Option<(String, std::time::Instant, Fetch<Option<CliffConfig>>)>,
    dialog: Option<Dialog>,
}

struct Dialog {
    config: CliffConfig,
    branch: String,
    /// Empty: unreleased.
    version: String,
    result: Option<Fetch<Generated>>,
    /// The Markdown source instead of its rendering.
    source: bool,
    cache: CommonMarkCache,
}

fn target(g: &GuiApp) -> ProjectTarget {
    ProjectTarget {
        base_url: g.app.base_url.clone(),
        token: g.app.token.clone(),
        project_id: g.app.project_id.clone(),
    }
}

/// The tracked project's git-cliff config, once known; asked in the background
/// (the read-only demo has one of its own).
pub fn config(g: &mut GuiApp, ctx: &egui::Context) -> Option<CliffConfig> {
    if g.app.read_only {
        return Some(crate::changelog::demo_config());
    }
    let project = g.app.project_id.clone();
    let stale = match &g.changelog.detected {
        Some((id, at, fetch)) => {
            id != &project || (!matches!(fetch, Fetch::Loading(_)) && at.elapsed() >= DETECT_EVERY)
        }
        None => true,
    };
    if stale {
        let fetch = Fetch::spawn(ctx, crate::changelog::detect(target(g)));
        g.changelog.detected = Some((project, std::time::Instant::now(), fetch));
    }
    let (_, _, fetch) = g.changelog.detected.as_mut()?;
    fetch.poll();
    match fetch {
        Fetch::Ready(found) => found.clone(),
        _ => None,
    }
}

/// A version for the milestone's title when it reads as one (`1.4.0`, `v1.4.0`).
fn version_of(title: &str) -> Option<String> {
    let bare = title.trim().trim_start_matches(['v', 'V']);
    let parts: Vec<&str> = bare.split('.').collect();
    (parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())))
    .then(|| title.trim().to_string())
}

/// Opens the dialog on `config`'s default branch, titled after the milestone
/// when its title is a version, and generates at once.
pub fn open(g: &mut GuiApp, ctx: &egui::Context, config: CliffConfig, milestone: &str) {
    let mut dialog = Dialog {
        branch: config.default_branch.clone(),
        version: version_of(milestone).unwrap_or_default(),
        config,
        result: None,
        source: false,
        cache: CommonMarkCache::default(),
    };
    generate(g, ctx, &mut dialog);
    g.changelog.dialog = Some(dialog);
}

fn generate(g: &GuiApp, ctx: &egui::Context, dialog: &mut Dialog) {
    let version = Some(dialog.version.trim().to_string()).filter(|v| !v.is_empty());
    if g.app.read_only {
        // The demo's merged MRs, as if squashed onto the branch.
        let titles: Vec<String> = g
            .app
            .mrs
            .iter()
            .filter(|mr| mr.state == crate::models::GitlabMrState::Merged)
            .map(|mr| mr.title.clone())
            .collect();
        dialog.result = Some(match crate::changelog::demo(&titles, version.as_deref()) {
            Ok(done) => Fetch::Ready(done),
            Err(e) => Fetch::Failed(e),
        });
        return;
    }
    dialog.result = Some(Fetch::spawn(
        ctx,
        crate::changelog::generate(
            target(g),
            dialog.config.clone(),
            dialog.branch.clone(),
            version,
        ),
    ));
}

/// The dialog, while open.
pub fn show(g: &mut GuiApp, ctx: &egui::Context) {
    let Some(mut dialog) = g.changelog.dialog.take() else {
        return;
    };
    let theme = g.theme;
    let mut branches: Vec<String> = vec![dialog.config.default_branch.clone()];
    for b in &g.app.branches {
        if !branches.contains(b) {
            branches.push(b.clone());
        }
    }
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(egui::CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(20));
    let mut close = false;
    let mut regenerate = false;
    let response = egui::Modal::new(egui::Id::unique("changelog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(640.0);
            ui.label(RichText::new(tl("Changelog")).strong().size(theme.text_size + 3.0));
            ui.add_space(2.0);
            ui.label(muted(
                &theme,
                tf(
                    "With the project's git-cliff config ({path})",
                    &[("path", &dialog.config.path)],
                ),
            ));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(tl("Branch"));
                let before = dialog.branch.clone();
                egui::ComboBox::from_id_salt("changelog-branch")
                    .selected_text(&dialog.branch)
                    .show_ui(ui, |ui| {
                        for b in &branches {
                            ui.selectable_value(&mut dialog.branch, b.clone(), b);
                        }
                    });
                regenerate |= dialog.branch != before;
                ui.add_space(12.0);
                ui.label(tl("Version"));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut dialog.version)
                        .hint_text(tl("Unreleased"))
                        .desired_width(110.0),
                );
                regenerate |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    regenerate |= button(ui, &theme, Some(Icon::Refresh), tl("Generate"), false)
                        .clicked();
                });
            });
            ui.add_space(12.0);
            let mut copy = None;
            match dialog.result.as_mut() {
                None => {}
                Some(fetch) => {
                    fetch.poll();
                    match fetch {
                        Fetch::Loading(_) => {
                            ui.horizontal(|ui| {
                                spinner(ui, 14.0, theme.palette.text_muted);
                                ui.label(muted(&theme, tl("Reading the commits…")));
                            });
                        }
                        Fetch::Failed(e) => {
                            ui.label(RichText::new(e.as_str()).color(theme.tones.red.fg));
                        }
                        Fetch::Ready(done) => {
                            let since = match &done.since {
                                Some(tag) => tf("since {tag}", &[("tag", tag)]),
                                None if done.truncated => tl("no release tag in the commits read").into(),
                                None => tl("no release tag yet").into(),
                            };
                            ui.label(muted(
                                &theme,
                                format!(
                                    "{} · {} · {}",
                                    tn(done.commits as u64, "{n} commit", "{n} commits"),
                                    since,
                                    dialog.branch
                                ),
                            ));
                            ui.add_space(8.0);
                            if done.markdown.is_empty() {
                                ui.label(muted(&theme, tl("Nothing to put in it: no commit since the last release, or every one is skipped by the config.")));
                            } else {
                                theme.card_frame().show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    egui::ScrollArea::vertical()
                                        .max_height(ctx.content_rect().height() * 0.55)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            if dialog.source {
                                                ui.label(mono(&done.markdown, theme.text_size - 1.0));
                                            } else {
                                                CommonMarkViewer::new().show(
                                                    ui,
                                                    &mut dialog.cache,
                                                    &done.markdown,
                                                );
                                            }
                                        });
                                });
                                copy = Some(done.markdown.clone());
                            }
                        }
                    }
                }
            }
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(text) = &copy {
                    if primary_button(ui, &theme, tl("Copy")).clicked() {
                        ui.ctx().copy_text(text.clone());
                        g.app.flash(tl("Changelog copied"), false);
                    }
                }
                if button(ui, &theme, None, tl("Close"), false).clicked() {
                    close = true;
                }
                if copy.is_some() {
                    let label = if dialog.source { tl("Preview") } else { tl("Markdown") };
                    if button(ui, &theme, None, label, false).clicked() {
                        dialog.source = !dialog.source;
                    }
                }
            });
        });
    if regenerate {
        generate(g, ctx, &mut dialog);
    }
    if !(close || response.should_close()) {
        g.changelog.dialog = Some(dialog);
    }
}

#[cfg(test)]
mod tests {
    use super::version_of;

    #[test]
    fn a_milestone_titled_as_a_version_names_the_release() {
        assert_eq!(version_of("1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(version_of(" v2.0.10 ").as_deref(), Some("v2.0.10"));
        assert_eq!(version_of("Sprint 12"), None);
        assert_eq!(version_of("1.4"), None);
        assert_eq!(version_of("1.4.x"), None);
    }
}
