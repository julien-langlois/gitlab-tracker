//! Analytics and correlation engine for gitlab-tracker MR data.
//!
//! # Crate layout
//!
//! - [`snapshot`]    — [`MrStatsSnapshot`]: owned, borrow-free input DTO built by the orchestrator.
//! - [`db`]          — [`StatsDb`] trait + [`SqliteStatsDb`]: persistence layer (SQLite via sqlx).
//! - [`metrics`]     — pure functions computing per-MR durations and rates.
//! - [`aggregator`]  — time-window and milestone aggregations over a set of snapshots.
//! - [`correlation`] — Spearman rank correlation between pairs of metrics.
//! - [`report`]      — [`StatReport`]: final output, serialisable to JSON or CSV.
//!
//! # Design constraints
//! - **No dependency** on `ratatui`, `crossterm`, or any TUI crate.
//! - **No dependency** on `gitlab-tracker-core` — this crate is fully standalone.
//! - All public types implement `serde::{Serialize, Deserialize}` for JSON/CSV output.

pub mod aggregator;
pub mod correlation;
pub mod db;
pub mod metrics;
pub mod report;
pub mod shortcuts;
pub mod snapshot;

pub use aggregator::{AggregatedStats, QueryFilter, TimeWindow};
pub use correlation::{CorrelationResult, CorrelationStrength, MetricPair};
pub use db::{SqliteStatsDb, StatsDb, StatsError};
pub use metrics::PerMrMetrics;
pub use report::StatReport;
pub use snapshot::{MrStatsSnapshot, SnapshotTrigger};
