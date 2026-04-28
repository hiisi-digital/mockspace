//! Bench framework command family.
//!
//! `mock bench init` scaffolds a `mock/benches/` directory in the
//! consumer with a starter `Cargo.toml`, a sample `Routine` impl, and
//! a README pointing at `mockspace-bench-core`.
//!
//! `mock bench run` and `mock bench report` are placeholders for v2.
//! When the bench harness lands they will dispatch to it. For v1 the
//! framework crate (`mockspace-bench-core`) is canonical and consumers
//! can write `Routine` impls + run them via `cargo bench` or hand-roll
//! a harness; mockspace's contribution is the framework surface plus
//! init scaffolding plus the seam for the harness.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use crate::config::Config;

pub fn cmd(cfg: &Config, args: &[&str]) -> ExitCode {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "init" => cmd_init(cfg),
        "run" => cmd_run_stub(),
        "report" => cmd_report_stub(),
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
    eprintln!("  run     [v2] run benches via the harness");
    eprintln!("  report  [v2] regenerate findings.md from cached results");
    eprintln!();
    eprintln!("v1 ships the framework crate `mockspace-bench-core`");
    eprintln!("  + scaffolding only. Harness orchestration (variant");
    eprintln!("  isolation, CSV cache, multi-process timing) lands in v2.");
}

fn cmd_run_stub() -> ExitCode {
    eprintln!("`mock bench run` is not yet implemented (v2).");
    eprintln!();
    eprintln!("v1 ships the framework crate `mockspace-bench-core`. Consumers");
    eprintln!("write `Routine` impls and run them via `cargo bench` (or hand-");
    eprintln!("roll a harness) until the canonical mockspace harness lands.");
    eprintln!();
    eprintln!("track: mockspace bench framework v2 (issue tracking forthcoming).");
    ExitCode::SUCCESS
}

fn cmd_report_stub() -> ExitCode {
    eprintln!("`mock bench report` is not yet implemented (v2).");
    eprintln!();
    eprintln!("Result aggregation + findings.md generation lands with the");
    eprintln!("v2 harness. Until then, consumers manage their own report");
    eprintln!("output.");
    ExitCode::SUCCESS
}

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
    if let Err(e) = fs::create_dir_all(bench_dir.join("src")) {
        eprintln!("error: failed to create benches/src: {e}");
        return ExitCode::FAILURE;
    }

    if let Err(e) = write_starter_files(&bench_dir) {
        eprintln!("error: scaffolding failed: {e}");
        return ExitCode::FAILURE;
    }

    eprintln!("scaffolded {} with starter Routine + README", bench_dir.display());
    eprintln!();
    eprintln!("next steps:");
    eprintln!("  1. add `mock/benches` to your workspace [members] in mock/Cargo.toml");
    eprintln!("  2. fill in src/lib.rs with your Routine impl(s)");
    eprintln!("  3. write variant impls under src/variants/");
    eprintln!("  4. (v2) run `mock bench run` once the harness lands");
    ExitCode::SUCCESS
}

fn write_starter_files(bench_dir: &Path) -> std::io::Result<()> {
    fs::write(bench_dir.join("Cargo.toml"), STARTER_CARGO_TOML)?;
    fs::write(bench_dir.join("src/lib.rs"), STARTER_LIB_RS)?;
    fs::write(bench_dir.join("README.md"), STARTER_README)?;
    Ok(())
}

const STARTER_CARGO_TOML: &str = r#"[package]
name = "benches"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
name = "benches"
path = "src/lib.rs"

[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", features = ["std"] }

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
"#;

const STARTER_LIB_RS: &str = r#"//! Bench routines for this consumer.
//!
//! One `Routine` impl per algorithm-under-test. Variants live in
//! `src/variants/<routine>/` and are loaded by the harness via dlopen
//! once the v2 harness lands.

use mockspace_bench_core::Routine;

/// Sample routine: identity-add.
///
/// Replace with your real routine; this exists so `cargo check` is
/// green right after `mock bench init`.
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
"#;

const STARTER_README: &str = r#"# benches

Canonical mockspace bench framework. Consumer-side scaffolding generated
by `mock bench init`.

## Layout

- `src/lib.rs`: `Routine` impls (one per algorithm-under-test).
- `src/variants/<routine>/`: variant impls per Routine. Each variant
  compiles to its own dylib and is loaded by the harness in isolation.
  (v2 harness; structure is reserved.)

## Workflow

1. Define a `Routine` in `src/lib.rs`. The trait specifies what is
   computed: input shape, output shape, validation, scoring, ops count.
2. Write variant impls under `src/variants/<routine>/`. Each variant is
   a self-contained crate that imports the Routine and implements the
   algorithm.
3. (v2) Run `mock bench run` to invoke the harness across variants.
4. (v2) Run `mock bench report` to regenerate `findings.md` from cached
   results.

## Status

v1 of the bench framework ships `mockspace-bench-core` with the Routine
trait, FFI bridge types, hardware counter timing (`CNTVCT_EL0` / `rdtsc`),
and the `timed!` macro. The harness orchestrator (variant isolation,
CSV cache, multi-process timing, findings generation) lands in v2.

Until v2: write Routine impls and time them via `cargo bench` or your
own harness. The Routine surface is forward-compatible with the v2
harness; impls written today will work when the harness lands.

## References

- `mockspace-bench-core` (the framework): see Routine trait docs.
- Origin: framework was extracted from `polka-dots/mock/benches/` (the
  substrate that drove arvo's strategy-marker design).
"#;
