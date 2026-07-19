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

mod index;
mod worker;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use index::{SummaryRow, fmt_ns, write_index};
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

/// Resolve the routine for one config: custom hook first, then the
/// declared byte dispatch.
pub(super) fn resolve_routine(
    registry: &DriverRegistry,
    config: &BenchConfig,
) -> Result<RoutineSpec, BenchError> {
    if let Some(spec) = (registry.routine_for)(config) {
        return Ok(spec);
    }
    match (registry.byte_dispatch.dispatch)(config.n, config.may_differ) {
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
                 dispatch list is {:?}. Add {} to the `byte_routine_dispatch!` \
                 declaration in your bench binary (each size is its own \
                 monomorphisation by design), or serve it from `routine_for`.",
                    config.bench_name, config.n, registry.byte_dispatch.sizes, config.n
                ),
            })
        },
    }
}

/// Per-config output paths under `results/<bench>/`.
fn output_paths(config: &BenchConfig) -> (PathBuf, String, String) {
    let dir = Path::new(RESULTS_DIR).join(&config.bench_name);
    let csv = dir
        .join(format!("{}_n{}.csv", config.bench_name, config.n))
        .display()
        .to_string();
    let findings = dir
        .join(format!("{}_n{}_findings.md", config.bench_name, config.n))
        .display()
        .to_string();
    (dir, csv, findings)
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

fn median(vals: &mut [f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    vals[vals.len() / 2]
}

/// The library-provided bench driver entry point. See the module docs.
pub fn drive(registry: &DriverRegistry) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cli = parse_cli(&args);

    if cli.worker {
        return drive_worker(registry, &cli);
    }

    let manifest = match BenchManifest::load(Path::new("bench.toml")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    // ── selection ──
    let all_names = manifest.bench_names();
    let selected: Vec<String> = if cli.only.is_empty() {
        all_names.clone()
    } else {
        let mut sel = Vec::new();
        for name in &cli.only {
            if all_names.contains(name) {
                sel.push(name.clone());
            } else {
                eprintln!(
                    "error: bench `{name}` not found in bench.toml. Available: {}",
                    all_names.join(", ")
                );
                return ExitCode::FAILURE;
            }
        }
        sel
    };

    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .unwrap_or_else(|_| PathBuf::from("."));

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

    // ── run plan ──
    eprintln!(
        "run plan: {} bench(es), {} config(s){}",
        selected.len(),
        configs.len(),
        if cli.report_only { " [report-only]" } else { "" }
    );

    let mut summary: Vec<SummaryRow> = Vec::new();
    let mut required_failure = false;
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

        let routine = match resolve_routine(registry, config) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        };
        let workload = (registry.build_workload)(&config.workload, config.n);
        let (dir, csv_path, findings_path) = output_paths(config);
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
        if let Err(e) = harness::write_csv(&result, &csv_path) {
            eprintln!("error: writing csv: {e}");
            return ExitCode::FAILURE;
        }
        if let Err(e) = crate::write_report_for_routine(&result, &routine, "warm", &findings_path) {
            eprintln!("error: writing report: {e}");
            return ExitCode::FAILURE;
        }

        // ── history + regressions + summary rows ──
        let benchmark_key = format!("{}_n{}", config.bench_name, config.n);
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
        let historical = history::load(&benchmark_key);
        let regressions = history::detect_regressions(&entries, &historical);
        for (bench, variant, delta, flagged) in &regressions {
            if *flagged {
                eprintln!(
                    "  REGRESSION: {bench} {variant} {:+.1}% vs history",
                    delta * 100.0
                );
            }
        }
        if let Err(e) = history::append(&benchmark_key, &entries) {
            eprintln!("  history append failed: {e}");
        }
        for e in &entries {
            let flagged = regressions.iter().any(|(_, v, _, f)| *f && v == &e.variant);
            summary.push(SummaryRow {
                bench:         config.bench_name.clone(),
                n:             config.n,
                variant:       e.variant.clone(),
                median_ns:     e.median_ns,
                ratio_vs_best: if best > 0.0 { e.median_ns / best } else { 1.0 },
                regression:    flagged,
            });
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
    if let Err(e) = write_index(&summary) {
        eprintln!("  index write failed: {e}");
    }

    let wall = started.elapsed().as_secs_f64();
    eprintln!("\ntotal: {wall:.1}s");

    if required_failure {
        eprintln!("FAILED: a `required = true` bench dropped variants in validation");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
