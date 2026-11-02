# Test timings on the bench machinery: which module serves which need

Ticket `the_game_s_test_runs_record_a_comparable_cost_per_test_and`, step 0. The bench harness (`bench-harness/src`) already collects, stores, reads and analyses timings. A test timing is fed through the same modules, extended where one lacks a field, and nothing is copied beside them. This map names, per need, the serving module and what it still lacks. Landed state is marked; the rest is the order the steps follow.

| Need | Module | Has | Lacks | Step |
|---|---|---|---|---|
| Where it ran: machine class, cores, load | `env.rs` (`EnvMeta`) | cpu, os, rustc, commit, time | machine class, cpus, load. Added: `machine_class`, `cpus_online`, `load_1m` | 1, landed |
| A work count the test did | `suite/junit.rs` | wall seconds per case | the `<marker> <unit> <n>` line from `system-err`. Added: `work_lines` | 2, landed |
| One row per test per run | `sample.rs` (`Sample`) | per-batch bench record, csv contract of four readers | a test row (test, member, flavour, secs, outcome, profile, threads, work). A sibling row type beside `Sample`, never columns bolted on the bench csv | 3 |
| The series, not the last value | `history.rs` (`HistoryEntry`, `append_in`) | append-only ledger per benchmark | per-cell key of test, class, flavour, profile; concurrent writers (shards by run id, spool on a rejected push) | 3, 4 |
| Baseline and outliers | `analysis.rs` (`median`, `bootstrap_ci_median`, `compare`, `bh_fdr_adjust`) | median, bootstrap CI, tests | a MAD modified z-score and the warming rule (under 8 clean samples flags nothing) | 5 |
| Verdicts held out of the aggregate | none | none | append-only verdict states (`clean`, `suspect`, `cleared`, `confirmed`) and `clear` / `confirm` with a required note | 6 |
| Shifts with their predicate | `history.rs` (`detect_regressions_window`), `report.rs` | regression window | a predicate line (class, flavour, profile, threads, load bound, windows, n); no shift across classes; loaded runs never a baseline | 7 |
| The write point | `suite/mod.rs`, beside `history.record_cases` | the selection cache, which stays as it is | the record call, after the planner work lands | 3 |

`suite/history.rs` keeps only tests at or over half of `heavy_after_secs` and rewrites on a large move. It is a selection cache, not a sample store, and is untouched.

The report is test wall time. Numbers that decide anything go through `mock/benches`.
