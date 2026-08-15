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
        "list" => cmd_list(cfg),
        "add" => cmd_add(cfg, &rest),
        "" => {
            print_help();
            ExitCode::SUCCESS
        },
        other => {
            eprintln!("error: unknown bench subcommand `{other}`");
            print_help();
            ExitCode::FAILURE
        },
    }
}

fn print_help() {
    eprintln!("mock bench. canonical bench framework commands");
    eprintln!();
    eprintln!("subcommands:");
    eprintln!("  init    scaffold mock/benches/ in this consumer");
    eprintln!("  run     build variants + bench binary, run the harness");
    eprintln!("  report  regenerate findings.md from cached results");
    eprintln!("  list    list benches, sizes, and variants from bench.toml");
    eprintln!("  add     scaffold a new variant crate: mock bench add <name>");
    eprintln!();
    eprintln!("`run` and `report` accept bench names to restrict the pass:");
    eprintln!("  mock bench run <name> [<name> ...]   run only the named benches");
    eprintln!("with no names, every bench in bench.toml runs");
    eprintln!();
    eprintln!("`mock/benches/` layout (created by `init`):");
    eprintln!("  Cargo.toml         the bench binary");
    eprintln!("  src/main.rs        Routine impl + run/report dispatch");
    eprintln!("  bench.toml         per-bench config (sizes, timing, variants)");
    eprintln!("  variants/<name>/   one cdylib per variant");
    eprintln!("  README.md");
}

// ── run ──

fn cmd_run(cfg: &Config, args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        eprintln!(
            "error: {} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    // Positional args are bench names: they restrict both the run
    // (forwarded as --only) and the variant builds.
    let names: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with("--"))
        .collect();
    let extra: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| a.starts_with("--"))
        .collect();

    let dirs = if names.is_empty() {
        None
    } else {
        match variant_dirs_for(&bench_dir, &names) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    };
    if let Err(e) = build_variants_and_bin_filtered(&bench_dir, dirs.as_deref()) {
        eprintln!("error: build failed: {e}");
        return ExitCode::FAILURE;
    }

    let bin_path = match locate_bench_bin(&bench_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: locating bench binary: {e}");
            return ExitCode::FAILURE;
        },
    };

    let mut cmd = Command::new(&bin_path);
    for n in &names {
        cmd.args(["--only", n]);
    }
    for e in &extra {
        cmd.arg(e);
    }
    let status = cmd.current_dir(&bench_dir).status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench binary exited with {:?}", s.code());
            ExitCode::FAILURE
        },
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        },
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
        },
    };

    let mut cmd = Command::new(&bin_path);
    cmd.arg("--report-only");
    for a in _args.iter().filter(|a| !a.starts_with("--")) {
        cmd.args(["--only", a]);
    }
    let status = cmd.current_dir(&bench_dir).status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench binary exited with {:?}", s.code());
            ExitCode::FAILURE
        },
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        },
    }
}

// ── helpers ──

/// Read `bench.toml` and map the named benches to the variant
/// directories their entries reference. Short names map to
/// `variants/<name>`; path entries starting with `variants/` map to
/// their first two components; anything else is ignored (an
/// out-of-tree path is not ours to build).
fn variant_dirs_for(bench_dir: &Path, names: &[&str]) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(bench_dir.join("bench.toml"))
        .map_err(|e| format!("reading bench.toml: {e}"))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("parsing bench.toml: {e}"))?;
    let bench = doc
        .get("bench")
        .and_then(|b| b.as_table())
        .ok_or_else(|| "bench.toml has no [bench.*] sections".to_string())?;
    let mut dirs: Vec<String> = Vec::new();
    let mut push_entry = |entry: &str, dirs: &mut Vec<String>| {
        let dir = if !entry.contains('/') {
            Some(entry.to_string())
        } else {
            entry
                .strip_prefix("variants/")
                .and_then(|rest| rest.split('/').next())
                .map(|d| d.to_string())
        };
        if let Some(d) = dir {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    };
    let collect_array = |item: Option<&toml_edit::Item>,
                         dirs: &mut Vec<String>,
                         push: &mut dyn FnMut(&str, &mut Vec<String>)| {
        if let Some(arr) = item.and_then(|v| v.as_array()) {
            for v in arr.iter() {
                if let Some(sv) = v.as_str() {
                    push(sv, dirs);
                }
            }
        }
    };
    for name in names {
        let section = bench.get(name).ok_or_else(|| {
            let available: Vec<&str> = bench.iter().map(|(k, _)| k).collect();
            format!(
                "bench `{name}` not found in bench.toml. Available: {}",
                available.join(", ")
            )
        })?;
        collect_array(section.get("variants"), &mut dirs, &mut push_entry);
        if let Some(sizes) = section.get("sizes") {
            if let Some(arr) = sizes.as_array_of_tables() {
                for t in arr.iter() {
                    collect_array(t.get("variants"), &mut dirs, &mut push_entry);
                }
            }
            if let Some(arr) = sizes.as_array() {
                for v in arr.iter() {
                    if let Some(t) = v.as_inline_table() {
                        if let Some(tv) = t.get("variants").and_then(|x| x.as_array()) {
                            for e in tv.iter() {
                                if let Some(sv) = e.as_str() {
                                    push_entry(sv, &mut dirs);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(dirs)
}

/// The release profile the framework guarantees, passed on every
/// build rather than left to the consumer's manifests.
///
/// A `[profile.release]` table is honoured only in a workspace root.
/// A variant that is a workspace member therefore loses its own
/// silently, and a consumer whose manifests never declared one never
/// had it at all. Both have been observed in the same tree: ninety
/// variant crates built at cargo's default `lto = false,
/// codegen-units = 16` while the framework's documentation promised
/// fat LTO and a single codegen unit.
///
/// Codegen-unit partitioning is not stable across builds, so the
/// default is a reproducibility defect and not only a slower one:
/// two runs of an unchanged variant can differ in inlining and
/// layout, which is exactly the contamination per-variant cdylib
/// isolation exists to prevent.
const PROFILE_ARGS: [&str; 6] = [
    "--config",
    "profile.release.opt-level=3",
    "--config",
    "profile.release.lto=\"fat\"",
    "--config",
    "profile.release.codegen-units=1",
];

/// The binary name a bench manifest declares, read rather than
/// assumed.
///
/// This was hardcoded to `benches`, the starter template's package
/// name. A consumer that renamed its bench package built everything
/// successfully and then failed with "no bench binary found",
/// because the name the scaffold happened to use was being treated
/// as part of the contract.
fn bench_bin_name(manifest: &Path) -> Result<String, String> {
    let text =
        fs::read_to_string(manifest).map_err(|e| format!("reading {}: {e}", manifest.display()))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("parsing {}: {e}", manifest.display()))?;
    if let Some(name) = doc
        .get("bin")
        .and_then(|b| b.as_array_of_tables())
        .and_then(|a| a.iter().next())
        .and_then(|t| t.get("name"))
        .and_then(|v| v.as_str())
    {
        return Ok(name.to_string());
    }
    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "{} declares neither a [[bin]] name nor a [package] name",
                manifest.display()
            )
        })
}

fn build_variants_and_bin_filtered(
    bench_dir: &Path,
    only_dirs: Option<&[String]>,
) -> Result<(), String> {
    let variants_dir = bench_dir.join("variants");
    if variants_dir.exists() {
        for entry in
            fs::read_dir(&variants_dir).map_err(|e| format!("reading variants dir: {e}"))?
        {
            let entry = entry.map_err(|e| format!("variants dir entry: {e}"))?;
            let path = entry.path();
            if let Some(dirs) = only_dirs {
                let dir_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();
                if !dirs.contains(&dir_name) {
                    continue;
                }
            }
            let manifest = path.join("Cargo.toml");
            if manifest.exists() {
                eprintln!("  building variant {}...", path.display());
                let status = Command::new("cargo")
                    .args(["build", "--release"])
                    .args(PROFILE_ARGS)
                    .arg("--manifest-path")
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
        .args(["build", "--release"])
        .args(PROFILE_ARGS)
        .arg("--manifest-path")
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
    // binary's name comes from the consumer's own manifest; assuming
    // the starter template's `benches` made every renamed bench
    // package unlocatable.
    let manifest = bench_dir.join("Cargo.toml");
    let name = bench_bin_name(&manifest)?;
    let release_dir = bench_dir.join("target/release");
    if !release_dir.exists() {
        return Err(format!(
            "target/release not found under {}; build did not produce artifacts",
            bench_dir.display()
        ));
    }
    let candidates = [release_dir.join(&name), release_dir.join(format!("{name}.exe"))];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(format!(
        "no bench binary found in {}; expected `{name}` or `{name}.exe`, the name \
         declared by {}",
        release_dir.display(),
        manifest.display()
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

    eprintln!(
        "scaffolded {} with starter bench binary + sample variant",
        bench_dir.display()
    );
    eprintln!();
    eprintln!("next steps:");
    eprintln!("  1. edit src/main.rs: replace IdentityAdd with your Routine");
    eprintln!("  2. edit bench.toml: set sizes + variant cdylib paths");
    eprintln!("  3. add a variant under variants/<name>/ for each impl");
    eprintln!("  4. run `mock bench run` to build + benchmark");
    eprintln!("  5. run `mock bench report` to regenerate findings.md from cache");
    ExitCode::SUCCESS
}

// ── list ──

fn cmd_list(cfg: &Config) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    let text = match fs::read_to_string(bench_dir.join("bench.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: reading bench.toml: {e}");
            return ExitCode::FAILURE;
        },
    };
    let doc: toml_edit::DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: parsing bench.toml: {e}");
            return ExitCode::FAILURE;
        },
    };
    let Some(bench) = doc.get("bench").and_then(|b| b.as_table()) else {
        eprintln!("no [bench.*] sections in bench.toml");
        return ExitCode::SUCCESS;
    };
    let mut names: Vec<&str> = bench.iter().map(|(k, _)| k).collect();
    names.sort();
    for name in names {
        let section = bench.get(name).unwrap();
        let title = section.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let mut sizes: Vec<String> = Vec::new();
        if let Some(item) = section.get("sizes") {
            if let Some(arr) = item.as_array() {
                for v in arr.iter() {
                    if let Some(n) = v.as_integer() {
                        sizes.push(n.to_string());
                    }
                }
            }
            if let Some(arr) = item.as_array_of_tables() {
                for t in arr.iter() {
                    if let Some(n) = t.get("n").and_then(|x| x.as_integer()) {
                        sizes.push(n.to_string());
                    }
                }
            }
        }
        let dirs = variant_dirs_for(&bench_dir, &[name]).unwrap_or_default();
        println!(
            "{name}  [{}]  variants: {}  {}",
            sizes.join(", "),
            dirs.join(", "),
            title
        );
    }
    println!();
    println!("run one with: mock bench run <name>");
    ExitCode::SUCCESS
}

// ── add ──

fn cmd_add(cfg: &Config, args: &[&str]) -> ExitCode {
    let Some(name) = args.first() else {
        eprintln!("usage: mock bench add <variant-name>");
        return ExitCode::FAILURE;
    };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        eprintln!("error: variant name must be [a-zA-Z0-9_] (it becomes a crate name)");
        return ExitCode::FAILURE;
    }
    let bench_dir = cfg.mock_dir.join("benches");
    let dir = bench_dir.join("variants").join(name);
    if dir.exists() {
        eprintln!("error: {} already exists", dir.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::create_dir_all(dir.join("src")) {
        eprintln!("error: creating {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let cargo = STARTER_VARIANT_CARGO_TOML.replace("sample", name);
    let lib = STARTER_VARIANT_LIB.replace("sample", name);
    if let Err(e) = fs::write(dir.join("Cargo.toml"), cargo)
        .and_then(|_| fs::write(dir.join("src/lib.rs"), lib))
    {
        eprintln!("error: writing variant files: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("scaffolded variants/{name}/. Next:");
    eprintln!("  1. implement the run block in variants/{name}/src/lib.rs");
    eprintln!("  2. reference \"{name}\" from a bench's `variants` list in bench.toml");
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
    fs::write(
        bench_dir.join("variants/sample/src/lib.rs"),
        STARTER_VARIANT_LIB,
    )?;
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

const STARTER_BIN_MAIN: &str = r##"//! Consumer bench binary: registrations only. The generic loop
//! (manifest iteration, filtering, report-only, preflight, seed
//! replay, validation, history, summary, findings index) lives in
//! `mockspace_bench_harness::driver::drive`.

use std::process::ExitCode;

use mockspace_bench_core::byte_routine_dispatch;
use mockspace_bench_harness::driver::{drive, DriverRegistry};
use mockspace_bench_harness::{self as harness, BenchConfig, RoutineSpec, Workload};

/// Build the workload program for a workload name. The workload
/// surrounds the measured call with realistic context so numbers
/// approximate the real calling environment; add named programs as
/// your benches need them.
fn build_workload(name: &str, _n: usize) -> Workload {
    let mut w = Workload::new();
    match name {
        "realistic" => {
            w.program("realistic", |b| {
                b.stage(vec![
                    harness::algo_call(),
                    harness::light_scalar(),
                    harness::heavy_memory(),
                    harness::branch_work(),
                ]);
            });
        }
        _ => {
            w.program("default", |b| {
                b.stage(vec![harness::algo_call(), harness::light_scalar()]);
            });
        }
    }
    w
}

/// Custom routines for benches whose inputs are not plain bytes
/// (graph shapes, sparse layouts). Return `None` to fall through to
/// the byte dispatch below.
fn routine_for(_config: &BenchConfig) -> Option<RoutineSpec> {
    None
}

fn main() -> ExitCode {
    drive(&DriverRegistry {
        build_workload,
        routine_for,
        // Every size stays its own monomorphisation: the declared
        // list is the strictly controlled input set. A manifest size
        // outside it is a targeted error naming this line.
        byte_dispatch: byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 4096, 16384]),
    })
}
"##;

const STARTER_BENCH_TOML: &str = r#"# Bench harness configuration. `mock bench run [names...]` runs it;
# `mock bench list` prints what is registered here.
#
# Each [bench.<name>] section is one bench:
#   variants = ["a", "b"]   variant short names (dirs under variants/)
#   sizes = [64, 256]       the N list; every N must be in the bench
#                           binary's byte_routine_dispatch! declaration
#                           (each size is its own monomorphisation)
#   may_differ = false      variants may produce different valid outputs
#   required = false        validation failure fails the whole run
#   threaded = false        variants spawn threads (skips the P-core pin)
#   [bench.<name>.timing]   per-bench override of any [timing] knob
#
# master_seed: integer, or a string ("0x...") for values past the TOML
# i64 cap; 0 picks a fresh random seed (printed, replayable with
# `--seed`).
#
# [docgen] enabled = true makes the docs regeneration pass emit a
# generated BENCHES.md (plus graphviz visualisations) under docs/
# from the bench history.

[bench.sample]
title = "Sample bench"
workload = "default"
variants = ["sample"]
sizes = [64, 256]

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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn bin_name_prefers_the_declared_bin_over_the_package() {
        let tmp = tempfile::tempdir().unwrap();
        let m = tmp.path().join("Cargo.toml");
        write(
            &m,
            "[package]\nname = \"arvo-benches\"\n\n[[bin]]\nname = \"custom-runner\"\npath = \
             \"src/main.rs\"\n",
        );
        assert_eq!(bench_bin_name(&m).unwrap(), "custom-runner");
    }

    #[test]
    fn bin_name_falls_back_to_the_package_name() {
        let tmp = tempfile::tempdir().unwrap();
        let m = tmp.path().join("Cargo.toml");
        write(&m, "[package]\nname = \"arvo-benches\"\n");
        assert_eq!(bench_bin_name(&m).unwrap(), "arvo-benches");
    }

    /// The regression this change exists for. A consumer whose bench
    /// package is not called `benches` was unlocatable: the build
    /// succeeded and the run then failed. The name must come from the
    /// manifest, so a package named anything else resolves.
    #[test]
    fn bin_name_is_not_the_starter_templates_name() {
        let tmp = tempfile::tempdir().unwrap();
        let m = tmp.path().join("Cargo.toml");
        write(&m, "[package]\nname = \"arvo-benches\"\n");
        let got = bench_bin_name(&m).unwrap();
        assert_ne!(got, "benches", "the hardcoded name must not survive");
        assert_eq!(got, "arvo-benches");
    }

    #[test]
    fn bin_name_errors_when_the_manifest_names_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let m = tmp.path().join("Cargo.toml");
        write(&m, "[dependencies]\n");
        let err = bench_bin_name(&m).unwrap_err();
        assert!(err.contains("declares neither"), "unhelpful error: {err}");
    }

    #[test]
    fn bin_name_errors_when_the_manifest_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let m = tmp.path().join("Cargo.toml");
        assert!(bench_bin_name(&m).is_err());
    }

    /// The profile is the framework's guarantee, so it travels on the
    /// command line where a workspace-member manifest cannot lose it.
    /// Asserted as pairs so a reordering or a dropped flag fails.
    #[test]
    fn profile_args_pin_all_three_settings() {
        let pairs: Vec<(&str, &str)> = PROFILE_ARGS.chunks(2).map(|c| (c[0], c[1])).collect();
        assert_eq!(pairs.len(), 3);
        for (flag, _) in &pairs {
            assert_eq!(*flag, "--config");
        }
        let values: Vec<&str> = pairs.iter().map(|(_, v)| *v).collect();
        assert!(values.contains(&"profile.release.lto=\"fat\""));
        assert!(values.contains(&"profile.release.codegen-units=1"));
        assert!(values.contains(&"profile.release.opt-level=3"));
    }
}
