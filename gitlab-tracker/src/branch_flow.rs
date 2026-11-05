//! The branch flow: what each tracked branch is for (feature, integration,
//! staging, production), and so where a merged MR still has to go.
//!
//! An MR merged into a branch goes **up** the flow (promotion: develop →
//! preproduction → master) and its changes have to come **down** into the
//! branches fed by its target (sync: develop → a feature branch rebased on it,
//! or master → develop after a hotfix). The tracked branches on neither side do
//! not concern the MR.
//!
//! Roles are guessed from the branch names; `[project.branch_flow]` in
//! `projects.toml` overrides them, and gives a feature branch its parent:
//!
//! ```toml
//! [project.branch_flow]
//! redesign_develop = { role = "feature", parent = "develop" }
//! ```

#![cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "only the window draws the branch flow")
)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a branch is for, from the furthest from production to production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchRole {
    /// A long-lived line of work fed by an integration branch (`redesign_develop`).
    Feature,
    /// Where the developers merge (`develop`).
    Integration,
    /// The pre-production environment (`preproduction`, `staging`, `recette`).
    Staging,
    /// Wired to production (`master`, `main`).
    Production,
}

impl BranchRole {
    pub const ALL: [BranchRole; 4] = [
        BranchRole::Feature,
        BranchRole::Integration,
        BranchRole::Staging,
        BranchRole::Production,
    ];

    /// The role a branch most likely has, from its name.
    pub fn guess(branch: &str) -> Self {
        let name = branch.to_ascii_lowercase();
        let name = name.as_str();
        let any = |words: &[&str]| words.iter().any(|w| name.starts_with(w));
        if any(&[
            "preprod", "pre-prod", "pre_prod", "staging", "stage", "recette", "uat", "qa",
            "release",
        ]) {
            BranchRole::Staging
        } else if matches!(name, "main" | "master" | "trunk" | "live") || any(&["prod"]) {
            BranchRole::Production
        } else if matches!(
            name,
            "develop" | "development" | "dev" | "devel" | "integration"
        ) {
            BranchRole::Integration
        } else {
            BranchRole::Feature
        }
    }

    /// [`BranchRole::guess`], beside the other tracked branches: `main` or
    /// `master` is where the developers merge when a `production` branch is
    /// tracked too.
    pub fn guess_among(branch: &str, tracked: &[String]) -> Self {
        let guess = Self::guess(branch);
        let named_prod = |b: &String| b.to_ascii_lowercase().starts_with("prod");
        if guess == BranchRole::Production
            && !named_prod(&branch.to_string())
            && tracked.iter().any(named_prod)
        {
            BranchRole::Integration
        } else {
            guess
        }
    }

    /// English label, translated by the caller.
    pub fn label(self) -> &'static str {
        match self {
            BranchRole::Feature => "Feature",
            BranchRole::Integration => "Integration",
            BranchRole::Staging => "Staging",
            BranchRole::Production => "Production",
        }
    }
}

/// What `projects.toml` says of a branch; what it leaves out is guessed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSetting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<BranchRole>,
    /// The branch a feature branch is fed by (default: the first integration branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// A short name for the narrow columns (default: see [`short_name`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl BranchSetting {
    pub fn is_empty(&self) -> bool {
        self.role.is_none() && self.parent.is_none() && self.alias.is_none()
    }
}

/// A short name for `branch`, where its full name does not fit: the usual
/// abbreviations (`preproduction` → `preprod`, `develop` → `dev`), a feature
/// branch without its `feature/` prefix or `_develop` suffix, cut at 8 letters.
pub fn short_name(branch: &str) -> String {
    let lower = branch.to_ascii_lowercase();
    let known = match lower.as_str() {
        "production" => Some("prod"),
        "preproduction" | "pre-production" | "pre_production" => Some("preprod"),
        "develop" | "development" | "devel" => Some("dev"),
        "integration" => Some("integ"),
        _ => None,
    };
    if let Some(short) = known {
        return short.to_string();
    }
    let mut name = branch;
    for prefix in ["feature/", "features/", "feat/"] {
        if lower.starts_with(prefix) && lower.len() > prefix.len() {
            name = &name[prefix.len()..];
            break;
        }
    }
    for suffix in ["_develop", "-develop", "_dev", "-dev"] {
        if name.to_ascii_lowercase().ends_with(suffix) && name.len() > suffix.len() {
            name = &name[..name.len() - suffix.len()];
            break;
        }
    }
    const MAX: usize = 8;
    if name.chars().count() > MAX {
        let cut: String = name.chars().take(MAX - 1).collect();
        format!("{cut}…")
    } else {
        name.to_string()
    }
}

/// A tracked branch with its resolved role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowBranch {
    pub name: String,
    pub role: BranchRole,
    /// Whether the role was guessed from the name (not set in `projects.toml`).
    pub guessed: bool,
    /// The branch it is fed by: a feature branch's integration branch.
    pub parent: Option<String>,
    /// Its name in the narrow columns: the alias set, else [`short_name`].
    pub alias: String,
}

/// Where a merged MR goes from its target branch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MrPath {
    /// The tracked branches it has to be promoted to, nearest first.
    pub promotion: Vec<String>,
    /// The tracked branches its changes have to come down to.
    pub sync: Vec<String>,
}

/// Where a branch stands for an MR merged into `target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    /// The MR's target branch.
    Target,
    /// Up the flow, towards production.
    Promotion,
    /// Fed by the target: the changes have to come down to it.
    Sync,
    /// Neither: the MR does not concern it.
    Off,
}

impl MrPath {
    /// `items` (one per tracked branch, `name` giving it) in the MR's order:
    /// its target, up to production, the branches to sync, then the others
    /// in their own order.
    pub fn arrange<T>(
        &self,
        target: &str,
        items: Vec<T>,
        name: impl Fn(&T) -> &str,
    ) -> Vec<(T, Place)> {
        let rank = |branch: &str| -> (Place, usize) {
            if branch == target {
                (Place::Target, 0)
            } else if let Some(i) = self.promotion.iter().position(|b| b == branch) {
                (Place::Promotion, i)
            } else if let Some(i) = self.sync.iter().position(|b| b == branch) {
                (Place::Sync, i)
            } else {
                (Place::Off, 0)
            }
        };
        let mut placed: Vec<((Place, usize), T)> = items
            .into_iter()
            .map(|item| (rank(name(&item)), item))
            .collect();
        // Stable: the branches off the path keep their order.
        placed.sort_by_key(|(rank, _)| *rank);
        placed
            .into_iter()
            .map(|((place, _), item)| (item, place))
            .collect()
    }
}

/// The tracked branches of a project and how they feed each other.
#[derive(Debug, Clone, Default)]
pub struct BranchFlow {
    /// In the tracked order.
    pub branches: Vec<FlowBranch>,
}

impl BranchFlow {
    pub fn new(tracked: &[String], settings: Option<&BTreeMap<String, BranchSetting>>) -> Self {
        let setting = |name: &str| settings.and_then(|s| s.get(name));
        let mut branches: Vec<FlowBranch> = tracked
            .iter()
            .map(|name| {
                let role = setting(name).and_then(|s| s.role);
                FlowBranch {
                    name: name.clone(),
                    role: role.unwrap_or_else(|| BranchRole::guess_among(name, tracked)),
                    guessed: role.is_none(),
                    parent: None,
                    alias: setting(name)
                        .and_then(|s| s.alias.clone())
                        .filter(|a| !a.trim().is_empty())
                        .unwrap_or_else(|| short_name(name)),
                }
            })
            .collect();
        let integration = branches
            .iter()
            .find(|b| b.role == BranchRole::Integration)
            .map(|b| b.name.clone());
        for branch in &mut branches {
            let set = setting(&branch.name)
                .and_then(|s| s.parent.clone())
                .filter(|p| p != &branch.name && tracked.contains(p));
            branch.parent = match branch.role {
                BranchRole::Feature => set.or_else(|| integration.clone()),
                _ => set,
            };
        }
        Self { branches }
    }

    /// The branches from the furthest from production to production (feature,
    /// integration, staging, production), those of a role in the tracked order.
    pub fn in_order(&self) -> Vec<&FlowBranch> {
        let mut ordered: Vec<&FlowBranch> = self.branches.iter().collect();
        ordered.sort_by_key(|b| b.role);
        ordered
    }

    pub fn get(&self, name: &str) -> Option<&FlowBranch> {
        self.branches.iter().find(|b| b.name == name)
    }

    /// The role of `branch`, tracked or not.
    pub fn role(&self, branch: &str) -> BranchRole {
        self.get(branch)
            .map_or_else(|| BranchRole::guess(branch), |b| b.role)
    }

    /// Where an MR merged into `target` goes: up to production, and down into
    /// the branches fed by `target`.
    pub fn path(&self, target: &str) -> MrPath {
        let promotion = self.promotion(target);
        let mut sync = Vec::new();
        let mut roots = vec![target.to_string()];
        // A fix merged in staging or production comes back down to the
        // integration branches (and, through them, to their features).
        if self.role(target) >= BranchRole::Staging {
            roots.extend(
                self.branches
                    .iter()
                    .filter(|b| b.role > BranchRole::Feature && b.role < self.role(target))
                    .filter(|b| !promotion.contains(&b.name))
                    .map(|b| b.name.clone()),
            );
        }
        for root in &roots {
            if root != target && !sync.contains(root) {
                sync.push(root.clone());
            }
            self.descendants(root, &mut sync, target);
        }
        sync.retain(|b| !promotion.contains(b));
        // Nearest to production first, as the promotion chain reads.
        sync.sort_by_key(|b| std::cmp::Reverse(self.role(b)));
        MrPath { promotion, sync }
    }

    /// The branches above `target`, nearest first: a feature branch goes up
    /// through its parent; the others to every tracked branch of a higher role.
    fn promotion(&self, target: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut current = target.to_string();
        // Up the parents (feature → feature → integration), cycles cut short.
        while let Some(parent) = self.get(&current).and_then(|b| b.parent.clone()) {
            if parent == target || chain.contains(&parent) {
                break;
            }
            chain.push(parent.clone());
            current = parent;
        }
        let from = self.role(&current);
        let mut above: Vec<&FlowBranch> = self
            .branches
            .iter()
            .filter(|b| b.role > from && b.role > BranchRole::Feature && b.name != target)
            .filter(|b| !chain.contains(&b.name))
            .collect();
        above.sort_by_key(|b| b.role);
        chain.extend(above.into_iter().map(|b| b.name.clone()));
        chain
    }

    /// Adds to `out` the branches fed by `branch`, transitively.
    fn descendants(&self, branch: &str, out: &mut Vec<String>, target: &str) {
        for child in self
            .branches
            .iter()
            .filter(|b| b.parent.as_deref() == Some(branch))
        {
            if child.name != target && !out.contains(&child.name) {
                out.push(child.name.clone());
                self.descendants(&child.name, out, target);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked() -> Vec<String> {
        ["master", "preproduction", "develop", "redesign_develop"]
            .map(String::from)
            .to_vec()
    }

    fn names(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    #[test]
    fn roles_are_guessed_from_the_names() {
        use BranchRole::*;
        for (name, role) in [
            ("master", Production),
            ("main", Production),
            ("production", Production),
            ("preproduction", Staging),
            ("pre-prod", Staging),
            ("staging", Staging),
            ("recette", Staging),
            ("develop", Integration),
            ("dev", Integration),
            ("redesign_develop", Feature),
            ("feature/login", Feature),
            ("release/1.2", Staging),
        ] {
            assert_eq!(BranchRole::guess(name), role, "{name}");
        }
    }

    #[test]
    fn an_mr_on_develop_goes_up_and_down_into_the_feature_branch() {
        let path = BranchFlow::new(&tracked(), None).path("develop");
        assert_eq!(names(&path.promotion), ["preproduction", "master"]);
        assert_eq!(names(&path.sync), ["redesign_develop"]);
    }

    #[test]
    fn an_mr_on_the_feature_branch_goes_up_through_its_parent() {
        let path = BranchFlow::new(&tracked(), None).path("redesign_develop");
        assert_eq!(
            names(&path.promotion),
            ["develop", "preproduction", "master"]
        );
        assert!(path.sync.is_empty());
    }

    #[test]
    fn a_hotfix_on_production_comes_back_down() {
        let path = BranchFlow::new(&tracked(), None).path("master");
        assert!(path.promotion.is_empty());
        assert_eq!(
            names(&path.sync),
            ["preproduction", "develop", "redesign_develop"]
        );
    }

    #[test]
    fn main_is_the_integration_branch_beside_a_production_branch() {
        let tracked = ["main", "staging", "production"].map(String::from).to_vec();
        let flow = BranchFlow::new(&tracked, None);
        assert_eq!(flow.get("main").unwrap().role, BranchRole::Integration);
        assert_eq!(
            names(&flow.path("main").promotion),
            ["staging", "production"]
        );
    }

    #[test]
    fn the_branches_are_arranged_along_the_path() {
        let flow = BranchFlow::new(&tracked(), None);
        let mut all = tracked();
        all.push("hotfix".into());
        let arranged = flow.path("develop").arrange("develop", all, |b| b.as_str());
        let arranged: Vec<(&str, Place)> = arranged.iter().map(|(b, p)| (b.as_str(), *p)).collect();
        assert_eq!(
            arranged,
            [
                ("develop", Place::Target),
                ("preproduction", Place::Promotion),
                ("master", Place::Promotion),
                ("redesign_develop", Place::Sync),
                ("hotfix", Place::Off),
            ]
        );
    }

    #[test]
    fn the_branches_go_from_feature_to_production() {
        let flow = BranchFlow::new(&tracked(), None);
        let order: Vec<&str> = flow.in_order().iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            order,
            ["redesign_develop", "develop", "preproduction", "master"]
        );
    }

    #[test]
    fn long_branch_names_are_shortened() {
        for (name, short) in [
            ("master", "master"),
            ("main", "main"),
            ("staging", "staging"),
            ("production", "prod"),
            ("preproduction", "preprod"),
            ("develop", "dev"),
            ("redesign_develop", "redesign"),
            ("feature/checkout-v2", "checkou…"),
            ("feature/login", "login"),
        ] {
            assert_eq!(short_name(name), short, "{name}");
        }
        let settings = BTreeMap::from([(
            "master".to_string(),
            BranchSetting {
                alias: Some("prod".into()),
                ..BranchSetting::default()
            },
        )]);
        let flow = BranchFlow::new(&tracked(), Some(&settings));
        assert_eq!(flow.get("master").unwrap().alias, "prod");
    }

    #[test]
    fn the_settings_override_the_guess() {
        let tracked = ["main", "next", "v2"].map(String::from).to_vec();
        let settings = BTreeMap::from([
            (
                "next".to_string(),
                BranchSetting {
                    role: Some(BranchRole::Integration),
                    parent: None,
                    alias: None,
                },
            ),
            (
                "v2".to_string(),
                BranchSetting {
                    role: None,
                    parent: Some("next".into()),
                    alias: None,
                },
            ),
        ]);
        let flow = BranchFlow::new(&tracked, Some(&settings));
        assert!(!flow.get("next").unwrap().guessed);
        assert_eq!(flow.get("v2").unwrap().role, BranchRole::Feature);
        let path = flow.path("next");
        assert_eq!(names(&path.promotion), ["main"]);
        assert_eq!(names(&path.sync), ["v2"]);
    }

    #[test]
    fn a_feature_fed_by_a_feature_goes_up_both() {
        let mut tracked = tracked();
        tracked.push("redesign_login".into());
        let settings = BTreeMap::from([(
            "redesign_login".to_string(),
            BranchSetting {
                role: None,
                parent: Some("redesign_develop".into()),
                alias: None,
            },
        )]);
        let flow = BranchFlow::new(&tracked, Some(&settings));
        assert_eq!(
            names(&flow.path("redesign_login").promotion),
            ["redesign_develop", "develop", "preproduction", "master"]
        );
        assert_eq!(
            names(&flow.path("develop").sync),
            ["redesign_develop", "redesign_login"]
        );
    }

    #[test]
    fn an_untracked_target_is_placed_by_its_name() {
        let path = BranchFlow::new(&tracked(), None).path("feature/x");
        // Not tracked, so no parent known: every branch above a feature.
        assert_eq!(
            names(&path.promotion),
            ["develop", "preproduction", "master"]
        );
        assert!(path.sync.is_empty());
    }
}
