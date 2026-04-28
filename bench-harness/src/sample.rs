//! Per-call timing record + run-level result aggregate.
//!
//! Field shape mirrors polka-dots' `harness::Sample` so the CSV cache
//! and downstream analysis (Round 5) can read v1 caches written by
//! polka-dots. Round 3 emits these from the orchestrator; Round 5
//! aggregates them into `analysis::DataSet`.

use std::io::BufRead;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::env::EnvMeta;
use crate::error::BenchError;

/// One per-batch timing record.
///
/// The orchestrator emits one [`Sample`] per batch (not per run-level
/// average) so distributional analysis (bootstrap CIs, sign test,
/// Pareto frontier) has the underlying samples to work from.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sample {
    /// Harness run index (outer loop, repeated for stability).
    pub run: usize,
    /// Pass index within a run.
    pub pass: usize,
    /// Cooldown before this sample, in milliseconds.
    pub cooldown_ms: u64,
    /// Mode label (`"normal"`, `"batched"`, etc.). Reserved for
    /// per-mode aggregation in Round 5.
    pub mode: String,
    /// Variant short name (extracted from the cdylib path).
    pub variant: String,
    /// End-to-end nanoseconds (harness-side measurement, includes
    /// bridge overhead).
    pub e2e_ns: f64,
    /// Algorithm-only nanoseconds (worker-reported; the timed `run {}`
    /// block via [`mockspace_bench_core::timed`]).
    pub algo_ns: f64,
    /// Bridge overhead = `e2e_ns - algo_ns`. Stored explicitly so
    /// downstream tools do not need to recompute.
    pub bridge_ns: f64,
    /// Batch index within the worker run.
    pub batch_idx: usize,
    /// Number of calls in this batch.
    pub batch_count: usize,
    /// Optional quality score (lower = better). Filled when the
    /// [`crate::core::Routine::score_output`] returns `Some`.
    pub score: Option<f64>,
    /// Optional input tag for per-pattern breakdown (e.g. sparsity
    /// pattern). Tag values are routine-defined.
    pub input_tag: Option<u8>,
}

/// What [`crate::run`] returns on success.
///
/// Round 1 ships the shape; Round 3 starts populating
/// [`Self::samples`]; Round 5 attaches analysis output; Round 6
/// attaches the `findings.md` path.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BenchResult {
    /// Title of the bench run, copied from the [`crate::BenchConfig`].
    pub title: String,
    /// Environment metadata captured at run start.
    pub env: EnvMeta,
    /// Per-batch samples emitted by the orchestrator.
    pub samples: Vec<Sample>,
    /// Path to the CSV the orchestrator wrote, if any. Empty when
    /// the result has not yet been persisted.
    pub cache_path: String,
    /// Path to the `findings.md` produced by report generation.
    /// Empty until [`crate::write_report`] runs.
    pub report_path: String,
}

/// Load `Sample` rows from a CSV produced by [`crate::write_csv`].
///
/// Used by `mock bench report --report-only` and by tooling that
/// wants to reuse a previous run's data without re-invoking the
/// orchestrator. Header row is skipped; trailing or empty lines are
/// tolerated. Missing optional columns (`score`, `input_tag`)
/// default to `None`.
pub fn load_samples_csv(path: &Path) -> Result<Vec<Sample>, BenchError> {
    let file = std::fs::File::open(path)
        .map_err(|e| BenchError::io("opening csv", e))?;
    let mut samples = Vec::new();
    for line in std::io::BufReader::new(file).lines().flatten() {
        if line.starts_with("run,") || line.is_empty() {
            continue;
        }
        let p: Vec<&str> = line.split(',').collect();
        if p.len() < 10 {
            continue;
        }
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
    Ok(samples)
}
