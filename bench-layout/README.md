# `mockspace-bench-layout`

> Where the mockspace bench harness keeps every run it measures, written down once.

Every run of a bench lands in its own directory under the bench tree's `results/`, keyed by the bench, the cell, the machine class and the run, so a run never replaces an earlier one and runs from different machines sit side by side:

```text
results/<bench>/<sweep>_n<point>/<machine class>/<run>/
    samples.csv  meta.json  report.md  summary.tsv
```

The run is the UTC time the run started and the commit it measured, `20261115T093000Z-1a2b3c4`. The history of a cell is the set of its runs, read across them, so there is no separate ledger to append to or to conflict on a merge.

This crate holds that grammar and nothing else: building the names, parsing them back, and telling a path the harness wrote apart from one an older layout left behind. The harness writes through it, `mock bench migrate` translates through it, and the lint that refuses results anywhere else reads through it, so the three cannot disagree about where a result belongs.

## Licence

MPL-2.0.
