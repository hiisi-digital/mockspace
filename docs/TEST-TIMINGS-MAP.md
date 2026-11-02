# Test timings on the bench machinery: which module serves which need

The bench harness (`bench-harness/src`) already collects, stores, reads and analyses timings. A test timing is fed through the same modules, extended where one lacks a field, and nothing is copied beside them. This map names, per need, the serving module and what it still lacks. A row marked planned is not built yet.

| Need | Module | Has | Lacks | State |
|---|---|---|---|---|
| Where it ran: machine class, cores, load | `env.rs` (`EnvMeta`) | cpu, os, rustc, commit, time | machine class, cpus, load. Present: `machine_class`, `cpus_online`, `load_1m` | present |
| A work count the test did | `suite/junit.rs` | wall seconds per case | the `<marker> <unit> <n>` line from `system-err`. Present: `work_lines` | present |
| One row per test per run | `sample.rs` (`Sample`) | per-batch bench record, csv contract of four readers | a test row (test, member, flavour, secs, outcome, profile, threads, work). Present: `test_sample::TestSample`, a sibling row type beside `Sample`, never columns bolted on the bench csv | spool present |
| The series, not the last value | `history.rs` (`HistoryEntry`, `append_in`) | append-only ledger per benchmark | per-cell key of test, class, flavour, profile; concurrent writers (shards by run id, spool on a rejected push) | spool present, publish planned |
| Baseline and outliers | `analysis.rs` (`median`, `bootstrap_ci_median`, `compare`, `bh_fdr_adjust`) | median, bootstrap CI, tests | the verdict store. Present: `test_baseline::judge`, a MAD modified z-score and the warming rule (under 8 clean samples flags nothing) | statistic present, verdicts planned |
| Verdicts held out of the aggregate | none | none | append-only verdict states (`clean`, `suspect`, `cleared`, `confirmed`) and `clear` / `confirm` with a required note | planned |
| Shifts with their predicate | `history.rs` (`detect_regressions_window`), `report.rs` | regression window | a predicate line (class, flavour, profile, threads, load bound, windows, n); no shift across classes; loaded runs never a baseline | planned |
| The write point | `suite/mod.rs`, beside `history.record_cases` | the selection cache, which stays as it is | Present: `suite/timings.rs` `record_run`, called once the report is read, spooling under the test state directory; publishing the spool to a tracked shard is planned | spool present |

`suite/history.rs` keeps only tests at or over half of `heavy_after_secs` and rewrites on a large move. It is a selection cache, not a sample store, and is untouched.

The report is test wall time. Numbers that decide anything go through `mock/benches`.
