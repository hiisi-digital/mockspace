//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
    pub run:          usize,
    /// Pass index within a run.
    pub pass:         usize,
    /// Cooldown before this sample, in milliseconds.
    pub cooldown_ms:  u64,
    /// Mode label (`"normal"`, `"batched"`, etc.). Reserved for
    /// per-mode aggregation in Round 5.
    pub mode:         String,
    /// Variant label: the name the variant's cdylib exports through its
    /// `bench_name` symbol, not anything derived from its path. Every
    /// grouping downstream keys on this string, so two variants exporting
    /// one name merge into a single arm; `validation::validate` refuses
    /// that pairing before a run reaches here.
    pub variant:      String,
    /// End-to-end nanoseconds (harness-side measurement, includes
    /// bridge overhead).
    pub e2e_ns:       f64,
    /// Algorithm-only nanoseconds (worker-reported; the timed `run {}`
    /// block via [`mockspace_bench_core::timed`]).
    pub algo_ns:      f64,
    /// Bridge overhead = `e2e_ns - algo_ns`. Stored explicitly so
    /// downstream tools do not need to recompute.
    pub bridge_ns:    f64,
    /// Batch index within the worker run.
    pub batch_idx:    usize,
    /// Number of calls in this batch.
    pub batch_count:  usize,
    /// Optional quality score (lower = better). Filled when the
    /// [`crate::core::Routine::score_output`] returns `Some`.
    pub score:        Option<f64>,
    /// Optional input tag for per-pattern breakdown (e.g. sparsity
    /// pattern). Tag values are routine-defined.
    pub input_tag:    Option<u8>,
    /// Hardware instructions retired for this batch's measured region (per call,
    /// mean over the batch). Zero when perf counters are unavailable / off.
    pub instructions: u64,
    /// Hardware cycles for this batch's measured region (per call, mean). Zero
    /// when perf counters are unavailable / off.
    pub cycles:       u64,
    /// One-time setup cost S in nanoseconds (per call, mean over the batch).
    /// Populated only by the matrix scaffold, which times setup on every call;
    /// zero for plain `timed!` / `timed_calibrated!` variants that measure only
    /// the run block. With this alongside [`Self::algo_ns`], the tier breakeven
    /// `k* = (S_b - S_a) / (I_a - I_b)` is computable directly from the samples.
    pub setup_ns:     f64,
    /// Cold first-touch cost in nanoseconds (per call, mean): the first run pass
    /// before the calibrated loop warms caches and the branch predictor. Matrix
    /// scaffold only; zero otherwise.
    pub first_ns:     f64,
    /// Reps-invariant fidelity digest for cross-variant validation. Computed on a
    /// fixed-seed, fixed-init single pass, so it is comparable across variants
    /// under calibration where the run-block output bytes are not. Matrix
    /// scaffold only; zero otherwise.
    pub digest:       u64,
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

/// The header row every sample CSV in this crate carries. One definition: the
/// column order is a contract between four places (this writer, this reader,
/// the cache's copy, and the cross-bench report's parser) and it drifted, so
/// `meta_report` read column 6 and called it `algo_ns` when column 6 is
/// `e2e_ns`.
pub const CSV_HEADER: &str = "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,\
bridge_ns,batch_count,score,input_tag,instructions,cycles,setup_ns,first_ns,digest\n";

/// Render samples as CSV, header included, in [`CSV_HEADER`]'s column order.
///
/// Nothing is quoted. `validation::first_unsafe_name` refuses a variant whose
/// exported name carries a delimiter before a run reaches here, because both
/// readers fall back silently on a short row and a shifted column comes back
/// as a wrong number rather than an error.
#[must_use]
pub fn to_csv(samples: &[Sample]) -> String {
    let mut csv = String::from(CSV_HEADER);
    for s in samples {
        let score_str = s.score.map(|v| format!("{:.2}", v)).unwrap_or_default();
        let tag_str = s.input_tag.map(|v| v.to_string()).unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{:.1},{:.1},{:.1},{},{},{},{},{},{:.1},{:.1},{}\n",
            s.run,
            s.pass,
            s.cooldown_ms,
            s.mode,
            s.variant,
            s.batch_idx,
            s.e2e_ns,
            s.algo_ns,
            s.bridge_ns,
            s.batch_count,
            score_str,
            tag_str,
            s.instructions,
            s.cycles,
            s.setup_ns,
            s.first_ns,
            s.digest
        ));
    }
    csv
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
            run:          p[0].parse().unwrap_or(0),
            pass:         p[1].parse().unwrap_or(0),
            cooldown_ms:  p[2].parse().unwrap_or(0),
            mode:         p[3].to_string(),
            variant:      p[4].to_string(),
            batch_idx:    p[5].parse().unwrap_or(0),
            e2e_ns:       p[6].parse().unwrap_or(0.0),
            algo_ns:      p[7].parse().unwrap_or(0.0),
            bridge_ns:    p[8].parse().unwrap_or(0.0),
            batch_count:  p[9].parse().unwrap_or(0),
            score:        p.get(10).and_then(|s| s.parse().ok()),
            input_tag:    p.get(11).and_then(|s| s.parse().ok()),
            // appended columns; absent in older CSVs, default 0.
            instructions: p.get(12).and_then(|s| s.parse().ok()).unwrap_or(0),
            cycles:       p.get(13).and_then(|s| s.parse().ok()).unwrap_or(0),
            // matrix-scaffold columns, appended after perf; absent in older CSVs.
            setup_ns:     p.get(14).and_then(|s| s.parse().ok()).unwrap_or(0.0),
            first_ns:     p.get(15).and_then(|s| s.parse().ok()).unwrap_or(0.0),
            digest:       p.get(16).and_then(|s| s.parse().ok()).unwrap_or(0),
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

        // newest format: 17 columns including the appended matrix columns
        // setup_ns,first_ns,digest after instructions,cycles.
        let newp = dir.join("new.csv");
        std::fs::write(
            &newp,
            "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag,instructions,cycles,setup_ns,first_ns,digest\n\
             1,1,0,warm,switch,0,120.0,100.0,20.0,64,,,4200,900,555.5,180.0,987654321\n",
        )
        .unwrap();
        let s = load_samples_csv(&newp).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].instructions, 4200);
        assert_eq!(s[0].cycles, 900);
        assert_eq!(s[0].setup_ns, 555.5);
        assert_eq!(s[0].first_ns, 180.0);
        assert_eq!(s[0].digest, 987654321);

        // 14-column format (perf but no matrix columns) must still load, matrix
        // columns defaulting to 0.
        let perfp = dir.join("perf.csv");
        std::fs::write(
            &perfp,
            "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag,instructions,cycles\n\
             1,1,0,warm,switch,0,120.0,100.0,20.0,64,,,4200,900\n",
        )
        .unwrap();
        let sp = load_samples_csv(&perfp).unwrap();
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].instructions, 4200);
        assert_eq!(sp[0].setup_ns, 0.0);
        assert_eq!(sp[0].digest, 0);

        // old format: 12 columns, no perf; must still load, all appended
        // columns defaulting to 0.
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
        assert_eq!(so[0].setup_ns, 0.0);
        assert_eq!(so[0].digest, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // The worker-line positional contract has no test. The one that stood here
    // built a tab-separated string with `format!` and then asserted that
    // splitting it returned the fields it had just interpolated, calling
    // neither the emitter nor the parser. It could not fail if a column moved,
    // which is the only drift it claimed to guard, so it reported coverage
    // that did not exist. Deleted rather than adjusted: a test that cannot
    // fail occupies the place where its absence would otherwise be noticed.
    //
    // The real thing needs the emitter and the orchestrator's parser in one
    // round trip, which is worth doing and is larger than the change that
    // removed this.
}

#[cfg(test)]
mod round_trip_tests {
    use super::*;
    use crate::sample::Sample;

    fn sample(variant: &str) -> Sample {
        Sample {
            run:          1,
            pass:         2,
            cooldown_ms:  600,
            mode:         "warm".into(),
            variant:      variant.into(),
            e2e_ns:       123.4,
            algo_ns:      100.1,
            bridge_ns:    23.3,
            batch_idx:    7,
            batch_count:  5000,
            score:        Some(0.5),
            input_tag:    Some(3),
            instructions: 4242,
            cycles:       2121,
            setup_ns:     9.9,
            first_ns:     8.8,
            digest:       0xFEED,
        }
    }

    /// Neither CSV writer quotes and neither reader unquotes, and every field on
    /// the read side falls back with `unwrap_or`, so a variant name carrying a
    /// delimiter shifts every column after it and the row comes back with wrong
    /// numbers in the wrong fields rather than failing.
    ///
    /// This is the law that ties `validation::first_unsafe_name` to what it
    /// guards: every name that check accepts survives the write-and-read
    /// round trip intact. Without the tie the check is a list of characters
    /// somebody thought were bad.
    #[test]
    fn every_name_the_validator_accepts_survives_the_csv_round_trip() {
        let candidates = [
            "fnv1a",
            "xx_hash-64",
            "v1.2",
            "a",
            "two words",
            "Mixed_Case9",
            "with(parens)",
            "with[brackets]",
            "utf8-\u{e4}\u{f6}",
        ];
        for name in candidates {
            let owned = vec![name.to_string()];
            assert_eq!(
                crate::validation::first_unsafe_name(&owned),
                None,
                "`{name}` is in this test because the validator accepts it"
            );
            let result = BenchResult {
                title:       "t".into(),
                env:         crate::env::EnvMeta::default(),
                samples:     vec![sample(name)],
                cache_path:  String::new(),
                report_path: String::new(),
            };
            let dir =
                std::env::temp_dir().join(format!("mockspace-bh-roundtrip-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("rt.csv");
            crate::harness::write_csv(&result, &path.display().to_string()).unwrap();
            let back = load_samples_csv(&path).unwrap();
            std::fs::remove_dir_all(&dir).ok();

            assert_eq!(back.len(), 1, "`{name}`: one row in, one row out");
            let b = &back[0];
            let a = &result.samples[0];
            assert_eq!(b.variant, a.variant, "`{name}`: variant");
            assert_eq!(b.run, a.run, "`{name}`: run");
            assert_eq!(b.pass, a.pass, "`{name}`: pass");
            assert_eq!(b.cooldown_ms, a.cooldown_ms, "`{name}`: cooldown");
            assert_eq!(b.mode, a.mode, "`{name}`: mode");
            assert_eq!(b.batch_idx, a.batch_idx, "`{name}`: batch_idx");
            assert_eq!(b.batch_count, a.batch_count, "`{name}`: batch_count");
            assert_eq!(b.instructions, a.instructions, "`{name}`: instructions");
            assert_eq!(b.cycles, a.cycles, "`{name}`: cycles");
            assert_eq!(b.digest, a.digest, "`{name}`: digest");
            assert_eq!(b.input_tag, a.input_tag, "`{name}`: input_tag");
            assert!((b.e2e_ns - a.e2e_ns).abs() < 0.05, "`{name}`: e2e_ns");
            assert!((b.algo_ns - a.algo_ns).abs() < 0.05, "`{name}`: algo_ns");
            assert!((b.setup_ns - a.setup_ns).abs() < 0.05, "`{name}`: setup_ns");
            assert!((b.first_ns - a.first_ns).abs() < 0.05, "`{name}`: first_ns");
        }
    }

    /// The negative control: a name the validator refuses does corrupt the row,
    /// so the guard is refusing something real rather than a list of characters
    /// somebody disliked.
    #[test]
    fn a_name_the_validator_refuses_does_corrupt_the_row() {
        let bad = "has,comma";
        assert!(
            crate::validation::first_unsafe_name(&[bad.to_string()]).is_some(),
            "the validator has to refuse this for the rest of the test to mean \
             anything"
        );
        let result = BenchResult {
            title:       "t".into(),
            env:         crate::env::EnvMeta::default(),
            samples:     vec![sample(bad)],
            cache_path:  String::new(),
            report_path: String::new(),
        };
        let dir =
            std::env::temp_dir().join(format!("mockspace-bh-roundtrip-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.csv");
        crate::harness::write_csv(&result, &path.display().to_string()).unwrap();
        let back = load_samples_csv(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(back.len(), 1);
        assert_ne!(
            back[0].variant, bad,
            "the comma did not split the field, so this format is safe and the \
             guard is unnecessary"
        );
        assert_ne!(
            back[0].digest, 0xFEED,
            "the columns did not shift, so the corruption this guard prevents \
             does not happen"
        );
    }
}
