//! *Internal crate of [gitlab-tracker](https://crates.io/crates/gitlab-tracker),
//! released with it under the same version: its API may change in any release.*

pub mod columns;
pub mod domain;
pub mod filters;
#[cfg(feature = "pacing")]
pub mod http;
pub mod i18n;
#[cfg(feature = "pacing")]
pub mod pacing;
pub mod provider;
#[cfg(feature = "secrets")]
pub mod secrets;
pub mod settings;
pub mod shortcuts;

pub use columns::{collect_all_columns, ColumnDef};
pub use domain::{GitlabMrState, MergeabilityStatus, PipelineState, Requirement};
pub use filters::{collect_all_filters, FilterDef, MrSnapshot};
pub use provider::{
    Activity, LabelColorMaps, LinkedTicket, TicketChange, TicketTransitionProvider,
    TicketTransitionTarget, TimeEntry, TimeEntryRequest, TrackerError, TrackerProvider,
    LINKED_TICKET_SCHEMA_VERSION,
};
pub use settings::{
    collect_all_project_settings, ProjectSettingDef, ProjectSettingFactory, ProjectSettingKind,
    ProjectSettingValue,
};
pub use shortcuts::{collect_all_blocks, ShortcutBlock, ShortcutEntry, ShortcutFactory};
