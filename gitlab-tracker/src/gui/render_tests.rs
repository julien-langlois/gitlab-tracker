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
const PAGES: &[Page] = &[
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
async fn demo_launch() -> Launch {
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

fn harness(launch: Launch) -> Harness<'static, GuiApp> {
    builder(egui::vec2(1440.0, 900.0))
        .build_eframe(move |cc| GuiApp::new(cc, launch, bevel_window::Frame::Desktop))
}

/// Opens `page` (with what it needs: a selected MR, a selected person) and
/// draws a few frames, for the layout to settle and the events to land.
fn show(harness: &mut Harness<'_, GuiApp>, page: Page) {
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
