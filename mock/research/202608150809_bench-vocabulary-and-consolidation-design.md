# Bench Vocabulary and Consolidation Design

The design derived from the survey one file back
(`202608150648_bench-ergonomics-survey.md`). The survey's counts are the
evidence base and are not restated except where a number decides a choice; new
measurements taken for this design are marked as such. This is a proposal and
the maintainer ratifies. The maintainer's three directions are quoted where
they bind: the cohesive structure with sane overridable defaults, named hooks
instead of one fat entrypoint, and per-bench nesting with results in one
shared tree ("I think results want to be in one shared dir though, per bench
there, so it can structure sensibly in one place").

Order of presentation: the empirical question first (what the shared arms
are), because the vocabulary falls out of it, and the layout falls out of the
vocabulary.

## 1. What the 36 shared arms actually are, opened and classified

The maintainer's suspicion, verbatim: "A variant shared across many seems
wrong to me, but that might be just because the variant concept has been used
for something else than being a variant."

All 36 arvo arms that appear under more than one `[bench.*]` section were
enumerated and classified (new measurement; full list in the appendix).
Grouping the 49 sections by any shared arm yields **seven closed families
plus 11 singletons, and no arm crosses a family boundary anywhere**:

| family | sections | shared arms |
|---|---|---|
| warm-container | 10 | 6 |
| warm-clamp | 8 | 6 |
| satfold | 7 | 9 |
| wide-rung | 5 | 5 |
| bitpack-footprint | 3 | 4 |
| bitpack-contend | 3 | 5 |
| bitpack-write-contend | 2 | 1 |

hilavitkutin's single shared arm is the same shape: `dispatch_direct` under
both `dispatch_static` and `dispatch_dynamic`, one dispatch family. vehje
shares 0 of 820.

Classification of the 36 by role, from names and the driver's own comments:

- **7 are baselines, controls, or references.** Three say so by name
  (`bench_bitpack_contend_d16_control`, `bench_bitpack_footprint_packed_naive`,
  `bench_warm_container_native`, the last documented as "plain Rust primitive
  arithmetic" at `arvo .../src/main.rs:295-298`). Four more carry the role in
  the driver's comments: the two footprint `dense` arms exist in the
  head-to-head section "so a packed-against-dense delta exists at all"
  (`main.rs:500-501`), `bench_bitpack_write_dense` is the dense side of the
  write family, and `bench_satfold_seq`, the only arm in all seven satfold
  sections, is the sequential reference every fold strategy is priced against.
- **29 are ordinary competitors** (`satfold_lanes16`, `satfold_neon`,
  `warm_clamp_acc64`, `wide_rung_ragged`, ...) that recur because the sibling
  sections sweep the same comparison along different axes: width at L1, width
  at L2, density at one width, alignment, DRAM residency.

So the hypothesis that these are mostly mis-labelled baselines is right in
kind and wrong in proportion. Roles exist and want names (section 2), but the
dominant phenomenon is different: **"bench" currently means two things.** A
section like `warm-container-width-l1` is not an independent comparison; it is
one axis sweep of the warm-container comparison, and the sections of a family
share arms because they are one bench. Named at the right granularity, arm
sharing is 0 of 81 in arvo, 0 of 82 in hilavitkutin, 0 of 820 in vehje.

The answer to the maintainer's "all variants are per bench anyway, no? By
design?" is therefore: **yes, by design, and the tree only appeared to say
otherwise because the config file spells one bench as many.** The role
phenomenon is real but orthogonal: a baseline is an arm with a declared role,
not an arm that lives somewhere else, and the harness already half-knows
this: `[bench.<name>.normalise]` names a `baseline` and a `floor` arm
(`bench-harness/src/config.rs:145-162`), vehje declares 157 baselines and 114
floors, and 96 of vehje's arm directories carry role names (`*_null_sink`,
`*_naive`, `*_scalar_ref`).

## 2. The vocabulary

Each entry: what it is, what it is not, where it lives. Derived from what
exists; renames are proposed only where a current name is wrong by the
one-term-per-concept rule, and each rename carries its cost.

**bench.** One named comparison: one question, one competitor set, one or
more sweeps, its own directory. What it is not: one `[bench.*]` section of
today's manifests, which is usually a sweep. Lives at `mock/benches/<bench>/`.
The word is kept; its granularity is corrected. arvo's 49 sections become 18
benches (7 families + 11 singletons); hilavitkutin's 29 become 28; vehje's
180 map one-to-one until someone chooses to group them (sharing 0 arms, vehje
loses nothing by not grouping).

**sweep.** One axis instantiation of a bench: its own point list, its own arm
subset where it needs one, its own `may_differ` / `required` / `threaded` /
`normalise` / timing override, its own result rows. What it is not: a bench.
Lives as a `[sweep.<name>]` section in the bench's own `bench.toml`; a bench
with one sweep may omit the name (a default sweep named after the bench).
This is the new word that dissolves the shared-arm problem; today's
`[bench.warm-container-width-l1]` becomes bench `warm-container`, sweep
`width-l1`.

**arm.** One measured implementation, compiled to a cdylib, dlopened and
timed in isolation. Replaces "variant". What it is not: a support crate, and
not a role. Lives at `mock/benches/<bench>/arms/<arm>/`. Basis for the
rename: the maintainer suspects the word, and the workspace's own standing
rules already use "arm" for exactly this concept throughout
(`evidence-lives-in-the-repo-or-it-never-happened.md`: "the arms must be the
alternatives someone might genuinely choose"; `the-test-gate.md`: "an arm
compared against itself"), so today the same concept has two names split
between the rules and the harness, which is the exact defect `vocabulary.md`
exists to prevent. Cost: the `variants/` directory name, the `variants =`
manifest key, and prose; the config schema accepts `arms` as canonical with
`variants` as a compat alias during migration. Deliberately not renamed: the
CSV schema's `variant` column and the `#[bench_variant]` attribute (1064
files), which are data format and API surface; new scaffolds may emit a
`#[bench_arm]` alias, old files are untouched, and report-only keeps reading
committed CSVs unmodified.

**role: baseline, floor.** Declared properties of an arm within a sweep, not
kinds of arm and not locations. `baseline` is the arm deltas are computed
against; `floor` is the null-cost arm subtracted from every arm first. Both
already exist as config (`config.rs:146-162`) and are promoted to vocabulary.
A control arm (a naive or reference implementation present so the comparison
is honest) is a baseline or floor by declaration; nothing new is built for it.
Lives in `[sweep.<name>] baseline = "x"` / `floor = "y"` (today's
`[bench.<name>.normalise]` table flattened; the `normalise` table name
disappears in the new schema since the two fields are the whole content, and
`mode` joins them as `delta = "subtract" | "ratio" | "percent" | "none"`).

**support crate.** A library crate that arms and the driver link and that is
never measured itself: the routine types, input builders, shared algorithm
cores. Today signalled only by a `-shared` name suffix (13 in arvo, 1 in
vehje). What it is not: an arm; it must not be a cdylib and must never appear
in an arm list. Lives at `mock/benches/<bench>/support/<name>/` when one
bench's arms use it, `mock/benches/support/<name>/` when several benches do
(vehje's `vehje-bench-carrier`, linked by 609 arm crates, is the existing
cross-bench case). Discovery distinguishes arm from support by location, not
by suffix, which closes the survey's finding that only naming convention
separates them today.

**routine.** The contract for what is computed: input construction from a
seed, output shape, validation semantics, ops count. Kept as is. A **bridge**
is one monomorphised adapter of a routine (`routine_bridge!`), internal
vocabulary, kept.

**workload.** The named program of surrounding context the timed call is
embedded in (scalar chains, pointer chases, cache pressure, branchy filler).
Kept as is. Declared in `[workload.<name>] stages = [...]` at the root; two
builtins ship, `default` (the starter's light mix) and `realistic` (the
six-stage mix hilavitkutin and vehje independently copied,
`hilavitkutin .../main.rs:193-206`, `vehje .../main.rs:17-30`).

**point.** The integer parameter of one cell, today `n`. The rename is
earned by the evidence that it is not a size anywhere but the hash benches:
arvo packs `W*1000 + NC*100 + D` and `N*10 + T` into it
(`arvo .../main.rs:470-471, 517-520`), vehje's small values are batch widths
and field counts (`vehje .../main.rs:45-47`). The new schema key is
`points = [...]`; `sizes` stays a compat alias. The `_n<point>` artifact
filename convention is kept (renaming it would orphan every committed
artifact name for zero information gain).

**cell.** One measured unit: one (bench, sweep, point) with its resolved
arms, producing one samples file, one meta, one report. Today this is the
anonymous `BenchConfig` loop iteration. The word is already in use for
exactly this in `bench-matrix` ("many isolated cells, each timed
individually", `bench-matrix/src/lib.rs:8`). Rust type names
(`BenchConfig`) follow in the reimplementation, not in this document.

**samples, meta, report.** The three per-cell artifacts. `samples` is the
raw CSV; `meta` the environment record beside it; **report** replaces
"findings" for the rendered analysis file. Basis: the code already calls it
a report everywhere (`generate_report`, `write_report_for_routine`,
`report.rs`) while the filename says findings, a same-codebase split of one
concept; and "findings" collides with what this workspace means by findings,
a human-authored deliverable. New trees write `<sweep>_n<point>_report.md`;
committed `_findings.md` files are grandfathered where they stand.

**history.** The append-only per-cell ledger of medians across runs
(timestamp, commit, median, CI), input to regression detection and docgen.
Two mechanisms, deliberately, as established in the survey: `results/` is a
replaceable snapshot with provenance, `history/` is a ledger that is never
overwritten. Lives in the shared output tree (section 3).

**hooks.** The named consumer extension points, section 4. `routine_for` and
`after_cell`.

Not renamed, explicitly: bench (the word), routine, workload, bridge, the
timing knobs (`passes`, `runs_per_pass`, `batch_size`, `harness_runs`,
`cooldowns_ms`), `results`, `master_seed`, `may_differ`, `required`,
`threaded`, the CSV schema, the `#[bench_variant]` attribute, the
`_n<point>` filename convention, and the artifact triple's stems. A
vocabulary pass that renamed those would be trading familiarity for churn
with no wrongness to correct.

## 3. The layout

Inputs nest per bench; outputs live in one shared tree that structures per
bench inside it, per the maintainer's call. A bench's directory then contains
only what a human authors, and everything generated lands in one place that
is uniformly browsable, cleanable, and (for `results/`) safely deletable.

```
mock/benches/
  bench.toml                  # globals: [timing] [tuning] [dispatch] [build] [workload.*]
  src/lib.rs                  # optional hooks crate (manifest generated if absent)
  support/<name>/             # support crates used by more than one bench
  <bench>/                    # one bench, one directory, never flat
    bench.toml                # [bench] title etc + [sweep.<name>] sections
    arms/<arm>/src/lib.rs     # measured cdylibs; Cargo.toml optional (escape hatch)
    support/<name>/           # this bench's support crates
  results/                    # generated; the driver is the only writer
    <bench>/<sweep>_n<point>.csv
    <bench>/<sweep>_n<point>.meta.json
    <bench>/<sweep>_n<point>_report.md
    void/<runid>/             # quarantined crash-borne runs (existing staging)
    INDEX.md                  # cross-bench run summary (existing)
  history/
    <bench>/<sweep>_n<point>.tsv
  target/                     # tool-owned build tree, ignored
  README.md
```

Discovery rules, so nothing is ambiguous to the tool or a reader:

- A first-level subdirectory of `mock/benches/` is a bench iff it contains a
  `bench.toml`. `support/`, `results/`, `history/`, `src/`, `target/` are
  reserved names and never benches.
- A directory under `<bench>/arms/` is an arm. A directory under either
  `support/` is a support crate. Arm versus support is decided by location;
  the tool refuses an arm whose crate is not a cdylib and a support crate
  that is, with an error naming the rule.
- An arm directory containing only `src/lib.rs` gets its manifest generated
  (name from the directory, `[workspace]` header, cdylib, pin-matched deps,
  support deps from `[sweep]`/`[bench]` declarations). An arm with its own
  `Cargo.toml` is built as found.
- A tree with no bench subdirectories and a flat root `bench.toml` with
  `[bench.*]` sections is a legacy tree and runs in compat mode, unchanged.

Why `history/` sits beside `results/` rather than inside it: both are
generated, but `results/` is safe to delete and regenerate while `history/`
is append-only and tracked (`feedback_bench_history_is_tracked`). Separating
them at the top level keeps "deletable" and "never delete" from sharing a
root, so `rm -rf results/` remains a safe operation. The dotfile name
`.bench_history/` is retired for new trees (tracked data should be visible
data); 13 committed documents across arvo and vehje cite `.bench_history`
paths, so for existing trees the old directory stays in place and continues
to be appended to until that tree migrates, and the reader accepts both
locations. No citation breaks in either mode.

The keep-list stands: arvo's five flat artifacts cited by ten research files
stay at the flat root untouched; `mock bench migrate` moves everything else
into `results/<bench>/` by the name-preserving mapping, with the keep-list
passed explicitly. vehje's five `results/`-path citations are unaffected
outright, since `results/` does not move under this design.

Sweep-name continuity: for singleton benches the sweep name equals today's
section name, so their artifact filenames are unchanged. For family benches
the artifact name drops the family prefix into the directory
(`warm-container-width-l1_n80003.csv` becomes
`warm-container/width-l1_n80003.csv`); the migrate command records the
old-name to new-path mapping in the tree so any uncited stale reference is
mechanically recoverable.

## 4. The hooks, derived line by line

Method, per the brief: every statement of the two hand-rolled drivers (arvo
672 lines, hilavitkutin 311 + 355 + 8) classified as (a) library-subsumed,
(b) consumer-specific and load-bearing, or (c) wrong. The full ledger is in
the survey's section 1b; the residue after deleting (a) and (c) is the hook
set, and it has exactly two members.

Classification residue:

- arvo: the `routine_for_n` table (`main.rs:206-595`) and nothing else.
  Lines 16-201 and 597-672 are the loop and worker the library driver
  subsumes; the wrong class is the flat output paths, the hand-kept
  validation and disasm wiring, and the manual path shaping arvo already
  removed after it caused the double-prefix outage.
- hilavitkutin: the disasm_5check pass with its exit-code policy
  (`main.rs:141-183`, `180-183`, plus `src/disasm_5check.rs`) and nothing
  else. The wrong class is `shape_variant_path` (`main.rs:251-260`,
  duplicating `resolve_variant_path`) and the hardcoded 13-name `may_differ`
  list (`main.rs:213-216`, duplicating a manifest flag that exists and that
  hilavitkutin's manifest never set: zero `may_differ` keys in its
  `bench.toml`, a new count).
- Workload construction appears in every driver (arvo `main.rs:50-53`,
  hilavitkutin `193-206`, vehje `17-30`, kirjo, the starter) and every
  observed program is a composition of the six harness stage constructors
  with integer arguments, so it migrates to `[workload.*]` config rather
  than to a hook. A workload hook would have zero residual callers after
  migration and is not proposed; a workload needing a custom stage primitive
  has never occurred and is served by the full-crate escape hatch if it ever
  does.

On the maintainer's sketched `on_init` / `after_init`: he said he does not
know what would be done in them, and the evidence answers: nothing, today.
Zero statements in 1581 lines of consumer driver code run before the
manifest loop or after it other than what the library driver already owns.
The intent (named hooks, harness-controlled order, no fat entrypoint) is
kept in full; the sketched names are replaced by the two with callers, per
the workspace's intent-versus-vehicle rule. The `Hooks` struct below grows a
field additively the day an init-time caller exists; shipping the field
before the caller is the speculative surface this design is meant to remove.

### The two hooks

```rust
pub struct Hooks {
    /// Serve a custom routine for a cell whose input is not plain bytes.
    /// None falls through to the generated byte dispatch.
    pub routine_for: Option<fn(&BenchConfig) -> Option<RoutineSpec>>,
    /// Run after a cell's artifacts are staged; verdict feeds the exit code.
    pub after_cell:  Option<fn(&AfterCell<'_>) -> CellVerdict>,
}

pub struct AfterCell<'a> {
    pub config:    &'a BenchConfig,   // bench, sweep, point, flags, seed
    pub result:    &'a BenchResult,   // the samples just collected
    pub arm_paths: &'a [PathBuf],     // resolved cdylibs, validation survivors
    pub dropped:   &'a [String],      // arms validation removed
    pub out_dir:   &'a Path,          // this cell's staged results directory
}

pub enum CellVerdict {
    Pass,
    Note(String),   // recorded in the summary, does not fail the run
    Fail(String),   // recorded, fails the process exit code after promotion
}
```

`routine_for` is the driver's existing registry slot kept under its existing
name (`bench-harness/src/driver/mod.rs:63`). Callers: arvo's 360-arm table
(`main.rs:206-595`, collapsing roughly ten to one under a `routine_table!`
macro, the survey's proposal) and mockspace's own self-bench `HashMix`
(`mockspace/benches/src/main.rs:15-31`).

`after_cell` is new. Caller: hilavitkutin's 5-check pass, line for line. It
receives everything that pass reads today (bench name and point from
`config`, resolved paths from `arm_paths`, an output directory it currently
improvises as cwd, `main.rs:147`), writes its extra artifact into `out_dir`
(so `_5check.md` lands in `results/<bench>/` and gains the staging
transaction it currently lacks), and returns `Fail` for the
`dispatch_static` regression case and `Note` for the expected
counter-example case, reproducing the exit-code policy of `main.rs:157-183`
without a consumer-owned loop. The second in-repo signal for this slot is
the `NormaliseSection` TODO, which already anticipates "an optional
post_process code symbol receiving the collected BenchResult"
(`config.rs:137-144`).

No third hook earned a caller. If a review finds one in a tree this survey
missed, it joins the struct by the same rule: name, payload, and position
derived from the code that needs it.

### Ordering, which is the point of naming them

The generated driver owns the sequence; a consumer can no longer run its
custom pass at the wrong moment because it no longer owns a loop. Per run:

1. Config is parsed and validated (strict keys per PR #20). Cells are
   resolved: every (bench, sweep, point) with arms, flags, timing.
2. Everything is generated and built: driver crate, missing arm manifests,
   all cdylibs, with the tool's target dir and the pinned profile
   (PR #19). The build set is derived from the resolved cells, every entry
   shape included, which retires the filed `variant_dirs_for` staleness
   defect on this path.
3. Preflight: every missing cdylib across all cells reported at once.
4. Per cell, in manifest order: `routine_for` is consulted first and the
   generated byte dispatch second; the workload program is built from
   config; the disasm duplicate check runs; cross-arm validation runs and
   drops non-agreeing arms; the timed run executes; samples, meta, and
   report are written to the staged results tree.
5. `after_cell` runs. Guaranteed already true when it fires: the cell's
   samples, meta, and report exist in `out_dir`; validation survivors and
   drops are final; nothing of the cell will change after it returns.
   Guaranteed not yet true: staging is not promoted, history is not
   appended, so a `Fail` verdict can never poison the ledger with a run
   that will be reported as failed.
6. After all cells: staging promotes, history appends, the index and
   summary render, docgen runs where enabled. Exit code is the disjunction
   of `required` validation failures and `after_cell` `Fail` verdicts.

### The optional lib, discovery, absence, and unknown hooks

If `mock/benches/src/lib.rs` exists, the generated driver crate adds a path
dependency on it (manifest generated if the consumer wrote none) and calls
its `pub fn hooks() -> Hooks`. If the file is absent, the generated crate
uses `Hooks::default()` and the consumer owns zero Rust. If the lib exists
but exports no `hooks()`, the generated crate fails to compile with the
missing-function error naming the expected signature, and
`mock bench init --hooks` scaffolds the stub.

A hook the harness does not know is unrepresentable rather than detected:
hooks are fields of the tool-owned `Hooks` struct, so a consumer writing
`on_frobnicate: ...` gets the compiler's unknown-field error listing the
fields that exist. There is no stringly registration to typo. This is the
same mechanism as today's `DriverRegistry` (fn pointer struct,
`driver/mod.rs:57-68`) and the same compiled-into-the-binary reasoning the
survey established for why the lints cdylib shape cannot carry routines.

## 5. Config: the two-level bench.toml

Root `mock/benches/bench.toml` carries only what is global, all of it
overridable defaults (section 6). Per-bench `bench.toml` carries the bench
and its sweeps. A real one, arvo's warm-container family, today 10 sections
and roughly 620 manifest lines plus 66 driver match arms; under the design,
one file of roughly 60 lines plus 10 macro lines:

```toml
# benches/warm-container/bench.toml
[bench]
title = "Warm/Precise container rule against deletion, rung(W+1), and primitives"
workload = "realistic"
master_seed = 0x1234_5678_9ABC_DEF0
arms = ["kernel", "headroom", "minimum", "plusone", "native", "lanes-deferred"]
baseline = "native"

[sweep.width-l1]
points = [80003, 130003, 160003, 320003, 600003, 640003]

[sweep.width-l2]
points = [81003, 131003, 161003, 321003, 601003, 641003]
arms = ["kernel", "headroom", "minimum", "plusone", "native"]   # subset override

[sweep.density-w13]
points = [130001, 130002, 130004, 130008, 130016]

# ... one short section per remaining axis ...
```

Bench-level `arms` and `baseline` apply to every sweep; a sweep overrides
only what differs, which the survey measured as rare (257 of 258 sweeps
share their bench's arm set). The per-point arm override form remains valid
for the one bench that needs it. Everything here parses with today's
semantics plus three schema additions (`[sweep.*]`, `points`/`arms` as
canonical spellings, flattened `baseline`/`floor`/`delta`), all of which
must land in PR #20's strict schema.

## 6. Defaults are ours, overrides are theirs

Every builtin the harness applies is a named default with a stated override
position. The release profile is the worked example: since PR #19 the tool
passes `opt-level = 3`, `lto = "fat"`, `codegen-units = 1` on the command
line where no manifest can silently drop them; under this design those
become `[build]` keys in the root config, and the tool always passes the
effective values via `--config`, so a consumer override wins by being
config rather than by being a manifest cargo may ignore.

| default | value | today | override lives at |
|---|---|---|---|
| timing | passes 10, runs 50000, batch 5000, harness_runs 3, cooldowns [0,100,600] | overridable (`config.rs:291-305`) | `[timing]`, `[sweep.*] timing` (unchanged) |
| tuning | validation seeds 100, determinism 10, quality 1000, bootstrap 10000 | hardcoded, unreachable from config (`config.rs:398`, `511-519`; bootstrap is a const, `config.rs:505-508`) | new `[tuning]`, wired through (the already-filed v3 item) |
| dispatch output width | 8 | hardcoded in every consumer's macro call | `[dispatch] out` |
| dispatch point list | union of every sweep's points | duplicated by hand in each main.rs | `[dispatch] points`, only for narrowing |
| build profile | opt 3, fat LTO, 1 cgu | tool-pinned CLI (PR #19) | `[build]`, tool still passes effective values on the CLI |
| workload programs | `default`, `realistic` | copied per consumer | `[workload.<name>] stages = [...]`, unknown stage is an error naming the six builtins |
| build location | tool-owned `target/` under the bench tree | consumer workspace accident (the arvo split-brain) | none; fixed, because every observed override was involuntary and two were outages |
| output roots | `results/`, `history/` | consts (`driver/mod.rs:54`, `history.rs:15`) | none; fixed, because paths are cited from committed documents and no caller wants them moved |

The last two rows are deliberate non-knobs: a default earns an override the
same way a hook earns a payload, by a caller, and these have none.

## 7. Migration, per consumer

Compat mode is the floor for every tree: a flat legacy tree keeps running
unchanged, including its own `main.rs` where it has one (built and located
via cargo's compiler-artifact records per the rebuilt PR #19). Migration is
per tree and never forced.

| consumer | steps | cost |
|---|---|---|
| kirjo | nest its one bench into `colour_space_dispatch/`, delete the 60-line bin crate, delete two arm manifests | minutes |
| vehje | delete the 51-line bin crate; sweeps map one-to-one from its 180 sections (0 shared arms, grouping optional and can happen later per family); scripted `git mv` of 902 arms under their benches (each belongs to exactly one section, mapping derived from the manifest); 1422 history TSVs into `history/<bench>/`; workload becomes the builtin `realistic`; 902 generated-manifest deletions optional | half a day, scripted; the 902-manifest pin-bump problem from the survey disappears with the manifests |
| hilavitkutin | lands after the `after_cell` hook: lib.rs keeps disasm_5check plus a ten-line `hooks()`; delete the 311-line main; add the 13 `may_differ = true` flags its code list encodes and its manifest lacks; the dispatch family nests with `dispatch_direct` present once as the declared baseline; 432 flat artifacts migrate to `results/`; `runs/run1` and `run2` retire (subsumed by history plus git); `engine_vs_std` is named out of convention and moves to sketches or onto the harness | a day |
| arvo | waits on the canon rebuild, which is the natural moment: regroup 49 sections into 18 bench dirs with zero duplicated arms (section 1's families); the routine table moves to lib.rs under `routine_table!`; the four-names-per-arm scheme collapses to directory name equals crate name; five cited artifacts on the keep-list stay flat, the rest migrate | the rebuild-shaped tree is this design; incremental cost over rebuilding at all is small |
| mockspace self-bench | adopts the full shape and becomes the reference implementation, which the survey found it currently is not | small, overdue independently |

Cost the convention does not impose: no tree duplicates a crate. The
maintainer's duplication worry was priced against arvo's 36 and the price is
zero, because at bench granularity the sharing does not exist (section 1).
Had the families been ignored and `benches/<section>/arms/` enforced
instead, arvo would have needed 165 duplicate arm copies (the sum of extra
appearances across the 36), which is the cost the bench/sweep distinction
avoids and the reason the vocabulary had to come first.

## 8. What is not changed

The survey's list stands: the harness measurement core and its statistics,
cdylib-per-arm isolation, subprocess workers, validation, the disasm
duplicate check, staging and quarantine, the history mechanism and its
tracked status, `bench-matrix`, `#[bench_variant]` and the arm lib.rs shape,
the CSV schema, the `_n<point>` filename convention, committed evidence and
every cited path (keep-list plus in-place `.bench_history`), and both open
PRs. PR #19's artifact-record location and profile pins are load-bearing for
compat mode and for the generated path's build step; PR #20's strict keys
are what make the new schema sections safe to add. Of the two filed
follow-ups, the stale-filtered-build defect is subsumed on the generated
path (step 2 of the ordering) and remains PR-shaped for compat mode; the
history-schema gap is untouched by this design and pairs naturally with
`[build]`: when that follow-up lands, the effective build settings the tool
now controls are exactly what the ledger should record.

## 9. Expressibility ledger

Every mechanism above, with its precedent or its flag:

- Generated driver crate with path dependency on a consumer rlib:
  `custom_lints.rs` (generated crate) plus `bench-matrix` (call by path,
  `bench-matrix/src/lib.rs:24-36`). Shipped, proven.
- Hooks as a fn-pointer struct compiled into driver and worker: today's
  `DriverRegistry` (`driver/mod.rs:57-68`). Shipped.
- Byte dispatch generated from config at generation time: same text-emission
  class as the lints collect lib. Shipped mechanism, new emission site.
- Two-level toml and the `[sweep.*]` schema: plain serde shapes, same class
  as the existing manifest; strict-key handling from PR #20.
- Staging promotion into per-bench result dirs: exists
  (`driver/staging.rs`, `driver/mod.rs:152-163`); history path change is a
  parameterised root the module already supports (`history.rs:34-38`,
  `append_in`/`load_in`).
- Arm-versus-support refusal by crate-type: reads the manifest the tool
  already parses or generates. Trivial.
- `routine_table!`: the one uncompiled piece, flagged as such in the survey
  and still. Same token class as the shipped `byte_routine_dispatch!`
  (nested `macro_rules!` repetition over literals producing
  `routine_bridge!($ty<$n>)` arms); a one-evening compile probe before any
  round commits to it, committed beside the round that does.

## Appendix: the 36, enumerated

Family membership and multiplicity, from the manifest parse (multiplicity is
the number of sections listing the arm): warm-container 10x native,
headroom, minimum, plusone; 9x kernel; 5x lanes_deferred. warm-clamp 8x
acc64, accfit, accfit_dyn, head, min_lanes, minimum. satfold 7x seq,
lanes16; 6x iterfold, lanes16_constl, lanes4_idx, lanes64, neon, nolaw; 2x
neon8. wide-rung 5x align16, ragged, ragged_overread, wordround,
wordround_alias. bitpack-footprint 2x dense, dense_alt, packed,
packed_naive. bitpack-contend 3x d16, packed_simd; 2x d16_control, d32,
pipe4. bitpack-write-contend 2x write_dense. Roles: control by name 3
(d16_control, packed_naive, native), reference by documented role 4 (dense,
dense_alt, write_dense, seq), competitors 29. hilavitkutin: dispatch_direct
2x, the declared baseline of both its sweeps
(`hilavitkutin .../main.rs:19-24`). vehje: none.
