use gitlab_tracker_core::{
    ProjectSettingDef, ProjectSettingFactory, ProjectSettingKind, ProjectSettingValue,
};

fn read_bool(project: &toml::Table, key: &str, default: bool) -> ProjectSettingValue {
    ProjectSettingValue::Bool(
        project
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default),
    )
}

fn write_bool(project: &mut toml::Table, key: &str, value: ProjectSettingValue) {
    if let ProjectSettingValue::Bool(enabled) = value {
        project.insert(key.to_string(), toml::Value::Boolean(enabled));
    }
}

fn read_u64(project: &toml::Table, key: &str, default: u64) -> ProjectSettingValue {
    ProjectSettingValue::U64(
        project
            .get(key)
            .and_then(toml::Value::as_integer)
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(default),
    )
}

fn write_u64(project: &mut toml::Table, key: &str, value: ProjectSettingValue) {
    if let ProjectSettingValue::U64(number) = value {
        project.insert(key.to_string(), toml::Value::Integer(number as i64));
    }
}

/// Longest project name kept, in characters: it shows in the title bar and tabs.
pub const MAX_PROJECT_NAME_CHARS: usize = 80;

/// The project name as it is saved: on one line (control characters, newlines
/// and tabs become spaces, runs of spaces one), trimmed, at most
/// [`MAX_PROJECT_NAME_CHARS`] long. `None` when nothing is left: no name.
pub fn clean_project_name(name: &str) -> Option<String> {
    let one_line: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned: String = one_line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_PROJECT_NAME_CHARS)
        .collect();
    let cleaned = cleaned.trim_end().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// The `name` key, when it is a string (anything else reads as no name).
fn read_name(project: &toml::Table) -> ProjectSettingValue {
    ProjectSettingValue::Text(
        project
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    )
}

/// Writes `name` as a TOML string (the serializer quotes and escapes it), or
/// removes the key when the cleaned name is empty.
fn write_name(project: &mut toml::Table, value: ProjectSettingValue) {
    if let ProjectSettingValue::Text(name) = value {
        match clean_project_name(&name) {
            Some(name) => {
                project.insert("name".to_string(), toml::Value::String(name));
            }
            None => {
                project.remove("name");
            }
        }
    }
}

fn project_name_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.name",
        section: "Core",
        label: "Project name",
        help: "Shown in the title bar and the project picker instead of the project id. \
               Empty: the project id.",
        priority: 5,
        kind: ProjectSettingKind::Text,
        default_value: ProjectSettingValue::Text(String::new()),
        read: read_name,
        write: write_name,
    }
}

fn show_cockpit_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.show_cockpit",
        section: "Core",
        label: "Show cockpit",
        help: "Display the operational cockpit pane when enough vertical space is available.",
        priority: 10,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(true),
        read: |project| read_bool(project, "show_cockpit", true),
        write: |project, value| write_bool(project, "show_cockpit", value),
    }
}

fn discover_new_mrs_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.discover_new_mrs",
        section: "Core",
        label: "Discover new MRs",
        help: "Automatically add newly created MRs during refresh cycles.",
        priority: 20,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(false),
        read: |project| read_bool(project, "discover_new_mrs", false),
        write: |project, value| write_bool(project, "discover_new_mrs", value),
    }
}

fn hide_merged_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.hide_merged",
        section: "Core",
        label: "Hide merged MRs",
        help: "Hide merged MRs from the table; they stay tracked. Also toggled with [m].",
        priority: 15,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(false),
        read: |project| read_bool(project, "hide_merged", false),
        write: |project, value| write_bool(project, "hide_merged", value),
    }
}

fn refresh_interval_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.refresh_interval_secs",
        section: "Core",
        label: "Refresh interval",
        help: "Seconds between automatic GitLab refresh cycles.",
        priority: 30,
        kind: ProjectSettingKind::U64 { min: 30, step: 30 },
        default_value: ProjectSettingValue::U64(900),
        read: |project| read_u64(project, "refresh_interval_secs", 900),
        write: |project, value| write_u64(project, "refresh_interval_secs", value),
    }
}

fn activity_recent_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.activity_recent_days",
        section: "Core",
        label: "Recent activity days",
        help: "MRs updated within this many days are shown as recently active.",
        priority: 40,
        kind: ProjectSettingKind::U64 { min: 1, step: 1 },
        default_value: ProjectSettingValue::U64(2),
        read: |project| read_u64(project, "activity_recent_days", 2),
        write: |project, value| write_u64(project, "activity_recent_days", value),
    }
}

fn activity_stale_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.activity_stale_days",
        section: "Core",
        label: "Stale activity days",
        help: "MRs inactive for at least this many days are marked stale.",
        priority: 50,
        kind: ProjectSettingKind::U64 { min: 1, step: 1 },
        default_value: ProjectSettingValue::U64(7),
        read: |project| read_u64(project, "activity_stale_days", 7),
        write: |project, value| write_u64(project, "activity_stale_days", value),
    }
}

fn review_reminder_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "core.review_reminder_hours",
        section: "Core",
        label: "Review reminder hours",
        help: "Notify when an MR has waited this many hours for your review, and when its author \
               pushes after your review (0 = off). Needs gitlab_username.",
        priority: 60,
        kind: ProjectSettingKind::U64 { min: 0, step: 4 },
        default_value: ProjectSettingValue::U64(24),
        read: |project| read_u64(project, "review_reminder_hours", 24),
        write: |project, value| write_u64(project, "review_reminder_hours", value),
    }
}

// ── Signed commits: `[project.signed_commits]` ─────────────────────────────

/// The `[project.signed_commits]` table, if any.
fn signed_commits(project: &toml::Table) -> Option<&toml::Table> {
    project
        .get("signed_commits")
        .and_then(toml::Value::as_table)
}

fn read_signed_bool(project: &toml::Table, key: &str, default: bool) -> ProjectSettingValue {
    signed_commits(project).map_or(ProjectSettingValue::Bool(default), |table| {
        read_bool(table, key, default)
    })
}

/// The `[project.signed_commits]` table, created when missing (a value of
/// another type is replaced).
fn signed_commits_mut(project: &mut toml::Table) -> &mut toml::Table {
    let entry = project
        .entry("signed_commits")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    entry.as_table_mut().expect("just made a table")
}

fn write_signed_bool(project: &mut toml::Table, key: &str, value: ProjectSettingValue) {
    // Options of an off check are not written: the table appears once enforced.
    if signed_commits(project).is_none()
        && value == ProjectSettingValue::Bool(false)
        && key == "enforce"
    {
        return;
    }
    write_bool(signed_commits_mut(project), key, value);
}

/// A boolean option of `[project.signed_commits]`, written only once the table
/// exists (the check was turned on), so that saving the settings of a project
/// that never enforced it leaves `projects.toml` as it was.
fn write_signed_option(project: &mut toml::Table, key: &str, value: ProjectSettingValue) {
    if signed_commits(project).is_some() {
        write_bool(signed_commits_mut(project), key, value);
    }
}

fn signed_enforce_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "signed_commits.enforce",
        section: "Signed commits",
        label: "Enforce signed commits",
        help: "Check the signature of every commit of the open MRs, and warn when one is missing \
               or not verified. Off: nothing is read or shown.",
        priority: 200,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(false),
        read: |project| read_signed_bool(project, "enforce", false),
        write: |project, value| write_signed_bool(project, "enforce", value),
    }
}

fn signed_rebase_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "signed_commits.tolerate_gitlab_rebase",
        section: "Signed commits",
        label: "Tolerate rebases from GitLab",
        help: "A commit that lost its signature in a rebase from GitLab's UI counts as signed \
               when an earlier version of the MR held a signed copy of it.",
        priority: 210,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(true),
        read: |project| read_signed_bool(project, "tolerate_gitlab_rebase", true),
        write: |project, value| write_signed_option(project, "tolerate_gitlab_rebase", value),
    }
}

fn signed_patch_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "signed_commits.require_same_patch",
        section: "Signed commits",
        label: "…only with the same changes",
        help: "The signed copy must change the same lines (one diff read per commit compared). \
               Without it, a commit borrowing the author, date and title of a signed one passes.",
        priority: 220,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(true),
        read: |project| read_signed_bool(project, "require_same_patch", true),
        write: |project, value| write_signed_option(project, "require_same_patch", value),
    }
}

fn signed_verified_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "signed_commits.require_verified",
        section: "Signed commits",
        label: "Require a verified signature",
        help: "A signature GitLab cannot verify (unknown or revoked key, another user's, \
               another e-mail) is reported.",
        priority: 230,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(true),
        read: |project| read_signed_bool(project, "require_verified", true),
        write: |project, value| write_signed_option(project, "require_verified", value),
    }
}

fn signed_system_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "signed_commits.accept_system_signed",
        section: "Signed commits",
        label: "Accept commits signed by GitLab",
        help: "Commits made in GitLab's web UI, signed with the instance's own key \
               (when it has one).",
        priority: 240,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(false),
        read: |project| read_signed_bool(project, "accept_system_signed", false),
        write: |project, value| write_signed_option(project, "accept_system_signed", value),
    }
}

fn signed_bots_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "signed_commits.exempt_bots",
        section: "Signed commits",
        label: "Exempted bots",
        help: "`username email` pairs, separated by commas (renovate-bot bot@renovate.example). \
               An MR is exempted when that account opened it, for the commits with that author \
               e-mail only. Empty: none.",
        priority: 250,
        kind: ProjectSettingKind::Text,
        default_value: ProjectSettingValue::Text(String::new()),
        read: |project| {
            ProjectSettingValue::Text(
                signed_commits(project)
                    .and_then(|t| t.get("exempt_bots"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        },
        write: |project, value| {
            let ProjectSettingValue::Text(text) = value else {
                return;
            };
            let text = text.trim();
            match signed_commits(project) {
                None if text.is_empty() => {}
                _ if text.is_empty() => {
                    signed_commits_mut(project).remove("exempt_bots");
                }
                _ => {
                    signed_commits_mut(project)
                        .insert("exempt_bots".into(), toml::Value::String(text.into()));
                }
            }
        },
    }
}

inventory::submit!(ProjectSettingFactory(signed_enforce_setting));
inventory::submit!(ProjectSettingFactory(signed_rebase_setting));
inventory::submit!(ProjectSettingFactory(signed_patch_setting));
inventory::submit!(ProjectSettingFactory(signed_verified_setting));
inventory::submit!(ProjectSettingFactory(signed_system_setting));
inventory::submit!(ProjectSettingFactory(signed_bots_setting));

inventory::submit!(ProjectSettingFactory(project_name_setting));
inventory::submit!(ProjectSettingFactory(show_cockpit_setting));
inventory::submit!(ProjectSettingFactory(hide_merged_setting));
inventory::submit!(ProjectSettingFactory(discover_new_mrs_setting));
inventory::submit!(ProjectSettingFactory(refresh_interval_setting));
inventory::submit!(ProjectSettingFactory(activity_recent_setting));
inventory::submit!(ProjectSettingFactory(activity_stale_setting));
inventory::submit!(ProjectSettingFactory(review_reminder_setting));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ProjectEntry;

    fn entry() -> toml::Table {
        toml::toml! {
            gitlab_url = "https://gitlab.example"
            project_id = "42"
        }
    }

    #[test]
    fn project_names_are_cleaned_to_one_trimmed_line() {
        assert_eq!(clean_project_name("  Backend  "), Some("Backend".into()));
        assert_eq!(
            clean_project_name("My\nCompany\t—  Backend\r"),
            Some("My Company — Backend".into())
        );
        assert_eq!(clean_project_name("   \n\t "), None);
        assert_eq!(clean_project_name(""), None);
        let long = "x".repeat(200);
        assert_eq!(
            clean_project_name(&long).map(|n| n.chars().count()),
            Some(MAX_PROJECT_NAME_CHARS)
        );
    }

    #[test]
    fn the_name_is_written_as_a_toml_string_and_reads_back() {
        // Quotes, backslashes, `#`, `=` and brackets: all inside the string.
        let tricky = r#"Team "A" \ back#end = [prod] 'é'"#;
        let mut table = entry();
        write_name(&mut table, ProjectSettingValue::Text(tricky.into()));
        assert_eq!(table.get("name"), Some(&toml::Value::String(tricky.into())));

        // Through projects.toml's own types, written and parsed again.
        let project: ProjectEntry = toml::Value::Table(table).try_into().unwrap();
        let text = toml::to_string_pretty(&project).unwrap();
        let parsed: ProjectEntry = toml::from_str(&text).unwrap();
        assert_eq!(parsed.name.as_deref(), Some(tricky));
        assert_eq!(parsed.project_id, "42");
    }

    #[test]
    fn an_empty_name_removes_the_key() {
        let mut table = entry();
        table.insert("name".into(), toml::Value::String("Old".into()));
        write_name(&mut table, ProjectSettingValue::Text("   ".into()));
        assert!(!table.contains_key("name"));
        let project: ProjectEntry = toml::Value::Table(table).try_into().unwrap();
        assert_eq!(project.name, None);
    }

    /// Every setting of the registry written into `table`, in registry order.
    fn write_all(table: &mut toml::Table, values: &[(&str, ProjectSettingValue)]) {
        let mut defs: Vec<ProjectSettingDef> = inventory::iter::<ProjectSettingFactory>
            .into_iter()
            .map(|f| (f.0)())
            .collect();
        defs.sort_by_key(|d| d.priority);
        for def in defs {
            let value = values
                .iter()
                .find(|(id, _)| *id == def.id)
                .map_or_else(|| (def.read)(table), |(_, v)| v.clone());
            (def.write)(table, value);
        }
    }

    #[test]
    fn saving_a_project_that_never_enforced_signatures_adds_nothing() {
        let mut table = entry();
        write_all(&mut table, &[]);
        assert!(!table.contains_key("signed_commits"));
    }

    #[test]
    fn signed_commits_settings_round_trip_through_projects_toml() {
        let mut table = entry();
        write_all(
            &mut table,
            &[
                ("signed_commits.enforce", ProjectSettingValue::Bool(true)),
                (
                    "signed_commits.accept_system_signed",
                    ProjectSettingValue::Bool(true),
                ),
                (
                    "signed_commits.exempt_bots",
                    ProjectSettingValue::Text(" renovate-bot bot@renovate.example ".into()),
                ),
            ],
        );
        let project: ProjectEntry = toml::Value::Table(table.clone()).try_into().unwrap();
        let config = project.signed_commits.clone().unwrap();
        assert!(config.enforce);
        assert_eq!(
            config.exempt_bots.as_deref(),
            Some("renovate-bot bot@renovate.example")
        );
        let policy = config.policy().unwrap();
        assert!(policy.accept_system_signed && policy.tolerate_gitlab_rebase);
        assert_eq!(policy.exempt_bots.len(), 1);
        assert_eq!(
            (signed_rebase_setting().read)(&table),
            ProjectSettingValue::Bool(true)
        );

        // Turned off again: the options stay, the check is off.
        write_all(
            &mut table,
            &[("signed_commits.enforce", ProjectSettingValue::Bool(false))],
        );
        let project: ProjectEntry = toml::Value::Table(table).try_into().unwrap();
        assert_eq!(project.signed_commits.and_then(|c| c.policy()), None);
    }

    #[test]
    fn a_name_that_is_not_a_string_reads_as_none() {
        let mut table = entry();
        table.insert("name".into(), toml::Value::Integer(12));
        assert_eq!(read_name(&table), ProjectSettingValue::Text(String::new()));
        table.insert("name".into(), toml::Value::String("Backend".into()));
        assert_eq!(
            read_name(&table),
            ProjectSettingValue::Text("Backend".into())
        );
    }
}
