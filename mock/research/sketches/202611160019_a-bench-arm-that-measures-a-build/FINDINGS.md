# Sketch findings: a bench arm that measures a build from clean

**Date:** 2026-11-16
**Outcome:** WORKS
**Unblocks:** a build time can be taken in the bench harness and not with a clock beside it, so the two measurements that wait on this (a layout settled at build time, and the compile time of a product over a large registry) have a place to be made.

The sketch was written unattended by a dispatched seat on 16 November 2026. The dispatcher opened the call records of the second passing run and of the control, the samples of the second passing run, and the end of the control's driver output, before writing this file, and reran nothing. Nothing here is authority.

Every statement is about the version one tree, the live code consumers run: the engine at the checkout root, `bench-core` and `bench-harness`. The version two tree under `mock/` has no bench harness of its own and was not used.

## Hypothesis

The bench harness can run an arm whose work is to build a fixture cargo workspace from clean, and record one build time per repetition, each repetition from clean. Whether the build contends with the harness's own target directory was part of the question and is not answered here: the arm's check for a wait on cargo's lock never fired and has no control, so it shows nothing either way.

## The vehicle

The arm shells out to cargo and the harness is unchanged. No shipped file is edited. `consumer/mock/` is a consumer's bench tree held inside the sketch. Its bench `build-from-clean` has one arm, `from_clean`, and a fixture workspace of four library crates in a diamond with no dependency outside the fixture, a committed lockfile and a toolchain pin. Its bench `build-without-clean` has the control arm. `support/clean-build/` does the work for both, and `src/lib.rs` is the hooks library.

One call of `from_clean` runs in three parts. The setup, untimed, removes the fixture's target directory under the shared build directory and checks that every fixture library is absent. The timed block, measured by the harness's own counter, is `cargo build --workspace --offline --locked --message-format=json` into that directory. The teardown, untimed, counts cargo's own messages for fixture crates it compiled and did not find fresh, checks every library is present, looks for cargo's lock-wait message, and appends one line to a call record.

The hooks empty the call record at the start, copy it into the cell's run directory as `calls.tsv`, and fail the cell if any call compiled other than four crates, found a library present before the build or missing after it, waited on a lock, or saw cargo fail.

`run.sh` runs the engine built from this checkout over the consumer tree with `bench run <bench>`. The runner refusal the engine applies to a bench tree ran at every attempt and did not refuse this one. The repository declares no canon, so its design documents stand in; the seat read the harness's own README and the bench layout note and found nothing that forbids an arm from starting a process.

## What was tried, in order

1. `build-from-clean` on a first layout of the tree. Bench exit 0, run `20261116T002225Z-2764b23-dirty`. Fourteen calls, each compiling four crates into an empty directory; six timed rows (`out/a1-*`). Not opened by the dispatcher. The script's own exit 1 there is a trailing `cat` of a file the hook had already moved.

2. `build-without-clean`, the control, started with the fixture's build directory removed by hand. Run `20261116T002449Z-2764b23-dirty`. The first call compiled four crates; calls 2 to 14 compiled none and found the libraries present before the build. The harness measured the no-op builds without complaint (median 22.57 ms). The hook failed the cell naming each of lines 2 to 14, the driver printed that the summary is withheld and exited 1, and the run directory has no `summary.tsv` (`out/a2-01-bench-run.txt`, and the run's `calls.tsv`). This is the case that must fail, and it does.

3. `build-from-clean` on the final tree. Bench exit 0, run `20261116T002537Z-2764b23-dirty`. It started with the control's built target still present; the first call removed it. All fourteen calls show four crates compiled, libraries absent before and present after, no wait on a lock, exit 0. `samples.csv` has six timed rows of one build each: 126.1, 114.1 and 127.6 ms in the harness's warm mode, 133.0, 112.5 and 141.1 ms in its cold mode. The part of each call outside the timed block is 1.9 to 3.6 ms, which is the removal of the target directory; the seat reports 38 microseconds for the control, which removes nothing.

The first run's outputs were written by an earlier `run.sh` on a tree that differs from the staged one: the support crate moved, its fixture path changed, the control bench was added, and one comment in a `bench.toml` was rewritten. The third run is the one on the final tree. After a reviewer's read, the worktree's absolute path was folded to `<worktree>` in three output files (`a1-01`, `a2-01`, `a3-01`); two others name the shared build directory as the tools printed it; nothing else in any output was edited after its run.

## The result

The harness as shipped runs an arm that builds a fixture workspace from clean and records one build time per timed call, in its own results layout. That each call built rests on the hook reading cargo's own record of what it compiled, and an arm that skips the clean is caught and its summary withheld.

The timings were taken to show the shape and decide nothing about any real build.

Holds for: the version one engine at `2764b23`, built in debug from this checkout; the bench crates from git `dev` at `2764b23`; toolchain `nightly-2026-05-28`; arms and driver at the harness's default release profile (opt-level 3, fat LTO, one codegen unit); the fixture of four dependency-free library crates built in cargo's default dev profile, offline and locked; the fixture's target directory a sibling of the harness's own profile directories under the shared build directory; `passes = 3`, `runs_per_pass = 1`, `batch_size = 1`, `cooldowns_ms = [0]`, both harness modes, one arm per bench; Linux, x86_64, four cores, machine class `linux-x86_64-c4-container-intel`; a tree with the sketch files uncommitted.

## What the vehicle costs to keep

Each worker makes an untimed warm-up call and the run opens with a warm-up spawn, so fourteen full builds give six timed samples. The warm and cold modes run the same clean build, and the report reads one mode. The run's `meta.json` records the arm's build profile and not the profile of the build being measured, so that lives only in the arm's code. A bench of one arm gets no validation and no determinism check from the harness, so the proof that each call built rests wholly on the hook. A worker is stopped after 300 seconds where no per-call limit is set, and a worker's warm-up and timed builds must fit inside that.

A kind of arm for builds in the harness would drop the warm-up builds and the doubling by mode and could record the measured build's own profile. It was not built.

## What is NOT established

- Contention with the harness's own target directory was not provoked: no call waited on a lock, and a fixture pointed at the harness's own profile directory was not tried.
- Anything larger than four small crates, a fixture with outside dependencies, a build longer than the worker limit, any other machine.
- A comparison of two arms, which a layout question needs, where the arms' outputs may differ.
- That the timed block excludes setup and teardown is read off the macro and off the size of the untimed part here; no committed test in `bench-core` asserts it.
- Whether the version two preset lint against starting processes at run time bears on such an arm. Version one does not run it.
