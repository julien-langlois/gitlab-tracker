//! The rail's badges for the pages under its rule (team, journal, environments,
//! CI, milestones, overlaps): one number, in the tone of the worst thing it
//! counts, and the sentence for the entry's tooltip. Each page says what its
//! badge counts in its own `badge` function; this module keeps them for a while,
//! as working them out reads every tracked MR.

use std::time::{Duration, Instant};

use bevel_theme::Tone;
use chrono::Utc;

use super::{ci, environments, journal, milestones, overlaps, team, GuiApp, Page};

/// What a rail entry's badge shows.
#[derive(Debug, Clone)]
pub struct Badge {
    pub n: usize,
    pub tone: Tone,
    /// What the number counts, and the worst of it, for the tooltip.
    pub hint: String,
}

/// The badges are worked out again at most this often.
const EVERY: Duration = Duration::from_secs(2);

/// The last badges, for the project they were worked out for.
#[derive(Default)]
pub struct Cache {
    at: Option<Instant>,
    project: String,
    badges: Vec<(Page, Badge)>,
}

impl Cache {
    /// The badge of `page`, if it has one.
    pub fn get(&self, page: Page) -> Option<&Badge> {
        self.badges.iter().find(|b| b.0 == page).map(|b| &b.1)
    }
}

/// Brings `g`'s badges up to date when they are older than [`EVERY`] or of
/// another project; else asks for a frame once they are, so data that arrived
/// meanwhile shows.
pub fn refresh(g: &mut GuiApp, ctx: &egui::Context) {
    let age = g.rail_badges.at.map(|at| at.elapsed());
    let same = g.rail_badges.project == g.app.project_id;
    if let Some(age) = age.filter(|age| same && *age < EVERY) {
        ctx.request_repaint_after(EVERY - age);
        return;
    }
    let badges = compute(g);
    g.rail_badges = Cache {
        at: Some(Instant::now()),
        project: g.app.project_id.clone(),
        badges,
    };
}

fn compute(g: &GuiApp) -> Vec<(Page, Badge)> {
    let now = Utc::now();
    [
        (Page::Team, team::badge(g, now)),
        (Page::Journal, journal::badge(g, now)),
        (Page::Environments, environments::badge(g, now)),
        (Page::Ci, ci::badge(g, now)),
        (Page::Milestones, milestones::badge(g, now)),
        (Page::Overlaps, overlaps::badge(g)),
    ]
    .into_iter()
    .filter_map(|(page, badge)| Some((page, badge?)))
    .collect()
}

/// "what: list", with the language's punctuation.
pub fn listed(what: impl AsRef<str>, list: impl AsRef<str>) -> String {
    crate::i18n::tf(
        "{what}: {list}",
        &[("what", what.as_ref()), ("list", list.as_ref())],
    )
}

/// Up to `most` names, then "+N".
pub fn names(list: &[String], most: usize) -> String {
    let mut out = list
        .iter()
        .take(most)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if list.len() > most {
        out.push_str(&format!(" +{}", list.len() - most));
    }
    out
}
