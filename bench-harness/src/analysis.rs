//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub count:       usize,
    pub mean:        f64,
    pub std_dev:     f64,
    pub median:      f64,
    pub best_20pct:  f64,
    pub mid_60pct:   f64,
    pub worst_20pct: f64,
    pub min:         f64,
    pub max:         f64,
}

impl Stats {
    pub fn from_values(vals: &mut Vec<f64>) -> Self {
        if vals.is_empty() {
            return Stats {
                count:       0,
                mean:        0.0,
                std_dev:     0.0,
                median:      0.0,
                best_20pct:  0.0,
                mid_60pct:   0.0,
                worst_20pct: 0.0,
                min:         0.0,
                max:         0.0,
            };
        }
        vals.sort_by(|a, b| a.total_cmp(b));
        let n = vals.len();
        let q = n / 5;
        let mean = vals.iter().sum::<f64>() / n as f64;
        let median = median(vals);
        let best = if q > 0 { vals[.. q].iter().sum::<f64>() / q as f64 } else { vals[0] };
        let worst_start = 4 * q;
        let worst_count = n - worst_start;
        let worst = vals[worst_start ..].iter().sum::<f64>() / worst_count as f64;
        let mid = vals[q .. worst_start].iter().sum::<f64>() / (worst_start - q).max(1) as f64;

        let variance =
            vals.iter().map(|&x| (x - mean) * (x - mean)).sum::<f64>() / (n as f64).max(1.0);
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
    pub name:              String,
    pub algo_all:          Stats,
    pub e2e_all:           Stats,
    pub bridge_all:        Stats,
    pub algo_per_cd:       BTreeMap<u64, Stats>,
    pub e2e_per_cd:        BTreeMap<u64, Stats>,
    /// One entry per `(run, pass)` at cooldown 0, keyed, in run-then-pass order.
    /// The value is the median of that pass's batch samples.
    ///
    /// Keyed because the report compares this series between variants and a bare
    /// `Vec<f64>` can only be compared by position, so a variant that lost a
    /// pass to a timeout shifted every comparison after it onto a different
    /// pass. One value per pass because the worker emits
    /// `runs_per_pass / batch_size` lines per pass, ten on the shipped defaults,
    /// and a per-batch series called per-pass makes the report's table thirty
    /// times longer than the run and turns a within-pass warm ramp into
    /// [`Self::autocorrelation`].
    pub nonstop_per_pass:  Vec<((usize, usize), f64)>,
    /// Lag-1 autocorrelation of the nonstop per-pass time series.
    /// Values near +1 indicate persistent warm-up or drift; near -1
    /// indicate alternating high/low (thermal throttling bounce). Near
    /// 0 is ideal.
    pub autocorrelation:   f64,
    /// All samples keyed by `(run, pass, cooldown_ms, algo_ns)` for
    /// paired statistical comparisons. The key tuple allows
    /// deterministic re-pairing of variant vs baseline even across
    /// independent collection runs.
    pub keyed_algo:        Vec<(usize, usize, u64, f64)>,
    /// Quality scores from samples that have a non-`None` score.
    pub scores:            Vec<f64>,
    /// Per-input-tag algo Stats (e.g. per sparsity pattern). Empty if
    /// no samples have `input_tag`.
    pub algo_per_tag:      BTreeMap<u8, Stats>,
    /// Mean hardware instructions retired per call across this variant's
    /// samples, and mean cycles. Zero when perf counters were off. `ipc` is the
    /// derived instructions/cycles ratio (0 when cycles is 0).
    pub mean_instructions: f64,
    pub mean_cycles:       f64,
    /// Mean one-time setup cost S (ns) and mean cold first-touch cost (ns) across
    /// this variant's samples. Populated only by matrix-scaffold benches; zero for
    /// plain timed! variants. `mean_setup_ns` is the term the review panel found
    /// hidden in untimed prep; with `algo_all.mean` it gives the per-variant
    /// (S, I) pair the tier-breakeven `k*` is computed from.
    pub mean_setup_ns:     f64,
    pub mean_first_ns:     f64,
}

/// Full dataset with per-variant analysis.
pub struct DataSet {
    pub variants:     Vec<VariantAnalysis>,
    pub baseline_idx: usize,
    /// Optional warning to surface in the report when cross-epoch
    /// data is merged but drift correction was skipped (confidence < 3).
    pub drift_note:   String,
    /// Methodology metadata for the report header.
    pub meta:         DataSetMeta,
    /// Tag index → display name (e.g. 0 → "random", 1 → "banded").
    /// Populated from the Routine's `input_tag()` by the caller.
    pub tag_names:    BTreeMap<u8, String>,
}

/// Methodology and environment metadata carried through to the report.
pub struct DataSetMeta {
    pub passes:           usize,
    pub runs_per_pass:    usize,
    pub batch_size:       usize,
    pub harness_runs:     usize,
    pub cooldowns_ms:     Vec<u64>,
    pub master_seed:      u64,
    pub counter_freq:     u64,
    pub drift_correction: String,
    /// Ops per algorithm call. When > 0, the report shows a
    /// throughput column.
    pub ops_per_call:     u64,
    /// How the baseline-relative column is framed: `"subtract"` (the Δ ns in
    /// the statistical table, always shown), `"percent"` (the Δ mean %, always
    /// shown), or `"ratio"` (a `× base` column, `variant / baseline`, added only
    /// in this mode). The ratio framing is the reference-floor lens: with the
    /// baseline set to a native-ceiling or null-dispatch floor variant, `× base`
    /// reads directly as "how many times the floor" each variant costs.
    pub normalise_mode:   String,
    /// Null-floor differencing target: a variant name whose per-call algo time is
    /// subtracted from every variant (including the baseline) before ratios, so the
    /// ratio isolates pure dispatch cost above the null-dispatch floor. Empty = no
    /// floor differencing. Distinct from `normalise_mode` (a baseline-relative
    /// framing): the floor is subtracted from all variants first, THEN the ratio is
    /// taken against the floor-differenced baseline.
    pub floor_variant:    String,
}

impl DataSet {
    /// Build a dataset from samples, filtering by `mode`
    /// (`"warm"` / `"cold"`).
    #[must_use]
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
            let mut keyed_algo: Vec<(usize, usize, u64, f64)> = Vec::new();
            let mut scores: Vec<f64> = Vec::new();
            let mut algo_by_tag: BTreeMap<u8, Vec<f64>> = BTreeMap::new();

            for s in vsamples.iter() {
                e2e_by_cd.entry(s.cooldown_ms).or_default().push(s.e2e_ns);
                algo_by_cd.entry(s.cooldown_ms).or_default().push(s.algo_ns);
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

            // One value per (run, pass) at cooldown 0: the median of that
            // pass's batches. `keyed_algo` is the per-batch series; this is the
            // per-pass one and they are different lengths.
            let mut nonstop_by_pass: BTreeMap<(usize, usize), Vec<f64>> = BTreeMap::new();
            for s in vsamples.iter() {
                if s.cooldown_ms == 0 {
                    nonstop_by_pass
                        .entry((s.run, s.pass))
                        .or_default()
                        .push(s.algo_ns);
                }
            }
            let nonstop_passes: Vec<((usize, usize), f64)> = nonstop_by_pass
                .into_iter()
                .map(|(k, v)| (k, median(&v)))
                .collect();
            let nonstop_series: Vec<f64> = nonstop_passes.iter().map(|(_, v)| *v).collect();
            let autocorrelation = lag1_autocorrelation(&nonstop_series);

            let algo_per_tag = algo_by_tag
                .into_iter()
                .map(|(k, mut v)| (k, Stats::from_values(&mut v)))
                .collect();

            // mean PMU counts across this variant's samples (0 when perf was off).
            let nsamp = vsamples.len().max(1) as f64;
            let mean_instructions =
                vsamples.iter().map(|s| s.instructions as f64).sum::<f64>() / nsamp;
            let mean_cycles = vsamples.iter().map(|s| s.cycles as f64).sum::<f64>() / nsamp;
            // mean matrix-scaffold costs (0 for plain timed! variants).
            let mean_setup_ns = vsamples.iter().map(|s| s.setup_ns).sum::<f64>() / nsamp;
            let mean_first_ns = vsamples.iter().map(|s| s.first_ns).sum::<f64>() / nsamp;

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
                mean_instructions,
                mean_cycles,
                mean_setup_ns,
                mean_first_ns,
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

    #[must_use]
    pub fn with_tag_names(mut self, names: &[(&str, u8)]) -> Self {
        for &(name, idx) in names {
            self.tag_names.insert(idx, name.into());
        }
        self
    }

    /// Carry the run parameters the report's methodology table states.
    ///
    /// Without this the table is gated shut (it renders only when `passes` or
    /// `harness_runs` is non-zero) and, worse, `meta.master_seed` stays 0, which
    /// is the fixed point of the bootstrap RNG: every resample then draws the
    /// same index and the interval collapses onto one order statistic.
    #[must_use]
    pub fn with_methodology(mut self, config: &crate::config::BenchConfig) -> Self {
        self.meta.passes = config.passes;
        self.meta.runs_per_pass = config.runs_per_pass;
        self.meta.batch_size = config.batch_size;
        self.meta.harness_runs = config.harness_runs;
        self.meta.cooldowns_ms = config.cooldowns_ms.clone();
        self.meta.master_seed = config.master_seed;
        self.meta.counter_freq = crate::core::counter::counter_frequency();
        self
    }

    #[must_use]
    pub fn with_baseline(mut self, name: &str) -> Self {
        if let Some(idx) = self.variants.iter().position(|v| v.name == name) {
            self.baseline_idx = idx;
        }
        self
    }

    /// Set the baseline-relative framing (`"subtract"` / `"percent"` / `"ratio"`)
    /// the report renders. See [`DataSetMeta::normalise_mode`].
    #[must_use]
    pub fn with_normalise_mode(mut self, mode: &str) -> Self {
        self.meta.normalise_mode = mode.to_string();
        self
    }

    /// Set the null-floor differencing target (a variant name). See
    /// [`DataSetMeta::floor_variant`]. A name not among the variants is ignored by
    /// the reporter (it falls back to raw ratios).
    #[must_use]
    pub fn with_floor(mut self, name: &str) -> Self {
        self.meta.floor_variant = name.to_string();
        self
    }

    /// The floor variant's mean per-call algo time, if a floor is set and present.
    /// `None` = no floor differencing (the reporter uses raw times).
    pub fn floor_mean(&self) -> Option<f64> {
        if self.meta.floor_variant.is_empty() {
            return None;
        }
        self.variants
            .iter()
            .find(|v| v.name == self.meta.floor_variant)
            .map(|v| v.algo_all.mean)
    }

    pub fn baseline(&self) -> &VariantAnalysis {
        &self.variants[self.baseline_idx]
    }
}

impl Default for DataSetMeta {
    fn default() -> Self {
        DataSetMeta {
            passes:           0,
            runs_per_pass:    0,
            batch_size:       0,
            harness_runs:     0,
            cooldowns_ms:     Vec::new(),
            master_seed:      0,
            counter_freq:     0,
            drift_correction: "none".into(),
            ops_per_call:     0,
            normalise_mode:   "subtract".into(),
            floor_variant:    String::new(),
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
#[must_use]
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
    let numer: f64 = (0 .. n - 1)
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

    for i in 0 .. m {
        let rank = (i + 1) as f64;
        p_values[i].1 = (p_values[i].1 * m as f64 / rank).min(1.0);
    }

    for i in (0 .. m - 1).rev() {
        if p_values[i].1 > p_values[i + 1].1 {
            p_values[i].1 = p_values[i + 1].1;
        }
    }
}

/// Compute percentage delta of `value` against `baseline`.
#[must_use]
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
///
/// This is the finaliser only. It is a bijection but it is not the generator:
/// `finalise(0) == 0`, so iterating it from a zero seed never leaves zero.
/// [`bootstrap_ci_median`] advances a counter by the golden-ratio increment and
/// finalises that, which is splitmix64 as specified and has no fixed point.
fn bootstrap_mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D049BB133111EB);
    x ^= x >> 31;
    x
}

/// The median of `vals`, interpolating between the two middle order statistics
/// on an even count. One definition, used by [`Stats`], by the bootstrap point
/// estimate, by the driver's summary and history rows, and by the cross-bench
/// report, so the same samples never yield two numbers both labelled "median".
#[must_use]
pub fn median(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

/// 95% bootstrap confidence interval on the median. Returns
/// `(lower, median, upper)`.
///
/// Fewer than three values cannot be resampled into an interval, so the range
/// of the data is returned instead of a bootstrap one. That is wide rather than
/// precise, which is the honest direction: the previous form returned
/// `(vals[0], vals[0], vals[0])` off the unsorted slice, so the "median" of two
/// samples was whichever the collection happened to record first and the
/// interval claimed zero width around it.
pub fn bootstrap_ci_median(vals: &[f64], seed: u64) -> (f64, f64, f64) {
    if vals.len() < 3 {
        if vals.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        return (lo, median(vals), hi);
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

    for _ in 0 .. BOOTSTRAP_ITERATIONS {
        let mut resample = Vec::with_capacity(n);
        for _ in 0 .. n {
            rng = rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let idx = (bootstrap_mix(rng) as usize) % n;
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

    let diffs: Vec<f64> = (0 .. n).map(|i| a[i] - b[i]).collect();
    bootstrap_ci_median(&diffs, seed)
}

/// A fitted `total(k) = S + k * I` cost model for a variant across program
/// sizes. `S` is the size-independent setup cost (the intercept: decode,
/// allocation, warmup that does not scale with the item count), `I` is the
/// per-item cost (the slope: the cost of one node/record/element), and `r2` is
/// the coefficient of determination (fit quality, 1.0 is a perfect line).
///
/// This is the cost-model lens on a variant: instead of one opaque time at one
/// size, the variant is two numbers (a fixed cost and a marginal cost) plus a
/// confidence that the linear model holds. Two variants compare on `I` (which
/// dispatches/represents/accesses a node faster) independently of `S` (which has
/// the cheaper setup), and a poor `r2` is the signal that the cost is not linear
/// in the item count and the single-size comparison was hiding a regime change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostModel {
    pub setup:    f64, // S: intercept, size-independent cost
    pub per_item: f64, // I: slope, cost per item (node/record/element)
    pub r2:       f64, // coefficient of determination in [0, 1]
    pub points:   usize,
}

/// Fit `total(k) = S + k * I` by ordinary least squares over `(k, total)`
/// points, where `k` is the program size (item count) and `total` is the
/// measured time (or any additive cost) at that size. Returns `None` if there
/// are fewer than two distinct `k` values (a line needs two points and a nonzero
/// spread in `k`).
///
/// The points are typically one representative time (the median or best-20pct of
/// the samples) per size, across the size sweep the harness already runs. The fit
/// is the honest way to separate a variant's fixed overhead from its marginal
/// cost, which a single size conflates.
pub fn fit_cost_model(points: &[(f64, f64)]) -> Option<CostModel> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let sum_k: f64 = points.iter().map(|p| p.0).sum();
    let sum_t: f64 = points.iter().map(|p| p.1).sum();
    let sum_kk: f64 = points.iter().map(|p| p.0 * p.0).sum();
    let sum_kt: f64 = points.iter().map(|p| p.0 * p.1).sum();
    // denom = n*Sigma(k^2) - (Sigma k)^2 = Sigma_{i<j}(k_i - k_j)^2 (Lagrange
    // identity), so it is >= 0 and zero iff all k are identical. Test it relative
    // to its own scale (n*Sigma(k^2)) rather than an absolute epsilon: at large k
    // the raw denom is huge even for near-collinear points, so an absolute floor
    // would pass a numerically unstable slope. The relative floor catches
    // near-collinear k at any magnitude.
    let denom = nf * sum_kk - sum_k * sum_k;
    if denom <= f64::EPSILON * (nf * sum_kk).max(1.0) {
        return None; // no (or negligible) spread in k: slope undefined
    }
    let per_item = (nf * sum_kt - sum_k * sum_t) / denom;
    let setup = (sum_t - per_item * sum_k) / nf;

    // R^2 = 1 - SS_res / SS_tot.
    let mean_t = sum_t / nf;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for &(k, t) in points {
        let pred = setup + per_item * k;
        ss_res += (t - pred) * (t - pred);
        ss_tot += (t - mean_t) * (t - mean_t);
    }
    let r2 = if ss_tot < f64::EPSILON {
        // No spread in the times: a flat line fits perfectly.
        1.0
    } else {
        1.0 - ss_res / ss_tot
    };
    Some(CostModel {
        setup,
        per_item,
        r2,
        points: n,
    })
}

/// Pairwise comparison result.
#[derive(Debug, Clone, Copy)]
pub struct Comparison {
    /// Median of `(variant - baseline)` in ns.
    pub median_diff_ns: f64,
    /// 95% CI lower bound on median difference.
    pub ci_lo_ns:       f64,
    /// 95% CI upper bound on median difference.
    pub ci_hi_ns:       f64,
    /// Percentage delta of medians.
    pub pct:            f64,
    /// Whether the CI excludes zero (statistically significant).
    pub significant:    bool,
    /// Sign test p-value (two-sided).
    pub sign_test_p:    f64,
    /// Number of tied pairs (`a[i] == b[i]`) dropped by the sign test.
    /// High tie count (> 10% of pairs) weakens the test and should be
    /// flagged.
    pub ties:           u32,
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
    for i in 0 .. n {
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

    for i in 0 ..= k {
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
            ci_lo_ns:       0.0,
            ci_hi_ns:       0.0,
            pct:            0.0,
            significant:    false,
            sign_test_p:    1.0,
            ties:           0,
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

    let pct = if base_median != 0.0 { (med_diff / base_median) * 100.0 } else { 0.0 };
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
    #[must_use]
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
    #[must_use]
    pub fn dataset_for_routine(&self, routine: &RoutineSpec, mode: &str) -> DataSet {
        let mut ds = DataSet::from_samples(&self.samples, mode);
        let probe_input = (routine.bridge.input_builder)(0);
        ds.meta.ops_per_call = (routine.bridge.ops_per_call)(&probe_input);
        ds
    }
}

#[cfg(test)]
mod bootstrap_rng_tests {
    use super::*;

    /// `bootstrap_mix` is splitmix64's finaliser, and the finaliser alone maps 0
    /// to 0. The generator has to advance a counter and finalise that; iterating
    /// the finaliser on itself never leaves zero, so every resample index was
    /// `0 % n` and every replicate was the same constant vector.
    ///
    /// Asserted over the drawn indices rather than over `bootstrap_mix`, because
    /// 0 mapping to 0 is correct for the finaliser and wrong for the generator.
    #[test]
    fn the_resample_indices_are_not_constant_at_any_seed() {
        // Nine distinct values, so a constant resample is impossible to mistake
        // for a lucky draw: the interval would sit on one order statistic.
        let vals: Vec<f64> = (0 .. 9).map(|i| i as f64).collect();
        for seed in [0u64, 1, 0xC0C0_CAFE, u64::MAX] {
            let (lo, med, hi) = bootstrap_ci_median(&vals, seed);
            assert!(
                lo < hi,
                "seed {seed:#x}: the bootstrap interval collapsed to a point \
                 ({lo}), which is what a constant resample produces"
            );
            assert!(
                lo <= med && med <= hi,
                "seed {seed:#x}: CI [{lo}, {hi}] excludes the median {med}"
            );
        }
    }

    /// The consequence, stated over the quantity the report reads. Paired
    /// differences symmetric about zero must not come back significant, at any
    /// seed. Under the fixed point the CI collapses onto the minimum
    /// difference, and `compare` reads a CI that excludes zero.
    #[test]
    fn a_symmetric_paired_difference_is_never_significant_at_any_seed() {
        let variant = [7.0f64, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0];
        let baseline = [10.0f64; 7];
        // diffs are -3..=3, median 0.
        for seed in [0u64, 1, 0xC0C0_CAFE, u64::MAX] {
            let cmp = compare(&variant, &baseline, seed);
            assert!(
                !cmp.significant,
                "seed {seed:#x}: symmetric differences reported significant, \
                 ci=[{}, {}]",
                cmp.ci_lo_ns, cmp.ci_hi_ns
            );
            assert!(
                cmp.ci_lo_ns <= cmp.median_diff_ns && cmp.median_diff_ns <= cmp.ci_hi_ns,
                "seed {seed:#x}: the CI [{}, {}] does not contain the point \
                 estimate {} it is reported beside",
                cmp.ci_lo_ns,
                cmp.ci_hi_ns,
                cmp.median_diff_ns
            );
        }
    }

    /// A CI is reported beside a point estimate, so it has to contain it, and
    /// the point estimate has to be the median rather than whichever value the
    /// collection happened to put first. Two samples is the smallest case that
    /// separates the three candidate answers.
    #[test]
    fn the_two_sample_median_does_not_depend_on_collection_order() {
        let (lo_a, med_a, hi_a) = bootstrap_ci_median(&[9.0, 1.0], 42);
        let (lo_b, med_b, hi_b) = bootstrap_ci_median(&[1.0, 9.0], 42);
        assert_eq!(
            med_a, med_b,
            "the reported median of the same two values changed with their \
             order: {med_a} vs {med_b}"
        );
        assert!(
            lo_a <= med_a && med_a <= hi_a,
            "CI [{lo_a}, {hi_a}] excludes {med_a}"
        );
        assert!(
            lo_b <= med_b && med_b <= hi_b,
            "CI [{lo_b}, {hi_b}] excludes {med_b}"
        );
    }

    /// Every median in this crate has to be the same median. `Stats` and the
    /// bootstrap interpolate on even counts; the driver summary and the history
    /// ledger take the upper of the two middles, so the ledger records a
    /// different number from the one the report prints for the same samples.
    #[test]
    fn one_definition_of_median_across_the_crate() {
        let vals = [1.0f64, 2.0, 3.0, 4.0];
        let stats = Stats::from_values(&mut vals.to_vec());
        let (_, boot_med, _) = bootstrap_ci_median(&vals, 0xABCD);
        assert_eq!(stats.median, 2.5, "Stats interpolates");
        assert_eq!(boot_med, 2.5, "the bootstrap point estimate interpolates");
        assert_eq!(
            crate::driver::median_for_tests(&mut vals.to_vec()),
            stats.median,
            "the driver's summary/history median disagrees with the report's"
        );
    }
}

#[cfg(test)]
mod cost_model_tests {
    use super::*;

    #[test]
    fn fits_a_clean_line() {
        // total = 100 + 3*k exactly; the fit recovers S=100, I=3, R2=1.
        let pts: Vec<(f64, f64)> = [64.0, 256.0, 1024.0, 4096.0]
            .iter()
            .map(|&k| (k, 100.0 + 3.0 * k))
            .collect();
        let m = fit_cost_model(&pts).expect("two+ distinct k");
        assert!((m.setup - 100.0).abs() < 1e-6, "setup {}", m.setup);
        assert!((m.per_item - 3.0).abs() < 1e-9, "per_item {}", m.per_item);
        assert!((m.r2 - 1.0).abs() < 1e-9, "r2 {}", m.r2);
    }

    #[test]
    fn noisy_line_has_high_but_imperfect_r2() {
        // A line with small perturbations: slope/intercept close, R2 below 1.
        let pts = vec![(64.0, 292.5), (256.0, 868.0), (1024.0, 3172.0), (4096.0, 12388.5)];
        let m = fit_cost_model(&pts).unwrap();
        assert!(
            m.per_item > 2.9 && m.per_item < 3.1,
            "per_item {}",
            m.per_item
        );
        assert!(m.r2 > 0.999, "r2 {}", m.r2);
    }

    #[test]
    fn declines_without_spread() {
        assert!(fit_cost_model(&[(512.0, 10.0)]).is_none(), "single point");
        assert!(
            fit_cost_model(&[(512.0, 10.0), (512.0, 20.0)]).is_none(),
            "no k spread"
        );
    }
}

#[cfg(test)]
mod nonstop_series_tests {
    use super::*;
    use crate::sample::Sample;

    fn s(variant: &str, run: usize, pass: usize, cd: u64, batch: usize, algo: f64) -> Sample {
        Sample {
            run,
            pass,
            cooldown_ms: cd,
            mode: "warm".into(),
            variant: variant.into(),
            e2e_ns: algo + 5.0,
            algo_ns: algo,
            bridge_ns: 5.0,
            batch_idx: batch,
            batch_count: 1,
            score: None,
            input_tag: None,
            instructions: 0,
            cycles: 0,
            setup_ns: 0.0,
            first_ns: 0.0,
            digest: 0,
        }
    }

    /// The series is per pass, not per batch. The worker emits
    /// `runs_per_pass / batch_size` lines per pass, ten on the shipped
    /// defaults, and pushing each of them made the report's per-pass table ten
    /// times longer than the run and fed `lag1_autocorrelation` a series whose
    /// correlation is mostly within-pass batch order, which the drift detector
    /// then reports as "warm-up / thermal drift" across passes.
    #[test]
    fn the_nonstop_series_holds_one_value_per_pass() {
        let mut samples = Vec::new();
        for run in 1 ..= 2 {
            for pass in 1 ..= 2 {
                for batch in 0 .. 3 {
                    // 10, 20, 30 in every pass: median 20.
                    samples.push(s("a", run, pass, 0, batch, 10.0 * (batch + 1) as f64));
                    // A cooldown cohort that must not enter the series at all.
                    samples.push(s("a", run, pass, 100, batch, 999.0));
                }
            }
        }
        let ds = DataSet::from_samples(&samples, "warm");
        let a = &ds.variants[0];
        assert_eq!(
            a.nonstop_per_pass.len(),
            4,
            "two runs of two passes is four entries, not the twelve batches"
        );
        assert_eq!(
            a.nonstop_per_pass
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>(),
            vec![(1, 1), (1, 2), (2, 1), (2, 2)],
            "keyed and in run-then-pass order"
        );
        for (key, v) in &a.nonstop_per_pass {
            assert!(
                (v - 20.0).abs() < 1e-9,
                "{key:?}: median of 10/20/30 is 20, got {v}"
            );
        }
        // The per-batch series is still available and is a different length.
        assert_eq!(
            a.keyed_algo.len(),
            24,
            "two runs x two passes x three batches x two cooldowns"
        );
    }

    /// A constant series has no correlation to report, and the previous shape
    /// would have found the within-pass 10/20/30 ramp instead.
    #[test]
    fn the_autocorrelation_is_computed_over_the_per_pass_series() {
        let mut samples = Vec::new();
        // Every pass has the identical rising batch pattern, so a per-batch
        // series is strongly patterned while the per-pass series is flat.
        for run in 1 ..= 3 {
            for pass in 1 ..= 4 {
                for batch in 0 .. 5 {
                    samples.push(s("a", run, pass, 0, batch, 100.0 + 20.0 * batch as f64));
                }
            }
        }
        let ds = DataSet::from_samples(&samples, "warm");
        let a = &ds.variants[0];
        assert_eq!(a.nonstop_per_pass.len(), 12);
        assert_eq!(
            a.autocorrelation, 0.0,
            "every pass has the same median, so there is nothing to correlate; \
             a non-zero value here is the batch ramp leaking in"
        );
    }

    /// A variant that lost a pass gets a shorter series, and the report looks
    /// each entry up by key. The gap must be visible as a gap rather than
    /// shifting every later pass onto its neighbour.
    #[test]
    fn a_variant_missing_a_pass_leaves_a_gap_rather_than_a_shift() {
        let mut samples = Vec::new();
        for pass in 1 ..= 3 {
            samples.push(s("aaa_base", 1, pass, 0, 0, 100.0 * pass as f64));
            // The rival is missing pass 2 entirely.
            if pass != 2 {
                samples.push(s("bbb_rival", 1, pass, 0, 0, 100.0 * pass as f64));
            }
        }
        let ds = DataSet::from_samples(&samples, "warm");
        let base = ds.variants.iter().find(|v| v.name == "aaa_base").unwrap();
        let rival = ds.variants.iter().find(|v| v.name == "bbb_rival").unwrap();
        assert_eq!(base.nonstop_per_pass.len(), 3);
        assert_eq!(rival.nonstop_per_pass.len(), 2);
        // Keyed, so pass 3 is still pass 3 and not slid into pass 2's row.
        assert_eq!(rival.nonstop_per_pass[1].0, (1, 3));
        assert!((rival.nonstop_per_pass[1].1 - 300.0).abs() < 1e-9);

        let md = crate::report::generate(&ds, "t");
        assert!(
            md.contains("| 1 | 3 |"),
            "the table is keyed by run and pass:\n{md}"
        );
        assert!(
            md.contains("won 0/2"),
            "the summary counts the two passes both ran, not the baseline's \
             three:\n{md}"
        );
    }
}
