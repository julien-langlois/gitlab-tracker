//! Views of the merge request table saved by name, per project, in
//! `projects.toml` (`[[project.views]]`).
//!
//! A view keeps up to four parts of the table, each one left as it is when the
//! view does not keep it: the filters (the filter and its value, the next-step
//! focus, the status picked, whether merged MRs are hidden), the search, the
//! order (sort and grouping) and the columns. Everything is written by its
//! stable id, so a view reads the same in any project: another project's views
//! can be imported, a filter on a branch this project does not track being
//! pointed out.

#![cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "only the window saves and applies views")
)]

use serde::{Deserialize, Serialize};

/// A saved view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Filters>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Order>,
    /// The ids of the columns shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
}

/// What filters the table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters {
    /// The filter's id (`all` for none).
    pub filter: String,
    /// Its value (a branch, a label…), empty for a filter without one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// The filters on top of it, every one to pass (absent from staging and
    /// from production, with a label…), in a stable order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub and: Vec<Filter>,
    /// The next-step focus's key ("Waiting on me"…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// The status picked on the status bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub hide_merged: bool,
}

/// One more filter: its id and its value.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Filter {
    pub filter: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

/// How the table is ordered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    /// `updated`, `id`, `milestone` or `title`.
    pub sort: String,
    #[serde(default)]
    pub descending: bool,
    /// `none`, `status`, `milestone`, `author` or `next_step`.
    #[serde(default = "no_group")]
    pub group: String,
}

fn no_group() -> String {
    "none".to_string()
}

/// The table as it stands, in a view's terms: what a new view keeps, and what
/// tells a view changed since it was applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub filters: Filters,
    pub search: String,
    pub order: Order,
    pub columns: Vec<String>,
}

/// The parts a new view keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keep {
    pub filters: bool,
    pub search: bool,
    pub order: bool,
    pub columns: bool,
}

impl Default for Keep {
    fn default() -> Self {
        Self {
            filters: true,
            search: true,
            order: true,
            columns: false,
        }
    }
}

impl SavedView {
    /// A view named `name` keeping `keep` of `now`.
    pub fn of(name: &str, now: &Snapshot, keep: Keep) -> Self {
        Self {
            name: name.trim().to_string(),
            filters: keep.filters.then(|| now.filters.clone()),
            search: keep.search.then(|| now.search.clone()),
            order: keep.order.then(|| now.order.clone()),
            columns: keep.columns.then(|| now.columns.clone()),
        }
    }

    /// The parts it keeps.
    pub fn keeps(&self) -> Keep {
        Keep {
            filters: self.filters.is_some(),
            search: self.search.is_some(),
            order: self.order.is_some(),
            columns: self.columns.is_some(),
        }
    }

    /// Whether the table differs from it in what it keeps.
    pub fn differs(&self, now: &Snapshot) -> bool {
        self.filters.as_ref().is_some_and(|f| *f != now.filters)
            || self.search.as_ref().is_some_and(|s| *s != now.search)
            || self.order.as_ref().is_some_and(|o| *o != now.order)
            || self.columns.as_ref().is_some_and(|c| {
                let mut a = c.clone();
                let mut b = now.columns.clone();
                a.sort();
                b.sort();
                a != b
            })
    }

    /// The same view keeping what it keeps of `now`: "update with the table".
    pub fn updated(&self, now: &Snapshot) -> Self {
        Self::of(&self.name, now, self.keeps())
    }

    /// The tracked branches its filters need ("absent from branch").
    pub fn branches(&self) -> Vec<&str> {
        let Some(f) = &self.filters else {
            return Vec::new();
        };
        std::iter::once((f.filter.as_str(), f.value.as_str()))
            .chain(f.and.iter().map(|a| (a.filter.as_str(), a.value.as_str())))
            .filter(|(id, value)| *id == "absent_from_branch" && !value.is_empty())
            .map(|(_, value)| value)
            .collect()
    }
}

/// A name not yet in `views`: `name`, else `name (2)`, `name (3)`…
pub fn free_name(views: &[SavedView], name: &str) -> String {
    let taken = |n: &str| views.iter().any(|v| v.name.eq_ignore_ascii_case(n));
    if !taken(name) {
        return name.to_string();
    }
    (2..)
        .map(|i| format!("{name} ({i})"))
        .find(|n| !taken(n))
        .expect("some number is free")
}

/// What importing `picked` into `views` gives: the views added (renamed when
/// their name was taken), and those whose branch `branches` lacks.
pub fn import(
    views: &mut Vec<SavedView>,
    picked: &[SavedView],
    branches: &[String],
) -> (usize, Vec<String>) {
    let mut missing = Vec::new();
    for view in picked {
        let mut view = view.clone();
        view.name = free_name(views, &view.name);
        let lacking: Vec<&str> = view
            .branches()
            .into_iter()
            .filter(|b| !branches.iter().any(|t| t == b))
            .collect();
        if !lacking.is_empty() {
            missing.push(format!("{} ({})", view.name, lacking.join(", ")));
        }
        views.push(view);
    }
    (picked.len(), missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            filters: Filters {
                filter: "label".into(),
                value: "payments".into(),
                and: Vec::new(),
                focus: None,
                status: None,
                hide_merged: true,
            },
            search: String::new(),
            order: Order {
                sort: "updated".into(),
                descending: true,
                group: "none".into(),
            },
            columns: vec!["status".into(), "next_step".into()],
        }
    }

    #[test]
    fn a_view_keeps_only_what_was_asked() {
        let view = SavedView::of(" Payments ", &snapshot(), Keep::default());
        assert_eq!(view.name, "Payments");
        assert!(view.filters.is_some() && view.order.is_some() && view.search.is_some());
        assert!(view.columns.is_none());
        // What it does not keep never makes it differ.
        let mut now = snapshot();
        now.columns.clear();
        assert!(!view.differs(&now));
        now.filters.value = "billing".into();
        assert!(view.differs(&now));
        // Updated: it keeps the same parts, with the table's values.
        let updated = view.updated(&now);
        assert_eq!(updated.filters.unwrap().value, "billing");
        assert!(updated.columns.is_none());
    }

    #[test]
    fn columns_compare_in_any_order() {
        let view = SavedView::of(
            "v",
            &snapshot(),
            Keep {
                columns: true,
                ..Keep::default()
            },
        );
        let mut now = snapshot();
        now.columns.reverse();
        assert!(!view.differs(&now));
    }

    #[test]
    fn views_read_and_write_as_toml() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Project {
            views: Vec<SavedView>,
        }
        let project = Project {
            views: vec![
                SavedView::of("Payments", &snapshot(), Keep::default()),
                SavedView {
                    name: "Mine".into(),
                    search: Some("@me".into()),
                    ..SavedView::default()
                },
            ],
        };
        let text = toml::to_string(&project).unwrap();
        assert!(text.contains("[[views]]"), "{text}");
        assert!(text.contains("filter = \"label\""), "{text}");
        assert_eq!(toml::from_str::<Project>(&text).unwrap(), project);
        // A hand-written view with the least.
        let hand: Project = toml::from_str(
            "[[views]]\nname = \"Blocked\"\n[views.filters]\nfilter = \"absent_from_branch\"\nvalue = \"production\"\n",
        )
        .unwrap();
        assert_eq!(hand.views[0].branches(), ["production"]);
        assert!(!hand.views[0].filters.as_ref().unwrap().hide_merged);
    }

    #[test]
    fn combined_filters_are_kept_and_compared() {
        let mut now = snapshot();
        now.filters.and = vec![
            Filter {
                filter: "absent_from_branch".into(),
                value: "staging".into(),
            },
            Filter {
                filter: "absent_from_branch".into(),
                value: "production".into(),
            },
        ];
        let view = SavedView::of("Not shipped", &now, Keep::default());
        assert_eq!(view.branches(), ["staging", "production"]);
        let text = toml::to_string(&view).unwrap();
        assert!(text.contains("[[filters.and]]"), "{text}");
        assert_eq!(toml::from_str::<SavedView>(&text).unwrap(), view);
        // One taken off: it differs.
        now.filters.and.pop();
        assert!(view.differs(&now));
        // A view saved before filters combined reads with none on top.
        let old: SavedView =
            toml::from_str("name = \"Old\"\n[filters]\nfilter = \"label\"\nvalue = \"x\"\n")
                .unwrap();
        assert!(old.filters.unwrap().and.is_empty());
    }

    #[test]
    fn imported_views_get_a_free_name_and_their_missing_branch_is_told() {
        let mut views = vec![SavedView {
            name: "Blocked".into(),
            ..SavedView::default()
        }];
        let blocked = SavedView {
            name: "blocked".into(),
            filters: Some(Filters {
                filter: "absent_from_branch".into(),
                value: "staging".into(),
                ..Filters::default()
            }),
            ..SavedView::default()
        };
        let (added, missing) = import(&mut views, &[blocked], &["main".into()]);
        assert_eq!(added, 1);
        assert_eq!(views[1].name, "blocked (2)");
        assert_eq!(missing, ["blocked (2) (staging)"]);
        assert_eq!(free_name(&views, "Other"), "Other");
    }
}
