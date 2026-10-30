# Incremental checks: what the gate spends and what a saved verdict returns

This note records the measurements behind the saved-verdict work, the commands that produced them, and what they show. The gate is `cargo mock --lint-only --commit`, the run the pre-commit hook makes. Every figure is in `results.tsv` beside this note, which has one row per number below: `value` is wall seconds, except the `lint-ms/` rows, which are milliseconds for one lint. Each is one run on one container, so a difference under about ten percent is noise. The profile is the release engine and the packs as each repository pins them, with the engine pointed at this branch.

The three repositories were picked for how different their gates are. The large one has 86 crates, a 12800 row registry, and asset and shader lints in a pack of its own. The small one has four crates. The docs-only one has no Rust crates, so its gate is the registry and the changelist lints alone. They are private, so they are labelled by shape here.

## What a run was spending before

Per lint wall time from the engine's `MOCKSPACE_TIMINGS=1` table on the large repository, with nothing built and nothing staged (32 s in total on the tree of that morning). The first five lints below were 23 of those seconds, and none of them declared what it read, so none could be skipped.

| lint | calls | ms |
|---|---|---|
| `no-superseded-citation` (pack repo lint) | 1 | 10303 |
| `no-source-include-in-test` (pack repo lint) | 1 | 4859 |
| `bundles-are-fresh` (the large repo's own pack) | 1 | 3973 |
| `design-doc-source-mismatch` (built in, per crate) | 86 | 2860 |
| `single-source` (built in, workspace) | 1 | 1562 |
| `slang-fresh` (the large repo's own pack; 14 s once its shader compiler is on the path, 5 ms without) | 1 | 14034 |

The last row comes from a later run than the others, which had no shader compiler on the path. Building the engine and the packs for a pin is a separate cost paid once per version: 207 s cold on the large repository and 114 s on the small one. No hook pays it again, and the launcher keeps its branch resolution for an hour.

## What changed

Every lint may declare a reach: `File`, `Crate`, `Workspace` or `Repository`, the last being what a lint that declares nothing gets. The engine saves each lint's raw findings under a digest of what its reach covers, its configured parameters and path filter, and the engine and pack binaries by content. Severity and allow markers are applied after replay. A repo lint, which is always the widest, saves per item through `RepoContext::remembered`.

## Where this departs from the brief

The brief asked for a work set per hook (staged files for pre-commit, pushed commits for pre-push) and a registry-only fast path. Neither is built, and op has not ruled on that. A work set only decides which files a lint is shown, and a content digest already makes an unchanged file free whichever hook asks, so a second mechanism would answer the same question twice and could disagree with the first. A registry-only path would skip lints that read code when only the registry moved, which is unsound: `no-superseded-citation` reads code and the registry, and a registry change that marks a row superseded creates a violation in code that did not move. With the code-reading lints saved by digest, a registry-only change already costs what the lints keyed on the registry cost.

## The numbers

Same tree and engine, store off against store on (`series.sh`, one store shared by the runs of a series):

| repository | full run | cold, store filling | warm, nothing moved | one line edited | edited, full |
|---|---|---|---|---|---|
| large | 53.6 | 51.9 | 6.0 | 7.4 | 53.6 |
| small | 1.04 | 1.47 | 0.49 | 0.40 | 1.04 |
| docs-only | 0.13 | 0.13 | 0.13 | 0.12 | 0.13 |

The full run column is the steady figure, the edited tree with the store off. The first run of each series is slower (79.3 s on the large repository) because it includes the launcher and engine start, and `results.tsv` keeps it as the `full` row.

The large repository goes from 54 s to 6 s warm and 7.4 s after a one line change in a Rust file, which is 9 and 7 times. The small and docs-only repositories have little to gain: their gate is a launcher start, the registry gate and a handful of lints, all under a second. The one line edit is not much above the warm run because what is left is not code reading: `single-source` (1.8 s, a workspace lint that reruns when any crate moves), the registry gate (1.1 s), and about thirty repo lints at 0.2 to 0.7 s each that declare nothing and run every time.

How the large repository's warm figure fell as each piece landed, each row the warm run on the same tree:

| saved | warm s |
|---|---|
| nothing (store off) | 48.7 |
| built in lints by file and crate | 40.4 |
| plus the shader lint per shader | 26.7 |
| plus the citation lint per file | 16.3 |
| plus the asset lints and the test include lint | 5.6 |

The last three rows come from lints in packs outside this repository. Without them the built in work alone returns the first improvement, 49 to 40 s, and no more, because the time is in the pack lints. Those pack changes are handed to the pack owners separately.

## What a saved verdict is, and what was checked

Equivalence is checked three ways. The lint runners have a test that runs a lint cold, warm and after an edit and compares each against an uncached run (`lint-rules/src/saving_tests.rs`). The declared reach of every built in lint is checked by moving one thing the lint claims not to read and comparing findings (`reach_check`, `reach_tests.rs`), and the test fails for any saved lint that finds nothing on its corpus, since a lint with nothing to say would pass trivially. That check found one real defect: `no-duplicate-fn` listed its findings in hash order, so a replay would have differed from a fresh run. And `tests/an_incremental_run_equals_a_full_one_over_a_history.rs` walks the first parent history of a repository, running the full and the incremental gate at each commit with one store shared by all of them, and compares the reports from the lint section on. It ran over the last eight commits of the large repository and six each of the other two, and the reports were identical.

An undeclared lint is never saved, so a pack that does nothing is exactly as correct as before. A declaration narrower than what the lint reads can replay a stale answer, which is why the declaration has a test that fails when it is.

## Commands

```
OUT=<dir> ENGINE=<cargo-mock> series.sh <label> <worktree> <file-to-touch>
OUT=<dir> timeit.sh <label> <dir> <cmd...>          wall seconds and exit code
MOCKSPACE_TIMINGS=1 cargo-mock --lint-only --commit
MOCKSPACE_NO_VERDICTS=1 cargo-mock --lint-only --commit        the full run
```

`timeit.sh` and `series.sh` are committed beside this note. None of it ran under the bench harness, so every figure here is an ad-hoc quick spike: it shows the size of the effect and the shape of where time goes, and is to be taken again under the harness before a number is used for a decision. The history walks ran at an earlier commit of this work and are to be run again at the final one.
