//! The window under load: the demo project's MRs copied until it tracks
//! [`SCALE`] times as many (about 500, a large project), every page drawn
//! headless, and the time of its frames measured. On every page, a frame with
//! nothing new stays within [`WARM_BUDGET`], one after new data (every
//! analysis worked out again) within [`COLD_BUDGET`]. The test prints each
//! page's times at the demo's size and at this one.
//!
//! Timings depend on the machine, so these tests are `#[ignore]`d in the usual
//! run; run them in release mode:
//!
//! ```text
//! cargo test --release -p gitlab-tracker --all-features load_ -- --ignored --nocapture
//! ```

use super::render_tests::{demo_launch, harness, show, PAGES};
use super::*;
use std::time::{Duration, Instant};

/// How many times the demo project's MRs are copied.
const SCALE: usize = 11;

/// The slowest frame with nothing new allowed on any page (60 fps).
const WARM_BUDGET: Duration = Duration::from_millis(16);

/// The slowest frame after new data allowed on any page.
const COLD_BUDGET: Duration = Duration::from_millis(250);

/// `mrs` copied `times` times: new ids and source branches, the rest kept.
fn scaled(mrs: &[crate::models::TrackedMr], times: usize) -> Vec<crate::models::TrackedMr> {
    let mut out = Vec::with_capacity(mrs.len() * times);
    for k in 0..times {
        for mr in mrs {
            let mut copy = mr.clone();
            if k > 0 {
                copy.id = format!("{}{k:03}", mr.id);
                copy.data.source_branch = format!("{}-{k}", mr.data.source_branch);
            }
            out.push(copy);
        }
    }
    out
}

/// The median of `runs` frames with nothing new, and one frame after new data.
fn frame_times(
    harness: &mut egui_kittest::Harness<'_, GuiApp>,
    runs: usize,
) -> (Duration, Duration) {
    let mut warm: Vec<Duration> = (0..runs)
        .map(|_| {
            let at = Instant::now();
            harness.step();
            at.elapsed()
        })
        .collect();
    warm.sort();
    memo::changed();
    let at = Instant::now();
    harness.step();
    (warm[runs / 2], at.elapsed())
}

/// Each page's frame times on the demo project, `times` times its MRs.
fn measure(times: usize) -> (usize, Vec<(Page, Duration, Duration)>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a test runtime");
    let launch = runtime.block_on(demo_launch());
    let _guard = runtime.enter();
    let mut harness = harness(launch);
    harness.run_steps(10);
    let gui = harness.state_mut();
    gui.app.mrs = scaled(&gui.app.mrs, times);
    let tracked = gui.app.mrs.len();
    memo::changed();
    harness.run_steps(4);
    let mut out = Vec::new();
    for &page in PAGES {
        show(&mut harness, page);
        let (warm, cold) = frame_times(&mut harness, 15);
        out.push((page, warm, cold));
    }
    (tracked, out)
}

#[test]
#[ignore = "timings: run in release mode, see the module docs"]
fn load_every_page_keeps_its_frames_short_with_many_mrs() {
    let (base, small) = measure(1);
    let (tracked, large) = measure(SCALE);
    println!(
        "{:<14} {:>22} {:>22}",
        "page",
        format!("warm {base} → {tracked} MRs"),
        "after new data"
    );
    let mut slow = Vec::new();
    for ((page, warm1, cold1), (_, warm, cold)) in small.iter().zip(&large) {
        println!(
            "{:<14} {:>9.2?} → {:>9.2?} {:>9.2?} → {:>9.2?}",
            format!("{page:?}"),
            warm1,
            warm,
            cold1,
            cold
        );
        if *warm > WARM_BUDGET || *cold > COLD_BUDGET {
            slow.push(format!(
                "{page:?}: {warm:.2?} warm, {cold:.2?} after new data"
            ));
        }
    }
    assert!(
        slow.is_empty(),
        "pages over budget with {tracked} MRs ({WARM_BUDGET:?} warm, {COLD_BUDGET:?} after new data):\n{}",
        slow.join("\n")
    );
}
