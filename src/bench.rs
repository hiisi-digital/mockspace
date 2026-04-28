//! Bench framework command family.
//!
//! `mock bench init` scaffolds a `mock/benches/` directory in the
//! consumer with a starter `bench.toml`, a binary that drives the
//! harness, an example variant, and a README. The layout is exactly
//! what the harness discovers at run time.
//!
//! `mock bench run` builds the consumer's bench binary + variants in
//! release mode and spawns the binary, which drives the harness via
//! `mockspace-bench-harness`.
//!
//! `mock bench report` invokes the bench binary with `--report-only`
//! to regenerate findings.md from the existing CSV cache without
//! re-running the full harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::config::Config;

pub fn cmd(cfg: &Config, args: &[&str]) -> ExitCode {
    let sub = args.first().copied().unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).copied().collect();
    match sub {
        "init" => cmd_init(cfg),
        "run" => cmd_run(cfg, &rest),
        "report" => cmd_report(cfg, &rest),
        "" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown bench subcommand `{other}`");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    eprintln!("mock bench. canonical bench framework commands");
    eprintln!();
    eprintln!("subcommands:");
    eprintln!("  init    scaffold mock/benches/ in this consumer");
    eprintln!("  run     build variants + bench binary, run the harness");
    eprintln!("  report  regenerate findings.md from cached results");
    eprintln!();
    eprintln!("`mock/benches/` layout (created by `init`):");
    eprintln!("  Cargo.toml         the bench binary");
    eprintln!("  src/main.rs        Routine impl + run/report dispatch");
    eprintln!("  bench.toml         per-bench config (sizes, timing, variants)");
    eprintln!("  variants/<name>/   one cdylib per variant");
    eprintln!("  README.md");
}

// ── run ──

fn cmd_run(cfg: &Config, _args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        eprintln!(
            "error: {} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    if let Err(e) = build_variants_and_bin(&bench_dir) {
        eprintln!("error: build failed: {e}");
        return ExitCode::FAILURE;
    }

    let bin_path = match locate_bench_bin(&bench_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: locating bench binary: {e}");
            return ExitCode::FAILURE;
        }
    };

    let status = Command::new(&bin_path)
        .current_dir(&bench_dir)
        .status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench binary exited with {:?}", s.code());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        }
    }
}

// ── report ──

fn cmd_report(cfg: &Config, _args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        eprintln!(
            "error: {} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    if let Err(e) = build_bin_only(&bench_dir) {
        eprintln!("error: build failed: {e}");
        return ExitCode::FAILURE;
    }

    let bin_path = match locate_bench_bin(&bench_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: locating bench binary: {e}");
            return ExitCode::FAILURE;
        }
    };

    let status = Command::new(&bin_path)
        .arg("--report-only")
        .current_dir(&bench_dir)
        .status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench binary exited with {:?}", s.code());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        }
    }
}

// ── helpers ──

fn build_variants_and_bin(bench_dir: &Path) -> Result<(), String> {
    let variants_dir = bench_dir.join("variants");
    if variants_dir.exists() {
        for entry in fs::read_dir(&variants_dir)
            .map_err(|e| format!("reading variants dir: {e}"))?
        {
            let entry = entry.map_err(|e| format!("variants dir entry: {e}"))?;
            let path = entry.path();
            let manifest = path.join("Cargo.toml");
            if manifest.exists() {
                eprintln!("  building variant {}...", path.display());
                let status = Command::new("cargo")
                    .args([
                        "build",
                        "--release",
                        "--manifest-path",
                    ])
                    .arg(&manifest)
                    .status()
                    .map_err(|e| format!("spawning cargo for {}: {e}", path.display()))?;
                if !status.success() {
                    return Err(format!("cargo build failed for {}", path.display()));
                }
            }
        }
    }

    build_bin_only(bench_dir)
}

fn build_bin_only(bench_dir: &Path) -> Result<(), String> {
    let manifest = bench_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!(
            "{} not found; the scaffold may have been deleted",
            manifest.display()
        ));
    }
    eprintln!("  building bench binary...");
    let status = Command::new("cargo")
        .args(["build", "--release", "--manifest-path"])
        .arg(&manifest)
        .status()
        .map_err(|e| format!("spawning cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed for bench binary".into());
    }
    Ok(())
}

fn locate_bench_bin(bench_dir: &Path) -> Result<PathBuf, String> {
    // Cargo's default target dir is `<manifest_dir>/target`. The
    // bench crate name is `benches` (set in the starter Cargo.toml).
    // Allow either macOS or Linux extension by simple existence check.
    let release_dir = bench_dir.join("target/release");
    if !release_dir.exists() {
        return Err(format!(
            "target/release not found under {}; build did not produce artifacts",
            bench_dir.display()
        ));
    }
    let candidates = [release_dir.join("benches"), release_dir.join("benches.exe")];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(format!(
        "no bench binary found in {}; expected `benches` or `benches.exe`",
        release_dir.display()
    ))
}

// ── init ──

fn cmd_init(cfg: &Config) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if bench_dir.exists() {
        eprintln!(
            "error: {} already exists. `mock bench init` is idempotent only when the dir is absent.",
            bench_dir.display()
        );
        eprintln!("delete the directory or pick a different scaffolding strategy.");
        return ExitCode::FAILURE;
    }

    if let Err(e) = fs::create_dir_all(&bench_dir) {
        eprintln!("error: failed to create {}: {}", bench_dir.display(), e);
        return ExitCode::FAILURE;
    }
    for sub in &["src", "variants/sample/src"] {
        if let Err(e) = fs::create_dir_all(bench_dir.join(sub)) {
            eprintln!("error: failed to create benches/{sub}: {e}");
            return ExitCode::FAILURE;
        }
    }

    if let Err(e) = write_starter_files(&bench_dir) {
        eprintln!("error: scaffolding failed: {e}");
        return ExitCode::FAILURE;
    }

    eprintln!("scaffolded {} with starter bench binary + sample variant", bench_dir.display());
    eprintln!();
    eprintln!("next steps:");
    eprintln!("  1. edit src/main.rs: replace IdentityAdd with your Routine");
    eprintln!("  2. edit bench.toml: set sizes + variant cdylib paths");
    eprintln!("  3. add a variant under variants/<name>/ for each impl");
    eprintln!("  4. run `mock bench run` to build + benchmark");
    eprintln!("  5. run `mock bench report` to regenerate findings.md from cache");
    ExitCode::SUCCESS
}

fn write_starter_files(bench_dir: &Path) -> std::io::Result<()> {
    fs::write(bench_dir.join("Cargo.toml"), STARTER_BIN_CARGO_TOML)?;
    fs::write(bench_dir.join("src/main.rs"), STARTER_BIN_MAIN)?;
    fs::write(bench_dir.join("bench.toml"), STARTER_BENCH_TOML)?;
    fs::write(bench_dir.join("README.md"), STARTER_README)?;
    fs::write(
        bench_dir.join("variants/sample/Cargo.toml"),
        STARTER_VARIANT_CARGO_TOML,
    )?;
    fs::write(bench_dir.join("variants/sample/src/lib.rs"), STARTER_VARIANT_LIB)?;
    Ok(())
}

const STARTER_BIN_CARGO_TOML: &str = r#"[package]
name = "benches"
version = "0.0.0"
edition = "2021"
publish = false

[[bin]]
name = "benches"
path = "src/main.rs"

[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev", features = ["std"] }
mockspace-bench-harness = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev" }

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
"#;

const STARTER_BIN_MAIN: &str = r#"//! Consumer bench binary. Defines one or more Routines and dispatches
//! to the mockspace bench harness. `mock bench run` invokes this in
//! release mode after building all variants under `variants/`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mockspace_bench_core::{routine_bridge, Routine};
use mockspace_bench_harness::{
    self as harness, BenchManifest, RoutineSpec, Workload,
};

/// Sample routine: identity-add. Replace with your real routine.
pub struct IdentityAdd;

impl Routine for IdentityAdd {
    type Input = u64;
    type Output = u64;

    fn build_input(seed: u64) -> Self::Input {
        seed
    }

    fn ops_per_call(_input: &Self::Input) -> u64 {
        1
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Worker subprocess path (re-execed by the orchestrator).
    if args.iter().any(|a| a == "--worker") {
        return run_worker(&args);
    }

    // Report-only path (called by `mock bench report`).
    let report_only = args.iter().any(|a| a == "--report-only");

    let manifest_path = Path::new("bench.toml");
    let manifest = match BenchManifest::load(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mock_benches_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Build a workload. Replace this with your real workload program.
    let mut workload = Workload::new();
    workload.program("default", |b| {
        b.stage(vec![harness::algo_call(), harness::light_scalar()]);
    });

    // Iterate manifest entries and run each (bench, size).
    for (bench_name, section) in &manifest.bench {
        for (size_idx, _size) in section.sizes.iter().enumerate() {
            let config = match manifest.for_size(bench_name, size_idx, &mock_benches_dir) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let routine = RoutineSpec {
                name: section.workload.clone(),
                bridge: routine_bridge!(IdentityAdd),
            };

            let csv_path = format!("{}_n{}.csv", bench_name, config.n);
            let report_path = format!("{}_n{}_findings.md", bench_name, config.n);

            if report_only {
                eprintln!(
                    "  report-only: skipping run for {} n={}",
                    bench_name, config.n
                );
            } else {
                let result = match harness::run(&config, &routine, &workload) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("error: bench `{bench_name}` n={}: {e}", config.n);
                        return ExitCode::FAILURE;
                    }
                };
                if let Err(e) = harness::write_csv(&result, &csv_path) {
                    eprintln!("error: writing csv: {e}");
                    return ExitCode::FAILURE;
                }
                if let Err(e) = harness::write_report_for_routine(
                    &result, &routine, "warm", &report_path,
                ) {
                    eprintln!("error: writing report: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    ExitCode::SUCCESS
}

fn run_worker(args: &[String]) -> ExitCode {
    // Parse the --worker flag set forwarded by the orchestrator.
    let get = |flag: &str| -> Option<String> {
        let pos = args.iter().position(|a| a == flag)?;
        args.get(pos + 1).cloned()
    };

    let dylib_path = match get("--worker") {
        Some(p) => p,
        None => {
            eprintln!("worker: missing --worker <path>");
            return ExitCode::FAILURE;
        }
    };
    let bench_name = get("--bench-name").unwrap_or_default();
    let seed: u64 = get("--seed").and_then(|s| s.parse().ok()).unwrap_or(0);
    let cooldown_ms: u64 = get("--cooldown").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mode = get("--mode").unwrap_or_else(|| "warm".into());
    let runs: usize = get("--runs").and_then(|s| s.parse().ok()).unwrap_or(0);
    let batch: usize = get("--batch").and_then(|s| s.parse().ok()).unwrap_or(1);
    let n: usize = get("--n").and_then(|s| s.parse().ok()).unwrap_or(1);
    let batch_k: usize = get("--batch-k").and_then(|s| s.parse().ok()).unwrap_or(1);
    let max_call_us: Option<u64> = get("--max-call-us").and_then(|s| s.parse().ok()).filter(|&v| v > 0);

    let routine = RoutineSpec {
        name: bench_name.clone(),
        bridge: routine_bridge!(IdentityAdd),
    };

    let mut workload = Workload::new();
    workload.program("default", |b| {
        b.stage(vec![harness::algo_call(), harness::light_scalar()]);
    });

    harness::run_worker(
        &routine, &workload, &dylib_path,
        seed, cooldown_ms, &mode,
        runs, batch, n, batch_k, max_call_us,
    );
    ExitCode::SUCCESS
}
"#;

const STARTER_BENCH_TOML: &str = r#"# Bench harness configuration.
#
# Each `[bench.<name>]` section defines one bench. Each
# `[[bench.<name>.sizes]]` row sets a logical size (N) and the variant
# cdylib paths to compare at that size. `[timing]` knobs apply
# globally (passes per harness run, runs per pass, batch size,
# cooldown cohorts).

# Note on master_seed: TOML 1.0 caps integers at i64 (0x7FFF_FFFF_FFFF_FFFF).
# Pick any value that fits, or set 0 to use a fresh random seed every run.
[bench.sample]
title = "Sample bench"
workload = "default"
master_seed = 0x1234_5678_9ABC_DEF0

[[bench.sample.sizes]]
n = 64
variants = [
    "variants/sample/target/release/libsample.dylib",
]

[timing]
passes = 4
runs_per_pass = 1000
batch_size = 100
harness_runs = 1
cooldowns_ms = [0]
"#;

const STARTER_VARIANT_CARGO_TOML: &str = r#"[package]
name = "sample"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
name = "sample"
path = "src/lib.rs"
crate-type = ["cdylib"]

[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev", features = ["std"] }

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
"#;

const STARTER_VARIANT_LIB: &str = r#"//! Sample variant cdylib.
//!
//! One cdylib per variant. Each one exports `bench_entry`,
//! `bench_name`, `bench_abi_hash` (extern "C") that the harness
//! looks up via dlsym after dlopen.

use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

/// The actual algorithm under test for this variant. Replace with
/// your real impl. Operates on the same Input / Output shape as the
/// Routine in the parent bench binary.
fn sample_impl(input: &u64, output: &mut u64) {
    *output = input.wrapping_add(1);
}

#[no_mangle]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { sample_impl(input, output); }
    }
}

#[no_mangle]
pub extern "C" fn bench_name() -> *const u8 {
    b"sample\0".as_ptr()
}

#[no_mangle]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}
"#;

const STARTER_README: &str = r#"# benches

Canonical mockspace bench framework. Consumer-side scaffolding generated
by `mock bench init`.

## Layout

- `Cargo.toml` + `src/main.rs`: the bench binary. Defines `Routine`
  impls, builds a workload program, dispatches to the harness.
- `bench.toml`: per-bench config (sizes, timing, variant cdylib paths).
- `variants/<name>/`: one workspace per variant. Each compiles to a
  cdylib that exports `bench_entry`, `bench_name`, `bench_abi_hash`.
- `target/release/benches`: the built bench binary.
- `target/release/lib<variant>.{dylib,so,dll}`: the built variant cdylibs.

## Workflow

1. Edit `src/main.rs`: replace `IdentityAdd` with the Routine you
   want to benchmark. The trait specifies what is computed (input
   shape, output shape, validation, scoring, ops count).
2. Edit `bench.toml`: set sizes and the cdylib path for each
   variant.
3. Add a variant under `variants/<name>/` for each implementation.
   Each variant exports `bench_entry` calling its own algorithm via
   the `timed!` macro.
4. `mock bench run` builds everything and runs the harness.
5. `mock bench report` regenerates `findings.md` from the CSV cache
   without re-running.

## Status

v2 of the bench framework. The harness ships full orchestration:
variant isolation via subprocess + dlopen, hardware counter timing
(`CNTVCT_EL0` / `rdtsc`), CSV cache with drift correction, validation
across variants (byte-exact / approximate / per-variant validity),
analysis (quintile + bootstrap CI + sign test + Pareto + multi-N
scaling), findings.md generator, history log with regression
detection, optional perf counter integration, asm dedup check.

## References

- `mockspace-bench-core` (the framework): see Routine trait docs.
- `mockspace-bench-harness` (the orchestrator): see `harness::run`
  and `harness::write_report`.
- Origin: framework was extracted from `polka-dots/mock/benches/`.
"#;
