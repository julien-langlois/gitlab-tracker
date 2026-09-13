# gitlab-tracker-stats

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker-stats)](https://crates.io/crates/gitlab-tracker-stats)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker-stats)](https://crates.io/crates/gitlab-tracker-stats)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.97+-orange.svg)](https://www.rust-lang.org/)

Optional analytics and velocity-tracking plugin for [gitlab-tracker](../README.md).

Automatically records MR snapshots into a local SQLite database and computes velocity metrics, aggregated statistics, and Spearman rank correlations — all without leaving your terminal.

![stats demo](./assets/stats.gif)

---

## ✨ What it measures

### Per-MR metrics

| Metric | Description |
| :--- | :--- |
| **Cycle time** | Elapsed time from MR creation to merge (in hours) |
| **Diff size** | Total changed lines (additions + deletions) |
| **Diff difficulty** | Pre-computed review score in \[0.0, 1.0\] (from your `complexity_profile`) |
| **Comment count** | Total human notes and discussion threads |
| **Comment density** | Notes per 100 changed lines — normalised discussion load |
| **Pipeline failure rate** | Fraction of pipeline runs that ended in `Failed` state |

### Aggregated statistics (per time window or milestone)

Aggregations operate on deduplicated MRs, not raw snapshot rows. Current-state metrics use the latest known snapshot per MR, while merge-related metrics use one deduplicated `on_merge` snapshot per MR.

| Statistic | Description |
| :--- | :--- |
| **Throughput** | MRs merged per calendar week, counted once per MR |
| **Cycle time median, P75 & P90** | Central tendency (P50), upper quartile (P75), and long-tail indicator (P90) — all measured from MR creation to merge |
| **Cycle time by author** | **Median** cycle time per MR author — robust to outlier MRs, highlights structural review patterns per contributor |
| **Cycle time by reviewer** | **Median** cycle time per assigned reviewer — surfaces review bottlenecks without being skewed by one-off long MRs |
| **Cycle time by milestone** | **Median** cycle time per milestone — per-sprint velocity comparison |
| **Backlog age** | Age distribution of currently open MRs based on their latest snapshot — identifies stagnant reviews |
| **Backlog aging buckets** | Counts of open MRs older than 7, 14, and 30 days |
| **Abandon rate** | Share of terminal MRs that were closed without being merged |
| **Comment density** | Average comments per 100 changed lines — normalises discussion volume by MR size |
| **Pipeline data coverage** | Share of MRs with pipeline data, used to qualify pipeline-related metrics |
| **Data confidence** | Sample-size and metadata coverage indicators for total MRs, merged cycle-time sample, pipelines, reviewers, and milestones |
| **MR size buckets** | Small / Medium / Large / Huge diff-size buckets with total MRs, merged MRs, and median cycle time |
| **Diff size / comments / pipeline failure rate** | Averages across deduplicated MRs in the window |

### Spearman rank correlations

All correlations use **Spearman's ρ** (rank-based, robust against outliers and non-normal distributions) with a two-tailed p-value approximated via the t-distribution.

| Pair | Question answered |
| :--- | :--- |
| **Diff size ↔ Cycle time** | Do larger MRs take longer to merge? |
| **Diff size ↔ Comments** | Do larger MRs generate more discussion? |
| **Comments ↔ Cycle time** | Are heavily commented MRs review bottlenecks? |
| **Pipeline failures ↔ Cycle time** | Does CI instability delay delivery? |
| **Diff difficulty ↔ Cycle time** | Does review complexity predict merge latency? |
| **Commits ↔ Cycle time** | Do MRs with more commits take longer? |

Correlations are colour-coded by strength (|ρ|) and filtered for significance (p < 0.05):

| Strength | ρ range | Colour |
| :--- | :--- | :--- |
| Negligible | < 0.10 | Dimmed |
| Weak | 0.10 – 0.29 | Muted |
| Moderate | 0.30 – 0.49 | 🟡 Yellow |
| Strong | 0.50 – 0.69 | 🟢 Green |
| Very strong | ≥ 0.70 | 🔵 Cyan |

### Poisson insights

The **Forecasts** tab uses Poisson probabilities for throughput forecasts and baseline-aware anomaly signals. Queue metrics are presented as a flow-pressure heuristic rather than an exact queueing model: they use observed merge throughput and median cycle time to estimate whether delivery is approaching capacity.

#### Throughput forecasts

Given the observed λ (mean merges per week), five forecasts answer distinct operational questions:

`P(X ≥ target) = 1 − CDF(target − 1)` under Poisson(λ × forecast_weeks).

| Forecast | Window | Question |
| :--- | :--- | :--- |
| **At pace** | 1 week | Will we match our usual weekly pace? |
| **At pace** | 2 weeks | Will we match our usual sprint pace? |
| **Sprint pace** | 2 weeks | Realistic sprint target (uses `round(λ×2)` instead of `ceil` to avoid near-0% bias on fractional λ) |
| **Stretch goal** | 2 weeks | How likely are we to beat our average by 20%? |
| **Floor check** | 1 week | Will we merge at least 1 MR? (sanity floor for slow periods) |

| Colour | Meaning |
| :--- | :--- |
| 🟢 Green | ≥ 75% probability — pace is sustainable |
| 🟡 Yellow | 40–74% — achievable but uncertain |
| 🔴 Red | < 40% — target is unlikely at current pace |

#### Flow pressure

The Overview tab exposes a lightweight M/M/1-inspired pressure estimate:

- **Throughput proxy λ** = observed merge throughput (merges/week)
- **Capacity proxy μ** = 168 h ÷ median cycle time (MRs/week)
- **Flow pressure ρ = λ/μ**

Because throughput is a departure rate and cycle time includes waiting time, this should be read as an operational pressure signal, not a precise queueing model.

| `QueueStatus` | Meaning |
| :--- | :--- |
| `Stable` | λ and μ are available and λ < μ; expected MRs in system and expected wait can be shown |
| `OverCapacity` | λ ≥ μ; wait-time formulas are not valid, but the dashboard highlights capacity pressure |
| `InsufficientData` | Throughput or cycle-time data is missing |

ρ is colour-coded: 🟢 < 0.60 · 🟡 0.60–0.79 · 🔴 ≥ 0.80.

#### Pressure and baseline anomaly signals

Signals are split into two families:

| Family | Source | Examples |
| :--- | :--- | :--- |
| **Pressure signals** | Deterministic thresholds on the current window | stale open MRs ≥7/14/30 days, high abandon rate, high pipeline failure rate, high comment density |
| **Historical-baseline anomalies** | Poisson right-tail probability against the previous equivalent time window | throughput spike, pipeline failure rate spike, comment density spike, abandon rate spike, cycle-time P90 spike |

Baseline selection is automatic when the active report has a duration:

| Current window | Baseline window |
| :--- | :--- |
| `LastDays(N)` | The previous `N` days immediately before the current window |
| `Range { from, to }` | The same duration immediately before `from` |
| `Milestone` / `All time` | No automatic baseline; only pressure signals are shown |

Severity is derived from the right-tail p-value P(X ≥ observed | λ) for baseline anomalies:

| Severity | p-value | Icon |
| :--- | :--- | :--- |
| Normal | > 0.10 | ✔ (not shown) |
| Elevated | 0.05 – 0.10 | ↑ Cyan |
| Warning | 0.01 – 0.05 | ⚠ Yellow |
| Critical | ≤ 0.01 | ✘ Red |

Pressure signals are displayed as `pressure signal`; baseline anomalies show `observed`, `baseline`, and `p`.

---

## ⚡ Enabling the feature

Add the `stats` feature flag when building or installing:

```bash
# From source
cargo build --release --features stats

# Install from the workspace
cargo install --path gitlab-tracker --features stats
```

Or set it as a default in `gitlab-tracker/Cargo.toml`:

```toml
[features]
default = ["notifications", "stats"]
```

---

## ⌨️ Keyboard shortcuts

| Key | Action |
| :--- | :--- |
| `g` / `G` | **Open / close** the Stats fullscreen overlay |
| `Tab` / `Shift+Tab` | Cycle Stats tabs forward / backward |
| `1`–`5` | Jump directly to Overview, Flow, Quality, Forecasts, or Correlations |
| `w` / `W` | **Cycle time window** — Last 30 days → 90 days → 365 days → All time |
| `r` / `R` | Refresh the current stats report |
| `j` / `↓` | Scroll content down |
| `k` / `↑` | Scroll content up |
| `PgDn` / `PgUp` | Scroll by 10 lines |
| `Esc` | Close the overlay |

All shortcuts are registered automatically via `inventory::submit!` and appear in the `[?]` help popup under the **Stats** section — no manual wiring needed.

---

## 🗄️ Data storage

Snapshots are stored in a **SQLite database** alongside `tracker_state.json` in the XDG config directory:

| Platform | Path |
| :--- | :--- |
| Linux | `~/.config/gitlab-tracker/stats.db` |
| macOS | `~/Library/Application Support/gitlab-tracker/stats.db` |
| Windows | `C:\Users\<User>\AppData\Roaming\gitlab-tracker\stats.db` |

### Snapshot triggers

Three events cause a snapshot to be written:

| Trigger | When | Data quality |
| :--- | :--- | :--- |
| `on_merge` | MR transitions to `Merged` | Complete — all timing fields present |
| `on_close` | MR transitions to `Closed` | Complete — distinguishes abandonment from merge in throughput |
| `on_refresh` | MR still open, at most once per calendar day | Partial — enables backlog age tracking and stagnation detection |

Idempotency is enforced at the DB level: `UNIQUE(mr_id, project_id, DATE(recorded_at), trigger)` prevents duplicate snapshots even if the application is restarted mid-day.

### Retention policy

Configure per-project data retention in `projects.toml` (defaults to 365 days):

```toml
[[project]]
gitlab_url = "https://gitlab.example.com"
project_id = "12345678"

[project.stats]
# Keep snapshots for 6 months, then purge automatically on startup. Default: 365.
retention_days = 180

# Sprint duration in weeks — controls the window used in throughput forecasts
# ("At pace", "Sprint pace", "Stretch goal" rows). Default: 2.
sprint_weeks = 3
```

> **Tip — team-wide stats coverage:** enable `discover_new_mrs = true` at the project level
> (not under `[project.stats]`) to automatically track all newly created MRs at each refresh
> cycle, including MRs already merged between two cycles. When the `stats` feature is active,
> discovered MRs are snapshotted automatically, improving throughput and cycle-time coverage
> across all reviewers.

---

## 📐 Architecture

```text
gitlab-tracker-stats         (library — zero TUI dependency)
    │
    ├── snapshot.rs          MrStatsSnapshot + SnapshotTrigger
    │                        ↑ constructed by the orchestrator (gitlab-tracker)
    │
    ├── db.rs                StatsDb trait + SqliteStatsDb
    │                        upsert_snapshot / query / purge_old_snapshots
    │
    ├── metrics.rs           PerMrMetrics — pure functions over StoredSnapshot
    │                        cycle_time_hours, pipeline_failure_rate, comment_density
    │
    ├── aggregator.rs        TimeWindow (LastDays / Milestone / Range)
    │                        aggregate() → AggregatedStats
    │                        flow, quality, confidence, stale backlog, size buckets
    │
    ├── correlation.rs       Spearman ρ with tie-handling, p-value via t-distribution
    │                        compute_all_correlations() → Vec<CorrelationResult>
    │
    ├── poisson.rs           Poisson forecasts, flow pressure, pressure signals,
    │                        historical-baseline anomalies, QueueStatus
    │
    ├── report.rs            StatReport::load() / build_with_baseline()
    │                        to_json() / to_csv_rows()
    │
    └── shortcuts.rs         inventory::submit! — auto-registers Stats shortcuts
                             in the [?] help popup

gitlab-tracker/src/ui/stats.rs                 (TUI shell in the binary crate)
    │
    └── stats/
        ├── common.rs       Shared layout, blocks, scrolling, formatting helpers
        ├── overview.rs     Overview tab: throughput, cycle time, backlog, pressure
        ├── flow.rs         Flow tab: author/reviewer/milestone cycle-time bars
        ├── quality.rs      Quality tab: data confidence and MR size buckets
        ├── forecasts.rs    Forecasts tab: throughput forecasts and signals
        └── correlations.rs Correlations tab: Spearman rows and significance styling
```

**Design constraints (same as `gitlab-tracker-core`):**
- ❌ No dependency on `ratatui`, `crossterm`, or any TUI crate
- ❌ No dependency on `gitlab-tracker` (the binary) — fully standalone
- ✅ All public types implement `serde::{Serialize, Deserialize}` — ready for future CLI export
- ✅ `StatsDb` is a trait — swappable with an in-memory implementation for unit tests

The analytics engine lives in `gitlab-tracker-stats` and has no TUI dependency. The TUI rendering lives in `gitlab-tracker/src/ui/stats.rs` and its `stats/` submodules, which depend on this crate but not vice-versa — the separation mirrors `gitlab-tracker-core` ↔ `gitlab-tracker/src/ui/inspector.rs`.

---

## 🔮 Future: standalone CLI export

The `StatReport` type is already serialisable. A future `gitlab-tracker-stats-cli` binary will expose:

```bash
# Not yet implemented — planned for a future release
gitlab-tracker-stats --window 30d --output json
gitlab-tracker-stats --milestone "Sprint 42" --output csv
```

The library API is stable and ready for this — only the binary wrapper is missing.

---

## 📄 License

Distributed under the MIT License. See [`LICENSE`](../LICENSE) for details.
