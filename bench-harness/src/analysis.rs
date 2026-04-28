//! Statistical analysis of benchmark results.
//!
//! Quintile analysis, per-cooldown breakdown, consistency metrics
//! (lag-1 autocorrelation), Benjamini-Hochberg FDR adjustment for
//! multi-comparison, percent delta, bootstrap confidence intervals on
//! the median and on paired differences, and a sign test producing a
//! [`Comparison`] result.

use std::collections::BTreeMap;

use crate::sample::{BenchResult, Sample};
use crate::spec::RoutineSpec;

/// Quintile statistics for a set of values.
///
/// Quintile boundaries use integer division (`n / 5`), which creates
/// a slight asymmetry for lengths not divisible by 5: `best_20pct`
/// covers `floor(n/5)` values and `worst_20pct` covers
/// `n - 4*floor(n/5)` values, so worst can have up to 4 more elements
/// than best when `n mod 5 != 0`. For uniformly distributed data the
/// effect on means is negligible. For highly skewed distributions the
/// `worst_20pct` mean may be pulled down slightly.
pub struct Stats {
    pub count: usize,
    pub mean: f64,
    pub std_dev: f64,
    pub median: f64,
    pub best_20pct: f64,
    pub mid_60pct: f64,
    pub worst_20pct: f64,
    pub min: f64,
    pub max: f64,
}

impl Stats {
    pub fn from_values(vals: &mut Vec<f64>) -> Self {
        if vals.is_empty() {
            return Stats {
                count: 0,
                mean: 0.0,
                std_dev: 0.0,
                median: 0.0,
                best_20pct: 0.0,
                mid_60pct: 0.0,
                worst_20pct: 0.0,
                min: 0.0,
                max: 0.0,
            };
        }
        vals.sort_by(|a, b| a.total_cmp(b));
        let n = vals.len();
        let q = n / 5;
        let mean = vals.iter().sum::<f64>() / n as f64;
        let median = if n % 2 == 0 {
            (vals[n / 2 - 1] + vals[n / 2]) / 2.0
        } else {
            vals[n / 2]
        };
        let best = if q > 0 {
            vals[..q].iter().sum::<f64>() / q as f64
        } else {
            vals[0]
        };
        let worst_start = 4 * q;
        let worst_count = n - worst_start;
        let worst = vals[worst_start..].iter().sum::<f64>() / worst_count as f64;
        let mid = vals[q..worst_start].iter().sum::<f64>() / (worst_start - q).max(1) as f64;

        let variance = vals.iter().map(|&x| (x - mean) * (x - mean)).sum::<f64>()
            / (n as f64).max(1.0);
        let std_dev = variance.sqrt();

        Stats {
            count: n,
            mean,
            std_dev,
            median,
            best_20pct: best,
            mid_60pct: mid,
            worst_20pct: worst,
            min: vals[0],
            max: vals[n - 1],
        }
    }
}

/// Per-variant analysis across modes and cooldowns.
pub struct VariantAnalysis {
    pub name: String,
    pub algo_all: Stats,
    pub e2e_all: Stats,
    pub bridge_all: Stats,
    pub algo_per_cd: BTreeMap<u64, Stats>,
    pub e2e_per_cd: BTreeMap<u64, Stats>,
    pub nonstop_per_pass: Vec<f64>,
    /// Lag-1 autocorrelation of the nonstop per-pass time series.
    /// Values near +1 indicate persistent warm-up or drift; near -1
    /// indicate alternating high/low (thermal throttling bounce). Near
    /// 0 is ideal.
    pub autocorrelation: f64,
    /// All samples keyed by `(run, pass, cooldown_ms, algo_ns)` for
    /// paired statistical comparisons. The key tuple allows
    /// deterministic re-pairing of variant vs baseline even across
    /// independent collection runs.
    pub keyed_algo: Vec<(usize, usize, u64, f64)>,
    /// Quality scores from samples that have a non-`None` score.
    pub scores: Vec<f64>,
    /// Per-input-tag algo Stats (e.g. per sparsity pattern). Empty if
    /// no samples have `input_tag`.
    pub algo_per_tag: BTreeMap<u8, Stats>,
}

/// Full dataset with per-variant analysis.
pub struct DataSet {
    pub variants: Vec<VariantAnalysis>,
    pub baseline_idx: usize,
    /// Optional warning to surface in the report when cross-epoch
    /// data is merged but drift correction was skipped (confidence < 3).
    pub drift_note: String,
    /// Methodology metadata for the report header.
    pub meta: DataSetMeta,
    /// Tag index → display name (e.g. 0 → "random", 1 → "banded").
    /// Populated from the Routine's `input_tag()` by the caller.
    pub tag_names: BTreeMap<u8, String>,
}

/// Methodology and environment metadata carried through to the report.
pub struct DataSetMeta {
    pub passes: usize,
    pub runs_per_pass: usize,
    pub batch_size: usize,
    pub harness_runs: usize,
    pub cooldowns_ms: Vec<u64>,
    pub master_seed: u64,
    pub counter_freq: u64,
    pub drift_correction: String,
    /// Ops per algorithm call. When > 0, the report shows a
    /// throughput column.
    pub ops_per_call: u64,
}

impl DataSet {
    /// Build a dataset from samples, filtering by `mode`
    /// (`"warm"` / `"cold"`).
    pub fn from_samples(samples: &[Sample], mode: &str) -> Self {
        let filtered: Vec<&Sample> = samples.iter().filter(|s| s.mode == mode).collect();

        let mut by_variant: BTreeMap<String, Vec<&Sample>> = BTreeMap::new();
        for s in &filtered {
            by_variant.entry(s.variant.clone()).or_default().push(s);
        }

        let mut variants = Vec::new();
        for (name, vsamples) in &by_variant {
            let mut e2e_vals: Vec<f64> = vsamples.iter().map(|s| s.e2e_ns).collect();
            let mut algo_vals: Vec<f64> = vsamples.iter().map(|s| s.algo_ns).collect();
            let mut bridge_vals: Vec<f64> = vsamples.iter().map(|s| s.bridge_ns).collect();

            let mut e2e_by_cd: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
            let mut algo_by_cd: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
            let mut nonstop_passes = Vec::new();
            let mut keyed_algo: Vec<(usize, usize, u64, f64)> = Vec::new();
            let mut scores: Vec<f64> = Vec::new();
            let mut algo_by_tag: BTreeMap<u8, Vec<f64>> = BTreeMap::new();

            for s in vsamples.iter() {
                e2e_by_cd.entry(s.cooldown_ms).or_default().push(s.e2e_ns);
                algo_by_cd.entry(s.cooldown_ms).or_default().push(s.algo_ns);
                if s.cooldown_ms == 0 {
                    nonstop_passes.push(s.algo_ns);
                }
                keyed_algo.push((s.run, s.pass, s.cooldown_ms, s.algo_ns));
                if let Some(sc) = s.score {
                    scores.push(sc);
                }
                if let Some(tag) = s.input_tag {
                    algo_by_tag.entry(tag).or_default().push(s.algo_ns);
                }
            }

            // Sort keyed_algo by (run, pass, cooldown_ms) so pairings
            // are deterministic regardless of collection order.
            keyed_algo.sort_by_key(|&(run, pass, cd, _)| (run, pass, cd));

            let e2e_per_cd = e2e_by_cd
                .into_iter()
                .map(|(k, mut v)| (k, Stats::from_values(&mut v)))
                .collect();
            let algo_per_cd = algo_by_cd
                .into_iter()
                .map(|(k, mut v)| (k, Stats::from_values(&mut v)))
                .collect();

            let autocorrelation = lag1_autocorrelation(&nonstop_passes);

            let algo_per_tag = algo_by_tag
                .into_iter()
                .map(|(k, mut v)| (k, Stats::from_values(&mut v)))
                .collect();

            variants.push(VariantAnalysis {
                name: name.clone(),
                algo_all: Stats::from_values(&mut algo_vals),
                e2e_all: Stats::from_values(&mut e2e_vals),
                bridge_all: Stats::from_values(&mut bridge_vals),
                algo_per_cd,
                e2e_per_cd,
                nonstop_per_pass: nonstop_passes,
                autocorrelation,
                keyed_algo,
                scores,
                algo_per_tag,
            });
        }

        let mut tag_names: BTreeMap<u8, String> = BTreeMap::new();
        for s in &filtered {
            if let Some(tag) = s.input_tag {
                tag_names
                    .entry(tag)
                    .or_insert_with(|| format!("tag-{}", tag));
            }
        }

        DataSet {
            variants,
            baseline_idx: 0,
            drift_note: String::new(),
            meta: DataSetMeta::default(),
            tag_names,
        }
    }

    pub fn with_tag_names(mut self, names: &[(&str, u8)]) -> Self {
        for &(name, idx) in names {
            self.tag_names.insert(idx, name.into());
        }
        self
    }

    pub fn with_baseline(mut self, name: &str) -> Self {
        if let Some(idx) = self.variants.iter().position(|v| v.name == name) {
            self.baseline_idx = idx;
        }
        self
    }

    pub fn baseline(&self) -> &VariantAnalysis {
        &self.variants[self.baseline_idx]
    }
}

impl Default for DataSetMeta {
    fn default() -> Self {
        DataSetMeta {
            passes: 0,
            runs_per_pass: 0,
            batch_size: 0,
            harness_runs: 0,
            cooldowns_ms: Vec::new(),
            master_seed: 0,
            counter_freq: 0,
            drift_correction: "none".into(),
            ops_per_call: 0,
        }
    }
}

/// Lag-1 autocorrelation coefficient of a time series.
///
/// `r = Σ (x_i - mean)(x_{i+1} - mean) / Σ (x_i - mean)²`
/// for `i in 0..n-1` (denominator uses all `n` elements).
///
/// Returns `0.0` for series shorter than 2 elements. Values near `+1`
/// indicate positive serial correlation (drift / warm-up); values near
/// `-1` indicate alternating pattern (e.g. thermal bounce).
pub fn lag1_autocorrelation(vals: &[f64]) -> f64 {
    let n = vals.len();
    if n < 2 {
        return 0.0;
    }
    let mean = vals.iter().sum::<f64>() / n as f64;
    let denom: f64 = vals.iter().map(|&x| (x - mean) * (x - mean)).sum();
    if denom == 0.0 {
        return 0.0;
    }
    let numer: f64 = (0..n - 1)
        .map(|i| (vals[i] - mean) * (vals[i + 1] - mean))
        .sum();
    numer / denom
}

/// Benjamini-Hochberg FDR correction for multiple comparisons.
///
/// Takes a mutable slice of `(index, p_value)` pairs and adjusts the
/// p-values in-place using the BH procedure:
///
/// 1. Sort by p ascending (tracking original index).
/// 2. `adjusted_p[i] = min(p[i] * m / rank, 1.0)` where `rank = i+1`,
///    `m = total`.
/// 3. Enforce monotonicity from the end: each
///    `adjusted_p[i] = min(adjusted_p[i], adjusted_p[i+1])`.
///
/// After the call the slice is sorted by ascending p-value (not
/// restored to original order); callers use the stored index to map
/// back to variants.
pub fn bh_fdr_adjust(p_values: &mut Vec<(usize, f64)>) {
    let m = p_values.len();
    if m == 0 {
        return;
    }

    p_values.sort_by(|a, b| a.1.total_cmp(&b.1));

    for i in 0..m {
        let rank = (i + 1) as f64;
        p_values[i].1 = (p_values[i].1 * m as f64 / rank).min(1.0);
    }

    for i in (0..m - 1).rev() {
        if p_values[i].1 > p_values[i + 1].1 {
            p_values[i].1 = p_values[i + 1].1;
        }
    }
}

/// Compute percentage delta of `value` against `baseline`.
pub fn pct_delta(value: f64, baseline: f64) -> f64 {
    if baseline == 0.0 {
        return 0.0;
    }
    ((value - baseline) / baseline) * 100.0
}

// ── Bootstrap confidence intervals ──

const BOOTSTRAP_ITERATIONS: usize = 10_000;
/// 2.5th percentile of the bootstrap distribution.
const CI_LOWER: f64 = 0.025;
/// 97.5th percentile of the bootstrap distribution.
const CI_UPPER: f64 = 0.975;

/// Splitmix64 for the bootstrap RNG (deterministic, no external deps).
fn bootstrap_mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D049BB133111EB);
    x ^= x >> 31;
    x
}

/// 95% bootstrap confidence interval on the median. Returns
/// `(lower, median, upper)`.
pub fn bootstrap_ci_median(vals: &[f64], seed: u64) -> (f64, f64, f64) {
    if vals.len() < 3 {
        let m = if vals.is_empty() { 0.0 } else { vals[0] };
        return (m, m, m);
    }

    let n = vals.len();
    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let true_median = if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    };

    let mut boot_medians = Vec::with_capacity(BOOTSTRAP_ITERATIONS);
    let mut rng = seed;

    for _ in 0..BOOTSTRAP_ITERATIONS {
        let mut resample = Vec::with_capacity(n);
        for _ in 0..n {
            rng = bootstrap_mix(rng);
            let idx = (rng as usize) % n;
            resample.push(sorted[idx]);
        }
        resample.sort_by(|a, b| a.total_cmp(b));
        let boot_med = if n % 2 == 0 {
            (resample[n / 2 - 1] + resample[n / 2]) / 2.0
        } else {
            resample[n / 2]
        };
        boot_medians.push(boot_med);
    }

    boot_medians.sort_by(|a, b| a.total_cmp(b));
    let lo_idx = (BOOTSTRAP_ITERATIONS as f64 * CI_LOWER) as usize;
    let hi_idx = (BOOTSTRAP_ITERATIONS as f64 * CI_UPPER) as usize;

    (
        boot_medians[lo_idx],
        true_median,
        boot_medians[hi_idx.min(boot_medians.len() - 1)],
    )
}

/// Bootstrap CI on the median of pairwise differences.
///
/// `a` and `b` must have the same length (paired samples). Returns
/// `(lower, median_diff, upper)` in the same units as input. If the
/// CI includes 0, the difference is not statistically significant.
pub fn bootstrap_ci_diff(a: &[f64], b: &[f64], seed: u64) -> (f64, f64, f64) {
    let n = a.len().min(b.len());
    if n < 3 {
        return (0.0, 0.0, 0.0);
    }

    let diffs: Vec<f64> = (0..n).map(|i| a[i] - b[i]).collect();
    bootstrap_ci_median(&diffs, seed)
}

/// Pairwise comparison result.
pub struct Comparison {
    /// Median of `(variant - baseline)` in ns.
    pub median_diff_ns: f64,
    /// 95% CI lower bound on median difference.
    pub ci_lo_ns: f64,
    /// 95% CI upper bound on median difference.
    pub ci_hi_ns: f64,
    /// Percentage delta of medians.
    pub pct: f64,
    /// Whether the CI excludes zero (statistically significant).
    pub significant: bool,
    /// Sign test p-value (two-sided).
    pub sign_test_p: f64,
    /// Number of tied pairs (`a[i] == b[i]`) dropped by the sign test.
    /// High tie count (> 10% of pairs) weakens the test and should be
    /// flagged.
    pub ties: u32,
}

/// Sign test: two-sided p-value for the null hypothesis that
/// `median(a - b) = 0`. Uses the exact binomial tail.
///
/// Returns `(p_value, ties)` where `ties` is the count of pairs where
/// `a[i] == b[i]` exactly. Ties are excluded from the test; a high
/// tie count (> 10% of total pairs) weakens reliability and should be
/// reported.
pub fn sign_test(a: &[f64], b: &[f64]) -> (f64, u32) {
    let n = a.len().min(b.len());
    let mut pos = 0u32;
    let mut neg = 0u32;
    let mut ties = 0u32;
    for i in 0..n {
        let d = a[i] - b[i];
        if d > 0.0 {
            pos += 1;
        } else if d < 0.0 {
            neg += 1;
        } else {
            ties += 1;
        }
    }
    let total = pos + neg;
    if total == 0 {
        return (1.0, ties);
    }

    let k = pos.min(neg) as usize;
    let m = total as usize;

    // P(X <= k) where X ~ Binomial(m, 0.5), two-sided = 2 * tail.
    let mut tail = 0.0f64;
    let mut log_comb = 0.0f64;
    let log_half_m = (m as f64) * (-std::f64::consts::LN_2);

    for i in 0..=k {
        tail += (log_comb + log_half_m).exp();
        if i < m {
            log_comb += ((m - i) as f64).ln() - ((i + 1) as f64).ln();
        }
    }

    ((2.0 * tail).min(1.0), ties)
}

/// Compare a variant against a baseline using paired samples. Samples
/// should be ordered consistently (same pass/seed order).
pub fn compare(variant: &[f64], baseline: &[f64], seed: u64) -> Comparison {
    let n = variant.len().min(baseline.len());
    if n < 3 {
        return Comparison {
            median_diff_ns: 0.0,
            ci_lo_ns: 0.0,
            ci_hi_ns: 0.0,
            pct: 0.0,
            significant: false,
            sign_test_p: 1.0,
            ties: 0,
        };
    }

    let (ci_lo, med_diff, ci_hi) = bootstrap_ci_diff(variant, baseline, seed);
    let (sign_p, ties) = sign_test(variant, baseline);

    let mut sorted_b = baseline.to_vec();
    sorted_b.sort_by(|a, b| a.total_cmp(b));
    let nb = sorted_b.len();
    let base_median = if nb % 2 == 0 {
        (sorted_b[nb / 2 - 1] + sorted_b[nb / 2]) / 2.0
    } else {
        sorted_b[nb / 2]
    };

    let pct = if base_median != 0.0 {
        (med_diff / base_median) * 100.0
    } else {
        0.0
    };
    // CI excludes zero
    let significant = ci_lo > 0.0 || ci_hi < 0.0;

    Comparison {
        median_diff_ns: med_diff,
        ci_lo_ns: ci_lo,
        ci_hi_ns: ci_hi,
        pct,
        significant,
        sign_test_p: sign_p,
        ties,
    }
}

// ── BenchResult ergonomics ──

impl BenchResult {
    /// Build a [`DataSet`] from `self.samples` filtered by `mode`
    /// (`"warm"` / `"cold"`). Equivalent to
    /// [`DataSet::from_samples(&result.samples, mode)`].
    pub fn dataset(&self, mode: &str) -> DataSet {
        DataSet::from_samples(&self.samples, mode)
    }

    /// Build a [`DataSet`] for `mode` and auto-fill
    /// `meta.ops_per_call` from the routine bridge by invoking
    /// `Routine::ops_per_call` on a synthetic `seed=0` input.
    ///
    /// The throughput / Gops/s tables in [`crate::generate_report`]
    /// only render when `meta.ops_per_call > 0`; routines that
    /// declare `ops_per_call` will get throughput tables for free.
    pub fn dataset_for_routine(&self, routine: &RoutineSpec, mode: &str) -> DataSet {
        let mut ds = DataSet::from_samples(&self.samples, mode);
        let probe_input = (routine.bridge.input_builder)(0);
        ds.meta.ops_per_call = (routine.bridge.ops_per_call)(&probe_input);
        ds
    }
}
