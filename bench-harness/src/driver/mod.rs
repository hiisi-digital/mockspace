//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Manifest-driven bench driver: the whole generic main loop, in the
//! library.
//!
//! Consumer bench binaries used to own copies of the same loop (load
//! bench.toml, iterate benches and sizes, bridge routines, spawn the
//! orchestrator, write CSV plus findings), and the copies drifted: hand-grown
//! size whitelists, hardcoded may-differ name lists, stale helper snapshots,
//! and results written wherever each copy chose. [`drive_spec`] owns that loop
//! once, and `mock bench run` generates the one binary that calls it from
//! `bench.toml`; a consumer-owned driver is refused by the tool and by a lint.
//!
//! The driver handles: bench-name filtering (`--only` or positional
//! names), report-only regeneration, a preflight that reports every
//! missing variant dylib at once, master-seed resolution with replay
//! (`--seed`), pre-run validation feeding the `required` manifest
//! flag, one run directory per cell under `results/` keyed by machine class
//! and run (see [`mockspace_bench_layout`]), regression detection against the
//! earlier runs of the same cell on the same machine class, an end-of-run
//! summary table, and the index over every run.

pub mod hooks;
mod index;
mod samples;
mod seed;
mod worker;
mod worker_args;

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
use mockspace_bench_layout::{self as layout, Cell};
use crate::sample::BenchResult;
use crate::spec::RoutineSpec;
use crate::workload::Workload;
use crate::{harness, validation};

/// Root directory (relative to `mock/benches/`) of every run.
pub use mockspace_bench_layout::RESULTS_DIR;

/// The driver specification: the registrations plus the named hooks. The
/// generated driver builds one from `bench.toml`.
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

fn parse_cli(args: &[String]) -> Result<Cli, String> {
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
                let v = args.get(i + 1).ok_or("`--seed` wants a value")?;
                seed_override = Some(seed::parse_seed(v)?);
                i += 1;
            },
            a if !a.starts_with("--") => only.push(a.to_string()),
            _ => {},
        }
        i += 1;
    }
    Ok(Cli {
        worker: args.iter().any(|a| a == "--worker"),
        report_only: args.iter().any(|a| a == "--report-only"),
        only,
        seed_override,
        raw: args.to_vec(),
    })
}


/// Resolve the routine for one config: the `routine_for` hook first,
/// then the declared byte dispatch.
pub(super) fn resolve_routine(
    spec: &DriverSpec,
    config: &BenchConfig,
) -> Result<RoutineSpec, BenchError> {
    if let Some(mut found) = spec.hooks.routine_for.and_then(|h| h(config)) {
        // A handed routine carries the flag as its own trait method, which
        // defaults to false, so without this a bench whose manifest says its
        // arms may differ is compared byte for byte anyway. Either saying so
        // is enough: the manifest widens a routine that said nothing, and
        // never narrows one that did. A routine declaring
        // `max_relative_error` has already said how far its arms may differ,
        // which is narrower than the manifest's yes and closer to the arms,
        // so its tolerance comparison stays and the manifest does not reach
        // it.
        let said_how_far = found.bridge.max_relative_error.is_some();
        found.bridge.outputs_may_differ |= config.may_differ && !said_how_far;
        return Ok(found);
    }
    // FIXME: a bench missing from `routine_for` whose own routine's `Output`
    // happens to be the byte routine's size falls through here silently and is
    // validated and scored as bytes; the preflight's size check catches only
    // a size that differs. Closing it needs the variant to name its routine
    // (an exported routine identity beside `bench_output_size`), which the
    // design does not yet say.
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
                 dispatch list is {:?}. Add {} to \
                 `[dispatch] points` in bench.toml (or leave `[dispatch]` out so the \
                 list defaults to the union of every bench's points), or serve it \
                 from the `routine_for` hook.",
                    config.bench_name, config.n, spec.byte_dispatch.sizes, config.n
                ),
            })
        },
    }
}

/// The cell a config measures.
fn cell_of(config: &BenchConfig) -> Cell {
    Cell {
        sweep: config.sweep.clone(),
        n:     config.n as u64,
    }
}

/// One cell's run directory under `root` with its samples and report paths.
/// During a run `root` is the in-flight staging tree, which promotes into
/// `results/` with the same relative paths.
fn run_paths(config: &BenchConfig, root: &Path, machine: &str, run: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = root.join(layout::run_rel(&config.bench, &cell_of(config), machine, run));
    let samples = dir.join(layout::SAMPLES_FILE);
    let report = dir.join(layout::REPORT_FILE);
    (dir, samples, report)
}

/// The runs a report-only pass renders a report for: those with none. A run's
/// own report states the methodology it ran under, which its samples cannot,
/// so one that exists is never rendered again.
fn reportless(runs: &[layout::RunDir]) -> Vec<&layout::RunDir> {
    runs.iter().filter(|r| !r.dir.join(layout::REPORT_FILE).exists()).collect()
}

/// The run id this run takes: `base`, or `base` with the first `-<k>` suffix
/// no run directory under `results_root` already carries on this machine
/// class, so two runs starting in the same second over the same commit sit side
/// by side rather than one replacing the other.
fn unique_run_id(results_root: &Path, machine: &str, base: &str) -> String {
    let taken: std::collections::BTreeSet<String> = layout::runs(results_root)
        .into_iter()
        .filter(|r| r.machine == machine)
        .map(|r| r.run)
        .collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    (2 ..)
        .map(|k| format!("{base}-{k}"))
        .find(|id| !taken.contains(id))
        .unwrap_or_else(|| base.to_string())
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
/// [`crate::analysis::median`] so the summary table, the run's summary rows and the
/// findings report cannot disagree about what "median" means on an even count.
fn median(vals: &mut [f64]) -> f64 {
    crate::analysis::median(vals)
}

/// The library-provided bench driver entry point. See the module and
/// [`Hooks`] docs for the hook ordering contract.
pub fn drive_spec(spec: &DriverSpec) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cli = match parse_cli(&args) {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

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

    // ── run identity: every cell of this run lands under one machine
    // class and one run id, a sibling of every earlier run ──
    let machine = crate::env::collect_run_env().machine_class;
    // The system every row of this run records, the target included, and the
    // tracking arms' sources `mock bench run` handed over.
    let system = crate::env::current_system(&machine);
    let sources = crate::kinds::Sources::from_env();
    let tree_root = std::env::current_dir().unwrap_or_default();
    let run = unique_run_id(
        &results_root,
        &machine,
        &layout::run_id(
            history::timestamp(),
            &crate::env::run_commit(),
        ),
    );

    // ── run plan ──
    eprintln!(
        "run plan: {} bench(es), {} config(s){}",
        selected.len(),
        configs.len(),
        if cli.report_only {
            " [report-only]".to_string()
        } else {
            format!(", run {run} on {system}")
        }
    );

    // ── after_init: cells resolved, preflight passed, staging
    // exists; nothing measured ──
    hooks::run_after_init(&spec.hooks, &RunPlan {
        cells:        &configs,
        report_only:  cli.report_only,
        results_root: &results_root,
    });

    let mut summary: Vec<SummaryRow> = Vec::new();
    let mut required_failure = false;
    let mut hook_failure = false;
    let mut refused_failure = false;
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

        if cli.report_only {
            // A run of the cell, on any machine class, that has samples and no
            // report gets one rendered from its samples. A run that has its
            // report keeps it: the report a run wrote carries the methodology
            // section, which samples alone cannot restate, so rendering it
            // again would lose what it said.
            let cell = cell_of(config);
            let cell_runs: Vec<layout::RunDir> = layout::runs(&results_root)
                .into_iter()
                .filter(|r| r.bench == config.bench && r.cell == cell)
                .filter(|r| r.dir.join(layout::SAMPLES_FILE).is_file())
                .collect();
            if cell_runs.is_empty() {
                eprintln!(
                    "error: report-only for `{}` n={}: no run of it has samples under {}\n\
                     hint: run the bench first",
                    config.bench_name,
                    config.n,
                    results_root.join(&config.bench).join(cell.name()).display()
                );
                return ExitCode::FAILURE;
            }
            for r in reportless(&cell_runs) {
                let report = r.dir.join(layout::REPORT_FILE);
                match crate::report_from_csv_for_routine(
                    &r.dir.join(layout::SAMPLES_FILE),
                    &report.display().to_string(),
                    "warm",
                    &config.title,
                    &routine,
                ) {
                    Ok(()) => eprintln!("  rendered {}", report.display()),
                    Err(e) => {
                        eprintln!("error: report-only for {}: {e}", r.dir.display());
                        return ExitCode::FAILURE;
                    },
                }
            }
            continue;
        }

        let Some(stage) = stage_root.as_deref() else {
            continue;
        };
        let (dir, _samples_path, report_path) = run_paths(config, stage, &machine, &run);
        let findings_path = report_path.display().to_string();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("error: creating {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }

        // ── master-seed resolution (replayable) ──
        let mut config = config.clone();
        // Before validation, the workers and the deadline, which all read it.
        config.max_call_us = samples::call_bound(config.max_call_us, routine.bridge.max_call_us, config.n);
        if config.master_seed == 0 {
            let random = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9)
                | 1;
            config.master_seed = random;
            eprintln!("  master seed: {random:#x} (replay with --seed {random:#x})");
        }

        // ── preflight: every variant loads and fits its routine ──
        // Before validation, which needs two variants and so never sees a
        // bench of one, and before any worker, which would run a refused
        // variant into an empty sample set. A refusal fails the run and
        // takes out only its own variant; the rest of the cell, and every
        // other cell, still runs and is promoted.
        // Each variant's source, by the name it exports: its own where `mock
        // bench run` handed one for its library path, the bench's otherwise.
        let mut variant_source: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        {
            let (n, output_size) = (config.n, routine.bridge.output_size);
            config.variant_paths.retain(|path| {
                match harness::preflight_variant(&path.display().to_string(), n, output_size) {
                    Ok(name) => {
                        let source = sources.of(&tree_root, path).unwrap_or(&config.source);
                        variant_source.insert(name, source.to_string());
                        true
                    },
                    Err(reason) => {
                        eprintln!("  REFUSED: {} :: {reason}", path.display());
                        refused_failure = true;
                        false
                    },
                }
            });
            if config.variant_paths.is_empty() {
                eprintln!(
                    "  SKIPPED: every variant of `{}` n={} was refused; nothing to measure",
                    config.bench_name, config.n
                );
                continue;
            }
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
            let outcome = validation::validate(
                &routine,
                &path_strings,
                config.n,
                &config.bench_name,
                config.max_call_us,
                Some(&config.tuning),
            );
            match &outcome {
                Ok(survivors) => {
                    let lost = path_strings.len() - survivors.len();
                    if lost > 0 {
                        eprintln!("  VALIDATION: {lost} variant(s) dropped");
                    }
                },
                Err(e) => eprintln!("  VALIDATION ERROR: {e}"),
            }
            let (timeable, lost) = timeable_after_validation(outcome.as_deref(), &path_strings);
            dropped.extend(lost);
            config.variant_paths = timeable.iter().map(PathBuf::from).collect();
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
        // A worker that failed or was killed leaves no samples and so no name
        // either, which reads exactly like a collision; no samples at all is
        // only ever that, and fewer names with some samples is either.
        {
            let labels: Vec<&str> = result.samples.iter().map(|s| s.variant.as_str()).collect();
            match samples::names(&labels, config.variant_paths.len()) {
                samples::Names::Whole => {},
                samples::Names::NoneLeft => {
                    eprintln!(
                        "error: bench `{}` n={} ran {} variants and none left a sample: \
                         every worker failed or was killed, as the lines above say.",
                        config.bench_name,
                        config.n,
                        config.variant_paths.len(),
                    );
                    return ExitCode::FAILURE;
                },
                samples::Names::Fewer {
                    names,
                } => {
                    eprintln!(
                        "error: bench `{}` n={} ran {} variants whose samples carry only \
                         {names} distinct names: a worker above failed or was killed, or \
                         two variants export the same `bench_name`, so their samples \
                         merged and the reported median would describe neither. Give each \
                         variant its own name.",
                        config.bench_name,
                        config.n,
                        config.variant_paths.len(),
                    );
                    return ExitCode::FAILURE;
                },
            }
        }
        if let Err(e) = harness::write_run(&result, &dir, &run) {
            eprintln!("error: writing the run: {e}");
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
            let md = crate::kinds::with_kind_paragraph(
                &crate::generate_report(&ds, &result.title),
                &crate::kinds::methodology(config.kind, &config.source, &result.env.git_commit, &system),
            );
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
        let cell = cell_of(&config);
        let per_variant = warm_medians(&result);
        let mut best = f64::INFINITY;
        let mut entries: Vec<HistoryEntry> = Vec::new();
        // One time for every row of the cell, so the rows of one run read as
        // one run wherever they are grouped.
        let at = history::timestamp();
        for (variant, vals) in &per_variant {
            let mut v = vals.clone();
            let m = median(&mut v);
            if m < best {
                best = m;
            }
            let (_, lo, hi) = bootstrap_ci_median(vals, config.master_seed);
            entries.push(HistoryEntry {
                timestamp:  at,
                git_commit: history::ledger_commit(&result.env.git_commit),
                benchmark:  cell.name(),
                variant:    variant.clone(),
                n:          config.n,
                mode:       "warm".into(),
                median_ns:  m,
                ci_lo_ns:   lo,
                ci_hi_ns:   hi,
                source:     variant_source.get(variant).cloned().unwrap_or_else(|| config.source.clone()),
                system:     system.clone(),
            });
        }
        // The earlier runs of this cell on this machine class; this run is
        // still staged, so it is not among them. Each row is read against its
        // own variant on its own system, by source point.
        let historical = history::load_cell(&results_root, &config.bench, &cell, &machine);
        let standing = crate::axes::standing(&entries, &historical);
        for f in &standing {
            eprintln!("  {}", f.render(&config.bench_name));
        }
        for e in &entries {
            summary.push(SummaryRow {
                bench:         config.bench_name.clone(),
                n:             config.n,
                variant:       e.variant.clone(),
                median_ns:     e.median_ns,
                ratio_vs_best: if best > 0.0 { e.median_ns / best } else { 1.0 },
                moved:         crate::axes::label_for(&standing, &e.variant),
            });
        }
        if cell_failed {
            // A gated-out run must not become the next run's regression
            // baseline: only an accepted run carries a summary. The staged
            // artifacts still promote as the evidence of what ran and why it
            // was failed.
            eprintln!("  summary withheld for {}: after_cell returned Fail", cell.name());
        } else if let Err(e) =
            harness::write_new(&dir.join(layout::SUMMARY_FILE), history::render_summary(&entries).as_bytes())
        {
            eprintln!("error: writing the run's summary: {e}");
            return ExitCode::FAILURE;
        }
    }

    // ── orderly completion: promote the staged run ──
    if let Some(stage) = &stage_root {
        if let Err(e) = staging::promote(&results_root, stage) {
            eprintln!("error: promoting staged results: {e}");
            return ExitCode::FAILURE;
        }
    }

    // ── summary table + index ──
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
                row.moved.map(|l| format!("  {l}")).unwrap_or_default()
            );
        }
    }
    if let Err(e) = write_index(&results_root) {
        eprintln!("  index write failed: {e}");
    }

    let wall = started.elapsed().as_secs_f64();
    eprintln!("\ntotal: {wall:.1}s");

    final_exit(refused_failure, required_failure, hook_failure)
}

/// Fold the three failure classes into the process exit. A variant
/// refused at preflight, a `required` validation drop and an
/// `after_cell` `Fail` verdict each fail the run; all three are
/// reported after promotion so the staged results and the ledger
/// reflect what actually ran.
fn final_exit(refused_failure: bool, required_failure: bool, hook_failure: bool) -> ExitCode {
    if refused_failure {
        eprintln!("FAILED: a variant was refused before it ran; its reason is above");
        return ExitCode::FAILURE;
    }
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

/// Which variants may be timed once validation has spoken, and which are lost.
///
/// **An `Err` yields nothing timeable.** An error means the comparison did not
/// happen at all, so no variant is known to agree with any other, and timing
/// them anyway is what puts unvalidated numbers in the corpus.
///
/// That was the defect this function exists to foreclose. The error arm used to
/// print `VALIDATION ERROR`, push a line into `dropped` and leave
/// `variant_paths` untouched; `required` defaults false, so the run did not fail
/// there either, and the `nothing to measure` branch could not fire because the
/// paths were still populated. **Every unvalidated variant was timed and
/// promoted, and the only trace was one line of stderr.**
///
/// Separated from the driver because that is the one seam a test can reach:
/// `validate` itself needs two compiled cdylibs and a routine bridge, so the
/// decision it feeds is otherwise only exercised by a full bench run.
fn timeable_after_validation(
    outcome: Result<&[String], &BenchError>,
    offered: &[String],
) -> (Vec<String>, Vec<String>) {
    match outcome {
        Ok(survivors) => {
            let lost = offered
                .iter()
                .filter(|p| !survivors.contains(p))
                .cloned()
                .collect();
            (survivors.to_vec(), lost)
        },
        Err(e) => (Vec::new(), vec![format!("(validation error: {e})")]),
    }
}

#[cfg(test)]
pub(crate) fn median_for_tests(vals: &mut [f64]) -> f64 {
    median(vals)
}

#[cfg(test)]
mod routine_tests;

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
            n,
            ..BenchConfig::default()
        }
    }

    fn paths(n: usize) -> Vec<String> {
        (0 .. n).map(|i| format!("/v{i}.dylib")).collect()
    }

    #[test]
    fn a_validation_error_leaves_nothing_to_time() {
        // The failing case, and the whole point. Before this, the error arm left
        // `variant_paths` populated, `required` defaulted false so the run did
        // not fail, and the `nothing to measure` branch could not fire because
        // the paths were still there. Every unvalidated variant got timed.
        let offered = paths(4);
        let err = BenchError::ValidationFailed {
            variant: "b".into(),
            reason:  "no arm was compared".into(),
        };
        let (timeable, lost) = timeable_after_validation(Err(&err), &offered);
        assert!(
            timeable.is_empty(),
            "an error must leave nothing timeable: {timeable:?}"
        );
        assert_eq!(lost.len(), 1, "and must say so once, not per variant");
        assert!(lost[0].contains("no arm was compared"), "{}", lost[0]);
    }

    #[test]
    fn survivors_are_timed_and_the_rest_are_recorded_lost() {
        // The positive control. Without it the function could return nothing
        // for every outcome and the test above would still pass, which is a
        // gate that blocks everything wearing the clothes of one that works.
        let offered = paths(4);
        let survivors = vec![offered[0].clone(), offered[2].clone()];
        let (timeable, lost) = timeable_after_validation(Ok(&survivors), &offered);
        assert_eq!(
            timeable, survivors,
            "survivors are exactly what may be timed"
        );
        assert_eq!(lost, vec![offered[1].clone(), offered[3].clone()]);

        // And an all-clear loses nobody.
        let (all, none) = timeable_after_validation(Ok(&offered), &offered);
        assert_eq!(all, offered);
        assert!(none.is_empty(), "{none:?}");
    }

    #[test]
    fn every_offered_variant_is_either_timed_or_recorded_lost() {
        // The law, over the whole subset lattice rather than a chosen case:
        // nothing may be silently neither. A variant that is dropped from the
        // timed set without appearing in `dropped` vanishes with no trace,
        // which is the same class of defect one level down.
        let offered = paths(5);
        for mask in 0 .. 1u32 << 5 {
            let survivors: Vec<String> = offered
                .iter()
                .enumerate()
                .filter(|(i, _)| mask >> i & 1 == 1)
                .map(|(_, p)| p.clone())
                .collect();
            let (timeable, lost) = timeable_after_validation(Ok(&survivors), &offered);
            assert_eq!(
                timeable.len() + lost.len(),
                offered.len(),
                "mask {mask:#07b} accounted for {} of {}",
                timeable.len() + lost.len(),
                offered.len()
            );
        }
    }

    const M: &str = "linux-x86_64-c4-container-intel";
    const R: &str = "20261115T093000Z-abc";

    /// A flat and a nested cell take the one layout: the bench, the cell, the
    /// machine class, the run. Neither keeps a name of its own any more.
    #[test]
    fn flat_and_nested_cells_land_in_the_one_layout() {
        let (dir, samples, report) =
            run_paths(&cfg("hash", "hash", 64, false), Path::new("results"), M, R);
        assert_eq!(dir, Path::new("results/hash/hash_n64").join(M).join(R));
        assert_eq!(samples, dir.join("samples.csv"));
        assert_eq!(report, dir.join("report.md"));
        let (dir, ..) = run_paths(&cfg("warm-container", "width-l1", 80003, true), Path::new("results"), M, R);
        assert_eq!(dir, Path::new("results/warm-container/width-l1_n80003").join(M).join(R));
        assert_eq!(
            mockspace_bench_layout::place(&dir.join("samples.csv"), |_| true),
            mockspace_bench_layout::Placement::InLayout,
            "what the driver writes is what the layout calls in place"
        );
    }

    /// A run id already taken on this machine class gets a suffix, so a run
    /// started in the same second over the same commit sits beside the first.
    /// The same id on another class is no collision.
    #[test]
    fn a_taken_run_id_takes_the_next_suffix() {
        let root = mockspace_manifest::scratch::held();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(unique_run_id(&root, M, R), R);
        let cell = Cell { sweep: "hash".into(), n: 64 };
        std::fs::create_dir_all(root.join(layout::run_rel("hash", &cell, M, R))).unwrap();
        assert_eq!(unique_run_id(&root, M, R), format!("{R}-2"));
        std::fs::create_dir_all(root.join(layout::run_rel("hash", &cell, M, &format!("{R}-2")))).unwrap();
        assert_eq!(unique_run_id(&root, M, R), format!("{R}-3"));
        assert_eq!(unique_run_id(&root, "macos-aarch64-c8-bare-apple", R), R);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Report-only renders a report only where a run has none: the report a
    /// run wrote carries its methodology, which the samples cannot restate.
    #[test]
    fn report_only_leaves_an_existing_report_alone() {
        let root = mockspace_manifest::scratch::held();
        let _ = std::fs::remove_dir_all(&root);
        let cell = Cell { sweep: "hash".into(), n: 64 };
        for (run, report) in [("20261114T000000Z-a", true), ("20261115T000000Z-b", false)] {
            let d = root.join(layout::run_rel("hash", &cell, M, run));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("samples.csv"), "x").unwrap();
            if report {
                std::fs::write(d.join("report.md"), "# with methodology").unwrap();
            }
        }
        let runs = layout::runs(&root);
        let picked: Vec<&str> = reportless(&runs).iter().map(|r| r.run.as_str()).collect();
        assert_eq!(picked, ["20261115T000000Z-b"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn each_failure_class_fails_the_exit_alone() {
        let f = failure();
        assert_eq!(format!("{:?}", final_exit(true, false, false)), f);
        assert_eq!(format!("{:?}", final_exit(false, true, false)), f);
        assert_eq!(format!("{:?}", final_exit(false, false, true)), f);
        assert_eq!(
            format!("{:?}", final_exit(false, false, false)),
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
        let root = mockspace_manifest::scratch::held();
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("bench.toml"),
            r#"
            [bench.b]
            kind = "tracking"
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
