# Incremental checks: what the gate spends and what a saved verdict returns

This note records the measurements behind the saved-verdict work on `work/incremental-checks`, the commands that produced them, and what they show. The gate here is `cargo mock --lint-only --commit`, the run the pre-commit hook makes. Every number is wall seconds on one container, one run each, so a difference under about ten percent is noise. The profile is the release engine and the packs as each repository pins them, with the engine pointed at this branch.

The three repositories were chosen for how different their gates are. kaski is the large one: 86 crates, a 12800 row registry, asset and shader lints in a pack of its own. homma has four crates. applets is a Deno repository with no Rust crates, so its gate is the registry and the changelist lints alone.

## What a run was spending before

Per lint wall time from the engine's `MOCKSPACE_TIMINGS=1` table, on kaski, with nothing built and nothing staged (`kaski-timed-warm.out`, 32 s total on the tree of that morning). The five lints below were 22 of those seconds, and not one of them declared what it read, so none could be skipped.

| lint | calls | ms |
|---|---|---|
| `no-superseded-citation` (pack repo lint) | 1 | 10300 |
| `no-source-include-in-test` (pack repo lint) | 1 | 4860 |
| `bundles-are-fresh` (kaski pack repo lint) | 1 | 3970 |
| `design-doc-source-mismatch` (built in, per crate) | 86 | 2860 |
| `single-source` (built in, workspace) | 1 | 1560 |
| `slang-fresh` (kaski repo lint, 14 to 16 s once `slangc` is on the path) | 1 | 14100 |

Building the engine and the packs for a pin is a separate, once per version cost: 207 s cold on kaski (`kaski-before-cold.time`), 114 s on homma. Nothing in a hook pays it again, and the launcher's branch resolution is kept for an hour.

## What changed

Every lint may now declare a reach: `File`, `Crate`, `Workspace` or `Repository`, the last being what a lint that declares nothing gets. The engine saves each lint's raw findings under a digest of what its reach covers, its configured parameters and path filter, and the engine and pack binaries by content. Severity and allow markers are applied after replay. A repo lint, which is always the widest, saves per item through `RepoContext::remembered`.

The brief asked for a work set per hook (staged files for pre-commit, pushed commits for pre-push) and a registry-only fast path. Neither was built, and the reasons are measured rather than assumed. A work set only decides which files a lint is shown, and a content digest already makes an unchanged file free whichever hook asks, so a second mechanism would answer the same question twice and could disagree with the first. The registry-only path would skip lints that read code when only the registry moved, and that is unsound: `no-superseded-citation` reads code and the registry, and a registry change that marks a row superseded creates a violation in code that did not move. With the code-reading lints saved by digest, a registry-only change already costs what the lints keyed on the registry cost, which the table below shows as the warm run.

## The numbers

Same tree, same engine, store off against store on (`measure/series.sh`, one directory per repository, the store shared by the runs of one series):

| repository | full run | cold, store filling | warm, nothing moved | one line edited | edited, full |
|---|---|---|---|---|---|
| kaski | 53.6 | 51.9 | 6.0 | 7.4 | 53.6 |
| homma | 1.04 | 1.47 | 0.49 | 0.40 | 1.04 |
| applets | 0.13 | 0.13 | 0.13 | 0.12 | 0.13 |

kaski goes from 54 s to 6 s warm and 7.4 s after a one line change in a Rust file, which is 9 and 7 times. homma and applets have little to gain: the gate there is a launcher start, the registry gate and a handful of lints, all under a second. The kaski one line edit is not a smaller number than the warm run by much because what is left is not code reading: `single-source` (1.8 s, a workspace lint that reruns when any crate moves), the registry gate (1.1 s), and about thirty repo lints at 0.2 to 0.7 s each that declare nothing and run every time.

How the kaski warm figure fell as each piece landed, each row the warm run on the same tree (`kaski-after-warm`, `-slang-warm`, `-sup-warm`, `-all-warm`):

| saved | warm s |
|---|---|
| nothing (store off) | 48.7 |
| built in lints by file and crate | 40.4 |
| plus `slang-fresh` per shader | 26.7 |
| plus `no-superseded-citation` per file | 16.3 |
| plus `bundles-are-fresh`, the two asset lints and `no-source-include-in-test` | 5.6 |

The last two rows are patches to repositories this branch cannot push to, `patches/mockspace-extra-lints.patch` and `patches/kaski-lints.patch`. Without them the built in work alone returns the first improvement, 49 to 40 s, and no more, because the time is in the pack lints.

## What a saved verdict is, and what was checked

Equivalence is checked three ways. The lint runners have a test that runs a lint cold, warm and after an edit and compares each against an uncached run (`lint-rules/src/saving_tests.rs`). The declared reach of every built in lint is checked by moving one thing the lint claims not to read and comparing findings (`reach_check`, `reach_tests.rs`); it found one real defect, `no-duplicate-fn` listing its findings in hash order, which would have made a replay differ from a fresh run. And `tests/an_incremental_run_equals_a_full_one_over_a_history.rs` walks the first parent history of a repository, running the full and the incremental gate at each commit with one store shared by all of them, and compares the reports from the lint section on. The results of those walks are in the report that came with the branch.

An undeclared lint is never saved, so a pack that does nothing is exactly as correct as before. A declaration that is too narrow is the way to get a stale answer, which is why the declaration has a test that fails when it is.

## Commands

```
measure/timeit.sh <label> <dir> <cmd...>        wall seconds and exit code
measure/series.sh <name> <worktree> <file>      full, cold, warm, edit, edited full
MOCKSPACE_TIMINGS=1 cargo-mock --lint-only --commit
MOCKSPACE_NO_VERDICTS=1 cargo-mock --lint-only --commit        the full run
```

`timeit.sh` and `series.sh` are committed beside this note, and `raw/` holds the `.time` and `.out` files the tables above were read from. None of it ran under the bench harness, so every figure here is an ad-hoc quick spike: it shows the size of the effect and the shape of where time goes, and is to be taken again under the harness before a number is used for a decision.
