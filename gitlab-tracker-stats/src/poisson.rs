//! Poisson-based forecasting and anomaly detection for MR activity metrics.
//!
//! # Background
//!
//! The Poisson distribution models the number of discrete, independent events
//! occurring in a fixed time interval. In the context of MR analytics, the
//! following quantities naturally follow a Poisson process:
//!
//! - Number of MRs merged per week (throughput)
//! - Number of pipeline failures per window
//! - Number of comments per MR
//!
//! Given an observed mean rate λ (lambda), this module provides:
//!
//! - **PMF / CDF**: exact probabilities for a given event count.
//! - **Throughput forecast**: probability of reaching a merge target in a future window.
//! - **Anomaly detection**: right-tail p-value flagging abnormally high counts.
//! - **Queue estimation**: expected backlog wait time via an M/M/1 approximation.

use serde::{Deserialize, Serialize};

use crate::aggregator::AggregatedStats;

// ── Core Poisson primitives ───────────────────────────────────────────────────

/// Computes P(X = k) under Poisson(λ) using the log-space formula to avoid
/// overflow for large k: exp(k·ln(λ) - λ - ln(k!)).
///
/// Returns 0.0 when λ ≤ 0 or when the result underflows to zero.
pub fn pmf(lambda: f64, k: u32) -> f64 {
    if lambda <= 0.0 {
        return if k == 0 { 1.0 } else { 0.0 };
    }
    let log_p = k as f64 * lambda.ln() - lambda - log_factorial(k);
    log_p.exp()
}

/// Computes P(X ≤ k) = Σ_{i=0}^{k} P(X = i) under Poisson(λ).
pub fn cdf(lambda: f64, k: u32) -> f64 {
    (0..=k).map(|i| pmf(lambda, i)).sum::<f64>().min(1.0)
}

/// Computes the right-tail exceedance probability P(X ≥ k).
///
/// This is the p-value used for anomaly detection: a small value (< 0.05)
/// means the observed count is statistically unlikely under the baseline λ.
///
/// Above the mean the tail is summed directly: `1 − cdf` cancels to 0 there,
/// exactly where the small p-values that matter live. At or below the mean the
/// tail is large and `1 − cdf` is accurate.
pub fn exceedance_probability(lambda: f64, k: u32) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k as f64 <= lambda {
        return (1.0 - cdf(lambda, k - 1)).max(0.0);
    }
    // k > λ: each term is the previous one × λ/i < 1, so the series converges fast.
    let mut term = pmf(lambda, k);
    let mut sum = 0.0;
    let mut i = k;
    while term > 0.0 && term > sum * f64::EPSILON {
        sum += term;
        i += 1;
        term *= lambda / i as f64;
    }
    sum.min(1.0)
}

/// Computes ln(k!) via Stirling-series (Lanczos) for numerical stability.
fn log_factorial(k: u32) -> f64 {
    if k == 0 {
        return 0.0;
    }
    // ln(k!) = ln(Γ(k+1))
    ln_gamma(k as f64 + 1.0)
}

/// Lanczos approximation of ln(Γ(x)) (g=7, n=9), accurate to ~1e-15 for x > 0.
pub(crate) fn ln_gamma(x: f64) -> f64 {
    const COEFFS: [f64; 9] = [
        0.999_999_999_999_809_3,
        676.520_368_121_885_1,
        -1_259.139_216_722_403,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_9,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let z = x - 1.0;
    let mut sum = COEFFS[0];
    for (i, &c) in COEFFS[1..].iter().enumerate() {
        sum += c / (z + i as f64 + 1.0);
    }
    let t = z + 7.5;
    0.5 * std::f64::consts::TAU.ln() + (z + 0.5) * t.ln() - t + sum.ln()
}

// ── Throughput forecast ───────────────────────────────────────────────────────

/// Forecast of the probability of merging at least `target` MRs in a future
/// window of `weeks` calendar weeks, given the observed weekly throughput λ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputForecast {
    /// Observed mean merges per week (λ per week).
    pub lambda_per_week: f64,

    /// Number of future weeks the forecast covers.
    pub forecast_weeks: u32,

    /// Minimum number of merges to reach (the "target").
    pub target_merges: u32,

    /// P(X ≥ target_merges) under Poisson(λ × forecast_weeks).
    /// Interpret as: "this is the probability we hit or exceed our target".
    pub probability: f64,

    /// Expected (mean) merges over the forecast window.
    pub expected_merges: f64,
}

impl ThroughputForecast {
    /// Builds a forecast for the given λ, window, and target.
    ///
    /// `lambda_per_week` must come from [`AggregatedStats::throughput_per_week`].
    pub fn new(lambda_per_week: f64, forecast_weeks: u32, target_merges: u32) -> Self {
        let lambda_window = lambda_per_week * forecast_weeks as f64;
        let probability = exceedance_probability(lambda_window, target_merges);
        Self {
            lambda_per_week,
            forecast_weeks,
            target_merges,
            probability,
            expected_merges: lambda_window,
        }
    }
}

// ── Anomaly detection ─────────────────────────────────────────────────────────

/// Severity level of an anomaly: from the right-tail p-value for Poisson count
/// tests, from `observed / baseline` for ratio tests (see [`AnomalySignal`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalySeverity {
    /// p > 0.10, or ratio < ×1.5 — within normal variation, no action needed.
    Normal,
    /// 0.05 < p ≤ 0.10, or ratio ≥ ×1.5 — slightly elevated, worth monitoring.
    Elevated,
    /// 0.01 < p ≤ 0.05, or ratio ≥ ×2 — significant.
    Warning,
    /// p ≤ 0.01, or ratio ≥ ×3 — highly anomalous, investigate immediately.
    Critical,
}

impl AnomalySeverity {
    /// Anything above `Normal`.
    pub fn is_anomalous(&self) -> bool {
        *self != AnomalySeverity::Normal
    }

    fn from_p_value(p: f64) -> Self {
        if p > 0.10 {
            Self::Normal
        } else if p > 0.05 {
            Self::Elevated
        } else if p > 0.01 {
            Self::Warning
        } else {
            Self::Critical
        }
    }
}

/// An anomaly signal for a single metric dimension.
///
/// Three kinds of signal share this shape; the optional fields say which one it is:
/// - **Poisson count test** (merges in the window vs the baseline pace):
///   `baseline` is the expected count λ and `p_value` is P(X ≥ observed);
/// - **ratio test** (rates and durations, compared to the baseline value in the same
///   unit): `baseline` and `ratio` (= observed / baseline) are set, `p_value` is not;
/// - **pressure signal** (fixed threshold, no baseline): only `observed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalySignal {
    /// Human-readable name of the metric being monitored.
    pub metric: String,

    /// Observed value in the current window, in the metric's own unit
    /// (count, share, hours, percent for pressure rates…).
    pub observed: f64,

    /// Baseline value in the same unit (expected λ for a Poisson count test).
    pub baseline: Option<f64>,

    /// `observed / baseline`, for ratio-tested metrics.
    pub ratio: Option<f64>,

    /// P(X ≥ observed) under Poisson(λ) — Poisson count tests only.
    pub p_value: Option<f64>,

    /// Qualitative severity.
    pub severity: AnomalySeverity,
}

impl AnomalySignal {
    /// Returns `true` when this signal is worth surfacing to the user.
    pub fn is_anomalous(&self) -> bool {
        self.severity.is_anomalous()
    }
}

// ── Queue / backlog estimation (M/M/1) ───────────────────────────────────────

/// Availability state for the queue pressure estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
    /// Throughput or cycle-time data is missing.
    InsufficientData,

    /// The observed flow is above the estimated capacity, so M/M/1 wait-time
    /// formulas are not valid. This is still useful as a pressure signal.
    OverCapacity,

    /// The estimate is stable enough to expose queue pressure values.
    Stable,
}

/// M/M/1-inspired flow pressure model: λ is the observed merge throughput and μ
/// is estimated from the median cycle time.
///
/// This is intentionally presented as a pressure heuristic rather than an exact
/// queueing model: throughput is a departure rate and cycle time includes waiting.
/// It is still useful to highlight whether the current flow is close to or above
/// the observed delivery capacity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueInsight {
    /// Availability state for the estimate.
    pub status: QueueStatus,

    /// Arrival/departure proxy in MRs per week.
    pub arrival_rate_per_week: Option<f64>,

    /// Service-capacity proxy in MRs per week, derived from median cycle time.
    pub service_rate_per_week: Option<f64>,

    /// Traffic intensity ρ = λ/μ when both values are available.
    pub traffic_intensity: Option<f64>,

    /// Expected number of MRs in the system (L = ρ / (1 - ρ)) for stable flow.
    pub expected_mrs_in_system: Option<f64>,

    /// Expected additional wait (in hours) for the next MR for stable flow.
    pub expected_wait_hours: Option<f64>,
}

impl QueueInsight {
    /// Derives a queue-pressure insight from aggregated stats.
    pub fn from_stats(stats: &AggregatedStats) -> Self {
        let Some(lambda) = stats.throughput_per_week else {
            return Self::insufficient_data();
        };
        let Some(cycle_time_hours) = stats.cycle_time_median_hours else {
            return Self::insufficient_data();
        };

        // Convert cycle time to a weekly service-capacity proxy:
        // μ = (168 hours/week) / cycle_time.
        let mu = 168.0 / cycle_time_hours;

        if lambda <= 0.0 || mu <= 0.0 {
            return Self::insufficient_data();
        }

        let rho = lambda / mu;
        if lambda >= mu {
            return Self {
                status: QueueStatus::OverCapacity,
                arrival_rate_per_week: Some(lambda),
                service_rate_per_week: Some(mu),
                traffic_intensity: Some(rho),
                expected_mrs_in_system: None,
                expected_wait_hours: None,
            };
        }

        let l = rho / (1.0 - rho);
        let w_hours = (l / lambda) * 168.0;

        Self {
            status: QueueStatus::Stable,
            arrival_rate_per_week: Some(lambda),
            service_rate_per_week: Some(mu),
            traffic_intensity: Some(rho),
            expected_mrs_in_system: Some(l),
            expected_wait_hours: Some(w_hours),
        }
    }

    fn insufficient_data() -> Self {
        Self {
            status: QueueStatus::InsufficientData,
            arrival_rate_per_week: None,
            service_rate_per_week: None,
            traffic_intensity: None,
            expected_mrs_in_system: None,
            expected_wait_hours: None,
        }
    }
}

// ── Top-level container ───────────────────────────────────────────────────────

/// All Poisson-derived insights for a single report.
///
/// Populated in [`crate::report::StatReport::build_with_baseline`] and serialised alongside
/// correlations as a distinct section of the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoissonInsights {
    /// 1-week and 2-week throughput forecasts for reaching the observed mean
    /// as a target (i.e., "will we at least match our usual pace?").
    pub throughput_forecasts: Vec<ThroughputForecast>,

    /// Anomaly signals for pipeline failures and comment volume.
    /// Only signals with severity ≥ Elevated are included.
    pub anomalies: Vec<AnomalySignal>,

    /// M/M/1-inspired queue pressure insight.
    pub queue_insight: QueueInsight,
}

impl PoissonInsights {
    /// Derives Poisson insights using an optional historical baseline.
    pub fn from_stats_with_baseline(
        stats: &AggregatedStats,
        baseline: Option<&AggregatedStats>,
        sprint_weeks: u32,
    ) -> Self {
        let throughput_forecasts = build_throughput_forecasts(stats, sprint_weeks);
        let anomalies = build_anomaly_signals(stats, baseline);
        let queue_insight = QueueInsight::from_stats(stats);

        Self {
            throughput_forecasts,
            anomalies,
            queue_insight,
        }
    }
}

/// Builds actionable throughput forecasts from the observed weekly rate λ.
///
/// Five forecasts are produced:
/// 1. **At-pace 1w** — P(≥ ceil(λ) in 1 week): will we match our usual weekly pace?
/// 2. **At-pace 2w** — P(≥ ceil(λ×2) in 2 weeks): will we match our usual sprint pace?
/// 3. **Sprint pace** — P(≥ round(λ×2) in 2w): realistic sprint target using round
///    instead of ceil to avoid near-0% probabilities when λ is fractional.
/// 4. **Stretch goal** — P(≥ λ×2 +20% in 2w): how likely are we to beat our average?
/// 5. **Floor check** — P(≥ 1 in 1w): sanity floor — are we merging anything at all?
fn build_throughput_forecasts(
    stats: &AggregatedStats,
    sprint_weeks: u32,
) -> Vec<ThroughputForecast> {
    let Some(lambda) = stats.throughput_per_week else {
        return vec![];
    };
    if lambda <= 0.0 {
        return vec![];
    }

    let sw = sprint_weeks as f64;

    // Original forecasts: exact-pace targets using ceil.
    let target_1w = lambda.ceil() as u32;
    let target_sprint_ceil = (lambda * sw).ceil() as u32;

    // Sprint target with round() avoids near-0% bias on fractional λ.
    let sprint_target = (lambda * sw).round().max(1.0) as u32;
    // Stretch: +20% above sprint pace.
    let stretch_target = (lambda * sw * 1.2).ceil() as u32;

    vec![
        ThroughputForecast::new(lambda, 1, target_1w),
        ThroughputForecast::new(lambda, sprint_weeks, target_sprint_ceil),
        ThroughputForecast::new(lambda, sprint_weeks, sprint_target),
        ThroughputForecast::new(lambda, sprint_weeks, stretch_target),
        ThroughputForecast::new(lambda, 1, 1),
    ]
}

/// Builds anomaly signals from the current window and, when available, a
/// historical baseline. Without a baseline, this falls back to deterministic
/// pressure signals that do not pretend to be statistically significant.
fn build_anomaly_signals(
    stats: &AggregatedStats,
    baseline: Option<&AggregatedStats>,
) -> Vec<AnomalySignal> {
    let mut signals = build_pressure_signals(stats);

    let Some(baseline) = baseline else {
        return signals;
    };

    push_count_anomaly(&mut signals, "throughput_spike", stats, baseline);
    push_ratio_anomaly(
        &mut signals,
        "pipeline_failure_rate_spike",
        baseline.avg_pipeline_failure_rate,
        stats
            .avg_pipeline_failure_rate
            .filter(|_| stats.pipeline_data_coverage >= 0.5),
        Some(MIN_SHARE_INCREASE),
    );
    push_ratio_anomaly(
        &mut signals,
        "comment_density_spike",
        baseline.avg_comment_density,
        stats.avg_comment_density,
        None,
    );
    push_ratio_anomaly(
        &mut signals,
        "abandon_rate_spike",
        baseline.abandon_rate,
        stats.abandon_rate,
        Some(MIN_SHARE_INCREASE),
    );
    push_ratio_anomaly(
        &mut signals,
        "cycle_time_p90_spike",
        baseline.cycle_time_p90_hours,
        stats.cycle_time_p90_hours,
        None,
    );

    signals
}

fn build_pressure_signals(stats: &AggregatedStats) -> Vec<AnomalySignal> {
    let mut signals = Vec::new();

    if stats.stale_open_mrs_30d > 0 {
        signals.push(pressure_signal(
            "stale_open_mrs_30d",
            stats.stale_open_mrs_30d as u32,
            AnomalySeverity::Critical,
        ));
    } else if stats.stale_open_mrs_14d > 0 {
        signals.push(pressure_signal(
            "stale_open_mrs_14d",
            stats.stale_open_mrs_14d as u32,
            AnomalySeverity::Warning,
        ));
    } else if stats.stale_open_mrs_7d > 0 {
        signals.push(pressure_signal(
            "stale_open_mrs_7d",
            stats.stale_open_mrs_7d as u32,
            AnomalySeverity::Elevated,
        ));
    }

    if let Some(abandon_rate) = stats.abandon_rate {
        let severity = if abandon_rate >= 0.25 {
            Some(AnomalySeverity::Warning)
        } else if abandon_rate >= 0.10 {
            Some(AnomalySeverity::Elevated)
        } else {
            None
        };

        if let Some(severity) = severity {
            signals.push(pressure_signal(
                "abandon_rate",
                (abandon_rate * 100.0).round() as u32,
                severity,
            ));
        }
    }

    if let Some(avg_failure_rate) = stats.avg_pipeline_failure_rate {
        if stats.pipeline_data_coverage >= 0.5 && avg_failure_rate >= 0.30 {
            signals.push(pressure_signal(
                "pipeline_failure_rate",
                (avg_failure_rate * 100.0).round() as u32,
                AnomalySeverity::Warning,
            ));
        }
    }

    if let Some(comment_density) = stats.avg_comment_density {
        if comment_density >= 5.0 {
            signals.push(pressure_signal(
                "comment_density_per_100_lines",
                comment_density.round() as u32,
                AnomalySeverity::Elevated,
            ));
        }
    }

    signals
}

fn pressure_signal(
    metric: impl Into<String>,
    observed: u32,
    severity: AnomalySeverity,
) -> AnomalySignal {
    AnomalySignal {
        metric: metric.into(),
        observed: f64::from(observed),
        baseline: None,
        ratio: None,
        p_value: None,
        severity,
    }
}

/// Minimum absolute increase for shares in [0, 1] (failure, abandon rates): going
/// from 1 % to 2 % doubles the rate but is noise, not an anomaly.
const MIN_SHARE_INCREASE: f64 = 0.05;

/// Severity of a rate or duration relative to its baseline (same unit, so the result
/// does not depend on the unit: hours or days give the same answer).
fn ratio_severity(ratio: f64) -> AnomalySeverity {
    if ratio >= 3.0 {
        AnomalySeverity::Critical
    } else if ratio >= 2.0 {
        AnomalySeverity::Warning
    } else if ratio >= 1.5 {
        AnomalySeverity::Elevated
    } else {
        AnomalySeverity::Normal
    }
}

/// Ratio test for rates and durations. Poisson models counts, not shares in [0, 1]
/// or hours: applied to those, any tiny increase over a low baseline was flagged.
fn push_ratio_anomaly(
    signals: &mut Vec<AnomalySignal>,
    metric: &'static str,
    baseline: Option<f64>,
    current: Option<f64>,
    min_increase: Option<f64>,
) {
    let (Some(baseline), Some(current)) = (baseline, current) else {
        return;
    };
    if baseline <= 0.0 || min_increase.is_some_and(|min| current - baseline < min) {
        return;
    }
    let ratio = current / baseline;
    let severity = ratio_severity(ratio);
    if severity.is_anomalous() {
        signals.push(AnomalySignal {
            metric: metric.to_string(),
            observed: current,
            baseline: Some(baseline),
            ratio: Some(ratio),
            p_value: None,
            severity,
        });
    }
}

/// Poisson test on the merge **count** of the current window against the baseline
/// pace scaled to the same duration: λ = baseline merges/week × window weeks.
fn push_count_anomaly(
    signals: &mut Vec<AnomalySignal>,
    metric: &'static str,
    stats: &AggregatedStats,
    baseline: &AggregatedStats,
) {
    let (Some(baseline_per_week), Some(current_per_week)) =
        (baseline.throughput_per_week, stats.throughput_per_week)
    else {
        return;
    };
    if baseline_per_week <= 0.0 || current_per_week <= 0.0 {
        return;
    }
    // throughput_per_week = merged_count / window_weeks, hence the window duration.
    let window_weeks = stats.merged_count as f64 / current_per_week;
    let expected = baseline_per_week * window_weeks;
    let observed = stats.merged_count as u32;
    if f64::from(observed) <= expected {
        return;
    }
    let p_value = exceedance_probability(expected, observed);
    let severity = AnomalySeverity::from_p_value(p_value);
    if severity.is_anomalous() {
        signals.push(AnomalySignal {
            metric: metric.to_string(),
            observed: f64::from(observed),
            baseline: Some(expected),
            ratio: None,
            p_value: Some(p_value),
            severity,
        });
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    #[test]
    fn rates_and_durations_use_ratios_not_poisson() {
        let signal = |baseline, current, min| {
            let mut signals = Vec::new();
            push_ratio_anomaly(&mut signals, "m", Some(baseline), Some(current), min);
            signals.pop().map(|s| s.severity)
        };
        // Share 5 % → 6 %: used to be ELEVATED under Poisson; now below the +5 pt floor.
        assert_eq!(signal(0.05, 0.06, Some(MIN_SHARE_INCREASE)), None);
        assert_eq!(
            signal(0.10, 0.35, Some(MIN_SHARE_INCREASE)),
            Some(AnomalySeverity::Critical)
        );
        // Durations: same answer whatever the unit.
        assert_eq!(signal(10.0, 16.0, None), Some(AnomalySeverity::Elevated)); // hours
        assert_eq!(
            signal(10.0 / 24.0, 16.0 / 24.0, None),
            Some(AnomalySeverity::Elevated)
        ); // days
        assert_eq!(signal(10.0, 21.0, None), Some(AnomalySeverity::Warning));
        assert_eq!(signal(10.0, 14.0, None), None);
    }

    #[test]
    fn throughput_is_a_poisson_count_over_the_window() {
        let filter = crate::aggregator::QueryFilter::default();
        let empty = crate::aggregator::compute_stats(&[], &[], &filter);
        let (mut current, mut baseline) = (empty.clone(), empty);
        // 20 merges in 4 weeks (5/week) against a usual pace of 2/week: λ = 8.
        current.merged_count = 20;
        current.throughput_per_week = Some(5.0);
        baseline.throughput_per_week = Some(2.0);
        let mut signals = Vec::new();
        push_count_anomaly(&mut signals, "throughput_spike", &current, &baseline);
        let signal = signals.pop().expect("20 merges vs λ = 8 is anomalous");
        assert_eq!(signal.baseline, Some(8.0));
        assert!(signal.p_value.unwrap() < 0.01);
        assert_eq!(signal.severity, AnomalySeverity::Critical);
        // At the usual pace: nothing.
        current.merged_count = 8;
        current.throughput_per_week = Some(2.0);
        push_count_anomaly(&mut signals, "throughput_spike", &current, &baseline);
        assert!(signals.is_empty());
    }

    use super::*;

    #[test]
    fn pmf_known_values() {
        // P(X=0 | λ=1) = e^{-1} ≈ 0.3679
        assert!((pmf(1.0, 0) - 0.367_879_441).abs() < 1e-6);
        // P(X=1 | λ=1) = e^{-1} ≈ 0.3679
        assert!((pmf(1.0, 1) - 0.367_879_441).abs() < 1e-6);
        // P(X=2 | λ=1) = e^{-1}/2 ≈ 0.1839
        assert!((pmf(1.0, 2) - 0.183_939_720).abs() < 1e-6);
    }

    #[test]
    fn cdf_sums_to_one_asymptotically() {
        // For λ=3, P(X ≤ 20) should be effectively 1.
        assert!((cdf(3.0, 20) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn exceedance_complement_of_cdf() {
        let lambda = 4.0;
        let k = 6u32;
        let p_exceed = exceedance_probability(lambda, k);
        let p_cdf = cdf(lambda, k - 1);
        assert!((p_exceed + p_cdf - 1.0).abs() < 1e-9);
    }

    #[test]
    fn exceedance_far_tail_does_not_cancel_to_zero() {
        // P(X ≥ 30 | λ=1) ≈ e^{-1}/30! ≈ 1.4e-33: `1 − cdf` returned exactly 0.
        let p = exceedance_probability(1.0, 30);
        let first_term = pmf(1.0, 30);
        assert!(p > 0.0);
        assert!(p >= first_term && p < first_term * 1.1);
        // Both branches agree around the mean.
        let direct = exceedance_probability(4.0, 5);
        assert!((direct + cdf(4.0, 4) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn anomaly_severity_classification() {
        assert_eq!(AnomalySeverity::from_p_value(0.20), AnomalySeverity::Normal);
        assert_eq!(
            AnomalySeverity::from_p_value(0.07),
            AnomalySeverity::Elevated
        );
        assert_eq!(
            AnomalySeverity::from_p_value(0.03),
            AnomalySeverity::Warning
        );
        assert_eq!(
            AnomalySeverity::from_p_value(0.005),
            AnomalySeverity::Critical
        );
    }

    #[test]
    fn throughput_forecast_expected_merges() {
        let forecast = ThroughputForecast::new(3.0, 2, 6);
        assert!((forecast.expected_merges - 6.0).abs() < 1e-9);
        // P(X ≥ 6 | λ=6) ≈ 0.554, should be > 0 and < 1
        assert!(forecast.probability > 0.0 && forecast.probability < 1.0);
    }

    #[test]
    fn queue_insight_stable_queue() {
        use std::collections::HashMap;
        let stats = AggregatedStats {
            total_mrs: 10,
            merged_count: 8,
            closed_count: 1,
            abandon_rate: Some(1.0 / 9.0),
            merged_today: 0,
            merged_this_week: 0,
            merged_this_month: 0,
            merged_last_7_days: 0,
            merged_last_30_days: 0,
            throughput_per_week: Some(4.0),      // λ = 4 MRs/week
            cycle_time_median_hours: Some(24.0), // μ = 168/24 = 7 MRs/week
            cycle_time_p75_hours: None,
            cycle_time_p90_hours: None,
            cycle_time_by_author: HashMap::new(),
            cycle_time_by_reviewer: HashMap::new(),
            cycle_time_by_milestone: HashMap::new(),
            avg_diff_size: 100.0,
            avg_comments: 2.0,
            avg_comment_density: Some(2.0),
            avg_pipeline_failure_rate: None,
            pipeline_data_coverage: 0.0,
            cycle_time_sample_size: 8,
            pipeline_sample_size: 0,
            reviewer_coverage: 0.0,
            milestone_coverage: 0.0,
            size_buckets: vec![],
            open_mr_ages_days: vec![],
            stale_open_mrs_7d: 0,
            stale_open_mrs_14d: 0,
            stale_open_mrs_30d: 0,
            throughput_by_milestone: HashMap::new(),
        };
        let insight = QueueInsight::from_stats(&stats);
        assert_eq!(insight.status, QueueStatus::Stable);
        // ρ = 4/7 ≈ 0.571
        assert!((insight.traffic_intensity.unwrap() - 4.0 / 7.0).abs() < 1e-6);
        // L = ρ/(1-ρ) ≈ 1.333
        let expected_l = (4.0 / 7.0) / (1.0 - 4.0 / 7.0);
        assert!((insight.expected_mrs_in_system.unwrap() - expected_l).abs() < 1e-6);
    }

    #[test]
    fn queue_insight_over_capacity_is_explicit() {
        use std::collections::HashMap;
        // λ > μ → unstable queue pressure.
        let stats = AggregatedStats {
            total_mrs: 10,
            merged_count: 10,
            closed_count: 0,
            abandon_rate: Some(0.0),
            merged_today: 0,
            merged_this_week: 0,
            merged_this_month: 0,
            merged_last_7_days: 0,
            merged_last_30_days: 0,
            throughput_per_week: Some(10.0),      // λ = 10 MRs/week
            cycle_time_median_hours: Some(168.0), // μ = 1 MR/week → over capacity
            cycle_time_p75_hours: None,
            cycle_time_p90_hours: None,
            cycle_time_by_author: HashMap::new(),
            cycle_time_by_reviewer: HashMap::new(),
            cycle_time_by_milestone: HashMap::new(),
            avg_diff_size: 0.0,
            avg_comments: 0.0,
            avg_comment_density: None,
            avg_pipeline_failure_rate: None,
            pipeline_data_coverage: 0.0,
            cycle_time_sample_size: 8,
            pipeline_sample_size: 0,
            reviewer_coverage: 0.0,
            milestone_coverage: 0.0,
            size_buckets: vec![],
            open_mr_ages_days: vec![],
            stale_open_mrs_7d: 0,
            stale_open_mrs_14d: 0,
            stale_open_mrs_30d: 0,
            throughput_by_milestone: HashMap::new(),
        };
        let insight = QueueInsight::from_stats(&stats);
        assert_eq!(insight.status, QueueStatus::OverCapacity);
        assert!(insight.traffic_intensity.unwrap() >= 1.0);
        assert!(insight.expected_wait_hours.is_none());
    }
}
