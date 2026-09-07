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

inventory::submit!(ProjectSettingFactory(show_cockpit_setting));
inventory::submit!(ProjectSettingFactory(discover_new_mrs_setting));
inventory::submit!(ProjectSettingFactory(refresh_interval_setting));
inventory::submit!(ProjectSettingFactory(activity_recent_setting));
inventory::submit!(ProjectSettingFactory(activity_stale_setting));
