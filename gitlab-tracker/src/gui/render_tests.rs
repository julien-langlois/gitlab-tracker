//! The window, rendered headless (egui_kittest: no display, no GPU) on the
//! demo's two projects: every page is opened and drawn for several frames, in
//! the dark and the light theme. A panic while drawing — an index out of
//! range, a slice cut inside a character, an `expect` that does not hold on
//! real-looking data — fails here instead of closing someone's window.

use super::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::collections::BTreeSet;
use std::sync::Mutex;

/// The characters no bundled font could draw: shown as a box ("tofu") on a
/// machine whose system fonts lack them too.
static MISSING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// A harness of `size` whose missing glyphs are drawn as tofu and listed in
/// [`MISSING`], instead of the system fonts silently filling in for them.
fn builder(size: egui::Vec2) -> egui_kittest::HarnessBuilder<GuiApp> {
    Harness::builder()
        .with_size(size)
        .allow_missing_glyphs()
        .with_context_setup(|ctx| {
            ctx.add_font_provider(Arc::new(
                |request: &egui::epaint::text::FallbackRequest<'_>| {
                    let c = request.cluster.chars().next().unwrap_or(' ');
                    MISSING
                        .lock()
                        .unwrap()
                        .insert(format!("{c} U+{:04X} in {:?}", c as u32, request.family));
                    None
                },
            ));
        })
}

/// Every page the icon rail and the shortcuts lead to.
pub(super) const PAGES: &[Page] = &[
    Page::Today,
    Page::MergeRequests,
    Page::Mr,
    Page::Cockpit,
    Page::Promote,
    Page::Tracker,
    #[cfg(feature = "stats")]
    Page::Stats,
    Page::Team,
    Page::Person,
    Page::Journal,
    Page::Environments,
    Page::Ci,
    Page::Milestones,
    Page::Overlaps,
    Page::Delivery,
    Page::Releases,
    Page::Stacks,
];

/// The demo window, as `gitlab-tracker --demo gui` opens it.
pub(super) async fn demo_launch() -> Launch {
    let config = AppConfig::default();
    let first = crate::demo::demo_app(config.clone()).await;
    let second = crate::demo::mobile_app(config.clone()).await;
    Launch {
        session: demo_session(first),
        more: vec![demo_session(second)],
        global_config: config,
        shared: Shared {
            palette: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
            #[cfg(feature = "stats")]
            stats_db: None,
        },
        demo: true,
    }
}

pub(super) fn harness(launch: Launch) -> Harness<'static, GuiApp> {
    builder(egui::vec2(1440.0, 900.0))
        .build_eframe(move |cc| GuiApp::new(cc, launch, bevel_window::Frame::Desktop))
}

/// Opens `page` (with what it needs: a selected MR, a selected person) and
/// draws a few frames, for the layout to settle and the events to land.
pub(super) fn show(harness: &mut Harness<'_, GuiApp>, page: Page) {
    let gui = harness.state_mut();
    if gui.app.selected_mr().is_none() {
        gui.app.table_state.select(Some(0));
    }
    if page == Page::Person && gui.team.selected.is_none() {
        gui.team.selected = gui.app.selected_mr().map(|mr| mr.data.author.clone());
    }
    gui.open_page(page);
    harness.run_steps(6);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_page_draws_on_the_demo_data_in_both_themes() {
    let launch = demo_launch().await;
    tokio::task::block_in_place(|| {
        let mut harness = harness(launch);
        harness.run_steps(10);
        assert!(
            !harness.state().app.mrs.is_empty(),
            "the demo project has MRs"
        );
        for dark in [true, false] {
            harness.ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            for &page in PAGES {
                show(&mut harness, page);
                assert_eq!(
                    harness.state().page(),
                    page,
                    "{page:?} fell back to another page (its prerequisites are missing)"
                );
            }
        }
    });
    let missing = MISSING.lock().unwrap().clone();
    assert!(
        missing.is_empty(),
        "characters no bundled font draws (a box on a machine without them; bundle a \
         font that has them, see `install_symbol_font`, or use another character):\n{}",
        missing.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_merge_requests_page_lists_the_demo_mrs() {
    let launch = demo_launch().await;
    tokio::task::block_in_place(|| {
        let mut harness = harness(launch);
        harness.run_steps(10);
        show(&mut harness, Page::MergeRequests);
        // The first rows' titles, as far as a narrow column shows them.
        let titles: Vec<String> = harness
            .state()
            .app
            .visible_mrs()
            .take(3)
            .map(|mr| mr.data.title.chars().take(12).collect())
            .collect();
        assert!(!titles.is_empty(), "the demo lists MRs");
        for title in titles {
            assert!(
                harness.query_all_by_label_contains(&title).next().is_some(),
                "a demo MR ({title}…) is on the page"
            );
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn a_narrow_window_draws_every_page_too() {
    let launch = demo_launch().await;
    tokio::task::block_in_place(|| {
        // The smallest size the window allows (`with_min_inner_size`).
        let mut harness = builder(egui::vec2(960.0, 600.0))
            .build_eframe(move |cc| GuiApp::new(cc, launch, bevel_window::Frame::Desktop));
        harness.run_steps(10);
        for &page in PAGES {
            show(&mut harness, page);
        }
    });
}

/// What a screen reader would announce as a nameless control: a node that
/// takes a click (or focus) but has neither a label nor a value. Per page,
/// with its role and where it is drawn.
fn nameless_controls(harness: &Harness<'_, GuiApp>) -> Vec<String> {
    use egui::accesskit::{Action, Role};
    use egui_kittest::kittest::{by, NodeT};
    harness
        .query_all(by().predicate(move |n| {
            let d = n.data();
            let interactive = d.supports_action(Action::Click);
            let named = n.label().is_some_and(|l| !l.trim().is_empty())
                || n.value().is_some_and(|v| !v.trim().is_empty());
            interactive && !named && !n.is_hidden() && n.role() != Role::Window
        }))
        .map(|node| {
            let n = node.accesskit_node();
            let at = n.bounding_box().map(|r| (r.x0 as i32, r.y0 as i32));
            format!("{:?} at {at:?}", n.role())
        })
        .collect()
}

/// Every page's nameless controls, with those of the shell (on every page:
/// rail, title bar, tabs) apart.
fn nameless_audit(harness: &mut Harness<'_, GuiApp>) -> (Vec<String>, Vec<(Page, Vec<String>)>) {
    let per_page: Vec<(Page, Vec<String>)> = PAGES
        .iter()
        .map(|&page| {
            show(harness, page);
            (page, nameless_controls(harness))
        })
        .collect();
    let shell: Vec<String> = per_page[0]
        .1
        .iter()
        .filter(|c| per_page.iter().all(|(_, cs)| cs.contains(c)))
        .cloned()
        .collect();
    let per_page = per_page
        .into_iter()
        .map(|(page, cs)| {
            (
                page,
                cs.into_iter().filter(|c| !shell.contains(c)).collect(),
            )
        })
        .collect();
    (shell, per_page)
}

/// A screen reader names every control of the shell: the rail's pages, the
/// title bar's buttons, the project tabs.
#[tokio::test(flavor = "multi_thread")]
async fn the_shell_names_every_control_for_a_screen_reader() {
    let launch = demo_launch().await;
    tokio::task::block_in_place(|| {
        let mut harness = harness(launch);
        harness.run_steps(10);
        let (shell, _) = nameless_audit(&mut harness);
        assert!(
            shell.is_empty(),
            "nameless controls in the shell:\n{}",
            shell.join("\n")
        );
    });
}

/// The screen-reader report of the pages' own controls (rows, chips, cards):
/// `cargo test --all-features nameless_controls_per_page -- --ignored --nocapture`.
/// A report, not a gate yet: most list rows have no accessible name so far.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn nameless_controls_per_page() {
    let launch = demo_launch().await;
    tokio::task::block_in_place(|| {
        let mut harness = harness(launch);
        harness.run_steps(10);
        let (shell, per_page) = nameless_audit(&mut harness);
        let mut total = shell.len();
        for (page, controls) in &per_page {
            total += controls.len();
            println!("{page:?}: {}", controls.len());
            for c in controls.iter().take(4) {
                println!("    {c}");
            }
        }
        println!("total: {total}");
    });
}

/// The analyses are kept between frames: a change made in the window without
/// an event (a snooze, here) must still show at once, not when they expire.
#[tokio::test(flavor = "multi_thread")]
async fn a_snooze_shows_at_once_on_the_kept_counts() {
    let launch = demo_launch().await;
    tokio::task::block_in_place(|| {
        let mut harness = harness(launch);
        harness.run_steps(10);
        let gui = harness.state_mut();
        let before = super::today::waiting_count(gui);
        let item = super::today::collect(gui)
            .0
            .iter()
            .find(|i| {
                i.snoozed.is_none()
                    && matches!(i.kind, super::today::Kind::Review | super::today::Kind::Fix)
            })
            .map(|i| (i.tab, i.id.clone()))
            .expect("the demo has an MR waiting on me");
        gui.edit_tab(item.0, |app| {
            let mr = app
                .mrs
                .iter_mut()
                .find(|m| m.id == item.1)
                .expect("in its tab");
            mr.snooze = Some(crate::snooze::Snooze::new(mr, None));
        });
        assert_eq!(super::today::waiting_count(harness.state()), before - 1);
    });
}
