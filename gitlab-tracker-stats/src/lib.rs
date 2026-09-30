//! Analytics and correlation engine for gitlab-tracker MR data.
//!
//! # Crate layout
//!
//! - [`snapshot`]    — [`MrStatsSnapshot`]: owned, borrow-free input DTO built by the orchestrator.
//! - [`db`]          — [`SqliteStatsDb`]: persistence layer (SQLite via sqlx).
//! - [`metrics`]     — pure functions computing per-MR durations and rates.
//! - [`aggregator`]  — time-window and milestone aggregations over a set of snapshots.
//! - [`correlation`] — Spearman rank correlation between pairs of metrics.
//! - [`report`]      — [`StatReport`]: final output, serialisable to JSON or CSV.
//!
//! # Design constraints
//! - **No dependency** on `ratatui`, `crossterm`, or any TUI crate.
//! - Depends on `gitlab-tracker-core` only for the plugin contracts (project settings
//!   and shortcut registration), never on the binary.
//! - All public types implement `serde::{Serialize, Deserialize}` for JSON/CSV output.

pub mod aggregator;
pub mod correlation;
pub mod db;
pub mod metrics;
pub mod poisson;
pub mod report;
pub mod settings;
pub mod shortcuts;
pub mod snapshot;

pub use aggregator::{AggregatedStats, MrSizeBucketStats, QueryFilter, TimeWindow};
pub use correlation::{CorrelationResult, CorrelationStrength, MetricPair};
pub use db::{SqliteStatsDb, StatsError};
pub use metrics::PerMrMetrics;
pub use poisson::{
    AnomalySeverity, AnomalySignal, PoissonInsights, QueueInsight, QueueStatus, ThroughputForecast,
};
pub use report::StatReport;
pub use snapshot::{MrStatsSnapshot, SnapshotTrigger};
