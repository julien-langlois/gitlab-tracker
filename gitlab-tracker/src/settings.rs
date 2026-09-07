use crate::app::App;
use crate::storage::ProjectEntry;
use gitlab_tracker_core::{
    collect_all_project_settings, ProjectSettingDef, ProjectSettingKind, ProjectSettingValue,
};

#[derive(Debug)]
pub struct SettingsEditorItem {
    pub def: ProjectSettingDef,
    pub value: ProjectSettingValue,
}

#[derive(Debug, Default)]
pub struct SettingsEditorState {
    pub cursor: usize,
    pub items: Vec<SettingsEditorItem>,
    original_project_table: toml::Table,
}

impl SettingsEditorState {
    pub fn from_project_entry(project: &ProjectEntry) -> Self {
        let project_table = toml::Value::try_from(project)
            .ok()
            .and_then(|value| value.as_table().cloned())
            .unwrap_or_default();

        Self::from_project_table(project_table)
    }

    pub fn from_project_table(project_table: toml::Table) -> Self {
        let items = collect_all_project_settings()
            .into_iter()
            .map(|def| {
                let value = normalize_value((def.read)(&project_table), &def);
                SettingsEditorItem { def, value }
            })
            .collect();

        Self {
            cursor: 0,
            items,
            original_project_table: project_table,
        }
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
    }

    pub fn cancel(&mut self) {
        *self = Self::from_project_table(self.original_project_table.clone());
    }

    pub fn selected_help(&self) -> &'static str {
        self.items
            .get(self.cursor)
            .map(|item| item.def.help)
            .unwrap_or("")
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let last = self.items.len().saturating_sub(1);
        self.cursor = (self.cursor + 1).min(last);
    }

    pub fn toggle_selected(&mut self) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            if matches!(item.def.kind, ProjectSettingKind::Bool) {
                if let ProjectSettingValue::Bool(enabled) = item.value {
                    item.value = ProjectSettingValue::Bool(!enabled);
                }
            }
        }
    }

    pub fn increment_selected(&mut self) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            match (&item.def.kind, &mut item.value) {
                (ProjectSettingKind::U64 { step, .. }, ProjectSettingValue::U64(value)) => {
                    *value = value.saturating_add(*step);
                }
                (ProjectSettingKind::U32 { step, .. }, ProjectSettingValue::U32(value)) => {
                    *value = value.saturating_add(*step);
                }
                _ => {}
            }
        }
    }

    pub fn decrement_selected(&mut self) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            match (&item.def.kind, &mut item.value) {
                (ProjectSettingKind::U64 { min, step }, ProjectSettingValue::U64(value)) => {
                    *value = value.saturating_sub(*step).max(*min);
                }
                (ProjectSettingKind::U32 { min, step }, ProjectSettingValue::U32(value)) => {
                    *value = value.saturating_sub(*step).max(*min);
                }
                _ => {}
            }
        }
    }

    pub fn push_char(&mut self, ch: char) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            if matches!(item.def.kind, ProjectSettingKind::Text) {
                if let ProjectSettingValue::Text(value) = &mut item.value {
                    value.push(ch);
                }
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            if matches!(item.def.kind, ProjectSettingKind::Text) {
                if let ProjectSettingValue::Text(value) = &mut item.value {
                    value.pop();
                }
            }
        }
    }

    pub fn to_project_table(&self) -> toml::Table {
        let mut table = self.original_project_table.clone();
        for item in &self.items {
            (item.def.write)(&mut table, item.value.clone());
        }
        table
    }

    pub fn apply_to_app(&self, app: &mut App) {
        for item in &self.items {
            match (item.def.id, &item.value) {
                ("core.show_cockpit", ProjectSettingValue::Bool(value)) => {
                    app.config.show_cockpit = *value;
                }
                ("core.discover_new_mrs", ProjectSettingValue::Bool(value)) => {
                    app.discovery_enabled = *value;
                }
                ("core.refresh_interval_secs", ProjectSettingValue::U64(value)) => {
                    app.config.refresh_interval_secs = Some(*value);
                    app.refresh_interval_secs = *value;
                    app.time_left = app.time_left.min(*value);
                }
                ("core.activity_recent_days", ProjectSettingValue::U64(value)) => {
                    app.config.activity_recent_days = *value;
                }
                ("core.activity_stale_days", ProjectSettingValue::U64(value)) => {
                    app.config.activity_stale_days = *value;
                }
                #[cfg(feature = "stats")]
                ("stats.sprint_weeks", ProjectSettingValue::U32(value)) => {
                    app.stats_view.sprint_weeks = *value;
                }
                _ => {}
            }
        }
    }
}

fn normalize_value(value: ProjectSettingValue, def: &ProjectSettingDef) -> ProjectSettingValue {
    match (&def.kind, value) {
        (ProjectSettingKind::Bool, ProjectSettingValue::Bool(value)) => {
            ProjectSettingValue::Bool(value)
        }
        (ProjectSettingKind::U64 { min, .. }, ProjectSettingValue::U64(value)) => {
            ProjectSettingValue::U64(value.max(*min))
        }
        (ProjectSettingKind::U32 { min, .. }, ProjectSettingValue::U32(value)) => {
            ProjectSettingValue::U32(value.max(*min))
        }
        (ProjectSettingKind::Text, ProjectSettingValue::Text(value)) => {
            ProjectSettingValue::Text(value)
        }
        _ => def.default_value.clone(),
    }
}
