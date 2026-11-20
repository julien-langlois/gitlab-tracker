//! The pages' analyses, kept between frames.
//!
//! egui draws a frame on every mouse move, and each page used to work out its
//! analysis (reading every tracked MR) on each of them. A [`Memo`] keeps the
//! last results and hands them back while they still hold: same project, same
//! parameters, no event applied since ([`changed`]), and younger than
//! [`FRESH`], as the analyses read the clock ("waiting for 3 h") and the
//! settings, which change without an event.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    /// Bumped each time events change a tab's data. Per thread: the window
    /// draws and applies its events on one, and tests don't see each other's.
    static REVISION: Cell<u64> = const { Cell::new(0) };
}

/// Says the data changed: every memo works its results out again.
pub fn changed() {
    REVISION.with(|r| r.set(r.get() + 1));
}

/// How long a result holds when nothing changed: the same as the rail badges.
pub const FRESH: Duration = Duration::from_secs(2);

/// The results kept per memo: a page's and its rail badge's, with room for a
/// period switched back and forth.
const SLOTS: usize = 4;

struct Slot<K, V> {
    at: Instant,
    revision: u64,
    project: String,
    key: K,
    value: Rc<V>,
}

/// The last results of one analysis, by parameters `K`.
pub struct Memo<K, V> {
    slots: RefCell<Vec<Slot<K, V>>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self {
            slots: RefCell::new(Vec::new()),
        }
    }
}

impl<K: PartialEq, V> Memo<K, V> {
    /// The result for `project` and `key`: the kept one while it holds, else
    /// `make`'s, kept for the next frames.
    pub fn get(&self, project: &str, key: K, make: impl FnOnce() -> V) -> Rc<V> {
        let revision = REVISION.with(Cell::get);
        {
            let mut slots = self.slots.borrow_mut();
            slots.retain(|s| s.revision == revision && s.at.elapsed() < FRESH);
            if let Some(slot) = slots.iter().find(|s| s.project == project && s.key == key) {
                return slot.value.clone();
            }
        }
        // Not borrowed while `make` runs: it may read another memo.
        let value = Rc::new(make());
        let mut slots = self.slots.borrow_mut();
        if slots.len() >= SLOTS {
            slots.remove(0);
        }
        slots.push(Slot {
            at: Instant::now(),
            revision,
            project: project.to_string(),
            key,
            value: value.clone(),
        });
        value
    }
}

/// The pages' memos.
#[derive(Default)]
pub struct Memos {
    /// Overlaps: (lock files, drafts).
    pub overlaps: Memo<(bool, bool), super::overlaps::Overlaps>,
    /// Overlaps' hot spots of the last 30 days.
    pub hot: Memo<(), Vec<(String, usize)>>,
    /// Bottlenecks: (by file, lock files).
    pub spots: Memo<(bool, bool), Vec<super::hotspots::Spot>>,
    /// Team: (days, bots).
    pub team: Memo<(i64, bool), super::team::Analysis>,
    /// CI: days.
    pub ci: Memo<i64, super::ci::Ci>,
    /// Delivery: (days, whether the project overview is known).
    pub delivery: Memo<(i64, bool), super::delivery::Delivery>,
    /// Milestones.
    pub milestones: Memo<(), Vec<super::milestones::Milestone>>,
    /// Stacked MRs.
    pub stacks: Memo<(), Vec<super::stacks::Stack>>,
    /// The cockpit's labels card: (rows, unlabelled).
    pub labels: Memo<(), (Vec<super::labels::LabelRow>, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_is_kept_for_the_same_project_and_key() {
        let memo: Memo<u8, u32> = Memo::default();
        let runs = Cell::new(0);
        let make = || {
            runs.set(runs.get() + 1);
            42
        };
        assert_eq!(*memo.get("p", 1, make), 42);
        assert_eq!(*memo.get("p", 1, make), 42);
        assert_eq!(runs.get(), 1);
        // Another key or project: worked out again, the first one kept.
        memo.get("p", 2, make);
        memo.get("q", 1, make);
        assert_eq!(runs.get(), 3);
        memo.get("p", 1, make);
        assert_eq!(runs.get(), 3);
    }

    #[test]
    fn a_change_of_data_drops_the_results() {
        let memo: Memo<(), u32> = Memo::default();
        let runs = Cell::new(0);
        let make = || {
            runs.set(runs.get() + 1);
            runs.get()
        };
        assert_eq!(*memo.get("p", (), make), 1);
        changed();
        assert_eq!(*memo.get("p", (), make), 2);
    }

    #[test]
    fn no_more_than_a_few_results_are_kept() {
        let memo: Memo<usize, usize> = Memo::default();
        for k in 0..10 {
            memo.get("p", k, || k);
        }
        assert_eq!(memo.slots.borrow().len(), SLOTS);
    }
}
