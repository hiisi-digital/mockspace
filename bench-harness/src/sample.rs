//! Per-call timing record + run-level result aggregate.
//!
//! Field shape mirrors polka-dots' `harness::Sample` so the CSV cache
//! and downstream analysis (Round 5) can read v1 caches written by
//! polka-dots. Round 3 emits these from the orchestrator; Round 5
//! aggregates them into `analysis::DataSet`.

use serde::{Deserialize, Serialize};

use crate::env::EnvMeta;

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
    /// Path to the CSV cache file written by Round 2's cache module.
    /// Empty in Round 1 (cache stub returns no path).
    pub cache_path: String,
    /// Path to the `findings.md` written by Round 6's report module.
    /// Empty until Round 6 lands.
    pub report_path: String,
}
