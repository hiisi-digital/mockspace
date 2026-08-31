//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Canonical bench harness for mockspace consumers.
//!
//! Loads variant cdylibs in subprocess isolation, drives them with
//! workload programs, collects per-batch samples, validates outputs
//! across variants, runs Pareto + multi-dim analysis, and emits
//! findings.md plus a CSV cache for historical comparison.
//!
//! ## Two entry points, and only one of them validates
//!
//! [`driver::drive_spec`] (and its compat wrapper [`driver::drive`]) is the
//! whole manifest-driven loop: it loads `mock/benches/bench.toml`, resolves
//! every `(bench, size)` cell, runs the duplicate-disassembly check and
//! [`validation::validate`] before any timing, drives the orchestrator, and
//! writes the CSV, the findings file, the history ledger and the index. A
//! consumer `main` shrinks to its registrations.
//!
//! [`run`] is the lower entry point: it takes one [`BenchConfig`] and calls
//! [`harness::run_orchestrator`] directly. **It does not validate.** No
//! cross-variant comparison, no per-variant `validate_output`, no determinism
//! check, no duplicate-disassembly check: it times whatever dylibs the config
//! names. A consumer that calls [`run`] and wants those checks calls
//! [`validation::validate`] itself and feeds it the surviving paths.
//!
//! A binary that will be spawned as a validation worker needs a
//! `--mode validate` arm dispatching to [`harness::run_worker_validate`].
//! Without one the worker prints no `VOUT` lines, every variant is skipped for
//! returning zero of N outputs, and the pass reports success over an empty set.

#![forbid(unsafe_op_in_unsafe_fn)]

pub use mockspace_bench_core as core;

pub mod analysis;
pub mod cache;
pub mod config;
pub mod disasm;
pub mod driver;
pub mod env;
pub mod error;
pub mod harness;
pub mod history;
pub mod inline_bench;
pub mod matrix;
pub mod meta_report;
pub mod perf;
pub mod quality;
pub mod report;
pub mod sample;
pub mod spec;
pub mod summary;
pub mod tree;
pub mod validation;
pub mod workload;

pub use analysis::{
    Comparison,
    CostModel,
    DataSet,
    DataSetMeta,
    Stats,
    VariantAnalysis,
    bh_fdr_adjust,
    bootstrap_ci_diff,
    bootstrap_ci_median,
    compare,
    fit_cost_model,
    lag1_autocorrelation,
    pct_delta,
    sign_test,
};
pub use cache::{
    Cache,
    CachedBatch,
    DEFAULT_CACHE_ROOT,
    apply_drift,
    config_hash,
    consensus_drift,
    dylib_hash,
    global_mean,
    global_mean_for_mode,
};
pub use config::{
    BenchConfig,
    BenchManifest,
    BenchSection,
    HarnessTuning,
    SizeSection,
    TimingSection,
};
pub use disasm::check_duplicates as check_disasm_duplicates;
pub use driver::{
    AfterCell,
    CellVerdict,
    DriverRegistry,
    DriverSpec,
    Hooks,
    InitContext,
    InitVerdict,
    RunPlan,
    drive,
    drive_spec,
};
pub use env::{EnvMeta, collect_env_meta};
pub use error::BenchError;
pub use harness::{run_orchestrator, run_worker, write_csv};
pub use history::{
    DEFAULT_HISTORY_DIR,
    HistoryEntry,
    Regression,
    append as append_history,
    append_in as append_history_in,
    detect_regressions,
    detect_regressions_window,
    flagged_for as regression_flagged_for,
    git_commit,
    load as load_history,
    load_in as load_history_in,
    timestamp,
};
pub use inline_bench::{InlineResult, InlineVariant, run_inline};
pub use meta_report::{VariantResult, classify_family, generate as generate_meta_report};
pub use perf::{
    PerfSnapshot,
    available as perf_available,
    read as perf_read,
    read_all_raw as perf_read_all_raw,
    setup as perf_setup,
    teardown as perf_teardown,
};
pub use quality::{VariantQuality, measure as measure_quality};
pub use report::generate as generate_report;
pub use sample::{BenchResult, Sample, load_samples_csv};
pub use spec::{RoutineSpec, VariantSpec};
pub use validation::validate;
pub use workload::{
    AllocHandle,
    Chain,
    OneOf,
    Program,
    ProgramBuilder,
    Shuffle,
    Stage,
    StageStrategy,
    Workload,
    WorkloadCtx,
    WorkloadItemKind,
    algo_call,
    branch_work,
    domain_work,
    graph_work,
    heavy_memory,
    light_scalar,
    mix,
    scalar_work,
};

/// Run the harness against one [`BenchConfig`] using the given
/// [`RoutineSpec`], spawning one worker subprocess per
/// `(variant × cooldown × pass × mode)`.
///
/// **No validation runs on this path.** See the crate docs: the checks live in
/// [`driver::drive_spec`], and a consumer calling this one directly calls
/// [`validation::validate`] itself if it wants them.
///
/// `workload` is accepted and not used. The orchestrator does not run the
/// workload; each worker subprocess builds its own from its own `main`, so the
/// value passed here reaches nothing. It stays in the signature because a
/// consumer holds one anyway and removing it would churn every call site for
/// no gain, but a workload passed here and a different one built in the
/// worker arm is not a configuration, it is a silent disagreement.
pub fn run(
    config: &BenchConfig,
    routine: &RoutineSpec,
    workload: &Workload,
) -> Result<BenchResult, BenchError> {
    harness::run_orchestrator(config, routine, workload)
}

/// Build a [`DataSet`] from a [`BenchResult`] for the given mode
/// (`"warm"` / `"cold"`), generate the markdown report via
/// [`generate_report`], and write it to `path`.
///
/// `mode` selects which subset of samples feeds the analysis.
/// Mockspace consumers typically call this twice (once per mode) and
/// emit `findings_warm.md` + `findings_cold.md`, or pick the mode
/// most representative of their workload.
///
/// **Throughput tables**: this variant does not auto-fill
/// `DataSet.meta.ops_per_call`, so the report skips the throughput /
/// Gops/s rows. Use [`write_report_for_routine`] to get throughput
/// tables filled from the Routine's `ops_per_call` declaration.
pub fn write_report(result: &BenchResult, mode: &str, path: &str) -> Result<(), BenchError> {
    let ds = result.dataset(mode);
    let md = generate_report(&ds, &result.title);
    std::fs::write(path, md).map_err(|e| BenchError::io("writing findings.md", e))?;
    Ok(())
}

/// Routine-aware [`write_report`]: auto-fills `meta.ops_per_call`
/// from the [`RoutineSpec`] bridge so `findings.md` includes
/// throughput / Gops/s tables when the routine declares ops.
pub fn write_report_for_routine(
    result: &BenchResult,
    routine: &RoutineSpec,
    mode: &str,
    path: &str,
) -> Result<(), BenchError> {
    let ds = result.dataset_for_routine(routine, mode);
    let md = generate_report(&ds, &result.title);
    std::fs::write(path, md).map_err(|e| BenchError::io("writing findings.md", e))?;
    Ok(())
}

/// Generate `findings.md` from a CSV cache file without re-running
/// the orchestrator (#604: v1-parity bench findings markdown
/// generator).
///
/// Loads samples from `csv_path` via [`load_samples_csv`], wraps
/// them in a synthetic [`BenchResult`] carrying the given `title`,
/// and renders the markdown report for `mode` (typically `"warm"`
/// or `"cold"`) to `output_path`.
///
/// The synthetic result carries the CSV path as its `cache_path`
/// and an empty [`EnvMeta`] (the original environment metadata is
/// not preserved through the CSV cache today; reports generated
/// from this path show "(env metadata unavailable)" in the
/// methodology section). For full environment metadata, run the
/// orchestrator via [`run`] and feed the [`BenchResult`] through
/// [`write_report`] directly.
///
/// Equivalent throughput-aware variant: [`report_from_csv_for_routine`].
pub fn report_from_csv(
    csv_path: &std::path::Path,
    output_path: &str,
    mode: &str,
    title: &str,
) -> Result<(), BenchError> {
    let samples = sample::load_samples_csv(csv_path)?;
    let result = BenchResult {
        title: title.to_string(),
        env: env::EnvMeta::default(),
        samples,
        cache_path: csv_path.display().to_string(),
        report_path: output_path.to_string(),
    };
    write_report(&result, mode, output_path)
}

/// Routine-aware [`report_from_csv`]: includes throughput / Gops/s
/// tables when the routine declares ops via its
/// [`mockspace_bench_core::RoutineBridge`].
pub fn report_from_csv_for_routine(
    csv_path: &std::path::Path,
    output_path: &str,
    mode: &str,
    title: &str,
    routine: &RoutineSpec,
) -> Result<(), BenchError> {
    let samples = sample::load_samples_csv(csv_path)?;
    let result = BenchResult {
        title: title.to_string(),
        env: env::EnvMeta::default(),
        samples,
        cache_path: csv_path.display().to_string(),
        report_path: output_path.to_string(),
    };
    write_report_for_routine(&result, routine, mode, output_path)
}
