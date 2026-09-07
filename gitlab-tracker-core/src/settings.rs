/// Generic value supported by the project settings dashboard.
///
/// The contract intentionally stays UI-agnostic: plugin crates expose typed values,
/// while the orchestrator decides how to render and edit them in Ratatui.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSettingValue {
    Bool(bool),
    U64(u64),
    U32(u32),
    Text(String),
}

/// Editing behaviour and bounds for a project setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSettingKind {
    Bool,
    U64 { min: u64, step: u64 },
    U32 { min: u32, step: u32 },
    Text,
}

/// A single setting exposed by the main app or by an optional plugin crate.
///
/// `read` and `write` work directly on the active `[[project]]` TOML table so
/// optional crates can own nested sections such as `[project.stats]` without the
/// main crate hardcoding their fields.
#[derive(Debug)]
pub struct ProjectSettingDef {
    /// Stable machine-readable identifier, e.g. `core.show_cockpit`.
    pub id: &'static str,
    /// Human-readable section shown in the settings popup, e.g. `Core` or `Stats`.
    pub section: &'static str,
    /// Human-readable setting label.
    pub label: &'static str,
    /// Short contextual help shown for the highlighted row.
    pub help: &'static str,
    /// Display order. Lower values appear first.
    pub priority: u16,
    /// Editing behaviour and numeric bounds.
    pub kind: ProjectSettingKind,
    /// Default value used when the TOML key is absent or invalid.
    pub default_value: ProjectSettingValue,
    /// Reads this setting from the active project TOML table.
    pub read: fn(&toml::Table) -> ProjectSettingValue,
    /// Writes this setting to the active project TOML table.
    pub write: fn(&mut toml::Table, ProjectSettingValue),
}

/// A registered settings factory.
///
/// Function-pointer factories mirror the existing shortcut registry pattern and
/// avoid heap allocation at registration time while keeping `inventory` happy.
#[derive(Debug)]
pub struct ProjectSettingFactory(pub fn() -> ProjectSettingDef);

inventory::collect!(ProjectSettingFactory);

/// Collects all project settings registered by linked crates.
///
/// Optional crates only contribute settings when their feature is enabled and the
/// crate is linked into the final binary.
pub fn collect_all_project_settings() -> Vec<ProjectSettingDef> {
    let mut settings: Vec<ProjectSettingDef> = inventory::iter::<ProjectSettingFactory>
        .into_iter()
        .map(|factory| (factory.0)())
        .collect();
    settings.sort_by_key(|setting| setting.priority);
    settings
}
