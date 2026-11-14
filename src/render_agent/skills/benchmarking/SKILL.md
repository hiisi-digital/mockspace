# Benchmarking

A bench answers a measurement question: which is faster, what does it cost, where is the band boundary. Any number that feeds a decision comes from here. Invoke before the first timing loop; never hand-roll an `Instant::now()` loop.

## The three layers

- `mockspace-bench-core` (what a variant links): `FfiBenchCall`, the `timed!` and `timed_calibrated!` macros, `abi_hash`, `byte_routine_dispatch!`.
  - A variant cdylib exports `bench_entry`, `bench_name`, `bench_abi_hash`, `bench_output_size`; the driver finds them by dlsym.
  - A variant whose `bench_output_size(n)` differs from the output buffer of the routine the bench resolved is refused before it runs. A bench missing from the `routine_for` table falls through to the byte routine at `[dispatch] out` bytes.
  - `#[bench_variant]` generates the export on rebuild. A hand-written arm (including every one `bench add` scaffolded before the export existed) adds `extern "C" fn bench_output_size(n: usize) -> usize` itself, or it is refused.
  - The check sees only a size that differs: a bench missing from `routine_for` whose output is the byte routine's size falls through to bytes unnoticed.
- `mockspace-bench-harness` (transport): cdylib-per-variant isolation, subprocess driver, workload programs, validation, analysis, history, and the spec-to-variant-crate codegen (`MatrixSpec` / `expand` / `render` / `generate`). It does not judge whether a measurement is honest.
- A driver that times arms out of a target directory other worktrees build into runs from a staged build: `<dir>/release/` holding the driver and its arms, `binaries.sha256` (`sha256sum`'s format, paths inside `<dir>`) and `build.txt` beside it. `mockspace_bench_harness::staging::StagedBuild::of_current_exe` refuses an unstaged driver and `verify` checks the digests, the running driver's among them; `link_into`, behind the `stage-link` feature, is built only on the targets the harness's `build.rs` names and returns a `StageLink` that removes the link when dropped. The consumer writes the staged build.
- `mockspace-bench-matrix` (the discipline): use it for any comparison across more than one axis.

## Commands

- `cargo mock bench init` scaffolds; `add <name>` adds a variant; `run [names...]` measures; `report` regenerates findings from cache; `list` prints what is registered; `test` runs `cargo test` in every crate under the bench tree.
- `test` walks the tree directly (a bare `cargo test` at the root sees only the driver crate and reports a misleading `0 passed`). A crate name narrows to the crates whose path contains it and a name matching nothing is refused with the list of what exists. Flags after the subcommand go to cargo (`--release`/`-r`, `-- --test-threads=1`); a value-taking flag's value (`--profile <name>`) travels with the flag.
- Every test count printed carries the profile it ran under. Report a timing or count with its profile.

## Why the framework

- Per-variant cdylib isolation: release, fat LTO, one codegen unit, own subprocess. Two variants in one binary share a codegen unit and the optimiser can erase the distinction under test.
- A shared realistic workload: scalar dependency chains, pointer chases, cache pressure and branchy context surround the measured call.
- Validation before timing: every variant runs across deterministic seeds, each in its own worker subprocess. A mismatch is the finding: the implementations drifted and the timing comparison is meaningless.
- Automatic disassembly dedup: identical `bench_entry` machine code across variants is reported.
- Statistics: quintiles, per-cooldown breakdown, lag-1 autocorrelation as a drift signal, bootstrap CIs on the median and on paired differences, a sign test, Benjamini-Hochberg FDR adjustment.
- History: every run appends timestamp, git commit, variant, N, mode, median and CI bounds to an append-only log; a later run flags entries whose CI no longer overlaps the baseline.
- Do not re-roll timing helpers, bump providers, stat collection or validation.

## Hardware counters

- The PMU says why a variant won (same instructions, fewer branch mispredicts).
- `PerfSnapshot` carries `instructions`, `cycles` and `ipc()`. Only those two fixed counters are wired. `cache_misses` and `branch_misses` are zero placeholders: never report them; zero means not wired.
- Available only with the `perf-counters` feature on Apple Silicon macOS, with the framework present, as root, and a passing support probe. `setup()` returns whether it armed. Fields are zero when unavailable, so check `setup()` before concluding.
- `teardown()` must run on every exit path. The PMU is a process-global claim; an early return that skips it breaks the next process.

## The matrix layer

- A semantic matrix declares a family, cells to compare, a sweep axis (one bench per value) and a size sweep. Each `(sweep value, cell, size)` is its own cdylib in its own subprocess.
- `bench_matrix!` emits the cell functions and `matrix_decls()` from one block carrying `name`, `crate_path`, `crate_dep`, `extra_deps`, `seed`, `sweep`, `sizes`, `baseline`, `floor` and `regime` (`warm` or `cold_cycle(M)`). Write it in a `pub mod bench` of the consumer library crate (sibling variant crates name cells by path), then a short generator binary.
- Cells are real typed functions, not strings. Fat LTO inlines across the consumer-crate boundary into the timed region.
- A cell is a generic parameter, never a `fn` pointer: a pointer adds an indirect call the deployment does not have. `scaffold::warm` and `scaffold::cold_cycle` enforce it.
- Disciplines the scaffold owns (do not reintroduce them elsewhere):
  - Anti-hoist chain: `acc` seeded from `output[0]`, one keep-alive folded per iteration and written back inside the timed loop.
  - Shared seed table: a fixed sixteen-entry table identical across sizes and cells; never draw seeds from the input buffer.
  - One keep-alive per iteration, not per node (an O(N) checksum in the inner loop dilutes every ratio).
  - S-versus-I split: `setup` is a required argument bracketed by counter reads; every cell reports per-iteration and setup cost, so breakeven `k* = (S_b - S_a) / (I_a - I_b)` is computable.
  - Reps-invariant fidelity digest: a separate digest on a fixed-seed, fixed-init single pass, on its own channel. It is a smoke check; the authoritative anchor is the consumer's byte-exact cross-validation `#[test]`s.
  - Cold regime: `cold_cycle(M)` cycles M distinct programs so the branch predictor cannot memorise one dispatch sequence. Warm measures the memorised-single-program shape; cold the many-distinct-inputs shape. Pick the deployment's, and say which.

## Configuration

- `{mock_dir}/benches/bench.toml` is the root: globals `[timing]`, `[dispatch]`, `[build]`, `[workload.*]`, optional `[bench.<name>]` sections.
- Each bench is a directory with its own `bench.toml`, fields at top level (no wrapper table; the directory name is the bench name). Optional `[sweep.<name>]` sections carry per-sweep points and overrides.
- Arms live in `<bench>/arms/<arm>/`, one measured cdylib each. `<bench>/support/` and root `support/` hold ordinary library crates the arms link.
- Membership is declared in the root `[benchspace]` table:

```toml
[benchspace]
members = ["**"]        # default when the table is absent
exclude = []
```

- `members` takes globs or literal directory names. `*` matches within one path component, `**` any number including zero. A literal entry must exist with its own `bench.toml` (a typo is an error naming it); a pattern may match nothing. A matched member owns its interior: a nested `bench.toml` belongs to that member. Nothing is inferred from a directory's contents or name.
- A member's `[timing]` overrides the root's only for the knobs it declares; a `[sweep.*]` override outranks both.
- Per-bench keys: `title`, `workload`, `arms`, `points`, `master_seed`, `may_differ`, `required`, `threaded`, roles (`baseline`, `floor`, `delta`), and a `timing` override of `passes`, `runs_per_pass`, `batch_size`, `harness_runs`, `cooldowns_ms`, `validation_seeds`, `determinism_check_seeds`. Points may carry their own arm subset. The legacy spellings (`variants`, `sizes`, a `normalise` table) are still accepted.
- Validation runs every arm over `validation_seeds` seeds and reruns the first `determinism_check_seeds` of them. An expensive arm lowers them instead of skipping validation. Zero is refused, as is a determinism count above the validation count; an undeclared determinism count follows a lowered validation count down.
- Set `may_differ = false` and `required = true` unless there is a stated reason otherwise.
- Each point is its own monomorphisation. With the generated driver the point list defaults to the union of every bench's points and `[dispatch] points` narrows it; only a consumer-owned driver binary maintains `byte_routine_dispatch!` by hand.

## Deliverable

- `results/` (CSV, meta, per-size findings) is tracked, never gitignored, including uninteresting runs.
- A decision-facing findings note in `{mock_dir}/research/`, written for the round that asked: what was measured, the numbers, what it decides, and what it does not decide (shapes not covered, what would change the answer, unchecked extrapolation). Name what dominates the measured loop and quote the absolute delta, not only the ratio, when one expensive call compresses it.

## Boundary against sketching

A sketch checks possibility and lives in `{mock_dir}/research/sketches/` (the `sketching` skill). A sketch reaching for a timer becomes a bench; a question with both halves splits into one of each.
