//! Dylib hash cache: skip running variants whose dylib has not
//! changed.
//!
//! Design:
//!
//! - Cached CSV files store RAW measurements, never modified on disk.
//! - Manifest maps `variant_path → (dylib_hash, csv_path,
//!   global_mean_at_record_time)`.
//! - Baseline (index 0) always re-runs as drift anchor.
//! - Drift computed from ALL overlapping variants (baseline weighted
//!   2x).
//! - Correction applied in memory at merge time, target = midpoint.
//! - Append mode: new data adds to cache, does not replace old.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

use crate::sample::Sample;

/// Default cache root, relative to cwd. Override via
/// [`Cache::load_in`] for non-cwd workflows.
pub const DEFAULT_CACHE_ROOT: &str = ".bench_cache";

/// Extract the last path component without extension for use in
/// filenames. E.g.
/// `variants/fnv1a/target/release/libfnv1a.dylib` → `libfnv1a`. Then
/// replace any characters that are not alphanumeric or `_` with `_`.
fn variant_short_name(path: &str) -> String {
    let stem = path.rsplit('/').next().unwrap_or(path);
    let stem = if let Some(dot) = stem.rfind('.') {
        &stem[..dot]
    } else {
        stem
    };
    stem.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// FNV-1a hash of a variant cdylib. Returns 0 if the file cannot be
/// read.
pub fn dylib_hash(path: &str) -> u64 {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    fnv1a(&data)
}

/// FNV-1a hash of a byte slice. Used for dylib and harness hashing.
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= data.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Compute the cache config hash. The hash includes timing knobs,
/// cooldown cohorts, the harness binary's own hash, and the workload
/// structure hash. Any change in those invalidates the cache.
pub fn config_hash(
    passes: usize,
    cooldowns: &[u64],
    runs: usize,
    batch: usize,
    harness_runs: usize,
    workload_hash: u64,
) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for v in &[passes as u64, runs as u64, batch as u64, harness_runs as u64] {
        h ^= v;
        h = h.wrapping_mul(0x100000001b3);
    }
    for &cd in cooldowns {
        h ^= cd;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Include harness binary hash. If the framework changes (workload,
    // measurement protocol, counter code), the cache is invalidated.
    h ^= harness_hash();
    h = h.wrapping_mul(0x100000001b3);
    // Include workload structure hash. Changes to program/stage/item
    // layout invalidate cached results.
    h ^= workload_hash;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// Hash the harness binary itself. Changes to the framework invalidate
/// all cached results.
fn harness_hash() -> u64 {
    match std::env::current_exe() {
        Ok(path) => dylib_hash(&path.to_string_lossy()),
        Err(_) => 0,
    }
}

struct ManifestEntry {
    dylib_hash: u64,
    csv_path: String,
    /// Mean algo_ns across warm-mode samples at the time this entry
    /// was recorded. Used as the warm-mode drift anchor.
    global_mean_warm: f64,
    /// Mean algo_ns across cold-mode samples at the time this entry
    /// was recorded. Used as the cold-mode drift anchor.
    global_mean_cold: f64,
}

/// On-disk + in-memory cache of per-variant CSV results.
pub struct Cache {
    dir: String,
    manifest: HashMap<String, ManifestEntry>,
}

/// One cached batch returned by [`Cache::partition`]: per-mode global
/// means at recording time + the raw samples.
pub struct CachedBatch {
    pub global_mean_warm: f64,
    pub global_mean_cold: f64,
    pub samples: Vec<Sample>,
}

impl Cache {
    /// Load (or initialise) the cache for one bench + config-hash
    /// combination. The on-disk root is
    /// [`DEFAULT_CACHE_ROOT`]`/<bench>/<cfg>` relative to cwd.
    /// Use [`Cache::load_in`] to override the root.
    pub fn load(bench_name: &str, cfg_hash: u64) -> Self {
        Self::load_in(Path::new(DEFAULT_CACHE_ROOT), bench_name, cfg_hash)
    }

    /// Load (or initialise) the cache rooted at `root` instead of the
    /// cwd-relative default. Useful when the harness runs from a
    /// directory other than the consumer's project root, or when a
    /// CI pipeline wants to mount the cache somewhere absolute.
    pub fn load_in(root: &Path, bench_name: &str, cfg_hash: u64) -> Self {
        let dir = format!("{}/{}/{:016x}", root.display(), bench_name, cfg_hash);
        let manifest_path = format!("{}/manifest.tsv", dir);
        let mut manifest = HashMap::new();

        if let Ok(file) = std::fs::File::open(&manifest_path) {
            for line in std::io::BufReader::new(file).lines().flatten() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 4 {
                    if let Ok(hash) = parts[1].parse::<u64>() {
                        // Support both old format (4 cols, single
                        // mean) and new format (5 cols: warm + cold).
                        // Old entries get cold=0 as a safe fallback;
                        // consensus_drift skips zero means.
                        let (gm_warm, gm_cold) = if parts.len() >= 5 {
                            let w = parts[3].parse::<f64>().unwrap_or(0.0);
                            let c = parts[4].parse::<f64>().unwrap_or(0.0);
                            (w, c)
                        } else {
                            // Legacy: treat single mean as warm, cold unknown.
                            let w = parts[3].parse::<f64>().unwrap_or(0.0);
                            (w, 0.0)
                        };
                        manifest.insert(
                            parts[0].to_string(),
                            ManifestEntry {
                                dylib_hash: hash,
                                csv_path: parts[2].to_string(),
                                global_mean_warm: gm_warm,
                                global_mean_cold: gm_cold,
                            },
                        );
                    }
                }
            }
        }

        Cache { dir, manifest }
    }

    /// Partition variants into to-run indices + cached batches.
    /// Always re-runs baseline (index 0) plus one additional cached
    /// variant as drift control. This gives consensus_drift at least
    /// 2 data points.
    pub fn partition(&self, variant_paths: &[String]) -> (Vec<usize>, Vec<CachedBatch>) {
        let mut to_run: Vec<usize> = vec![0];
        let mut cached: Vec<CachedBatch> = Vec::new();
        let mut picked_control = false;

        for (i, path) in variant_paths.iter().enumerate() {
            if i == 0 {
                continue;
            }

            let current_hash = dylib_hash(path);
            if let Some(entry) = self.manifest.get(path) {
                if entry.dylib_hash == current_hash {
                    if let Ok(samples) = load_csv(&entry.csv_path) {
                        if !picked_control {
                            picked_control = true;
                            eprintln!(
                                "  Cache hit (control): {} ({} samples)",
                                path,
                                samples.len()
                            );
                            to_run.push(i);
                            cached.push(CachedBatch {
                                global_mean_warm: entry.global_mean_warm,
                                global_mean_cold: entry.global_mean_cold,
                                samples,
                            });
                        } else {
                            eprintln!("  Cache hit: {} ({} samples)", path, samples.len());
                            cached.push(CachedBatch {
                                global_mean_warm: entry.global_mean_warm,
                                global_mean_cold: entry.global_mean_cold,
                                samples,
                            });
                        }
                        continue;
                    }
                }
            }
            to_run.push(i);
        }

        (to_run, cached)
    }

    /// Load old cached data for a variant (for drift comparison).
    /// Returns `(global_mean_warm, global_mean_cold, samples)`.
    pub fn load_old(&self, variant_path: &str) -> Option<(f64, f64, Vec<Sample>)> {
        self.manifest.get(variant_path).and_then(|entry| {
            load_csv(&entry.csv_path).ok().map(|s| {
                (entry.global_mean_warm, entry.global_mean_cold, s)
            })
        })
    }

    /// Save raw results for a variant with per-mode global means.
    pub fn save_variant(
        &mut self,
        variant_path: &str,
        hash: u64,
        global_mean_warm: f64,
        global_mean_cold: f64,
        samples: &[Sample],
    ) {
        let _ = std::fs::create_dir_all(&self.dir);
        let short = variant_short_name(variant_path);
        let csv_path = format!("{}/{}_{:016x}.csv", self.dir, short, hash);
        write_csv(&csv_path, samples);
        self.manifest.insert(
            variant_path.to_string(),
            ManifestEntry {
                dylib_hash: hash,
                csv_path,
                global_mean_warm,
                global_mean_cold,
            },
        );
    }

    /// Write the manifest TSV to disk. Call after `save_variant`
    /// updates so the manifest reflects new entries.
    pub fn flush(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let manifest_path = format!("{}/manifest.tsv", self.dir);
        if let Ok(mut f) = std::fs::File::create(&manifest_path) {
            for (path, entry) in &self.manifest {
                let _ = writeln!(
                    f,
                    "{}\t{}\t{}\t{:.2}\t{:.2}",
                    path, entry.dylib_hash, entry.csv_path,
                    entry.global_mean_warm, entry.global_mean_cold
                );
            }
        }
    }
}

/// Compute consensus drift ratio from all overlapping variants.
///
/// Drift is computed using only samples matching `mode` (e.g. `"warm"`).
/// This avoids mixing warm and cold measurements, which have different
/// cache-state profiles and thus different characteristic latencies.
///
/// For each variant that has both old and new data, compute the ratio
/// `new_median / old_median`. Average these ratios, weighting the
/// baseline 2x. Using the median per variant is more robust to outlier
/// batches than the mean.
///
/// Returns `(consensus_ratio, confidence)`: `confidence` is the number
/// of overlapping variants used. Higher = more trustworthy.
pub fn consensus_drift(
    fresh_samples: &[Sample],
    old_data: &[(String, Vec<Sample>)],
    baseline_variant: &str,
    mode: &str,
) -> (f64, usize) {
    let mut ratios: Vec<(f64, f64)> = Vec::new();

    for (name, old_samples) in old_data {
        let old_mean = variant_median(old_samples, name, mode);
        let new_mean = variant_median(fresh_samples, name, mode);
        if old_mean == 0.0 || new_mean == 0.0 {
            continue;
        }

        let ratio = new_mean / old_mean;
        let weight = if name == baseline_variant { 2.0 } else { 1.0 };
        ratios.push((ratio, weight));
    }

    if ratios.is_empty() {
        return (1.0, 0);
    }

    let total_weight: f64 = ratios.iter().map(|(_, w)| w).sum();
    let weighted_ratio: f64 = ratios.iter().map(|(r, w)| r * w).sum::<f64>() / total_weight;

    (weighted_ratio, ratios.len())
}

/// Compute drift correction scale factors from cached batches.
///
/// Returns `(cached_scale, fresh_scale)`. The caller must apply these
/// to COPIES of the sample data used for analysis. Never apply to the
/// stored raw data: cached CSVs on disk and in-memory raw batches must
/// remain unmodified.
pub fn apply_drift(
    _cached_batches: &[CachedBatch],
    consensus_ratio: f64,
    confidence: usize,
) -> (f64, f64) {
    if confidence == 0 || (consensus_ratio - 1.0).abs() < 0.001 {
        return (1.0, 1.0);
    }

    // Minimum confidence threshold: with fewer than 3 overlapping
    // variants, drift correction is based on too few data points and
    // can make results worse. Skip correction and suggest full re-run.
    if confidence < 3 {
        eprintln!(
            "  Drift: confidence={} (< 3 variants overlap), skipping correction",
            confidence
        );
        eprintln!("  Recommend: full re-run (--no-cache) for reliable comparisons");
        return (1.0, 1.0);
    }

    // Target = midpoint: scale cached up by half the drift, fresh
    // down by half. If consensus_ratio = new/old = 1.04 (4% faster
    // now): cached_scale = sqrt(1.04) ≈ 1.02, fresh_scale = 1/sqrt(1.04)
    // ≈ 0.98. Both meet in the middle.
    let midpoint_scale = consensus_ratio.sqrt();
    let cached_scale = midpoint_scale;
    let fresh_scale = 1.0 / midpoint_scale;

    eprintln!(
        "  Drift: consensus ratio {:.4}x ({} variants), midpoint scale: cached {:.4}x, fresh {:.4}x",
        consensus_ratio, confidence, cached_scale, fresh_scale
    );

    (cached_scale, fresh_scale)
}

fn variant_median(samples: &[Sample], name: &str, mode: &str) -> f64 {
    let mut vals: Vec<f64> = samples
        .iter()
        .filter(|s| s.variant == name && s.mode == mode)
        .map(|s| s.algo_ns)
        .collect();
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.total_cmp(b));
    let n = vals.len();
    if n % 2 == 0 {
        (vals[n / 2 - 1] + vals[n / 2]) / 2.0
    } else {
        vals[n / 2]
    }
}

/// Global mean across all samples (for cache metadata), filtering by
/// mode.
pub fn global_mean_for_mode(samples: &[Sample], mode: &str) -> f64 {
    let vals: Vec<f64> = samples
        .iter()
        .filter(|s| s.mode == mode)
        .map(|s| s.algo_ns)
        .collect();
    if vals.is_empty() {
        return 0.0;
    }
    vals.iter().sum::<f64>() / vals.len() as f64
}

/// Global mean across all samples (for cache metadata).
pub fn global_mean(samples: &[Sample]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(|s| s.algo_ns).sum::<f64>() / samples.len() as f64
}

fn load_csv(path: &str) -> Result<Vec<Sample>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let mut samples = Vec::new();
    for line in std::io::BufReader::new(file).lines().flatten() {
        if line.starts_with("run,") {
            continue;
        }
        let p: Vec<&str> = line.split(',').collect();
        if p.len() >= 10 {
            samples.push(Sample {
                run: p[0].parse().unwrap_or(0),
                pass: p[1].parse().unwrap_or(0),
                cooldown_ms: p[2].parse().unwrap_or(0),
                mode: p[3].to_string(),
                variant: p[4].to_string(),
                batch_idx: p[5].parse().unwrap_or(0),
                e2e_ns: p[6].parse().unwrap_or(0.0),
                algo_ns: p[7].parse().unwrap_or(0.0),
                bridge_ns: p[8].parse().unwrap_or(0.0),
                batch_count: p[9].parse().unwrap_or(0),
                score: p.get(10).and_then(|s| s.parse().ok()),
                input_tag: p.get(11).and_then(|s| s.parse().ok()),
            });
        }
    }
    Ok(samples)
}

fn write_csv(path: &str, samples: &[Sample]) {
    let mut csv = String::from(
        "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag\n",
    );
    for s in samples {
        let score_str = s.score.map(|v| format!("{:.2}", v)).unwrap_or_default();
        let tag_str = s.input_tag.map(|v| v.to_string()).unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{:.1},{:.1},{:.1},{},{},{}\n",
            s.run, s.pass, s.cooldown_ms, s.mode, s.variant,
            s.batch_idx, s.e2e_ns, s.algo_ns, s.bridge_ns,
            s.batch_count, score_str, tag_str
        ));
    }
    let _ = std::fs::write(path, &csv);
}
