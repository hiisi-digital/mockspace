# Benchmarking

Any number that feeds a decision comes from `{mock_dir}/benches/`. Never hand-roll an `Instant::now()` loop.

## Workflow

1. `cargo mock bench init` once: root `bench.toml`, a `README.md`, a `sample` bench to copy.
2. `cargo mock bench add <bench> [<arm>]` (or `<bench>/<arm>`): one bench is one question, its arms the competing answers.
3. Arm code in `<bench>/arms/<arm>/src/lib.rs`, inside `timed! { run { ... } }`. Its manifest is generated; a hand-written `Cargo.toml` there takes over. Shared code: `<bench>/support/` or root `support/`, plain library crates.
4. `<bench>/bench.toml`: `title`, `workload`, `arms`, `points`, `baseline` (every delta is against it), `floor` (null-cost arm, subtracted first).
5. `cargo mock bench run [<bench>...]`: generates the driver, builds each arm with the pinned release profile, validates, measures, writes the run. `cargo mock bench list` shows what is registered.
6. Write the findings note (below) and commit it with the run.

## The hooks library

The generated driver is the only runner. What your own driver would do goes in `{mock_dir}/benches/src/lib.rs`, compiled into the driver:

```rust
use mockspace_bench_harness::driver::{AfterCell, CellVerdict, Hooks};

pub fn hooks() -> Hooks {
    Hooks { routine_for: Some(routines), after_cell: Some(check), ..Hooks::default() }
}
```

- `routine_for`: a routine for a cell whose input is not plain bytes (build with `routine_table!`); `None` falls through to the byte dispatch.
- `after_cell`: after each cell's artifacts are staged; its verdict feeds the exit code.
- `on_init`: once after the manifest loads, before building; may abort.
- `after_init`: once after every cell resolves and preflight passes.

`run`, `report`, `test` and the lint `a-bench-runs-only-on-the-generated-driver` refuse any other runner: a `Cargo.toml` or `src/main.rs` at the bench root, a crate with a binary, cargo bench or example, a script (`.sh`, `.bash`, `.zsh`, `.py`, `.nu`, any shebang), `Makefile`, `justfile`, or `.cargo/config.toml` aliases or `runner`.

## Commands

- `cargo mock bench init|add <bench> [<arm>]|run [<bench>...]|list` as above.
- `report`: renders a report for a run that has none; never touches one the run wrote.
- `test`: `cargo test` in every crate under the bench tree (a bare `cargo test` at the root sees only the driver). A name narrows by path; flags after it go to cargo. Counts print with their profile.
- `cargo mock bench migrate [--machine-class <class>] [--dry-run]`: moves an older results layout into run directories.

## The layers

- `mockspace-bench-core`, what an arm links: `FfiBenchCall`, `timed!`, `timed_calibrated!`, `abi_hash`, `byte_routine_dispatch!`. An arm cdylib exports `bench_entry`, `bench_name`, `bench_abi_hash`, `bench_output_size`; `#[bench_variant]` generates them. A hand-written arm adds `extern "C" fn bench_output_size(n: usize) -> usize` or is refused. An arm whose output size differs from the resolved routine's buffer is refused; a bench missing from `routine_for` falls through to bytes at `[dispatch] out`, unnoticed if the sizes match.
- `mockspace-bench-harness`, transport: cdylib-per-arm isolation, subprocess driver, workloads, validation, analysis, history, codegen (`MatrixSpec` / `expand` / `render` / `generate`). It does not judge honesty. A driver timing arms from a shared target dir runs from a staged build: `<dir>/release/`, `binaries.sha256`, `build.txt`; `staging::StagedBuild::of_current_exe` refuses an unstaged driver, `verify` checks digests, `link_into` (feature `stage-link`) returns a `StageLink` removed on drop.
- `mockspace-bench-matrix`: any comparison over more than one axis.

What the framework already does, never re-rolled: per-arm release, fat LTO, one codegen unit, own subprocess; a realistic workload around the call; validation over deterministic seeds before timing (a mismatch is the finding); disassembly dedup; quintiles, autocorrelation, bootstrap CIs, sign test, Benjamini-Hochberg; per-run `summary.tsv` compared only within one machine class.

## Hardware counters

`PerfSnapshot` has `instructions`, `cycles`, `ipc()`. `cache_misses` and `branch_misses` are unwired zeros: never report them. Only with feature `perf-counters`, Apple Silicon macOS, as root; check `setup()` returned true. `teardown()` on every exit path.

## The matrix layer

- One `bench_matrix!` block (`name`, `crate_path`, `crate_dep`, `extra_deps`, `seed`, `sweep`, `sizes`, `baseline`, `floor`, `regime` = `warm` | `cold_cycle(M)`) in a `pub mod bench` of the library, plus a short generator binary. Each `(sweep value, cell, size)` is its own cdylib and subprocess.
- Cells are real typed functions, passed as a generic parameter, never a `fn` pointer.
- The scaffold owns, never reintroduced: the anti-hoist `acc` chain, the shared sixteen-seed table, one keep-alive per iteration, the setup/iteration split (breakeven `k* = (S_b - S_a) / (I_a - I_b)`), the fidelity digest. Pick `warm` or `cold_cycle(M)` by the deployment and say which.

## Configuration

- Root `{mock_dir}/benches/bench.toml`: `[timing]`, `[dispatch]`, `[build]`, `[workload.*]`, optional `[bench.<name>]`, and membership in `[benchspace]` (`members = ["**"]` default, `exclude = []`; globs or literal dirs, a literal must exist).
- A bench's `bench.toml` has fields at top level: `title`, `workload`, `arms`, `points`, `master_seed`, `may_differ`, `required`, `threaded`, `baseline`, `floor`, `delta`, and a `timing` override (`passes`, `runs_per_pass`, `batch_size`, `harness_runs`, `cooldowns_ms`, `validation_seeds`, `determinism_check_seeds`). `[sweep.<name>]` overrides both.
- Set `may_differ = false`, `required = true` unless stated otherwise. An expensive arm lowers the seed counts; zero is refused.

## Deliverable

- Each run lands in `results/<bench>/<sweep>_n<point>/<machine class>/<run>/` (`samples.csv`, `meta.json`, `report.md`, `summary.tsv`, hook output), run = `<UTC start>-<commit>`. Written once, tracked, never replaced; reruns sit beside it. Output anywhere else is refused by the lint `bench-results-live-in-the-harness-layout`.
- The recorded commit is `-dirty` only when a file the arms build from differs.
- A findings note in `{mock_dir}/research/`: what was measured, the numbers with run directory and profile, what it decides and what it does not. Quote the absolute delta, not only the ratio.

A feasibility question is a sketch (the `sketching` skill); a question with both halves splits.
