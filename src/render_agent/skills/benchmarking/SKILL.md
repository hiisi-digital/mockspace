# Benchmarking

A bench answers a measurement question: which of these is faster, what does this cost, where is the
band boundary. Any number that feeds a decision comes from here.

Invoke this skill before writing the first timing loop. The failure it prevents is a hand-rolled
`Instant::now()` loop whose numbers look authoritative, cannot be reproduced, and were very probably
measuring something other than the thing under test.

## The three layers

**`mockspace-bench-core`** is what a variant links: `FfiBenchCall`, the `timed!` macro, `abi_hash`,
and `byte_routine_dispatch!`. A variant cdylib exports `bench_entry`, `bench_name` and
`bench_abi_hash`, and the driver finds them by dlsym.

**`mockspace-bench-harness`** is the transport. Cdylib-per-variant isolation, the subprocess driver,
the workload programs, validation, analysis, history, and the multi-axis spec-to-variant-crate codegen
(`MatrixSpec` / `expand` / `render` / `generate`). It deliberately knows nothing about whether a given
measurement is honest.

**`mockspace-bench-matrix`** is the opinionated discipline on top, and is where most of the value
sits. Reach for it whenever the question is a comparison across more than one axis.

Commands: `mock bench init` scaffolds, `add <name>` adds a variant, `run [names...]` measures,
`report` regenerates findings from cache, `test` runs `cargo test` in every crate under the
bench tree (a bare `cargo test` at the tree root sees only the driver crate, since arms and
support crates are path dependencies rather than workspace members, and reports a misleading
`0 passed` there is nothing to see), `list` prints what is registered.

## Why the framework and not a timing loop

Each reason is a way a hand-rolled measurement lies.

**Per-variant cdylib isolation.** Every variant is its own dynamic library, release, fat LTO, one
codegen unit, run in its own subprocess. Two variants timed inside one binary share a codegen unit and
the optimiser is free to erase exactly the distinction under test.

**A shared realistic workload.** The measured call is surrounded by a common program of scalar
dependency chains, pointer chases, cache pressure and branchy context, so the numbers approximate a
real calling environment rather than an empty loop with a warm cache and an idle predictor.

**Validation before timing.** Every variant runs across deterministic seeds first, each in its own
worker subprocess, so a variant's cached per-process state lives and dies with its worker and the
orchestrator's memory stays bounded. A mismatch is the finding: the implementations drifted, and the
timing comparison is meaningless rather than merely failed.

**Automatic disassembly dedup.** Before timing, `bench_entry`'s machine code is extracted from each
dylib and compared. Two variants with identical code will bench identically, and the harness says so
instead of letting you conclude something from noise.

**Real statistics.** Quintiles, per-cooldown breakdown, lag-1 autocorrelation as a drift signal,
bootstrap confidence intervals on the median and on paired differences, a sign test, and
Benjamini-Hochberg FDR adjustment when many variants are compared at once.

**History and regression detection.** Every run appends timestamp, git commit, variant, N, mode,
median and CI bounds to an append-only log, and a later run flags entries whose CI no longer overlaps
the historical baseline.

**Do not re-roll any of this.** Timing helpers, bump providers, stat collection, validation: they are
the framework's, and a bench crate that reimplements them is reinventing what it sits inside.

## Hardware counters, and the quirks that come with them

Wall-clock says which variant is faster. The PMU says **why**: "threaded won because it retired the
same instructions with far fewer branch mispredicts" is a different and better finding than "threaded
won".

`PerfSnapshot` carries `instructions`, `cycles`, and `ipc()` derived from them. **Only those two fixed
counters are wired.** `cache_misses` and `branch_misses` exist as fields and are currently zero: the
configurable counters need work the harness has not done. Do not report cache or branch-miss numbers
from a bench run; they are placeholders, and a zero there means "not wired", not "none happened".

Availability is narrow: the feature must be on, on Apple Silicon, with the framework present, as root,
and the support probe must pass. `setup()` returns whether it armed. Fields are zero when unavailable,
which is indistinguishable from a real zero, so check that setup succeeded before drawing a
conclusion.

**`teardown()` must run on every exit path.** The PMU is a process-global claim, and leaving it held
breaks the next process that wants it, not this one. That is the failure mode most likely to be
introduced by an early return.

## The matrix layer, for anything with more than one axis

A semantic matrix declares a **family**, a set of **cells** to compare within it, a **sweep** axis
producing one bench per value, and a **size** sweep. Each `(sweep value, cell, size)` becomes its own
cdylib in its own subprocess.

`bench_matrix!` emits the cell functions and `matrix_decls()` from one declarative block, carrying
`name`, `crate_path`, `crate_dep`, `extra_deps`, `seed`, the `sweep`, `sizes`, `baseline`, `floor`,
and `regime` (`warm` or `cold_cycle(M)`). Write it in a `pub mod bench` of the consumer **library**
crate, because the sibling variant crates name the cells by path, then a short generator binary.

**Cells are real typed functions, not strings.** An earlier approach spliced each cell in as Rust
source text, which meant no type checking until a generated crate built. Fat LTO already inlines
across the consumer-crate boundary into the timed region, so a cell can be an ordinary typed function
and still be measured as if written inline.

**A generic parameter, never a `fn` pointer.** A `fn` pointer reintroduces an indirect call the
optimiser may decline to devirtualize, so the measured region would include a call the real deployment
does not have. That one signature choice is the whole isolation argument, and `scaffold::warm` and
`scaffold::cold_cycle` enforce it.

### The disciplines the scaffold owns

Each of these was a real distortion in a hand-rolled version of exactly this matrix, found by a
four-expert review. They are listed because recognising them is what stops them being reintroduced
somewhere the scaffold does not reach.

**Anti-hoist chain.** `acc` is seeded from `output[0]`, one keep-alive is folded per iteration and
written back inside the timed loop, so the calibrated reps form a loop-carried dependency the
optimiser cannot collapse.

**A shared seed table.** A fixed sixteen-entry table, identical across sizes and cells, so a
cross-size or cross-cell comparison varies only the thing under test. Drawing seeds from the input
buffer instead means the harness's per-size fill becomes part of what is being compared.

**One keep-alive per iteration, not per node.** Folding an O(N) checksum inside the inner loop dilutes
every ratio in the matrix.

**The S-versus-I split, always measured.** `setup` is a required argument bracketed by counter reads,
so one-time build cost can never hide in untimed prep. Every cell reports its per-iteration cost and
its setup cost, which makes the tier breakeven `k* = (S_b - S_a) / (I_a - I_b)` computable from data
the matrix always carries.

**A reps-invariant fidelity digest.** Under calibration the reps count is timing-dependent, so the
final output bytes are reps-variant and cannot serve as a cross-cell fidelity witness. The scaffold
computes a separate digest on a fixed-seed, fixed-init single pass, on its own channel. It is a smoke
check: the authoritative fidelity anchor is still the consumer's byte-exact cross-validation `#[test]`s.

**The cold regime is a different question, not a worse warm.** `cold_cycle(M)` cycles M distinct
programs so no single dispatch sequence is memorised by the branch predictor. Warm measures the
memorised-single-program shape; cold measures the many-distinct-inputs shape. Pick the one the
deployment actually has, and say which you picked.

## Configuration

### The tree, and which directories are in it

`mock/benches/bench.toml` is the root. It carries the globals (`[timing]`, `[dispatch]`,
`[build]`, `[workload.*]`) and, optionally, `[bench.<name>]` sections of its own.

Each bench is a directory under it holding its own `bench.toml`. **That file has no wrapper
table: its fields sit at the top level**, because the directory name is already the bench's
name. Optional `[sweep.<name>]` sections carry per-sweep points and overrides. A bench's arms
live in `<bench>/arms/<arm>/`, one measured cdylib each; `<bench>/support/` and the root
`support/` hold ordinary library crates the arms link.

**Membership is declared, never detected.** The root file's `[benchspace]` table says which
directories are members:

```toml
[benchspace]
members = ["**"]        # the default when the table is absent
exclude = []
```

`members` takes glob patterns or literal directory names. `*` matches within one path
component and never crosses `/`; `**` matches any number of components, including zero. A
literal entry is explicit and must exist with its own `bench.toml`, so a typo is an error
naming it; a pattern may match nothing without complaint. **A matched member owns its
interior**, so a `bench.toml` nested inside a member belongs to that member rather than
becoming a second one.

The default `**` takes every subdirectory carrying a `bench.toml`. Narrow it when a tree holds
directories that are not benches. Nothing is inferred from a directory's contents or its name.

A member's own `[timing]` overrides the root's **only for the knobs it declares**; the rest
fall through to the root. A `[sweep.*]` override outranks both.

### The per-bench keys

`bench.toml` per bench: `title`, `workload`, `arms`, `points`, `master_seed`, `may_differ`,
`required`, `threaded`, declared roles (`baseline`, `floor`, `delta`), and a per-bench `timing`
override of `passes`, `runs_per_pass`, `batch_size`, `harness_runs` and `cooldowns_ms`. Points may
carry their own arm subset. The legacy spellings (`variants`, `sizes`, a `normalise` table) stay
accepted; the canonical ones are these.

Set `may_differ = false` and `required = true` unless there is a stated reason otherwise: identical
output across arms is the premise that makes the comparison mean anything.

Each point is its own monomorphisation. With the generated driver the declared list defaults to the
union of every bench's points and `[dispatch] points` narrows it; only a consumer-owned driver binary
maintains a `byte_routine_dispatch!` declaration by hand.

## The deliverable

**The raw artifacts.** `results/` holds CSV, meta and the harness's own per-size findings. These are
**tracked**, never gitignored, including runs that produced nothing interesting, because the history
is what makes a later regression visible.

**A decision-facing findings note** in `{mock_dir}/research/`, written for the round that asked the
question rather than for the harness. It states what was measured, the numbers, what the result
decides, and, load-bearing, **what it does not decide**: the shapes not covered, what would change the
answer, the extrapolation nobody checked. Name what dominates the measured loop, and quote the
absolute delta rather than only the ratio when one expensive call compresses it.

A result that only says which variant won has answered less than was asked.

## The boundary against sketching

A bench measures. A **sketch** establishes whether something is possible at all, lives in
`{mock_dir}/research/sketches/`, and produces WORKS / FAILS / INCONCLUSIVE. See the sketching skill.

The moment a sketch reaches for a timer it has become a bench and moves here. A question with both
halves splits into one of each.
