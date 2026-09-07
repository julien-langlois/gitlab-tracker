use gitlab_tracker_core::{
    ProjectSettingDef, ProjectSettingFactory, ProjectSettingKind, ProjectSettingValue,
};

fn stats_table(project: &toml::Table) -> Option<&toml::Table> {
    project.get("stats").and_then(toml::Value::as_table)
}

fn stats_table_mut(project: &mut toml::Table) -> &mut toml::Table {
    project
        .entry("stats".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .expect("stats setting must be a TOML table")
}

fn read_u32(project: &toml::Table, key: &str, default: u32) -> ProjectSettingValue {
    ProjectSettingValue::U32(
        stats_table(project)
            .and_then(|stats| stats.get(key))
            .and_then(toml::Value::as_integer)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(default),
    )
}

fn write_u32(project: &mut toml::Table, key: &str, value: ProjectSettingValue) {
    if let ProjectSettingValue::U32(number) = value {
        stats_table_mut(project).insert(key.to_string(), toml::Value::Integer(number as i64));
    }
}

fn retention_days_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "stats.retention_days",
        section: "Stats",
        label: "Retention days",
        help: "Local stats snapshots older than this many days are purged on startup.",
        priority: 110,
        kind: ProjectSettingKind::U32 { min: 1, step: 30 },
        default_value: ProjectSettingValue::U32(365),
        read: |project| read_u32(project, "retention_days", 365),
        write: |project, value| write_u32(project, "retention_days", value),
    }
}

fn sprint_weeks_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "stats.sprint_weeks",
        section: "Stats",
        label: "Sprint weeks",
        help: "Sprint duration used by throughput forecasts.",
        priority: 120,
        kind: ProjectSettingKind::U32 { min: 1, step: 1 },
        default_value: ProjectSettingValue::U32(2),
        read: |project| read_u32(project, "sprint_weeks", 2),
        write: |project, value| write_u32(project, "sprint_weeks", value),
    }
}

inventory::submit!(ProjectSettingFactory(retention_days_setting));
inventory::submit!(ProjectSettingFactory(sprint_weeks_setting));
