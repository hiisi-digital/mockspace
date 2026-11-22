---
date: 2026-11-16
scope: review round two of `work/bench-kinds` (head 7745033, base origin/dev 01ff5bc, dev now at e92d1c8 and the merge is clean), mockspace v1
verdict: MERGE AFTER FIXES
---

# Review of the bench kinds, round two

Canon gate passed: mockspace v1 declares no canon (no `mockspace.toml`, which is expected), so the governing text is op's three quotes in `mock/research/202611160800_bench-kinds.md:11,53,55`, and the branch's intent matches them. This read is not independent of round one, since I read that review before forming my own; the second independent read round one said was owed is still owed.

Test gate: no tautological or assertion-free test in the touched surface; every test body in `axes_tests.rs`, `system_tests.rs`, `kinds_tests.rs`, `tree_kind_tests.rs`, `bench_sources_tests.rs`, `bench_kinds_tests.rs`, the lint's `tests.rs` and `setup_tests.rs`, the inputs test and the three end-to-end binaries asserts a value with a control beside it. The gate fails on two sampled laws, named in findings 3 and 4. Results at the head: `mockspace-bench-layout` 15 of 15, `mockspace-bench-matrix` all pass, `mockspace-lint-rules` 633 of 633, `mockspace-bench-harness` 280 of 282 with the two known `disasm` failures, its `smoke` 6 of 6, the root crate's bench tests 126 of 129 with the three known `bench_builds` sweep failures, and `bench_tracking_points_e2e`, `bench_generated_e2e`, `bench_output_size_e2e` and `mock_test_benches_when_moved` all green.

The workspace's `pr-review` sweep reports the repository's history as contaminated: the bodies of pull requests 218 to 251 and commit 6a63b3d, which it already lists as ruled unrepairable and kept. None of it is this branch. Its eleven commits carry no trailer, byline or advert, and the branch lands by a local merge with no pull request, so this is recorded here and not counted against the branch.

## Verdict

MERGE AFTER FIXES. Findings 1 and 2 make `mock bench run` refuse a run that would build and measure fine; findings 3 to 5 are places where op's "not thoroughly and not in the analysis" is still true.

## Round one, claim by claim

1. Workspace-inherited crates: fixed. `src/bench_sources.rs:63-123` asks `cargo metadata` and walks the resolved path packages, adding each one's inherited root manifest (`:131-145`). `bench_sources_tests.rs:97` and the end-to-end test both use the `workspace = true` spelling, with the lint pack's own reading as the control. Finding 1 below is a new defect in that call.
2. Setup frozen: partly fixed. `setup:` lines now go into the lock and into the tracking source, and an unrelated root edit moves nothing. They lock the raw root tables, not the resolved values round one asked for (finding 4).
3. Unknown source: fixed. `axes::known` (`axes.rs:142`) keeps `""` and `-` out of every point, step and history, and `Sources::for_variant` (`kinds.rs:126-141`) refuses a tracking arm with no handed source. Tested with controls.
4. One definition of a point: fixed for a new point, still false for a source the code returns to (finding 3).
5. System asked of cargo: fixed. `system.rs:111-153`, with `system_tests.rs:56` covering a config file, a target table, the triple's variable, `RUSTFLAGS` applied and overridden, and `native` named by its CPU. Finding 5 is the case it does not reach.
6. Swallowed `plan` failure: fixed. `cmd_test` (`src/bench.rs:342-348`) and `inputs` refuse with the reason, and a `None` digest makes `mock test` run the tree, where `bench test` fails loudly (`src/entry/test.rs:212`, `:358-370`).
7. Stale first-version text and the defaulted kind: fixed in the lint and builtins; `init` and `add` take `--kind` and refuse without it. Some scaffold text is still stale (finding 6).
8. Tests: the inherited spelling, the unknown source, the window and the section variant end to end are now covered.
9. File sizes: `env.rs` is back to 460 and `bench_generated_e2e.rs` is split.
10. Dead code: `flagged_for`, `Regression`, `detect_regressions` and `meta_report.rs` are gone; a grep finds no reference left.

## Findings

1. `src/bench_sources.rs:65` [correctness, high]: `cargo metadata --offline` with no `--filter-platform` needs every package of every platform downloaded, and `cargo build` downloads only the host's. An arm whose graph has a dependency for another platform builds fine, and then `mock bench run` stops with "taking the tracking arms' sources" and refuses the whole run. Failure scenario: a tracking arm reaching any crate with a `[target.'cfg(windows)'.dependencies]` table, which `hilavitkutin-linking` has (`hilavitkutin/mock/crates/hilavitkutin-linking/Cargo.toml:17`). The same call fails on this repository's own `benches/variants/multiply-xor`. Probe below. Fix: pass `--filter-platform` with the triple the arms were built for (the host, or the target the system probe found), which also stops other platforms' path crates being digested. Add a test with an arm carrying a `cfg(windows)` dependency.
2. `src/bench_sources.rs:194-226` [correctness, medium]: `write_sources` takes `cargo metadata` for every tracking arm in the tree that has a manifest on disk, and ignores the run's filter (`wanted` in `run_generated`, `src/bench.rs:897`). Failure scenario: `mock bench run a` fails because tracking bench `b`, which has its own `Cargo.toml` and was never built on this machine, has no dependencies fetched, which with finding 1 is an error. It also costs one metadata call per unselected arm on every run. Fix: take sources only for the arms this run selects, which the driver alone reads anyway.
3. `bench-harness/src/axes.rs:239-246` [correctness, medium]: when the code returns to a source it had before, the run is read differently from the trail. `standing` compares the current row alone against the latest other point. `trail` compares every row of the returning source, old and new, against that same point. The module doc (`axes.rs:25-26`), the design note's amendment and the builtins (`SKILL.md`, "The run's label and the trail read the same points, every row of each") all say the two never disagree. They do. Probe below: history `a` at 100 three times, then `b` at 120, then a run back at `a` at 160. The run is labelled `REGRESSION` and `INDEX.md` shows the step as `FASTER`. This is the "run at an older revision" case op asked for in his third quote, so it is not a corner. Severity medium. Fix: decide what a return is. If it is the existing point, read it for `DRIFT` against `point_of(own, cur.source)`. If it is a step, read the step from the point including the run, as the trail does. Then make the doc state that. The test `the_run_s_label_and_the_trail_s_step_agree` (`axes_tests.rs:155`) samples only a brand-new source, which is why this is green. Add the returning case.
4. `lint-rules/src/a_bench_keeps_the_contract_of_its_kind/setup.rs:45-57` [spec, medium]: the setup lines freeze the root tables as written, not the values the bench runs under. Three consequences, each with a failure scenario:
   - A bench that overrides every `[timing]` field in its own `bench.toml` (`tree.rs:430-489` merges the override) still has its lock broken by an edit to the root `[timing]` it does not use.
   - An undeclared key is frozen as `absent`. The framework's defaults (`opt-level` 3, `lto` fat, `codegen-units` 1 at `src/bench.rs:767-771`, and the timing defaults in `config.rs`) can change in a mockspace upgrade, which moves every snapshot's real setup while its lock stays valid, and the run calls the result `DRIFT` of the machine. Writing the current default out explicitly breaks every lock though nothing changed.
   - The root is shared. One edit to the root `[timing]` breaks every snapshot in the tree at once (thirty in hilavitkutin). The refusal's remedy, "make the changed code a new bench beside it", cannot clear the old lock, and `freeze` refuses to rewrite it. The tree is left with no way out but reverting.
   Fix: lock the resolved values per bench, the merged timing, the profile values `profile_args_for` passes and the workload stages, so the line names what was run. `setup_tests.rs` never declares a per-bench override or relies on a default, so the sampled law is green.
5. `bench-harness/src/system.rs:86-94` [spec, medium]: the CPU is named only when the build's features differ from the host's baseline. The machine class is only os, arch, a power-of-two thread bucket, bare or container, and vendor (`env.rs:74-86`). Failure scenario: two 16-thread AMD machines of different generations, both building with default flags, are one system. The second machine's first run is read against the first machine's point and labelled `DRIFT`, and `INDEX.md` pools them in one column. Op asked that "each target system that runs the shit should also be considered a variant". Fix: always append the CPU `host_cpu` already resolves (`system.rs:151`), a name from rustc's closed list, so no free text reaches the label. `system_tests.rs:19` asserts the pooled form as correct and changes with the fix.
6. `src/bench.rs:1650`, `:1632-1634`, `:1666-1671`, `:202`, `:229`, `:310` [drift, low]: the scaffolded `README.md` that every consumer's tree receives says `mock bench add <bench> <arm>` scaffolds a bench, which is now refused without `--kind`. It describes the lock as the files alone, with no setup lines, the tracking source with no setup, and the system as "the machine class and any target", with no CPU or features. Three refusals still say "Run `mock bench init` first", which now refuses without `--kind`. Fix the text in this branch.
7. `src/bench.rs` (2302 to 2537), `bench-harness/src/config.rs` (1750 to 1821), `bench-harness/src/matrix.rs` (605 to 629), `tree.rs` (1512 to 1535), `driver/mod.rs` (1181 to 1209) [rules, low]: files already past 500 lines grew. `src/bench.rs` is ticketed (6f9261da377b); the rest are not. `src/bench_sources.rs` and `bench_kinds.rs`, both new and small, were the right home for the additions to `src/bench.rs`; `run_generated`'s new probe and sources block (`:1055-1078`) belongs in `bench_sources`.

## Verified

- No path skips, deduplicates or refuses a run because data exists for its point, revision or system. Every row is read back, and unknown rows are counted in the trail view.
- `freeze` writes once and refuses to rewrite. `migrate --kinds` declares, then freezes against the file it just wrote. The lint and `run` ask one predicate (`bench_kinds`).
- A tracking arm's source is the same in another checkout, moves with an inherited crate, the root manifest, a moved branch or a newer release, and does not move with a test file or the instrument.
- The builtin rule and the skill match the source except the sentence named in finding 3.
- No em-dashes, bold or italics in the added lines. The eleven commit subjects are `type: lowercase imperative`, under 72 characters, with no trailers or bylines.
- The branch merges cleanly onto dev at e92d1c8.

## Probe for finding 1

An ad-hoc spike checking one existence claim, outside the repository. Its source and output are kept here as the record.

```
Cargo.toml: [package] name = "arm" ... [workspace]
            [target.'cfg(windows)'.dependencies] windows-sys = "=0.48.0"
src/lib.rs: pub fn f(){}

cargo build                                   -> exit 0
cargo metadata --format-version 1 --offline   -> exit 101, "attempting to make an HTTP request, but --offline was specified"
cargo metadata ... --offline --filter-platform x86_64-unknown-linux-gnu -> exit 0
```

The same `--offline` call against `benches/variants/multiply-xor/Cargo.toml` in this worktree fails with the same error.

## Probe for finding 3

A binary depending on `mockspace-bench-harness` at the head, built in a scratch crate.

```rust
let hist = vec![row(1, "a", 100.0), row(2, "a", 100.0), row(3, "a", 100.0), row(4, "b", 120.0)];
let cur = row(9, "a", 160.0); // interval +-2 ns throughout
label_for(&standing(&[cur], &hist), "fnv");          // the run's label
step(&t[len-2], &t[len-1]) where t = trail(hist + cur) // the INDEX step
```

Output:

```
run label: Some("REGRESSION")
trail step b->a: Faster
| S | fnv | `a` | 4 | 100 ns |  |
| S | fnv | `b` | 1 | 120 ns | REGRESSION |
| S | fnv | `a` | 4 | 100 ns | FASTER |
```
