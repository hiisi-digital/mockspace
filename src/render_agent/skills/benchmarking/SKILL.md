# Benchmarking

Any number that feeds a decision comes from `{mock_dir}/benches/`. Never hand-roll an `Instant::now()` loop.

## Workflow

1. `cargo mock bench init` once: root `bench.toml`, a `README.md`, a `sample` bench to copy.
2. `cargo mock bench add <bench> [<arm>]` (or `<bench>/<arm>`): one bench is one question, its arms the competing answers.
3. Arm code in `<bench>/arms/<arm>/src/lib.rs`, inside `timed! { run { ... } }`. Its manifest is generated; a hand-written `Cargo.toml` there takes over. Shared code: `<bench>/support/` or root `support/`, plain library crates.
4. `<bench>/bench.toml`: `title`, `workload`, `arms`, `points`, `baseline` (every delta is against it), `floor` (null-cost arm, subtracted first).
5. `cargo mock bench run [<bench>...]`: generates the driver, builds each arm and the driver with the pinned release profile in the workspace's shared target, validates, measures, writes the run, then removes the arm and driver builds it made (`--keep-builds` keeps them for a second run). `cargo mock bench list` shows what is registered.
6. Write the findings note (below) and commit it with the run.

An arm that has no path of its own for a target (a SIMD arm with no kernel there) runs the code that target does have, under the same arm name. That result is valid and wanted: on that target the approach buys nothing as the code stands. Never rename the arm, gate it off the target or drop its run for it. When the path for the target is written, the next run shows the win.

## The two kinds

Every bench declares one in its `bench.toml`, and the kind decides what it may build from and how its numbers are read. The lint `a-bench-keeps-the-contract-of-its-kind`, and `run` and `report` before they build, refuse a bench with no kind or out of its contract.

- `kind = "snapshot"`: isolated reference code that stays as it is. Every arm builds only from crates and files inside `{mock_dir}/benches/`: its own `arms/`, its own `support/`, a root `support/` crate an arm's own manifest names by path. No crate outside the tree, no git or registry dependency (the `mockspace-bench-*` crates are the instrument and do not count). A generated arm manifest takes only the bench's own `support/`. `cargo mock bench freeze <bench>` writes `<bench>/snapshot.lock` (a section's is `<section>.snapshot.lock`): the sha256 of every file it builds from, its `bench.toml` included. Those files never change after; changed reference code is a new snapshot beside the old one. Its source is the lock's id.
- `kind = "tracking"`: built against living code, at least one arm reaching a crate outside the bench tree (the repository's own crates, in the usual case). No lock. Each arm's source is the digest `run` takes of every file it builds from, followed out of the tree through its path dependencies (manifest, build script, `src/`), and of the versions its other dependencies resolved to in the lockfile its build left. Uncommitted edits count; the commit does not.
- Variant axes (`mockspace_bench_harness::axes`): every summary row records its `source` and its `system` (machine class, plus `target=`, `target-cpu=`, `target-feature=` from `CARGO_BUILD_TARGET` and the rustflags it was handed). Rows of one source on one system are one point; every run adds to its point, at any commit, an older one included, and nothing is skipped or deduplicated as already measured. A run at the point its history stands at is read for `DRIFT` (machine, toolchain or harness); a run at a new point against the point before it, `REGRESSION` or `FASTER`. Another system is another variant: never compared, never pooled, shown side by side in `results/INDEX.md` and `BENCHES.md`, each with its trail of points.
- `report.md` opens with the kind, the system and how to read it. Compare arms within a run freely; compare runs only along the axes.
- A tree from before the kinds: `cargo mock bench migrate --kinds [--dry-run]` writes the kind each bench's reach admits and freezes the snapshots.
- A matrix family declares its kind in `bench_matrix!` (`kind: "snapshot"`), by what its carrier crate reaches.

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
- `cargo mock bench freeze [<bench>...]`: writes the lock of each snapshot that has none; refuses to rewrite one its files no longer match.

## The layers

- `mockspace-bench-core`, what an arm links: `FfiBenchCall`, `timed!`, `timed_calibrated!`, `abi_hash`, `byte_routine_dispatch!`. An arm cdylib exports `bench_entry`, `bench_name`, `bench_abi_hash`, `bench_output_size`; `#[bench_variant]` generates them. A hand-written arm adds `extern "C" fn bench_output_size(n: usize) -> usize` or is refused. An arm whose output size differs from the resolved routine's buffer is refused; a bench missing from `routine_for` falls through to bytes at `[dispatch] out`, unnoticed if the sizes match.
- `mockspace-bench-harness`, transport: cdylib-per-arm isolation, subprocess driver, workloads, validation, analysis, history, codegen (`MatrixSpec` / `expand` / `render` / `generate`). It does not judge honesty. A driver timing arms from a shared target dir runs from a staged build: `<dir>/release/`, `binaries.sha256`, `build.txt`; `staging::StagedBuild::of_current_exe` refuses an unstaged driver, `verify` checks digests, `link_into` (feature `stage-link`) returns a `StageLink` removed on drop.
- `mockspace-bench-matrix`: any comparison over more than one axis.

What the framework already does, never re-rolled: per-arm release, fat LTO, one codegen unit, own subprocess; a realistic workload around the call; validation over deterministic seeds before timing (a mismatch is the finding); disassembly dedup; quintiles, autocorrelation, bootstrap CIs, sign test, Benjamini-Hochberg; per-run `summary.tsv` compared only within one variant, one system and along its source points.

## Hardware counters

`PerfSnapshot` has `instructions`, `cycles`, `ipc()`. `cache_misses` and `branch_misses` are unwired zeros: never report them. Only with feature `perf-counters`, Apple Silicon macOS, as root; check `setup()` returned true. `teardown()` on every exit path.

## The matrix layer

- One `bench_matrix!` block (`name`, `kind`, `crate_path`, `crate_dep`, `extra_deps`, `seed`, `sweep`, `sizes`, `baseline`, `floor`, `regime` = `warm` | `cold_cycle(M)`) in a `pub mod bench` of the library, plus a short generator binary. Each `(sweep value, cell, size)` is its own cdylib and subprocess.
- Cells are real typed functions, passed as a generic parameter, never a `fn` pointer.
- The scaffold owns, never reintroduced: the anti-hoist `acc` chain, the shared sixteen-seed table, one keep-alive per iteration, the setup/iteration split (breakeven `k* = (S_b - S_a) / (I_a - I_b)`), the fidelity digest. Pick `warm` or `cold_cycle(M)` by the deployment and say which.

## Configuration

- Root `{mock_dir}/benches/bench.toml`: `[timing]`, `[dispatch]`, `[build]`, `[workload.*]`, optional `[bench.<name>]`, and membership in `[benchspace]` (`members = ["**"]` default, `exclude = []`; globs or literal dirs, a literal must exist).
- A bench's `bench.toml` has fields at top level: `kind` (required, never in a sweep), `title`, `workload`, `arms`, `points`, `master_seed`, `may_differ`, `required`, `threaded`, `baseline`, `floor`, `delta`, and a `timing` override (`passes`, `runs_per_pass`, `batch_size`, `harness_runs`, `cooldowns_ms`, `validation_seeds`, `determinism_check_seeds`). `[sweep.<name>]` overrides both.
- Set `may_differ = false`, `required = true` unless stated otherwise. An expensive arm lowers the seed counts; zero is refused.

## Deliverable

- Each run lands in `results/<bench>/<sweep>_n<point>/<machine class>/<run>/` (`samples.csv`, `meta.json`, `report.md`, `summary.tsv`, hook output), run = `<UTC start>-<commit>`. Written once, tracked, never replaced; reruns sit beside it. Output anywhere else is refused by the lint `bench-results-live-in-the-harness-layout`.
- The recorded commit is `-dirty` only when a file the arms build from differs.
- A findings note in `{mock_dir}/research/`: what was measured, the numbers with run directory and profile, what it decides and what it does not. Quote the absolute delta, not only the ratio.

A feasibility question is a sketch (the `sketching` skill); a question with both halves splits.
