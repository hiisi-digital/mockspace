---
date: 2026-11-16
scope: review round one of `work/bench-kinds` (head 540fa27, base origin/dev 01ff5bc), mockspace v1
verdict: MERGE AFTER FIXES
---

# Review of the bench kinds, round one

Canon gate passed: mockspace v1 declares no canon (no `mockspace.toml`, expected), so the governing text is op's three quotes in `mock/research/202611160800_bench-kinds.md:11,53,55`, and the branch's intent matches them. This is the first read; a second independent read is owed before any claim that the vehicle fits his intent.

Test gate: the suite is real (no tautologies found), but it fails the gate on setup that helps and on a sampled law (finding 8), and the green it shows is how finding 1 shipped. The five reds are pre-existing: run on a worktree of 01ff5bc, `disasm::tests` fails the same two and `bench_builds` the same three.

## Verdict

MERGE AFTER FIXES. Findings 1 to 5 are blockers, each one a place where op's second quote ("not thoroughly and not in the analysis") is still true.

## Findings

1. `lint-rules/src/a_bench_keeps_the_contract_of_its_kind/reach.rs:356` [correctness]: a tracking arm's source never follows a `workspace = true` dependency. It is classed `Elsewhere` and not walked, and `src/bench_sources.rs:47` drops every lockfile package without a `source`, which is every path package, so the lockfile does not catch it either. hilavitkutin, the consumer this was built for, spells its siblings exactly this way (`mock/crates/hilavitkutin/Cargo.toml:10-15`: `hilavitkutin-kit`, `notko-hlist` and the rest are `.workspace = true`). An edit to `hilavitkutin-kit` leaves every tracking arm at its old point and gets read as `DRIFT`. The probe below shows it. The fix is to stop hand-resolving: after the build, take the path packages from `cargo metadata --manifest-path <arm>` (each `manifest_path`) and digest them as built, together with the workspace root manifest each one inherits from, since its `[profile]` and `[patch]` change the code too.
2. `lint-rules/src/a_bench_keeps_the_contract_of_its_kind.rs:81`, `reach.rs:80` [spec]: the snapshot lock does not freeze the setup. Only the bench's own `bench.toml` is locked. The root `[workload.*]` the bench names, `[timing]` and `[build]` (`opt-level`, `lto`, `codegen-units`) sit outside it, and in hilavitkutin every workload lives in the root file (`mock/benches/bench.toml:28-66`). Op asked for "static bench setup". As shipped, a changed workload or opt level is reported as `DRIFT` of "the machine, the toolchain or the harness" (`bench-harness/src/kinds.rs:141`), which is the wrong attribution. Lock the resolved values the bench uses, not the whole root file, so an unrelated edit does not break every lock. Fold the same values into the tracking source.
3. `bench-harness/src/history.rs:243`, `bench-harness/src/driver/mod.rs:39,81` [correctness]: a row whose source is unknown is still treated as a point. A pre-`schema_v2` row reads `""` and an arm with no handed source writes `-`. `axes::trail` and `standing` treat both as real sources, which has two effects. All of a consumer's committed tracking history, across every commit, pools into one point, which is the swing op wanted gone. And the first run of every snapshot after migration (`""` to its lock id) is labelled `REGRESSION` or `FASTER` "of the code" when no code moved. An unknown source has to be its own state, never a point and never a step. Where `mock bench run` handed sources, a tracking arm that finds none should be refused rather than silently written as `-`.
4. `bench-harness/src/axes.rs:221` with `history.rs:183` [correctness]: there are two definitions of a point. `standing` compares against `detect_regressions`, which uses only the last 5 rows, while `trail` and INDEX use every row (`axes.rs:147-153`). So the run's label and the INDEX step can disagree on the same data, and "every run adds to its point" is false at run time once a point has more than 5 rows. Build `standing` from the same `Point` that `trail` builds.
5. `bench-harness/src/env.rs:166-170` [correctness]: the system axis is a hole, not an acceptable limit. It does not read `[build] rustflags`, `[build] target` or `[target.<triple>] rustflags` from config files, nor `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`. Cargo ignores `RUSTFLAGS` when `CARGO_ENCODED_RUSTFLAGS` is set, yet both are read, so the system can name flags that were never applied. And `target-cpu=native` names a flag rather than a CPU, so two CPUs that share one machine class (vendor and core bucket) pool. A config file is the usual home of `target-cpu`, so one line there pools two systems with nothing said. Ask cargo for the effective configuration (the pinned nightly has `cargo -Zunstable-options config get`), or record what `rustc --print cfg` reports under the effective flags (the `target_feature` set and the resolved CPU).
6. `src/entry/test_benches.rs:240`, `src/bench.rs:334` [drift]: in an unmigrated consumer, `bench_gen::plan` now fails, and both sites swallow the failure. `cargo mock test` and `bench test` then stop generating the arm manifests for the whole tree without a word. These should fail with `no_kind_reason`. The rest of the rollout holds: the hard-error lint blocks every commit in all seven consumers on the pin bump, `migrate --kinds` is the complete way through, and v1 summaries parse. Finding 3 is what hits at the first run after it.
7. `src/bench.rs:1425`, `lint-rules/src/a_bench_keeps_the_contract_of_its_kind.rs:12` [drift]: both still describe the first version ("history is read across commits"), which the amendment replaced with source points. The scaffold at `src/bench.rs:1427` also writes `kind = "snapshot"` by default, against the note's "No kind is defaulted"; make `mock bench add`/`init` take the kind.
8. `lint-rules/src/a_bench_keeps_the_contract_of_its_kind/tests.rs:371`, `tests/bench_tracking_points_e2e.rs:87` [tests]: setup that helps. Both feed only plain `path =` dependencies and never the `workspace = true` spelling the real consumer uses, which is why finding 1 is green. `axes_tests.rs:86` exercises `standing` with at most 2 history rows, so the window in finding 4 is never entered. Missing entirely: an unknown (`""` or `-`) source, a `[bench.<name>]` variant's source lookup (`bench_sources.rs:95`) end to end, and the system read from a config file.
9. `bench-harness/src/env.rs` (460 to 514 lines), `tests/bench_generated_e2e.rs` (482 to 510) [rules]: both crossed 500 lines on this branch. `system` belongs with the axes, not with machine detection.
10. `bench-harness/src/history.rs:173`, `meta_report.rs` [drift]: `flagged_for` and `Regression::render` have no caller left outside tests, and `meta_report.rs` pools whatever it is handed and has no caller at all. Delete them now, before a caller arrives.

## Verified

- No path skips, deduplicates or drops a run because data exists: run directories are refused only on collision (`staging.rs:132`), and every row is read back (`run_rows`, `load_cell`).
- `standing` and the views never compare across systems: `on::<System>` in `standing`, one column per system in `render_systems`, a split by system in `render_trails`. Tests carry the control.
- `freeze` writes once and refuses to rewrite, the lint names each file that changed, is new or is gone, and a lock beside a tracking bench is refused.
- Older summaries parse, with empty `source` and `system`, and `system` is filled from the machine directory.
- The builtins match the code, except the claim of "every file it builds from, in the tree and out" (`templates.rs` +67, `SKILL.md` +14), which finding 1 makes false.
- No em-dashes or styling in the added lines. The four commit subjects are in shape, with no bylines.
- Tests: the bench, layout, matrix and lint crates pass apart from the 2 pre-existing disasm failures; the root crate's bench tests, `bench_generated_e2e` (2) and `bench_tracking_points_e2e` (1) pass.

## Probe for finding 1

Built against `lint-rules` at 540fa27. It is an ad-hoc spike that checks one existence claim, and its source is kept here as the record.

```rust
// mock/benches/live/arms/a takes engine by path; engine takes kit by `kit.workspace = true`,
// kit declared `{ path = "crates/kit" }` in mock/Cargo.toml [workspace.dependencies].
let before = code(root);                       // bench_arm_code(..).files
w(root, "mock/crates/kit/src/lib.rs", "pub fn g() { loop {} }\n");
let after = code(root);
w(root, "mock/crates/engine/src/lib.rs", "pub fn f() { 1; }\n"); // control
```

Output:

```
files read: ../crates/engine/Cargo.toml, ../crates/engine/src/lib.rs, live/arms/a/Cargo.toml, live/arms/a/src/lib.rs, live/bench.toml
kit edited, arm code moved: false
control, engine edited, arm code moved: true
```
