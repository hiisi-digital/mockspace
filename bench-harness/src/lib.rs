//! Canonical bench harness for mockspace consumers.
//!
//! Loads variant cdylibs in subprocess isolation, drives them with
//! workload programs, collects per-batch samples, validates outputs
//! across variants, runs Pareto + multi-dim analysis, and emits
//! findings.md plus a CSV cache for historical comparison.
//!
//! ## Status
//!
//! v2 of the bench framework. v1 (`mockspace-bench-core`) shipped the
//! `Routine` trait, FFI types, hardware counter timing, and the
//! `timed!` macro. v2 adds the orchestrator (this crate). v2 is being
//! ported one round at a time on `feat/bench-harness-v2`. Round 1
//! defines the public API surface; subsequent rounds fill in workload,
//! cache, orchestrator, validation, analysis, report, sensors, history.
//!
//! ## Entry point
//!
//! Consumers invoke the harness via `mock bench run`. The CLI loads
//! `mock/benches/bench.toml` into a [`BenchManifest`], converts each
//! `(bench, size)` entry into a [`BenchConfig`], and calls [`run`]
//! once per config with the consumer-provided [`RoutineSpec`].

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
pub mod tree;
pub mod summary;
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
pub use driver::{DriverRegistry, drive};
pub use env::{EnvMeta, collect_env_meta};
pub use error::BenchError;
pub use harness::{run_orchestrator, run_worker, write_csv};
pub use history::{
    DEFAULT_HISTORY_DIR,
    HistoryEntry,
    append as append_history,
    append_in as append_history_in,
    detect_regressions,
    detect_regressions_window,
    git_commit,
    load as load_history,
    load_in as load_history_in,
    timestamp,
};
pub use inline_bench::{InlineResult, InlineVariant, run_inline};
pub use meta_report::{VariantResult, classify_family, generate as generate_meta_report};
pub use perf::{
    PerfSnapshot, available as perf_available, read as perf_read, read_all_raw as perf_read_all_raw,
    setup as perf_setup, teardown as perf_teardown,
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
/// [`RoutineSpec`] and [`Workload`].
///
/// Delegates to [`harness::run_orchestrator`]. The orchestrator
/// re-execs `std::env::current_exe()` with `--worker` flags to
/// dispatch each `(variant × cooldown × pass × mode)` combination
/// into an isolated subprocess.
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
