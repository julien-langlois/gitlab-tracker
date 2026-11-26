//! Personas: which pages the left rail shows, and the page the window opens on.
//!
//! A persona is a starting set of rail pages for a role; each page can then be
//! shown or hidden on its own. Nothing else changes with it: every page stays
//! reachable (Ctrl+K, links from the other pages), and the data, filters and
//! notifications are the same for everyone. Both settings are the person's,
//! for every project (Settings › Pages and › Startup), kept in the window's
//! preferences.

use crate::i18n::{tc, tl};

mod view;

pub use view::*;

/// The rail's pages a persona shows or hides, by their key in the window's
/// preferences, in the rail's order.
pub const PAGES: &[&str] = &[
    "today",
    "mrs",
    "cockpit",
    "promote",
    "tracker",
    "stats",
    "team",
    "journal",
    "environments",
    "ci",
    "milestones",
    "overlaps",
    "delivery",
    "releases",
    "stacks",
];

/// The ticket tracker's page shows while one is linked, whatever the persona.
pub const TRACKER: &str = "tracker";

/// A page's name, as the rail shows it.
pub fn page_label(key: &str) -> &'static str {
    match key {
        "today" => tl("Today"),
        "mrs" => tl("MRs"),
        "cockpit" => tl("Cockpit"),
        "promote" => tl("Promote"),
        "tracker" => tl("Tracker"),
        "stats" => tl("Stats"),
        "team" => tl("Team"),
        "journal" => tl("Journal"),
        "environments" => tl("Envs"),
        "ci" => tl("CI"),
        "milestones" => tl("Plan"),
        "overlaps" => tc("rail", "Overlaps"),
        "delivery" => tc("rail", "Delivery"),
        "releases" => tc("rail", "Releases"),
        "stacks" => tc("rail", "Stacks"),
        _ => "",
    }
}

/// What a page is for, in a few words (Settings › Pages).
pub fn page_about(key: &str) -> &'static str {
    tl(match key {
        "today" => "What waits on you, across projects",
        "mrs" => "The merge requests table",
        "cockpit" => "Counters and what needs attention",
        "promote" => "What is ready for the next branch",
        "tracker" => "Linked tickets · shown while a tracker is linked",
        "stats" => "Local statistics over time",
        "team" => "What waits on whom",
        "journal" => "What moved, in order",
        "environments" => "Environments and deployments",
        "ci" => "Pipelines and slow jobs",
        "milestones" => "Milestones",
        "overlaps" => "MRs changing the same files",
        "delivery" => "Flow: lead time, deploys, failures",
        "releases" => "Draft release notes",
        "stacks" => "Stacked MRs",
        _ => "",
    })
}

/// A role, and the rail pages it starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Persona {
    Developer,
    TechLead,
    ReleaseManager,
    EngineeringManager,
    /// Every page, as before personas: the default, so nothing disappears
    /// from the rail of someone who never picked one.
    #[default]
    Everything,
}

impl Persona {
    pub const ALL: [Persona; 5] = [
        Persona::Developer,
        Persona::TechLead,
        Persona::ReleaseManager,
        Persona::EngineeringManager,
        Persona::Everything,
    ];

    /// Its key in the window's preferences.
    pub fn key(self) -> &'static str {
        match self {
            Persona::Developer => "developer",
            Persona::TechLead => "tech_lead",
            Persona::ReleaseManager => "release_manager",
            Persona::EngineeringManager => "engineering_manager",
            Persona::Everything => "everything",
        }
    }

    /// The persona saved as `key`; `Everything` for an unknown one.
    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.key() == key)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        tl(match self {
            Persona::Developer => "Developer",
            Persona::TechLead => "Tech lead",
            Persona::ReleaseManager => "Release manager",
            Persona::EngineeringManager => "Engineering manager",
            Persona::Everything => "Everything",
        })
    }

    /// What the role looks at, under its name on the first launch.
    pub fn blurb(self) -> &'static str {
        tl(match self {
            Persona::Developer => {
                "My MRs, what breaks, what collides with mine, my stacked branches."
            }
            Persona::TechLead => {
                "The developer view, plus the team’s review load, the risks and the flow."
            }
            Persona::ReleaseManager => {
                "What goes out, where and when: promotions, pipelines, environments, releases, freezes."
            }
            Persona::EngineeringManager => {
                "The overview and what blocks the team. What waits on whom, never who works how much."
            }
            Persona::Everything => "Every page, as today. Hide the ones you never use later.",
        })
    }

    /// The rail pages it shows (the tracker's aside, see [`TRACKER`]).
    pub fn pages(self) -> &'static [&'static str] {
        match self {
            Persona::Developer => &["today", "mrs", "journal", "ci", "overlaps", "stacks"],
            Persona::TechLead => &[
                "today", "mrs", "cockpit", "stats", "team", "journal", "ci", "overlaps",
                "delivery", "stacks",
            ],
            Persona::ReleaseManager => &[
                "today",
                "mrs",
                "cockpit",
                "promote",
                "stats",
                "journal",
                "environments",
                "ci",
                "milestones",
                "delivery",
                "releases",
            ],
            Persona::EngineeringManager => &[
                "today",
                "mrs",
                "cockpit",
                "stats",
                "team",
                "journal",
                "milestones",
                "delivery",
            ],
            Persona::Everything => &[
                "today",
                "mrs",
                "cockpit",
                "promote",
                "stats",
                "team",
                "journal",
                "environments",
                "ci",
                "milestones",
                "overlaps",
                "delivery",
                "releases",
                "stacks",
            ],
        }
    }

    /// The page it opens on.
    pub fn start(self) -> &'static str {
        match self {
            Persona::Developer | Persona::TechLead => "today",
            Persona::ReleaseManager | Persona::EngineeringManager => "cockpit",
            Persona::Everything => "mrs",
        }
    }
}

/// The page the window opens on (Settings › Startup).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Start {
    /// The persona's page ([`Persona::start`]).
    Persona,
    /// Always this page, by its key.
    Page(String),
    /// Each project's tab on the page it showed when the window closed: the
    /// default, as the window did before this setting.
    #[default]
    LeftOff,
}

impl Start {
    /// Its value in the window's preferences: `persona`, `left_off` or a page key.
    pub fn key(&self) -> &str {
        match self {
            Start::Persona => "persona",
            Start::Page(page) => page,
            Start::LeftOff => "left_off",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "persona" => Start::Persona,
            page if PAGES.contains(&page) => Start::Page(page.to_string()),
            _ => Start::LeftOff,
        }
    }
}

/// The rail's pages and the start page: a persona, the pages shown or hidden
/// on top of it, and where the window opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rail {
    pub persona: Persona,
    /// Pages shown that the persona hides.
    pub added: Vec<String>,
    /// Pages hidden that the persona shows.
    pub removed: Vec<String>,
    pub start: Start,
}

impl Rail {
    /// The page the window always opens on, which cannot be hidden: `None`
    /// with "Where I left off".
    pub fn start_page(&self) -> Option<&str> {
        match &self.start {
            Start::Persona => Some(self.persona.start()),
            Start::Page(page) => Some(page),
            Start::LeftOff => None,
        }
    }

    /// Whether `key`'s switch is greyed: the start page.
    pub fn locked(&self, key: &str) -> bool {
        self.start_page() == Some(key)
    }

    /// Whether the rail shows `key` (the tracker's page shows while a tracker
    /// is linked, and the start page always).
    pub fn shows(&self, key: &str) -> bool {
        if key == TRACKER || self.locked(key) {
            return true;
        }
        if self.persona.pages().contains(&key) {
            !self.removed.iter().any(|k| k == key)
        } else {
            self.added.iter().any(|k| k == key)
        }
    }

    /// Shows `key` if hidden, hides it if shown; not the tracker's page nor
    /// the start page.
    pub fn toggle(&mut self, key: &str) {
        if key == TRACKER || self.locked(key) {
            return;
        }
        let list = if self.persona.pages().contains(&key) {
            &mut self.removed
        } else {
            &mut self.added
        };
        match list.iter().position(|k| k == key) {
            Some(at) => {
                list.remove(at);
            }
            None => list.push(key.to_string()),
        }
    }

    /// Picks `persona`, its pages as they are (the changes on top dropped).
    pub fn set_persona(&mut self, persona: Persona) {
        self.persona = persona;
        self.added.clear();
        self.removed.clear();
    }

    /// Whether pages were shown or hidden on top of the persona.
    pub fn customised(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }

    /// How many of the rail's pages show (the tracker's aside).
    #[cfg(test)]
    pub fn shown_count(&self) -> usize {
        PAGES
            .iter()
            .filter(|k| **k != TRACKER && self.shows(k))
            .count()
    }

    /// The page a project's tab opens on, given the one it showed when the
    /// window closed: with "Where I left off", that one while the rail shows
    /// it, else the persona's.
    pub fn opening_page<'a>(&'a self, left_off: Option<&'a str>) -> &'a str {
        match self.start_page() {
            Some(page) => page,
            None => left_off
                .filter(|page| PAGES.contains(page) && self.shows(page))
                .unwrap_or(self.persona.start()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_persona_shows_known_pages_and_opens_on_one_of_them() {
        for persona in Persona::ALL {
            assert!(persona.pages().iter().all(|p| PAGES.contains(p)));
            assert!(!persona.pages().contains(&TRACKER));
            assert!(persona.pages().contains(&persona.start()));
            assert_eq!(Persona::from_key(persona.key()), persona);
        }
        // Everything is every page, the tracker's aside.
        assert_eq!(Persona::Everything.pages().len(), PAGES.len() - 1);
        assert_eq!(Persona::from_key("unknown"), Persona::Everything);
    }

    #[test]
    fn a_page_is_shown_or_hidden_on_top_of_the_persona_and_reset_with_it() {
        let mut rail = Rail {
            persona: Persona::TechLead,
            start: Start::Persona,
            ..Rail::default()
        };
        assert!(rail.shows("stats") && !rail.shows("releases"));
        rail.toggle("stats");
        rail.toggle("releases");
        assert!(!rail.shows("stats") && rail.shows("releases"));
        assert!(rail.customised());
        assert_eq!(rail.added, vec!["releases".to_string()]);
        assert_eq!(rail.removed, vec!["stats".to_string()]);
        // Toggled back: no longer a change.
        rail.toggle("releases");
        assert!(rail.added.is_empty());
        rail.set_persona(Persona::Developer);
        assert!(!rail.customised());
        assert_eq!(rail.shown_count(), Persona::Developer.pages().len());
    }

    #[test]
    fn the_start_page_and_the_tracker_cannot_be_hidden() {
        let mut rail = Rail {
            persona: Persona::Developer,
            start: Start::Page("stacks".into()),
            ..Rail::default()
        };
        assert!(rail.locked("stacks"));
        rail.toggle("stacks");
        rail.toggle(TRACKER);
        assert!(rail.shows("stacks") && rail.shows(TRACKER));
        // A persona without that page still shows it while it is the start page.
        rail.set_persona(Persona::EngineeringManager);
        assert!(rail.shows("stacks"));
        // With "Where I left off", no page is locked.
        rail.start = Start::LeftOff;
        assert!(!rail.locked("stacks") && !rail.shows("stacks"));
    }

    #[test]
    fn the_opening_page_follows_the_start_setting() {
        let mut rail = Rail {
            persona: Persona::ReleaseManager,
            ..Rail::default()
        };
        // Where I left off: the page shown last while the rail shows it…
        assert_eq!(rail.opening_page(Some("promote")), "promote");
        // …else the persona's, as for a tab never shown before.
        assert_eq!(rail.opening_page(Some("stacks")), "cockpit");
        assert_eq!(rail.opening_page(None), "cockpit");
        rail.start = Start::Persona;
        assert_eq!(rail.opening_page(Some("promote")), "cockpit");
        rail.start = Start::Page("journal".into());
        assert_eq!(rail.opening_page(Some("promote")), "journal");
        assert_eq!(Start::from_key("journal"), rail.start);
        assert_eq!(Start::from_key("persona"), Start::Persona);
        assert_eq!(Start::from_key("nonsense"), Start::LeftOff);
    }
}
