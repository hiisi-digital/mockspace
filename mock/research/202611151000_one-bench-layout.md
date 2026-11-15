---
date: 2026-11-15
scope: where every bench result and its history live, one layout for every bench and every machine
status: built on `fix/bench-one-layout`; the two lints and `mock bench migrate` ship with it
---

# One bench layout

Bench results ended up wherever the runner that wrote them chose. The generated driver wrote
`results/<bench>/<sweep>_n<point>.csv` with a meta and a report beside it, and a ledger to
`history/<bench>/` or `.bench_history/`. Consumer-owned drivers, the escape hatch a `Cargo.toml` at
the bench root opened, wrote the same names into their working directory, into `runs/run1/`, or into
a member's own directory. No name carried the machine or the run, so a re-run on any machine replaced
the files of the last one in place, and the only trace of the replacement was the `cpu` field of a
meta nobody reads. In hilavitkutin every committed result carries `Apple M1`, and a re-run in a
four-core Xeon container would have overwritten all of them with prose that read as before.

The intent this answers, in op's words: the bench data ends up in one and the same place for all
benches, written under new or old conventions, so it is trackable over time; custom runners are
disallowed outright rather than reconciled, because the reconciling cannot be done without fault; and
results anywhere else are refused by a builtin lint that names the migration where one is
deterministic.

## The layout

```text
<mock>/benches/results/<bench>/<sweep>_n<point>/<machine class>/<run>/
    samples.csv   meta.json   report.md   summary.tsv
```

The run is `<UTC start>-<commit>`, `20261115T093000Z-1a2b3c4-dirty`, taken once per `mock bench run`
so every cell of one run shares it. The machine class is `EnvMeta::machine_class`, the coarse label the
harness already computed and never used for anything (`linux-x86_64-c4-container-intel`). A run
directory is written once: `promote` refuses a target that exists, and a run id already taken on the
machine class gets a `-2` suffix, so nothing is replaced, ever.

Why these four levels and in this order. The cell first, because every reader asks about one cell: the
regression check, the index and the docs all start from a cell and fan out. The machine class next,
because a timing is only comparable with timings from the same class of machine, and grouping by it
makes that the shape of the tree rather than a filter every reader has to remember. The run last,
because it is the only part that grows. A sortable UTC stamp at the head of the run id makes a
directory listing oldest first without reading anything.

Why the class and not a host name. The class carries no hostname, user or serial (it is built from
closed vocabularies and buckets, which its own tests hold), so committing it publishes nothing about a
machine. Two machines of the same class pool their history, which is what the class is for; a run on a
container and a run on bare metal do not.

## History is the runs

The append-only ledger is gone. Each run writes its own `summary.tsv`, in the ledger's own schema, and
a cell's history is the summaries of its runs on the same machine class. Two reasons. A ledger appended
on two machines conflicts on every merge, while two run directories merge without touching each other.
And the ledger mixed machines, so the regression check compared an M1 against a Xeon and flagged the
difference as a regression; reading by class makes that impossible. A run an `after_cell` hook failed
promotes its samples as evidence and writes no summary, so it never becomes a baseline, which is what
the withheld ledger append did before.

`results/INDEX.md` is derived from the runs alone and regenerated from them, so two machines'
checkouts produce the same index once both runs are in, and a report-only pass keeps every row.

## Runners

The generated driver is the only runner. `mock bench run` refuses, and the builtin lint
`a-bench-runs-only-on-the-generated-driver` errors on, a `Cargo.toml` or a `src/main.rs` at the bench
root, any crate in the tree with a `[[bin]]`, `[[bench]]`, `src/main.rs`, `src/bin/` or `benches/`, and
any shell script in the tree. `driver::drive` and `DriverRegistry`, the surface a consumer's own
`main` called, are deleted; what a runner did goes in the hooks library.

## Older layouts and the migration

The builtin lint `bench-results-live-in-the-harness-layout` errors on every bench output outside a run
directory. The grammar both the lint and the harness read is one crate, `mockspace-bench-layout`, so
what the driver writes is by construction what the lint accepts.

`mock bench migrate` translates exactly four older shapes, the ones whose bench and cell the path
spells out: the harness's flat `results/<bench>/<sweep>_n<point>.*`, a root driver's
`<bench>_n<point>.*` at the bench root, `.bench_history/<bench>_n<point>.tsv`, and
`history/<bench>/<sweep>_n<point>.tsv`. A result triple moves byte for byte into the run its meta's
`timestamp` and `git_commit` name. A ledger splits into one `summary.tsv` per run it recorded, a run
being the rows sharing a timestamp and a commit, each row as written.

The machine is the one fact the old files do not hold. A ledger never recorded one, and a meta older
than the class has only `cpu` and `os`. `--machine-class` supplies it, and the migration checks it
against every meta of the same cell, refusing a class whose os or cpu family disagrees with what the
meta kept. Without the flag those files stay where they are and are named. Everything else (a
member's own results, `runs/`, a meta whose samples are gone, a target already holding other bytes) is
listed for moving by hand, and the command exits non-zero while anything is left. It was run against
hilavitkutin's committed files, copied into `tests/fixtures/bench-migrate/hilavitkutin/`.

## The dirty mark

`-dirty` meant `git status --porcelain` said anything about the whole tree, so the run's own results,
generated files a repository forgot to ignore and kaski's two tracked paths differing only in case
each marked every later timing dirty; arvo's gate refused 255 of 256 timings for it with no code
edited. The mark is now asked of the bench tree's build inputs, the set `mock test` already keys the
bench tree on: the tracked files under the bench tree less every bench output, every local crate a
bench manifest reaches by `path`, and the workspace manifests, lockfiles, toolchain pin and cargo
configuration. Untracked files never count. The tool computes it and hands it to the driver in
`MOCKSPACE_BENCH_COMMIT`; a driver run by hand falls back to any tracked change in the tree.

## What this leaves for consumers

hilavitkutin, kirjo and arvo each carry a root driver crate and results at the bench root, so all
three are refused until they port their drivers to hooks and migrate. arvo's own lint
`a-committed-timing-records-what-built-it` reads metas at the bench root and has to read them from run
directories instead.
