use serde::{Deserialize, Serialize};

use crate::metrics::PerMrMetrics;

/// A pair of metrics to correlate using Spearman's rank correlation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricPair {
    /// Do larger MRs take longer to merge?
    DiffSizeVsCycleTime,
    /// Do larger MRs generate more comments?
    DiffSizeVsComments,
    /// Do more commented MRs take longer to merge (review bottleneck signal)?
    CommentsVsCycleTime,
    /// Do MRs with more pipeline failures have longer cycle times?
    PipelineFailuresVsCycleTime,
    /// Do MRs with more commits take longer to merge?
    CommitsCountVsCycleTime,
    /// Do more difficult MRs (diff_difficulty) take longer to merge?
    DiffDifficultyVsCycleTime,
}

/// Qualitative strength label derived from the absolute Spearman ρ value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationStrength {
    /// |ρ| < 0.10 — no meaningful relationship.
    Negligible,
    /// 0.10 ≤ |ρ| < 0.30 — small, possibly noise.
    Weak,
    /// 0.30 ≤ |ρ| < 0.50 — moderate, worth investigating.
    Moderate,
    /// 0.50 ≤ |ρ| < 0.70 — strong relationship.
    Strong,
    /// |ρ| ≥ 0.70 — very strong, highly reliable.
    VeryStrong,
}

impl CorrelationStrength {
    fn from_rho(rho: f64) -> Self {
        let abs = rho.abs();
        if abs < 0.10 {
            Self::Negligible
        } else if abs < 0.30 {
            Self::Weak
        } else if abs < 0.50 {
            Self::Moderate
        } else if abs < 0.70 {
            Self::Strong
        } else {
            Self::VeryStrong
        }
    }
}

/// Result of a Spearman rank correlation for a specific [`MetricPair`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationResult {
    pub pair: MetricPair,

    /// Spearman's rank correlation coefficient ρ in [-1.0, 1.0].
    /// Positive: both metrics increase together. Negative: inverse relationship.
    pub rho: f64,

    /// Two-tailed p-value for the null hypothesis H₀: ρ = 0.
    /// Values < 0.05 are conventionally considered statistically significant.
    /// Approximated via the t-distribution (reliable for n > 10).
    pub p_value: f64,

    /// Number of data points used (MRs with both metrics present).
    pub sample_size: usize,

    /// Human-readable interpretation of |ρ|.
    pub interpretation: CorrelationStrength,
}

/// Computes all defined [`MetricPair`] correlations over a slice of metrics.
///
/// MRs missing either value for a given pair are silently excluded from that
/// pair's sample — this avoids artificially deflating ρ with imputed zeros.
pub fn compute_all_correlations(metrics: &[PerMrMetrics]) -> Vec<CorrelationResult> {
    let pairs = [
        MetricPair::DiffSizeVsCycleTime,
        MetricPair::DiffSizeVsComments,
        MetricPair::CommentsVsCycleTime,
        MetricPair::PipelineFailuresVsCycleTime,
        MetricPair::CommitsCountVsCycleTime,
        MetricPair::DiffDifficultyVsCycleTime,
    ];

    pairs
        .into_iter()
        .filter_map(|pair| compute_correlation(metrics, pair))
        .collect()
}

/// Computes the Spearman ρ for a single [`MetricPair`].
///
/// Returns `None` when fewer than 3 data points are available (ρ would be
/// meaningless and the p-value approximation breaks down).
fn compute_correlation(metrics: &[PerMrMetrics], pair: MetricPair) -> Option<CorrelationResult> {
    // Extract the (x, y) pairs for this metric combination.
    let points: Vec<(f64, f64)> = metrics
        .iter()
        .filter_map(|m| extract_pair(m, &pair))
        .collect();

    let n = points.len();
    if n < 3 {
        return None;
    }

    let rho = spearman_rho(&points);
    let p_value = spearman_p_value(rho, n);

    Some(CorrelationResult {
        interpretation: CorrelationStrength::from_rho(rho),
        pair,
        rho,
        p_value,
        sample_size: n,
    })
}

/// Extracts the (x, y) value pair for a given [`MetricPair`] from a single MR.
fn extract_pair(m: &PerMrMetrics, pair: &MetricPair) -> Option<(f64, f64)> {
    match pair {
        MetricPair::DiffSizeVsCycleTime => Some((m.diff_size as f64, m.cycle_time_hours?)),
        MetricPair::DiffSizeVsComments => Some((m.diff_size as f64, m.user_notes_count as f64)),
        MetricPair::CommentsVsCycleTime => Some((m.user_notes_count as f64, m.cycle_time_hours?)),
        MetricPair::PipelineFailuresVsCycleTime => {
            Some((m.pipeline_failure_rate?, m.cycle_time_hours?))
        }
        MetricPair::CommitsCountVsCycleTime => Some((m.commits_count as f64, m.cycle_time_hours?)),
        MetricPair::DiffDifficultyVsCycleTime => Some((m.diff_difficulty?, m.cycle_time_hours?)),
    }
}

/// Computes Spearman's ρ from a slice of (x, y) pairs.
///
/// Algorithm:
/// 1. Replace each value by its rank (average ranks for ties).
/// 2. Compute the Pearson correlation of the rank vectors.
///
/// This is mathematically equivalent to `1 - 6Σd²/n(n²-1)` but handles ties
/// correctly, which the simplified formula does not.
fn spearman_rho(points: &[(f64, f64)]) -> f64 {
    let n = points.len();
    let xs: Vec<f64> = points.iter().map(|(x, _)| *x).collect();
    let ys: Vec<f64> = points.iter().map(|(_, y)| *y).collect();

    let rx = rank_vector(&xs);
    let ry = rank_vector(&ys);

    pearson_r(&rx, &ry, n)
}

/// Assigns ranks to a vector of values, using average ranks for ties.
fn rank_vector(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    // Build (value, original_index) pairs sorted by value.
    let mut indexed: Vec<(f64, usize)> = values
        .iter()
        .copied()
        .enumerate()
        .map(|(i, v)| (v, i))
        .collect();
    indexed.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut ranks = vec![0.0f64; n];
    let mut i = 0;
    while i < n {
        // Find the extent of the tie group.
        let mut j = i;
        // Exact equality: ties are identical source values (counts, hours), and an
        // absolute epsilon would be meaningless across the metrics' scales.
        while j < n && indexed[j].0 == indexed[i].0 {
            j += 1;
        }
        // Average rank for the tie group (1-based ranks).
        let avg_rank = (i + 1 + j) as f64 / 2.0;
        for k in i..j {
            ranks[indexed[k].1] = avg_rank;
        }
        i = j;
    }
    ranks
}

/// Pearson correlation coefficient of two equal-length vectors.
fn pearson_r(x: &[f64], y: &[f64], n: usize) -> f64 {
    let mean_x = x.iter().sum::<f64>() / n as f64;
    let mean_y = y.iter().sum::<f64>() / n as f64;

    let cov: f64 = x
        .iter()
        .zip(y)
        .map(|(xi, yi)| (xi - mean_x) * (yi - mean_y))
        .sum();
    let var_x: f64 = x.iter().map(|xi| (xi - mean_x).powi(2)).sum();
    let var_y: f64 = y.iter().map(|yi| (yi - mean_y).powi(2)).sum();

    let denom = (var_x * var_y).sqrt();
    if denom < f64::EPSILON {
        0.0
    } else {
        (cov / denom).clamp(-1.0, 1.0)
    }
}

/// Approximates the two-tailed p-value for a Spearman ρ using the t-distribution.
///
/// The test statistic `t = ρ√(n-2) / √(1-ρ²)` follows a t-distribution with
/// `n-2` degrees of freedom under H₀: ρ = 0. The approximation is accurate
/// for n > 10 and degrades gracefully for smaller samples.
///
/// The CDF is approximated via the regularised incomplete beta function
/// using a continued-fraction expansion (Abramowitz & Stegun 26.5.8).
fn spearman_p_value(rho: f64, n: usize) -> f64 {
    if n <= 2 {
        return 1.0;
    }
    let df = (n - 2) as f64;
    let t = rho * (df / (1.0 - rho * rho).max(f64::EPSILON)).sqrt();
    let x = df / (df + t * t);
    // Two-tailed p-value = I_x(df/2, 1/2).
    regularised_incomplete_beta(x, df / 2.0, 0.5)
}

/// Regularised incomplete beta function I_x(a, b) via continued-fraction expansion.
///
/// Used exclusively for the p-value approximation. Accurate to ~1e-7 for the
/// parameter ranges encountered in Spearman p-value computation.
fn regularised_incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Use the symmetry relation when x > (a+1)/(a+b+2) for better convergence.
    if x > (a + 1.0) / (a + b + 2.0) {
        return 1.0 - regularised_incomplete_beta(1.0 - x, b, a);
    }

    let log_beta = ln_beta(a, b);
    let front = (x.powf(a) * (1.0 - x).powf(b)) / a;
    let cf = beta_continued_fraction(x, a, b);
    (front * cf * (-log_beta).exp()).clamp(0.0, 1.0)
}

/// Lentz's continued-fraction algorithm for the incomplete beta function.
fn beta_continued_fraction(x: f64, a: f64, b: f64) -> f64 {
    const MAX_ITER: usize = 200;
    const EPS: f64 = 1e-10;

    let mut c = 1.0f64;
    let mut d = 1.0 - (a + b) / (a + 1.0) * x;
    if d.abs() < f64::MIN_POSITIVE {
        d = f64::MIN_POSITIVE;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=MAX_ITER {
        let m = m as f64;
        // Even step.
        let num_even = m * (b - m) * x / ((a + 2.0 * m - 1.0) * (a + 2.0 * m));
        d = 1.0 + num_even * d;
        if d.abs() < f64::MIN_POSITIVE {
            d = f64::MIN_POSITIVE;
        }
        c = 1.0 + num_even / c;
        if c.abs() < f64::MIN_POSITIVE {
            c = f64::MIN_POSITIVE;
        }
        d = 1.0 / d;
        h *= d * c;

        // Odd step.
        let num_odd = -(a + m) * (a + b + m) * x / ((a + 2.0 * m) * (a + 2.0 * m + 1.0));
        d = 1.0 + num_odd * d;
        if d.abs() < f64::MIN_POSITIVE {
            d = f64::MIN_POSITIVE;
        }
        c = 1.0 + num_odd / c;
        if c.abs() < f64::MIN_POSITIVE {
            c = f64::MIN_POSITIVE;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;

        if (delta - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

/// Natural log of the beta function: ln B(a, b) = ln Γ(a) + ln Γ(b) - ln Γ(a+b).
fn ln_beta(a: f64, b: f64) -> f64 {
    use crate::poisson::ln_gamma;
    ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_positive_correlation() {
        let points: Vec<(f64, f64)> = (1..=10).map(|i| (i as f64, i as f64)).collect();
        let rho = spearman_rho(&points);
        assert!((rho - 1.0).abs() < 1e-9, "expected ρ ≈ 1.0, got {rho}");
    }

    #[test]
    fn p_value_matches_reference() {
        // Reference values from scipy.stats (t-distribution, two-tailed).
        let cases = [(0.5, 20, 0.0248), (0.3, 30, 0.1072), (0.8, 10, 0.0055)];
        for (rho, n, expected) in cases {
            let p = spearman_p_value(rho, n);
            assert!(
                (p - expected).abs() < 5e-4,
                "rho={rho} n={n}: expected p≈{expected}, got {p}"
            );
        }
    }

    #[test]
    fn perfect_negative_correlation() {
        let points: Vec<(f64, f64)> = (1..=10).map(|i| (i as f64, (11 - i) as f64)).collect();
        let rho = spearman_rho(&points);
        assert!((rho + 1.0).abs() < 1e-9, "expected ρ ≈ -1.0, got {rho}");
    }

    #[test]
    fn rank_ties_handled() {
        // All x values identical → ρ should be 0 (no rank variation on x).
        let points: Vec<(f64, f64)> = vec![(1.0, 1.0), (1.0, 2.0), (1.0, 3.0)];
        let rho = spearman_rho(&points);
        assert_eq!(rho, 0.0);
    }

    #[test]
    fn strength_classification() {
        assert_eq!(
            CorrelationStrength::from_rho(0.05),
            CorrelationStrength::Negligible
        );
        assert_eq!(
            CorrelationStrength::from_rho(0.20),
            CorrelationStrength::Weak
        );
        assert_eq!(
            CorrelationStrength::from_rho(0.40),
            CorrelationStrength::Moderate
        );
        assert_eq!(
            CorrelationStrength::from_rho(0.60),
            CorrelationStrength::Strong
        );
        assert_eq!(
            CorrelationStrength::from_rho(0.80),
            CorrelationStrength::VeryStrong
        );
        // Negative values mirror positive.
        assert_eq!(
            CorrelationStrength::from_rho(-0.65),
            CorrelationStrength::Strong
        );
    }
}
