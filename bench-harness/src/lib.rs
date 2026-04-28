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

pub mod cache;
pub mod config;
pub mod env;
pub mod error;
pub mod harness;
pub mod sample;
pub mod spec;
pub mod workload;

pub use cache::{Cache, CachedBatch, apply_drift, config_hash, consensus_drift, dylib_hash, global_mean, global_mean_for_mode};
pub use config::{BenchConfig, BenchManifest, BenchSection, SizeSection, TimingSection};
pub use env::{EnvMeta, collect_env_meta};
pub use error::BenchError;
pub use harness::{run_orchestrator, run_worker, write_csv};
pub use sample::{BenchResult, Sample};
pub use spec::{RoutineSpec, VariantSpec};
pub use workload::{
    AllocHandle, Chain, OneOf, Program, ProgramBuilder, Shuffle, Stage, StageStrategy, Workload,
    WorkloadCtx, WorkloadItemKind, algo_call, branch_work, domain_work, graph_work, heavy_memory,
    light_scalar, mix, scalar_work,
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
