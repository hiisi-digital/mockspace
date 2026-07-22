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
    pub run:         usize,
    /// Pass index within a run.
    pub pass:        usize,
    /// Cooldown before this sample, in milliseconds.
    pub cooldown_ms: u64,
    /// Mode label (`"normal"`, `"batched"`, etc.). Reserved for
    /// per-mode aggregation in Round 5.
    pub mode:        String,
    /// Variant short name (extracted from the cdylib path).
    pub variant:     String,
    /// End-to-end nanoseconds (harness-side measurement, includes
    /// bridge overhead).
    pub e2e_ns:      f64,
    /// Algorithm-only nanoseconds (worker-reported; the timed `run {}`
    /// block via [`mockspace_bench_core::timed`]).
    pub algo_ns:     f64,
    /// Bridge overhead = `e2e_ns - algo_ns`. Stored explicitly so
    /// downstream tools do not need to recompute.
    pub bridge_ns:   f64,
    /// Batch index within the worker run.
    pub batch_idx:   usize,
    /// Number of calls in this batch.
    pub batch_count: usize,
    /// Optional quality score (lower = better). Filled when the
    /// [`crate::core::Routine::score_output`] returns `Some`.
    pub score:       Option<f64>,
    /// Optional input tag for per-pattern breakdown (e.g. sparsity
    /// pattern). Tag values are routine-defined.
    pub input_tag:   Option<u8>,
    /// Hardware instructions retired for this batch's measured region (per call,
    /// mean over the batch). Zero when perf counters are unavailable / off.
    pub instructions: u64,
    /// Hardware cycles for this batch's measured region (per call, mean). Zero
    /// when perf counters are unavailable / off.
    pub cycles:       u64,
}

/// What [`crate::run`] returns on success.
///
/// Round 1 ships the shape; Round 3 starts populating
/// [`Self::samples`]; Round 5 attaches analysis output; Round 6
/// attaches the `findings.md` path.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BenchResult {
    /// Title of the bench run, copied from the [`crate::BenchConfig`].
    pub title:       String,
    /// Environment metadata captured at run start.
    pub env:         EnvMeta,
    /// Per-batch samples emitted by the orchestrator.
    pub samples:     Vec<Sample>,
    /// Path to the CSV the orchestrator wrote, if any. Empty when
    /// the result has not yet been persisted.
    pub cache_path:  String,
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
    let file = std::fs::File::open(path).map_err(|e| BenchError::io("opening csv", e))?;
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
            run:         p[0].parse().unwrap_or(0),
            pass:        p[1].parse().unwrap_or(0),
            cooldown_ms: p[2].parse().unwrap_or(0),
            mode:        p[3].to_string(),
            variant:     p[4].to_string(),
            batch_idx:   p[5].parse().unwrap_or(0),
            e2e_ns:      p[6].parse().unwrap_or(0.0),
            algo_ns:     p[7].parse().unwrap_or(0.0),
            bridge_ns:   p[8].parse().unwrap_or(0.0),
            batch_count: p[9].parse().unwrap_or(0),
            score:       p.get(10).and_then(|s| s.parse().ok()),
            input_tag:   p.get(11).and_then(|s| s.parse().ok()),
            // appended columns; absent in older CSVs, default 0.
            instructions: p.get(12).and_then(|s| s.parse().ok()).unwrap_or(0),
            cycles:       p.get(13).and_then(|s| s.parse().ok()).unwrap_or(0),
        });
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_parses_perf_columns_and_is_backward_compatible() {
        let dir = std::env::temp_dir().join(format!("perf_csv_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // new format: 14 columns including the appended instructions,cycles.
        let newp = dir.join("new.csv");
        std::fs::write(
            &newp,
            "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag,instructions,cycles\n\
             1,1,0,warm,switch,0,120.0,100.0,20.0,64,,,4200,900\n",
        )
        .unwrap();
        let s = load_samples_csv(&newp).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].instructions, 4200);
        assert_eq!(s[0].cycles, 900);

        // old format: 12 columns, no perf; must still load, perf defaulting 0.
        let oldp = dir.join("old.csv");
        std::fs::write(
            &oldp,
            "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag\n\
             1,1,0,warm,switch,0,120.0,100.0,20.0,64,,\n",
        )
        .unwrap();
        let so = load_samples_csv(&oldp).unwrap();
        assert_eq!(so.len(), 1);
        assert_eq!(so[0].instructions, 0);
        assert_eq!(so[0].cycles, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worker_line_positional_contract() {
        // The worker emits instructions/cycles at tab columns 7,8 (before the
        // optional score at 9); the orchestrator parser (harness.rs) reads those
        // exact positions. This guards the emitter and parser against drift.
        let with_score = format!(
            "switch\twarm\t0\t120.0\t100.0\t20.0\t64\t{}\t{}\t42.00",
            4200u64, 900u64
        );
        let p: Vec<&str> = with_score.split('\t').collect();
        assert_eq!(p[7], "4200", "instructions at col 7");
        assert_eq!(p[8], "900", "cycles at col 8");
        assert_eq!(p[9], "42.00", "score at col 9");

        let no_score =
            format!("switch\twarm\t0\t120.0\t100.0\t20.0\t64\t{}\t{}", 4200u64, 900u64);
        let q: Vec<&str> = no_score.split('\t').collect();
        assert_eq!(q.len(), 9, "no-score line has exactly the 9 fixed columns");
        assert!(q.get(9).is_none(), "no score column when absent");
    }
}
