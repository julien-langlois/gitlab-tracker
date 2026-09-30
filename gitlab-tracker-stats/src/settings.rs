use gitlab_tracker_core::{
    ProjectSettingDef, ProjectSettingFactory, ProjectSettingKind, ProjectSettingValue,
};

pub const DEFAULT_RETENTION_DAYS: u32 = 365;
pub const DEFAULT_SPRINT_WEEKS: u32 = 2;

fn stats_table(project: &toml::Table) -> Option<&toml::Table> {
    project.get("stats").and_then(toml::Value::as_table)
}

/// Returns the `[stats]` table, replacing any non-table value (e.g. `stats = 1`)
/// instead of panicking on a malformed user config.
fn stats_table_mut(project: &mut toml::Table) -> &mut toml::Table {
    let entry = project
        .entry("stats".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    match entry {
        toml::Value::Table(table) => table,
        _ => unreachable!(),
    }
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
        default_value: ProjectSettingValue::U32(DEFAULT_RETENTION_DAYS),
        read: |project| read_u32(project, "retention_days", DEFAULT_RETENTION_DAYS),
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
        default_value: ProjectSettingValue::U32(DEFAULT_SPRINT_WEEKS),
        read: |project| read_u32(project, "sprint_weeks", DEFAULT_SPRINT_WEEKS),
        write: |project, value| write_u32(project, "sprint_weeks", value),
    }
}

inventory::submit!(ProjectSettingFactory(retention_days_setting));
inventory::submit!(ProjectSettingFactory(sprint_weeks_setting));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_table_stats_value_is_replaced_not_panicking() {
        let mut project: toml::Table = toml::from_str("stats = 1").unwrap();
        write_u32(&mut project, "retention_days", ProjectSettingValue::U32(30));
        assert_eq!(
            read_u32(&project, "retention_days", 365),
            ProjectSettingValue::U32(30)
        );
    }
}
