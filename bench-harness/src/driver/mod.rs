//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Manifest-driven bench driver: the whole generic main loop, in the
//! library.
//!
//! Every consumer bench binary used to own a copy of the same loop
//! (load bench.toml, iterate benches and sizes, bridge routines,
//! spawn the orchestrator, write CSV plus findings), and the copies
//! drifted: hand-grown size whitelists, hardcoded may-differ name
//! lists, stale helper snapshots. [`drive`] owns that loop once. A
//! consumer `main` shrinks to its registrations:
//!
//! ```ignore
//! fn main() -> std::process::ExitCode {
//!     mockspace_bench_harness::driver::drive(&DriverRegistry {
//!         build_workload,
//!         routine_for,
//!         byte_dispatch: byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 16384]),
//!     })
//! }
//! ```
//!
//! The driver handles: bench-name filtering (`--only` or positional
//! names), report-only regeneration, a preflight that reports every
//! missing variant dylib at once, master-seed resolution with replay
//! (`--seed`), pre-run validation feeding the `required` manifest
//! flag, per-bench organised output directories, history append plus
//! regression detection, an end-of-run summary table, and a findings
//! index.

pub mod hooks;
mod index;
mod worker;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

pub use hooks::{AfterCell, CellVerdict, Hooks, InitContext, InitVerdict, RunPlan};
use index::{SummaryRow, fmt_ns, write_index};

mod staging;
use worker::drive_worker;

use crate::analysis::bootstrap_ci_median;
use crate::config::{BenchConfig, BenchManifest};
use crate::core::ByteDispatch;
use crate::error::BenchError;
use crate::history::{self, HistoryEntry};
use crate::sample::BenchResult;
use crate::spec::RoutineSpec;
use crate::workload::Workload;
use crate::{harness, validation};

/// Root directory (relative to `mock/benches/`) for organised
/// per-bench outputs.
pub const RESULTS_DIR: &str = "results";

/// Consumer registrations the driver dispatches through.
pub struct DriverRegistry {
    /// Build the workload program for `(workload_name, n)`.
    pub build_workload: fn(&str, usize) -> Workload,
    /// Custom routine hook for benches whose inputs are not plain
    /// bytes (graph shapes, sparse layouts). Return `None` to fall
    /// through to the byte dispatch.
    pub routine_for:    fn(&BenchConfig) -> Option<RoutineSpec>,
    /// The declared const byte-size dispatch
    /// (`byte_routine_dispatch!`). Manifest sizes must be members of
    /// its list; the driver errors by name otherwise.
    pub byte_dispatch:  ByteDispatch,
}

/// The full driver specification: the registrations plus the named
/// hooks. [`DriverRegistry`] remains the hook-less compat surface;
/// `drive` adapts it onto this.
pub struct DriverSpec {
    /// Build the workload program for `(workload_name, n)`.
    pub build_workload: fn(&str, usize) -> Workload,
    /// The declared const byte-size dispatch
    /// (`byte_routine_dispatch!`).
    pub byte_dispatch:  ByteDispatch,
    /// The consumer hooks. See [`Hooks`] for the ordering contract.
    pub hooks:          Hooks,
}

pub(super) struct Cli {
    pub(super) worker:        bool,
    pub(super) report_only:   bool,
    pub(super) only:          Vec<String>,
    pub(super) seed_override: Option<u64>,
    pub(super) raw:           Vec<String>,
}

fn parse_cli(args: &[String]) -> Cli {
    let mut only = Vec::new();
    let mut seed_override = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--only" => {
                if let Some(v) = args.get(i + 1) {
                    only.push(v.clone());
                    i += 1;
                }
            },
            "--seed" => {
                if let Some(v) = args.get(i + 1) {
                    seed_override = parse_seed(v);
                    i += 1;
                }
            },
            a if !a.starts_with("--") => only.push(a.to_string()),
            _ => {},
        }
        i += 1;
    }
    Cli {
        worker: args.iter().any(|a| a == "--worker"),
        report_only: args.iter().any(|a| a == "--report-only"),
        only,
        seed_override,
        raw: args.to_vec(),
    }
}

fn parse_seed(s: &str) -> Option<u64> {
    let t = s.replace('_', "");
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u64>().ok()
    }
}

/// Resolve the routine for one config: the `routine_for` hook first,
/// then the declared byte dispatch.
pub(super) fn resolve_routine(
    spec: &DriverSpec,
    config: &BenchConfig,
) -> Result<RoutineSpec, BenchError> {
    if let Some(found) = spec.hooks.routine_for.and_then(|h| h(config)) {
        return Ok(found);
    }
    match (spec.byte_dispatch.dispatch)(config.n, config.may_differ) {
        Some(bridge) => {
            Ok(RoutineSpec {
                name: config.workload.clone(),
                bridge,
            })
        },
        None => {
            Err(BenchError::InvalidConfig {
                reason: format!(
                    "bench `{}` n={} has no monomorphised byte routine: the compiled \
                 dispatch list is {:?}. With the generated driver, add {} to \
                 `[dispatch] points` in bench.toml (or leave `[dispatch]` out so the \
                 list defaults to the union of every bench's points). With a \
                 consumer-owned driver, add it to the `byte_routine_dispatch!` \
                 declaration, or serve it from `routine_for`.",
                    config.bench_name, config.n, spec.byte_dispatch.sizes, config.n
                ),
            })
        },
    }
}

/// Per-config output paths under `<root>/<bench>/`. During a run
/// `root` is the in-flight staging tree; report-only reads the
/// canonical `results/` directly.
///
/// A flat-tree cell keeps the historical `<bench>_n<n>_findings.md`
/// naming so committed artifacts and their citations stay reachable;
/// a nested-tree cell writes `<sweep>_n<point>_report.md` under its
/// bench's directory.
fn output_paths(config: &BenchConfig, root: &Path) -> (PathBuf, String, String) {
    let dir = root.join(&config.bench);
    let stem = format!("{}_n{}", config.sweep, config.n);
    let report_suffix = if config.nested { "_report.md" } else { "_findings.md" };
    let csv = dir.join(format!("{stem}.csv")).display().to_string();
    let report = dir
        .join(format!("{stem}{report_suffix}"))
        .display()
        .to_string();
    (dir, csv, report)
}

/// Load the manifest for a bench tree root: the root file plus every
/// declared or defaulted benchspace member, composed. A tree with no
/// members loads as its flat root manifest unchanged.
fn load_manifest(root: &Path) -> Result<BenchManifest, BenchError> {
    crate::tree::load(root).map(|t| t.manifest)
}

/// Match the requested names against the manifest keys. A request
/// matches its exact key, and in a nested tree a bench name selects
/// every sweep of that bench (`warm-container` selects
/// `warm-container/width-l1`, `warm-container/width-l2`, ...).
fn select_names(all: &[String], requested: &[String]) -> Result<Vec<String>, String> {
    if requested.is_empty() {
        return Ok(all.to_vec());
    }
    let mut selected: Vec<String> = Vec::new();
    for name in requested {
        let prefix = format!("{name}/");
        let matches: Vec<&String> = all
            .iter()
            .filter(|k| *k == name || k.starts_with(&prefix))
            .collect();
        if matches.is_empty() {
            return Err(format!(
                "bench `{name}` not found in bench.toml. Available: {}",
                all.join(", ")
            ));
        }
        for m in matches {
            if !selected.contains(m) {
                selected.push(m.clone());
            }
        }
    }
    Ok(selected)
}

/// The history root and key for one cell. A nested cell's ledger is
/// `history/<bench>/<sweep>_n<point>.tsv`; a flat cell keeps the
/// historical `.bench_history/<bench>_n<n>.tsv`.
fn history_root_and_key(config: &BenchConfig, root: &Path) -> (PathBuf, String) {
    if config.nested {
        (
            root.join("history").join(&config.bench),
            format!("{}_n{}", config.sweep, config.n),
        )
    } else {
        (
            root.join(history::DEFAULT_HISTORY_DIR),
            format!("{}_n{}", config.bench_name, config.n),
        )
    }
}

/// Compute warm-mode medians per variant from a result.
fn warm_medians(result: &BenchResult) -> BTreeMap<String, Vec<f64>> {
    let mut per_variant: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for s in &result.samples {
        if s.mode == "warm" {
            per_variant
                .entry(s.variant.clone())
                .or_default()
                .push(s.algo_ns);
        }
    }
    per_variant
}

/// The crate's one median, over a vector the caller owns. Delegates to
/// [`crate::analysis::median`] so the summary table, the history ledger and the
/// findings report cannot disagree about what "median" means on an even count.
fn median(vals: &mut [f64]) -> f64 {
    crate::analysis::median(vals)
}

/// The hook-less compat entry point: adapts a [`DriverRegistry`]
/// onto [`drive_spec`]. Existing consumer binaries keep building and
/// keep their behaviour; `routine_for` becomes the hook of the same
/// name.
pub fn drive(registry: &DriverRegistry) -> ExitCode {
    drive_spec(&DriverSpec {
        build_workload: registry.build_workload,
        byte_dispatch:  registry.byte_dispatch,
        hooks:          Hooks {
            routine_for: Some(registry.routine_for),
            ..Hooks::default()
        },
    })
}

/// The library-provided bench driver entry point. See the module and
/// [`Hooks`] docs for the hook ordering contract.
pub fn drive_spec(spec: &DriverSpec) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cli = parse_cli(&args);

    if cli.worker {
        return drive_worker(spec, &cli);
    }

    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .unwrap_or_else(|_| PathBuf::from("."));
    drive_parsed(spec, &cwd, &cli)
}

/// The drive loop against an explicit tree root, so it is callable
/// without owning the process cwd. `root` is the benches directory.
fn drive_parsed(spec: &DriverSpec, root: &Path, cli: &Cli) -> ExitCode {
    let manifest = match load_manifest(root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    // ── on_init: the manifest exists, nothing is resolved yet ──
    if let Err(reason) = hooks::run_on_init(&spec.hooks, &InitContext {
        manifest:    &manifest,
        requested:   &cli.only,
        report_only: cli.report_only,
    }) {
        eprintln!("error: on_init aborted the run: {reason}");
        return ExitCode::FAILURE;
    }

    // ── selection ──
    let all_names = manifest.bench_names();
    let selected = match select_names(&all_names, &cli.only) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    let cwd = root.to_path_buf();

    // ── resolve configs + preflight ──
    let mut configs: Vec<BenchConfig> = Vec::new();
    for name in &selected {
        let section = &manifest.bench[name];
        for idx in 0 .. section.sizes.len() {
            match manifest.for_size(name, idx, &cwd) {
                Ok(mut c) => {
                    if let Some(seed) = cli.seed_override {
                        c.master_seed = seed;
                    }
                    configs.push(c);
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                },
            }
        }
    }
    if configs.is_empty() {
        eprintln!("error: nothing selected (no bench/size entries)");
        return ExitCode::FAILURE;
    }

    if !cli.report_only {
        let mut missing: Vec<String> = Vec::new();
        for c in &configs {
            for p in &c.variant_paths {
                if !p.exists() {
                    missing.push(format!(
                        "  {} (bench `{}` n={})",
                        p.display(),
                        c.bench_name,
                        c.n
                    ));
                }
            }
        }
        if !missing.is_empty() {
            eprintln!(
                "error: {} variant dylib(s) missing. Build them (`cargo mock bench run` \
                 does this) or fix the bench.toml entries:\n{}",
                missing.len(),
                missing.join("\n")
            );
            return ExitCode::FAILURE;
        }
    }

    // ── transactional results: quarantine crash-borne trees, then
    // stage this run's outputs; promotion happens only on orderly
    // completion (see the staging module docs) ──
    let results_root = root.join(RESULTS_DIR);
    let stage_root: Option<PathBuf> = if cli.report_only {
        None
    } else {
        staging::quarantine_stale(&results_root);
        match staging::create_stage_root(&results_root) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("error: creating staging root: {e}");
                return ExitCode::FAILURE;
            },
        }
    };

    // ── run plan ──
    eprintln!(
        "run plan: {} bench(es), {} config(s){}",
        selected.len(),
        configs.len(),
        if cli.report_only { " [report-only]" } else { "" }
    );

    // ── after_init: cells resolved, preflight passed, staging
    // exists; nothing measured ──
    hooks::run_after_init(&spec.hooks, &RunPlan {
        cells:        &configs,
        report_only:  cli.report_only,
        results_root: &results_root,
    });

    let mut summary: Vec<SummaryRow> = Vec::new();
    // History appends are deferred to after promotion so the
    // regression log never records a crash-borne run.
    let mut deferred_history: Vec<(PathBuf, String, Vec<HistoryEntry>)> = Vec::new();
    let mut required_failure = false;
    let mut hook_failure = false;
    let total = configs.len();
    let started = Instant::now();

    for (idx, config) in configs.iter().enumerate() {
        let elapsed = started.elapsed().as_secs_f64();
        let eta = if idx > 0 {
            let per = elapsed / idx as f64;
            format!(", eta {:.0}s", per * (total - idx) as f64)
        } else {
            String::new()
        };
        eprintln!(
            "[{}/{}] {} n={} (elapsed {:.0}s{})",
            idx + 1,
            total,
            config.bench_name,
            config.n,
            elapsed,
            eta
        );

        let routine = match resolve_routine(spec, config) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        };
        let workload = (spec.build_workload)(&config.workload, config.n);
        let out_root = stage_root.as_deref().unwrap_or(results_root.as_path());
        let (dir, csv_path, findings_path) = output_paths(config, out_root);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("error: creating {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }

        if cli.report_only {
            match crate::report_from_csv_for_routine(
                Path::new(&csv_path),
                &findings_path,
                "warm",
                &config.title,
                &routine,
            ) {
                Ok(()) => eprintln!("  regenerated {findings_path}"),
                Err(e) => {
                    eprintln!(
                        "error: report-only for `{}` n={}: {e}\nhint: run the bench \
                         first to produce {}",
                        config.bench_name, config.n, csv_path
                    );
                    return ExitCode::FAILURE;
                },
            }
            continue;
        }

        // ── master-seed resolution (replayable) ──
        let mut config = config.clone();
        if config.master_seed == 0 {
            let random = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9)
                | 1;
            config.master_seed = random;
            eprintln!("  master seed: {random:#x} (replay with --seed {random:#x})");
        }

        // ── pre-run validation (feeds `required`) ──
        let mut dropped: Vec<String> = Vec::new();
        if config.variant_paths.len() >= 2 {
            let path_strings: Vec<String> = config
                .variant_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            // Disassembly dup-check before timing: two variants that compile to
            // identical machine code will benchmark identically, so a measured
            // "difference" between them would be pure noise. This is a fairness
            // guard (it catches a comparison that is secretly against itself, e.g.
            // an axis value that the optimizer collapsed into another), and it is
            // cheap (one objdump per variant, once, before the run). Warn-only: a
            // legitimately-identical pair is the bench's call, not a hard error.
            crate::disasm::check_duplicates(&path_strings);
            match validation::validate(
                &routine,
                &path_strings,
                config.n,
                &config.bench_name,
                config.max_call_us,
                Some(&config.tuning),
            ) {
                Ok(survivors) => {
                    for p in &path_strings {
                        if !survivors.contains(p) {
                            dropped.push(p.clone());
                        }
                    }
                    if !dropped.is_empty() {
                        eprintln!(
                            "  VALIDATION: {} variant(s) dropped: {}",
                            dropped.len(),
                            dropped.join(", ")
                        );
                        config.variant_paths = survivors.iter().map(PathBuf::from).collect();
                    }
                },
                Err(e) => {
                    eprintln!("  VALIDATION ERROR: {e}");
                    dropped.push(format!("(validation error: {e})"));
                },
            }
            if config.required && !dropped.is_empty() {
                required_failure = true;
            }
            if config.variant_paths.is_empty() {
                eprintln!(
                    "  SKIPPED: every variant of `{}` n={} failed validation; \
                     nothing to measure",
                    config.bench_name, config.n
                );
                continue;
            }
        } else {
            // A single-variant bench has nothing to cross-validate
            // against; `validate` requires two, so it is skipped and
            // the `required` flag has no validation to act on here.
        }

        // ── timed run ──
        let result = match harness::run_orchestrator(&config, &routine, &workload) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: bench `{}` n={}: {e}", config.bench_name, config.n);
                if config.required {
                    required_failure = true;
                }
                continue;
            },
        };
        // Samples are labelled by the variant's exported `bench_name`, and
        // every grouping downstream keys on that label. Two dylibs exporting
        // one name therefore merge into a single arm carrying samples from
        // both, and the median that comes out is internally consistent and
        // describes neither of them, which is the worst shape a wrong number
        // can take. The manifest cannot catch this: the name lives in the
        // dylib, not in the path.
        {
            let mut labels: Vec<&str> = result.samples.iter().map(|s| s.variant.as_str()).collect();
            labels.sort_unstable();
            labels.dedup();
            if labels.len() < config.variant_paths.len() {
                eprintln!(
                    "error: bench `{}` n={} ran {} variants whose samples carry only \
                     {} distinct names ({}); two of them export the same `bench_name`, \
                     so their samples merged and the reported median would describe \
                     neither. Give each variant its own name.",
                    config.bench_name,
                    config.n,
                    config.variant_paths.len(),
                    labels.len(),
                    labels.join(", "),
                );
                return ExitCode::FAILURE;
            }
        }
        if let Err(e) = harness::write_csv(&result, &csv_path) {
            eprintln!("error: writing csv: {e}");
            return ExitCode::FAILURE;
        }
        {
            // Report generation, normalised against the declared baseline
            // variant when `[bench.<name>.normalise]` is set (otherwise the
            // default first-variant baseline). Selecting the baseline makes
            // every rendered delta (the % column and the paired absolute
            // Δ-median-ns + CI in the statistical comparison table) relative
            // to it, which cancels the shared common cost.
            let mut ds = result
                .dataset_for_routine(&routine, "warm")
                .with_methodology(&config);
            if let Some(bl) = config.normalise_baseline.as_deref() {
                ds = ds.with_baseline(bl);
            }
            if let Some(mode) = config.normalise_mode.as_deref() {
                ds = ds.with_normalise_mode(mode);
            }
            if let Some(floor) = config.normalise_floor.as_deref() {
                ds = ds.with_floor(floor);
            }
            let md = crate::generate_report(&ds, &result.title);
            if let Err(e) = std::fs::write(&findings_path, md) {
                eprintln!("error: writing report: {e}");
                return ExitCode::FAILURE;
            }
            // Per-bench stdout highlights: the same detector engine as the
            // report, but headlines-only, naming the baseline and ending by
            // directing the reader to the full findings file. Readers who see
            // only stdout still get the computed verdict, not swamped raw rows.
            if ds.variants.len() > 1 {
                let rs = crate::summary::summarise(&ds, &result.title, config.master_seed);
                eprint!("{}", rs.render_terminal(&findings_path));
            }
        }

        // ── after_cell: artifacts staged, drops final, nothing
        // promoted or appended yet; a Fail withholds the ledger ──
        let cell_failed = hooks::run_after_cell(&spec.hooks, &AfterCell {
            config:    &config,
            result:    &result,
            arm_paths: &config.variant_paths,
            dropped:   &dropped,
            out_dir:   &dir,
        });
        if cell_failed {
            hook_failure = true;
        }

        // ── history + regressions + summary rows ──
        let (history_root, benchmark_key) = history_root_and_key(&config, root);
        let per_variant = warm_medians(&result);
        let mut best = f64::INFINITY;
        let mut entries: Vec<HistoryEntry> = Vec::new();
        for (variant, vals) in &per_variant {
            let mut v = vals.clone();
            let m = median(&mut v);
            if m < best {
                best = m;
            }
            let (_, lo, hi) = bootstrap_ci_median(vals, config.master_seed);
            entries.push(HistoryEntry {
                timestamp:  history::timestamp(),
                git_commit: history::git_commit(),
                benchmark:  benchmark_key.clone(),
                variant:    variant.clone(),
                n:          config.n,
                mode:       "warm".into(),
                median_ns:  m,
                ci_lo_ns:   lo,
                ci_hi_ns:   hi,
            });
        }
        let historical = history::load_in(&history_root, &benchmark_key);
        let regressions = history::detect_regressions(&entries, &historical);
        for r in &regressions {
            if r.flagged {
                eprintln!("  REGRESSION: {}", r.render(&config.bench_name));
            }
        }
        for e in &entries {
            summary.push(SummaryRow {
                bench:         config.bench_name.clone(),
                n:             config.n,
                variant:       e.variant.clone(),
                median_ns:     e.median_ns,
                ratio_vs_best: if best > 0.0 { e.median_ns / best } else { 1.0 },
                regression:    history::flagged_for(&regressions, &e.variant),
            });
        }
        if cell_failed {
            // A gated-out run must not become the next run's
            // regression baseline: the ledger records accepted runs.
            // The staged artifacts still promote as the evidence of
            // what ran and why it was failed.
            eprintln!("  history withheld for {benchmark_key}: after_cell returned Fail");
        } else {
            deferred_history.push((history_root, benchmark_key, entries));
        }
    }

    // ── orderly completion: promote staged results, then history ──
    if let Some(stage) = &stage_root {
        if let Err(e) = staging::promote(&results_root, stage) {
            eprintln!("error: promoting staged results: {e}");
            return ExitCode::FAILURE;
        }
        for (hroot, key, entries) in &deferred_history {
            if let Err(e) = history::append_in(hroot, key, entries) {
                eprintln!("  history append failed: {e}");
            }
        }
    }

    // ── summary table + findings index ──
    if !cli.report_only && !summary.is_empty() {
        eprintln!();
        eprintln!(
            "{:<24} {:>10} {:<20} {:>12} {:>8}",
            "bench", "n", "variant", "median", "vs best"
        );
        for row in &summary {
            eprintln!(
                "{:<24} {:>10} {:<20} {:>12} {:>7.2}x{}",
                row.bench,
                row.n,
                row.variant,
                fmt_ns(row.median_ns),
                row.ratio_vs_best,
                if row.regression { "  REGRESSION" } else { "" }
            );
        }
    }
    if let Err(e) = write_index(&results_root, &summary) {
        eprintln!("  index write failed: {e}");
    }

    let wall = started.elapsed().as_secs_f64();
    eprintln!("\ntotal: {wall:.1}s");

    final_exit(required_failure, hook_failure)
}

/// Fold the two failure classes into the process exit. A `required`
/// validation drop and an `after_cell` `Fail` verdict each fail the
/// run; both are reported after promotion so the staged results and
/// the ledger reflect what actually ran.
fn final_exit(required_failure: bool, hook_failure: bool) -> ExitCode {
    if required_failure {
        eprintln!("FAILED: a `required = true` bench dropped variants in validation");
        return ExitCode::FAILURE;
    }
    if hook_failure {
        eprintln!("FAILED: an after_cell hook returned Fail");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
pub(crate) fn median_for_tests(vals: &mut [f64]) -> f64 {
    median(vals)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use super::*;

    fn cfg(bench: &str, sweep: &str, n: usize, nested: bool) -> BenchConfig {
        BenchConfig {
            bench_name: if nested && bench != sweep {
                format!("{bench}/{sweep}")
            } else {
                bench.to_string()
            },
            bench: bench.to_string(),
            sweep: sweep.to_string(),
            nested,
            n,
            ..BenchConfig::default()
        }
    }

    #[test]
    fn flat_output_naming_is_unchanged() {
        let (dir, csv, report) =
            output_paths(&cfg("hash", "hash", 64, false), Path::new("results"));
        assert_eq!(dir, Path::new("results/hash"));
        assert!(csv.ends_with("results/hash/hash_n64.csv"), "{csv}");
        assert!(
            report.ends_with("results/hash/hash_n64_findings.md"),
            "flat trees keep the committed suffix: {report}"
        );
    }

    #[test]
    fn nested_output_naming_uses_the_sweep_stem_and_the_report_suffix() {
        let c = cfg("warm-container", "width-l1", 80003, true);
        let (dir, csv, report) = output_paths(&c, Path::new("results"));
        assert_eq!(dir, Path::new("results/warm-container"));
        assert!(
            csv.ends_with("results/warm-container/width-l1_n80003.csv"),
            "{csv}"
        );
        assert!(
            report.ends_with("results/warm-container/width-l1_n80003_report.md"),
            "{report}"
        );
    }

    #[test]
    fn history_partitions_per_bench_in_nested_trees_and_stays_put_in_flat_ones() {
        let root = Path::new("/tree");
        let (hroot, key) = history_root_and_key(&cfg("warm", "width-l1", 80003, true), root);
        assert_eq!(hroot, Path::new("/tree/history/warm"));
        assert_eq!(key, "width-l1_n80003");
        let (hroot, key) = history_root_and_key(&cfg("hash", "hash", 64, false), root);
        assert_eq!(hroot, Path::new("/tree/.bench_history"));
        assert_eq!(key, "hash_n64");
    }

    #[test]
    fn each_failure_class_fails_the_exit_alone() {
        let f = failure();
        assert_eq!(format!("{:?}", final_exit(true, false)), f);
        assert_eq!(format!("{:?}", final_exit(false, true)), f);
        assert_eq!(
            format!("{:?}", final_exit(false, false)),
            format!("{:?}", ExitCode::SUCCESS)
        );
    }

    #[test]
    fn selection_matches_exact_keys_and_bench_prefixes() {
        let all: Vec<String> = ["hash", "warm/density-w13", "warm/width-l1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(select_names(&all, &[]).unwrap(), all);
        assert_eq!(select_names(&all, &["warm".to_string()]).unwrap(), vec![
            "warm/density-w13".to_string(),
            "warm/width-l1".to_string()
        ]);
        assert_eq!(
            select_names(&all, &["warm/width-l1".to_string()]).unwrap(),
            vec!["warm/width-l1".to_string()]
        );
        let err = select_names(&all, &["nope".to_string()]).unwrap_err();
        assert!(err.contains("Available"), "lists what exists: {err}");
        assert!(err.contains("warm/width-l1"), "{err}");
    }

    // ── hook ordering, end to end against a temp tree ──

    fn failure() -> String {
        format!("{:?}", ExitCode::FAILURE)
    }

    fn spec_with(hooks: Hooks) -> DriverSpec {
        fn workload(_: &str, _: usize) -> Workload {
            Workload::new()
        }
        fn no_dispatch(_: usize, _: bool) -> Option<crate::core::RoutineBridge> {
            None
        }
        DriverSpec {
            build_workload: workload,
            byte_dispatch: crate::core::ByteDispatch {
                dispatch: no_dispatch,
                sizes:    &[],
            },
            hooks,
        }
    }

    fn temp_tree(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mockspace-driver-test-{}-{name}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("bench.toml"),
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["missing-arm/lib.dylib"]
            sizes = [64]
        "#,
        )
        .unwrap();
        root
    }

    static ORDER_A: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    #[test]
    fn on_init_runs_before_preflight_and_after_init_does_not_survive_a_failed_preflight() {
        fn log_on_init(_: &InitContext<'_>) -> InitVerdict {
            ORDER_A.lock().unwrap().push("on_init");
            InitVerdict::Proceed
        }
        fn log_after_init(_: &RunPlan<'_>) {
            ORDER_A.lock().unwrap().push("after_init");
        }
        let root = temp_tree("preflight");
        let spec = spec_with(Hooks {
            on_init: Some(log_on_init),
            after_init: Some(log_after_init),
            ..Hooks::default()
        });
        let cli = Cli {
            worker:        false,
            report_only:   false,
            only:          Vec::new(),
            seed_override: None,
            raw:           Vec::new(),
        };
        let code = drive_parsed(&spec, &root, &cli);
        assert_eq!(
            format!("{code:?}"),
            failure(),
            "the missing dylib fails preflight"
        );
        assert_eq!(
            *ORDER_A.lock().unwrap(),
            vec!["on_init"],
            "after_init must not fire when init never completed"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    static ORDER_B: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    #[test]
    fn after_init_fires_once_init_completes_and_before_any_cell() {
        fn log_on_init(ctx: &InitContext<'_>) -> InitVerdict {
            // the payload really is the pre-resolution manifest
            assert!(ctx.manifest.bench.contains_key("b"));
            ORDER_B.lock().unwrap().push("on_init");
            InitVerdict::Proceed
        }
        fn log_after_init(plan: &RunPlan<'_>) {
            assert_eq!(plan.cells.len(), 1, "cells are resolved by now");
            ORDER_B.lock().unwrap().push("after_init");
        }
        let root = temp_tree("report-only");
        let spec = spec_with(Hooks {
            on_init: Some(log_on_init),
            after_init: Some(log_after_init),
            ..Hooks::default()
        });
        // report-only: no preflight, so init completes; the first
        // cell then fails on the missing csv, after after_init.
        let cli = Cli {
            worker:        false,
            report_only:   true,
            only:          Vec::new(),
            seed_override: None,
            raw:           Vec::new(),
        };
        let code = drive_parsed(&spec, &root, &cli);
        assert_eq!(
            format!("{code:?}"),
            failure(),
            "the missing csv fails the cell"
        );
        assert_eq!(*ORDER_B.lock().unwrap(), vec!["on_init", "after_init"]);
        std::fs::remove_dir_all(&root).ok();
    }

    static ORDER_C: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    #[test]
    fn an_on_init_abort_stops_the_run_before_selection() {
        fn veto(_: &InitContext<'_>) -> InitVerdict {
            ORDER_C.lock().unwrap().push("on_init");
            InitVerdict::Abort("not today".into())
        }
        fn log_after_init(_: &RunPlan<'_>) {
            ORDER_C.lock().unwrap().push("after_init");
        }
        let root = temp_tree("abort");
        let spec = spec_with(Hooks {
            on_init: Some(veto),
            after_init: Some(log_after_init),
            ..Hooks::default()
        });
        // the requested name does not exist, so reaching selection
        // would produce a selection error; the abort must come first
        let cli = Cli {
            worker:        false,
            report_only:   false,
            only:          vec!["no-such-bench".to_string()],
            seed_override: None,
            raw:           Vec::new(),
        };
        let code = drive_parsed(&spec, &root, &cli);
        assert_eq!(format!("{code:?}"), failure());
        assert_eq!(*ORDER_C.lock().unwrap(), vec!["on_init"]);
        std::fs::remove_dir_all(&root).ok();
    }
}
