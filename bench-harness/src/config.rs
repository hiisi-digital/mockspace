//! Bench configuration shapes.
//!
//! Two tiers, mirroring the polka-dots split:
//!
//! - [`BenchManifest`]: TOML-loadable hierarchical shape that lives at
//!   `mock/benches/bench.toml`. Multi-bench, multi-size; the file the
//!   consumer authors.
//! - [`BenchConfig`]: flat per-run shape the orchestrator consumes.
//!   One [`BenchConfig`] per `(bench, size)` entry in the manifest.
//!
//! Round 1 ships both shapes and a [`BenchManifest::load`] that reads
//! TOML; the manifest-to-flat-config conversion (`for_size`) is
//! present but the orchestrator that consumes the result is stubbed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::BenchError;

// ── Tier 1: TOML-loadable manifest (`mock/benches/bench.toml`) ──

/// Hierarchical TOML shape lifted from polka-dots `framework/config.rs`.
///
/// Format example:
///
/// ```toml
/// [bench.content_hash]
/// title = "Content Hash"
/// workload = "hash"
/// master_seed = 0xDEADBEEFCAFEBABE
///
/// [[bench.content_hash.sizes]]
/// n = 64
/// variants = [
///     "variants/fnv1a/target/release/libfnv1a.dylib",
///     "variants/xxhash3/target/release/libxxhash3.dylib",
/// ]
///
/// [timing]
/// passes = 10
/// runs_per_pass = 50000
/// batch_size = 5000
/// harness_runs = 3
/// cooldowns_ms = [0, 100, 600]
/// ```
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BenchManifest {
    /// Named bench entries. Key is the bench short name.
    #[serde(default)]
    pub bench: HashMap<String, BenchSection>,
    /// Shared timing parameters applied to every bench unless
    /// overridden. Round 1 has no override mechanism; later rounds
    /// may add per-bench timing overrides.
    #[serde(default)]
    pub timing: TimingSection,
}

/// One named bench inside a [`BenchManifest`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchSection {
    /// Display title for the bench (used in findings.md).
    pub title: String,
    /// Workload program identifier (matches a name registered with
    /// the harness's workload module in Round 2).
    pub workload: String,
    /// Master seed for input generation. `0` means "use a fresh
    /// random seed every run"; any other value reproduces.
    #[serde(default)]
    pub master_seed: u64,
    /// One [`SizeSection`] per N value the bench should run at.
    #[serde(default)]
    pub sizes: Vec<SizeSection>,
}

/// One `(N, [variants])` pair inside a [`BenchSection`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SizeSection {
    /// Logical size parameter passed into `bench_entry(... , n: usize)`.
    pub n: usize,
    /// Cdylib paths, one per variant, relative to `mock/benches/`.
    pub variants: Vec<String>,
}

/// Shared timing knobs.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TimingSection {
    /// Outer pass count per harness run.
    #[serde(default = "default_passes")]
    pub passes: usize,
    /// Inner runs per pass.
    #[serde(default = "default_runs")]
    pub runs_per_pass: usize,
    /// Calls per emitted [`crate::Sample`].
    #[serde(default = "default_batch")]
    pub batch_size: usize,
    /// Outer harness runs (the whole pipeline repeated for stability).
    #[serde(default = "default_harness_runs")]
    pub harness_runs: usize,
    /// Cooldown durations injected between cohorts, in milliseconds.
    /// Each cooldown becomes a separate cohort in the cache; analysis
    /// uses the spread to detect thermal drift.
    #[serde(default = "default_cooldowns")]
    pub cooldowns_ms: Vec<u64>,
}

fn default_passes() -> usize { 10 }
fn default_runs() -> usize { 50_000 }
fn default_batch() -> usize { 5_000 }
fn default_harness_runs() -> usize { 3 }
fn default_cooldowns() -> Vec<u64> { vec![0, 100, 600] }

impl Default for TimingSection {
    fn default() -> Self {
        TimingSection {
            passes: default_passes(),
            runs_per_pass: default_runs(),
            batch_size: default_batch(),
            harness_runs: default_harness_runs(),
            cooldowns_ms: default_cooldowns(),
        }
    }
}

impl BenchManifest {
    /// Load a manifest from a TOML file. The file is read in full;
    /// missing keys fall back to [`Default`].
    pub fn load(path: &Path) -> Result<Self, BenchError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| BenchError::io("reading bench.toml", e))?;
        toml::from_str(&text).map_err(|e| BenchError::InvalidConfig {
            reason: format!("bench.toml parse error: {e}"),
        })
    }

    /// Build a flat [`BenchConfig`] for one `(bench, size_idx)` entry.
    /// Returns [`BenchError::InvalidConfig`] when either index is
    /// missing.
    ///
    /// Cdylib paths are resolved against `mock_benches_dir` so the
    /// orchestrator does not need to know how the manifest was
    /// loaded.
    pub fn for_size(
        &self,
        bench_name: &str,
        size_idx: usize,
        mock_benches_dir: &Path,
    ) -> Result<BenchConfig, BenchError> {
        let section = self.bench.get(bench_name).ok_or_else(|| BenchError::InvalidConfig {
            reason: format!("bench `{bench_name}` not found in manifest"),
        })?;
        let size = section.sizes.get(size_idx).ok_or_else(|| BenchError::InvalidConfig {
            reason: format!(
                "bench `{bench_name}` has no size at index {size_idx} (have {})",
                section.sizes.len()
            ),
        })?;
        let variant_paths = size
            .variants
            .iter()
            .map(|p| mock_benches_dir.join(p))
            .collect();
        Ok(BenchConfig {
            bench_name: bench_name.to_string(),
            title: section.title.clone(),
            workload: section.workload.clone(),
            master_seed: section.master_seed,
            n: size.n,
            variant_paths,
            passes: self.timing.passes,
            runs_per_pass: self.timing.runs_per_pass,
            batch_size: self.timing.batch_size,
            harness_runs: self.timing.harness_runs,
            cooldowns_ms: self.timing.cooldowns_ms.clone(),
            batch_k: 1,
            max_call_us: None,
        })
    }
}

// ── Tier 2: flat per-run config the orchestrator consumes ──

/// Flat per-run config. One [`BenchConfig`] feeds one [`crate::run`]
/// invocation.
///
/// Construct manually for ad-hoc runs, or via
/// [`BenchManifest::for_size`] for manifest-driven runs.
#[derive(Clone, Debug)]
pub struct BenchConfig {
    /// Manifest key for this bench (e.g. `"content_hash"`).
    pub bench_name: String,
    /// Display title (for findings.md).
    pub title: String,
    /// Workload program identifier.
    pub workload: String,
    /// Master seed (`0` = fresh random).
    pub master_seed: u64,
    /// Logical size N (passed into `bench_entry(... n)` and
    /// `Routine::max_call_us(n)`).
    pub n: usize,
    /// Resolved cdylib paths (one per variant).
    pub variant_paths: Vec<PathBuf>,
    /// Outer pass count.
    pub passes: usize,
    /// Inner runs per pass.
    pub runs_per_pass: usize,
    /// Calls per emitted sample.
    pub batch_size: usize,
    /// Outer harness runs.
    pub harness_runs: usize,
    /// Cooldown cohorts (milliseconds).
    pub cooldowns_ms: Vec<u64>,
    /// Batch-amortised mode. `1` = normal (one timed call per batch
    /// entry). `>1` = K calls between one outer counter pair, then
    /// per-call time = total / K. Useful when bridge overhead
    /// dominates measured time at small N.
    pub batch_k: usize,
    /// Per-call timeout in microseconds. If a worker's batch mean
    /// exceeds this, the worker aborts and reports
    /// [`BenchError::WorkerFailed`]. `None` = no timeout. Round 1
    /// stores it; Round 3 enforces it.
    pub max_call_us: Option<u64>,
}

impl Default for BenchConfig {
    fn default() -> Self {
        BenchConfig {
            bench_name: String::new(),
            title: "Benchmark".into(),
            workload: String::new(),
            master_seed: 0,
            n: 64,
            variant_paths: Vec::new(),
            passes: default_passes(),
            runs_per_pass: default_runs(),
            batch_size: default_batch(),
            harness_runs: default_harness_runs(),
            cooldowns_ms: default_cooldowns(),
            batch_k: 1,
            max_call_us: None,
        }
    }
}
