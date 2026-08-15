# The Matrix and the Sweep: where bench configuration actually lives

An investigation into one testable disagreement. The consolidation design
(`mock/research/202608150809_bench-vocabulary-and-consolidation-design.md`, on
branch `docs/bench-ergonomics-survey`) proposes that a sweep stays inline in
`bench.toml` on the evidence that sweeps carry almost no configuration. The
maintainer doubts the measurement and proposes a mechanism for the doubt: that
the configuration migrated into Rust and the count was taken on the wrong
surface.

The doubt is correct, and the mechanism he names is the right one. This file
states what was counted, why the existing number is arithmetically right and
answers a different question, and what a sweep carries in this workspace today.

Every count here is reproducible from the probes committed beside this file in
`202608151243_probes/`, each of which prints a negative control before its
result. Consumer paths are written `<repo>/mock/benches/...` and were read on
the workspace clones at the branches they happened to be on
(arvo `feat/arvo-shape-topic`, hilavitkutin `docs/adopt-notko-hlist`, vehje
`feat/extension-point-contract`, kirjo `feat/kirjo-prior-art-research`). No
workspace clone was modified.

## 0. The two standing gates

**Canon gate: no canon exists to check this against, and that is the finding
about authority rather than about the work.** `mockspace` has no `mock/canon/`
directory. `where-the-canon-lives.md` says the address "is recent and not yet
adopted. No repo has migrated". So the two redesign documents are design-tier
proposals with no canon above them, `do-not-question-the-tier-above.md` applies
in its second form ("a design that descends from an absent or unsettled canon
carries no such warrant. It is a proposal wearing a design's clothes"), and
challenging their measurement is in scope rather than a relitigation of settled
material. Both documents say so themselves ("This is a proposal and the
maintainer ratifies", design line 8).

**Test gate: the suite passes and one test in it is a tautology.**
`cargo test --workspace --no-fail-fast` on `dev` (6ab55ee): 551 passed, 0
failed, 18 ignored across 28 suites, exit 0. Raw output committed as
`202608151243_probes/p0_test_suite_run.out`. The 18 ignored are catalogue-reds
carrying a reason and a tracked id, which is the discipline
`catalogue-edge-cases-as-tests.md` asks for.

The one defect: `bench-core/src/lib.rs:637-651`,
`abi_hash_reflects_four_field_layout`, transcribes the body of `abi_hash()`
(`bench-core/src/lib.rs:464-481`) with its literals substituted and asserts the
transcription equals the function. It compares a computation to the same
computation and can only fail if someone edits one copy, which is the
tautological shape `the-test-gate.md` says is deleted rather than improved. The
one non-trivial fact it touches, `size_of::<FfiBenchCall>() == 32`, is already
pinned by its sibling `ffi_bench_call_is_four_u64` two tests above. PR #21's
branch deletes it; on `dev` it stands. This does not disqualify the suite and it
does not block the assigned work, so the work proceeded.

## 1. What the matrix is, and what the maintainer's "sensors" are

There are two layers and they were built one day apart.

**The harness layer, `bench-harness/src/matrix.rs`, landed 2026-07-22 in
`1315109` ("declarative axis-matrix + variant-crate generation in
bench-harness").** A `MatrixSpec` (`matrix.rs:56-95`) holds a list of `AxisSpec`
(`matrix.rs:49-52`), each a named axis with values. `expand`
(`matrix.rs:147-183`) takes the cartesian product; each point is a `Composition`
(`matrix.rs:138-142`) carrying the chosen value per axis, the union of the cargo
features those values require, and a substitution map. `render_variant`
(`matrix.rs:240-272`) turns one composition into one variant crate: a
`Cargo.toml` pinning `crate-type = ["cdylib"]` and `[profile.release] opt-level
= 3, lto = "fat", codegen-units = 1` (`matrix.rs:263`), and a `src/lib.rs`
rendered from the consumer's template. `render_bench_section`
(`matrix.rs:276-310`) emits the `[bench.*]` section that wires them together.

So at this layer a **cell is one point of the cartesian product**, and it becomes
one isolated cdylib.

**The discipline layer, `bench-matrix`, landed 2026-07-23 in `2471643`
("mockspace-bench-matrix semantic-matrix layer").** Its README states the
vocabulary outright (`bench-matrix/README.md:12-19`): a *family* (the bench), a
set of *cells* to compare within it, a *sweep* axis producing one bench per
value, and a *size* sweep. "Each `(sweep value, cell, size)` becomes its own
release cdylib."

**The maintainer's recollection maps onto this almost exactly, and his
uncertainty was in the right place.** His "several sensors in one bench setup"
are the **cells**: `bench-matrix/src/lib.rs:4-5` says "one bench, many isolated
cells, each timed individually, swept across parameter axes". His "the matrix
runs the different configs of that one bench with different things plugged in"
is the pair of axes: the cells are the things plugged in
(`CellDecl.op_path`, `decl.rs:44-52`), and the sweep values are the configs
(`SweepAxis`, `decl.rs:57-61`, "one bench per value. The value is passed to
`setup` as its first argument"). The word "sensors" maps onto **cells**, and
nothing in the code is called a sensor.

## 2. Was the concept renamed? No. Two concepts, nested, introduced a day apart

`git log -S'SweepAxis' --all` returns exactly two commits, both on 2026-07-23;
`git log -S'MatrixSpec' --all` returns four, the earliest 2026-07-22. There is
no commit in which a matrix became a sweep. The words name different objects and
they compose:

| word | where | what it is | what it produces |
|---|---|---|---|
| axis | `bench-harness/src/matrix.rs:49` | one named dimension with values | a factor of the cartesian product |
| cell | `bench-matrix/src/decl.rs:44` | one measured implementation, by function path | one variant cdylib |
| sweep | `bench-matrix/src/decl.rs:58` | the outer family axis | one whole `[bench.*]` section per value |
| size | `bench-matrix/src/decl.rs:80` | the point list | one `[[sizes]]` row per value |

The nesting is visible in the generator. `generate.rs:95` builds
`axes: vec![AxisSpec { name: "cell".to_string(), values }]`: **exactly one
axis, always, named "cell"**. The sweep is handled by a plain loop outside the
matrix (`generate.rs:113-124`) that calls `to_spec(decl, val)` once per sweep
value and emits a separate bench section for each. So the harness's general
N-axis machinery is present and `bench-matrix` uses one dimension of it, hand-
rolling the second.

**The design conflates two of these four words, and cites the code that
contradicts it.** Design line 158-164 defines "cell" as "one (bench, sweep,
point) with its resolved arms, producing one samples file, one meta, one
report", then says "The word is already in use for exactly this in
`bench-matrix`". It is not. In `bench-matrix` a cell is **one arm**:
`CellDecl` carries a tag, an op path, an optional setup override and a feature
list (`decl.rs:43-52`); `to_spec` maps each cell to one `AxisValue` of the
"cell" axis (`generate.rs:61-80`); the variant name is `"{bench}_{cell}"`
(`generate.rs:93`). One cell, one cdylib, one arm. Under the design's own
proposed vocabulary the existing `bench-matrix` cell **is** the new word "arm",
and the design assigns "cell" to a different object while claiming continuity.
That is the one-term-per-concept defect the design's own §2 says it exists to
prevent, introduced by the entry that invokes it. The supporting citation is
also off: the quoted phrase is at `bench-matrix/src/lib.rs:4-5`, not `:8`.

## 3. The crux: where the configuration lives

### 3a. Reproducing the existing measurement, and what it counted

The design's number reproduces exactly. Over the four consumer manifests, 258
benches use the per-size arm-list form and exactly one of them has an arm set
that actually differs between its sizes: `arvo bitpack-write-contend-race`
(`p5_reproduce_257_of_258.py`). 257 of 258. The arithmetic is right.

What that number measures is whether **an arm list varies from point to point
inside one bench**. It is silent on whether a sweep carries configuration,
because an arm list is not configuration and the two questions do not touch.

A second count, closer to the claim under doubt, is the set of keys a
`[bench.*]` section carries beyond its point list and its arm list
(`p1_count_section_config.py`):

| tree | sections | keys present |
|---|---|---|
| arvo | 49 | title 49, workload 49, master_seed 49, threaded 6 |
| hilavitkutin | 29 | title 29, workload 29, master_seed 29 |
| vehje | 180 | title 180, workload 180, master_seed 180, normalise 157 |
| kirjo | 1 | title 1, workload 1, master_seed 1 |

Zero uses of `may_differ`, `required` or `timing` anywhere, out of 259 sections,
though all three are in the schema (`bench-harness/src/config.rs:119, 124, 136`).
So on this surface the observation is not merely "almost no configuration", it is
stronger than the design states: three of the schema's own per-bench knobs have
never been used by any consumer.

**And that is the tell.** A schema key that nobody uses is either dead or being
satisfied somewhere else. Sections 3b to 3e establish that it is the second, in
four independent ways.

### 3b. bench.toml is a generated artifact for the largest consumer

`generate_all` (`bench-matrix/src/generate.rs:102-134`) writes the variant
crates and then **rewrites `bench.toml`**, replacing everything after the marker
`# >>> bench_matrix (generated by mockspace-bench-matrix)` (`generate.rs:41`).

In vehje's manifest that marker is at line 1068. Of 4956 lines, 3888 (78%) sit
after it. Of 180 `[bench.*]` sections, **135 (75%) are generated**. Of 902 arm
crates under `variants/`, **594 (66%) carry the generated header** in their
`src/lib.rs`.

So for the tree that supplies 180 of the 259 sections in the count above, the
file the measurement was taken on is the generator's **output**. Counting
configuration keys there and concluding that sweeps carry little configuration
is counting a rendering, not a source. The design's §7 migration entry for vehje
("sweeps map one-to-one from its 180 sections") treats those 135 renderings as
135 sweeps to be mapped, when they are the six-way expansion of roughly 22
sweeps that already exist and already have names.

### 3c. The sweep already exists in Rust, with ten configuration keys on it

The input to that generator is `bench_matrix!`. A real one, complete, from
`vehje/mock/benches/carrier/src/bench/cfg.rs:31-40`:

```
name, crate_path, crate_dep, extra_deps, seed,
sweep <axis> in [<values>], sizes, baseline, regime,
setup |profile, n| -> St { ... }
```

plus, per cell, an optional `#[feature = "..."]` gate and an optional per-cell
`setup` override (`cfg.rs:48-60`). `MatrixDecl` (`bench-matrix/src/decl.rs:65-89`)
is the same list as data, with `floor` in addition.

Classifying those against the `bench.toml` schema:

| what the Rust declaration carries | expressible in bench.toml today |
|---|---|
| `seed` | yes, `master_seed` |
| `baseline`, `floor` | yes, `[bench.*.normalise]` |
| `sizes` | yes, `sizes` |
| the arm set (`cell` list) | yes, `variants` |
| **the sweep axis, named, with its values** | **no** |
| **`regime`: warm / cold_cycle(m) / stream** | **no** |
| **`setup` path, and per-cell setup override** | **no** |
| **per-cell cargo features** | **no** |
| **`crate_path` / `crate_dep` / `extra_deps`** | **no** |

Five of the nine categories have no config expression at all. Two of them
(`regime`, per-cell features) change what is measured rather than how it is
wired, and `regime` is the strongest case: `bench-matrix/src/decl.rs:14-27`
defines Warm (one state, measure the op over it), ColdCycle(m) (m distinct
programs so the branch predictor cannot memorise one) and Stream (sweep the
input byte stream). Two runs of the same arms under different regimes are
different measurements of different things, and nothing in `bench.toml` can say
which one produced a row.

**The consumer has already paid for that gap, visibly.** `carrier_coldcycle`
(`vehje/mock/benches/carrier/src/bench/coldcycle.rs:24-33`) is a whole separate
family that duplicates `carrier_predecode`'s sweep axis, seed, baseline and cell
set, and differs in one key: `regime: cold_cycle(16)`. Its own doc comment says
what it is for: "Compare cell-for-cell against `carrier_predecode` (memorized):
the delta is the memorization the warm numbers hide" (`coldcycle.rs:10-11`). A
comparison the author explicitly tells the reader to make cell-for-cell across
two benches is, in the proposed vocabulary, **one bench with two sweeps**. It
could not be written that way, so it was forked, and the join is left to the
reader.

### 3d. arvo packs two to four sweep fields into the single integer point

`#[bench_variant]` requires "exactly one const generic parameter (the dispatched
size)" (`bench-macro/src/lib.rs:55-56`), and a cell gets one `usize` from
`SizeSection.n` (`bench-harness/src/config.rs:193`). Five arvo bench families
need more than one parameter, so they encode several fields into that integer
and decode them in Rust. Each shared crate states its own encoding in its module
doc:

| shared crate | encoding | fields |
|---|---|---|
| `warm-container-shared` | `W*10000 + NC*1000 + OP*100 + D` (`src/lib.rs:59`) | width, element-count class, semantics, op density |
| `warm-clamp-shared` | `W*10000 + NC*1000 + LOG2A*10 + OP` (`src/lib.rs:83`) | width, count class, log2 arity, semantics |
| `satfold-shared` | `LI*1000 + NC*100 + AL*10 + OP` (`src/lib.rs:100`) | length index, count class, alignment, operator |
| `wide-rung-shared` | `W*1000 + NC*100 + D` (`src/lib.rs:43`) | width, count class, density |
| `bitpack-contend-shared` | `N*10 + T` (`src/routine.rs:23-25`) | element count, **thread count** |

`warm-container-shared/src/lib.rs:56-58` says why, in one sentence: "The harness
dispatches a variant by a single `usize` per size row, and this bench varies four
things, so the size field is a key."

Decoding every arvo section's declared points with the right decoder
(`p2_decode_packed_points.py`):

- **36 of arvo's 49 sections carry a packed key**; 13 use a plain point.
- Across those 36, **40 axis instantiations vary** and **87 field values are
  held constant for the whole section**.
- 28 sections vary exactly one field, 6 vary two, 2 vary none.

The 87 constants are the load-bearing number. Each one is a setting chosen once
for a whole sweep, of exactly the kind a `[sweep.*]` section would hold, and each
is currently expressible only as a digit position inside an integer. Read as a
table, `warm-container-width-l1` is the sweep `W in {8,13,16,32,60,64}` held at
`NC=0, OP=0, D=3`; its sibling `precise-container-width-l1` is the identical
sweep held at `OP=1`. Those are two sweeps of one bench differing in one setting,
and the manifest can only see two unrelated names and twelve unrelated integers.

The costs are not hypothetical:

1. **The artifacts cannot name their own axes.** Every CSV, meta and report file
   is keyed `<bench>_n<point>`, so a committed result reads
   `warm-container-width-l1_n130003.csv` where the honest name is
   `W=13, NC=0, OP=0, D=3`. A reader has to fetch a shared crate's module doc to
   decode a filename.
2. **The history ledger is keyed on the opaque integer.** Re-encoding a key
   orphans every prior row for that cell, silently, because nothing relates the
   old integer to the new one.
3. **Nothing checks that a comparison holds the other fields fixed.** The
   harness cannot know which digit is the axis, so a sweep that accidentally
   moved two fields at once is indistinguishable from one that moved one.

### 3e. Three more instances, each a different shape of the same thing

**hilavitkutin hardcodes a manifest flag in Rust, for thirteen benches.**
`hilavitkutin/mock/benches/src/main.rs:213-216` is a 13-name `matches!` list
setting `may_differ`, which selects the third const parameter of
`ByteRoutine<N, 8, MAY_DIFFER>`. The manifest key exists
(`config.rs:114-119`), the generic byte dispatch already reads it from config
(`bench-core/src/lib.rs:385-396`), and hilavitkutin's `bench.toml` sets it zero
times. vehje's driver comment records that the duplication is known:
"`may_differ` comes from the manifest, not a name list"
(`vehje/mock/benches/src/main.rs:33`).

**vehje flattens a two-dimensional sweep into nine arms.** `carrier_entgrid`
(`vehje/mock/benches/carrier/src/bench/entgrid.rs:34-60`) sweeps
`op_correlation x locality_window`, a 3x3 grid. `bench_matrix!` has one sweep
axis, so the grid becomes nine cells named `c0_w4 ... c900_wmax`, each with a
setup override, all running the identical measured expression
`c::predecode::interpret_predecoded(&s.pd, seed, &mut s.r)`. Its declared sweep
axis is empty (`sweep kind in []`, `entgrid.rs:40`) and its `baseline` is a grid
point (`c0_w64`).

`bench-matrix/TODO.md:60-63` names this exact situation and both ways out,
written a month before the redesign: "**multi-sweep.** The entropy grid is a 2D
`op_correlation x locality_window` product; the macro's `sweep` is a single
axis. Either add cartesian multi-sweep, or keep it a single flattened sweep with
composite values." **arvo independently chose the second option**, in a
different repository, with the composite values packed into the integer. Two
consumers, one framework limitation, two flattenings.

**The arvo table restates the manifest, and has already drifted.**
`routine_for_n` (`arvo/mock/benches/src/main.rs:206-595`) is 256 keyed rows over
47 bench names using 15 distinct bridge types, and **no bench name needs more
than one type** (`p3_classify_drivers.py`). Its non-redundant content is
therefore 47 `bench -> type` facts; the other 209 rows exist because each row
must also name the point, and the point is already in `bench.toml`. Diffing the
two sets (`p4_manifest_vs_table_drift.py`): 6 pairs exist in Rust with no
manifest entry (4 in `bitpack-contention`, 2 in `satfold-length-dram-wrap`) and
6 exist in the manifest with no Rust arm (the deliberately disabled
`spectral-bisection` and `structural-decomposition`, documented at
`main.rs:216-221`). The first six are dead code that nothing reports.

Of arvo's 672 driver lines: 186 comment or blank, 256 keyed table rows, 222 loop
and worker code the library driver subsumes, 8 other.

## 4. The answer to the question asked

**Is there substantial per-sweep configuration currently expressed in Rust that
would belong in a config file? Yes.** Counted:

- **Five categories of per-sweep configuration have no expression in
  `bench.toml` at all**: the sweep axis itself, the regime, the setup entry
  point, per-arm cargo features, and the variant crates' dependency lines. All
  five live in `MatrixDecl` / `bench_matrix!` in Rust
  (`bench-matrix/src/decl.rs:65-89`).
- **87 per-sweep constant settings plus 40 varying axes** are packed inside
  arvo's integer point field across 36 of its 49 sections.
- **13 per-bench `may_differ` decisions** live in a Rust `matches!` list in
  hilavitkutin for a manifest key it never sets.
- **135 of vehje's 180 sections and 594 of its 902 arm crates are generated**,
  so for that tree the surface the measurement was taken on is the generator's
  output rather than anyone's input.

**The existing measurement is arithmetically correct and answers a different
question.** "257 of 258 sweeps share their bench's arm set" reproduces exactly.
It establishes that arm lists do not vary from point to point. It cannot
establish that sweeps carry no configuration, because the configuration that
exists was never in the file it counted: it is in `bench_matrix!` declarations,
in packed integers, and in a `matches!` arm.

**The maintainer's mechanism is the right one and his instinct about magnitude
is right too.** The one correction to his recollection is that "matrix" was not
renamed to "sweep": they are two different, nested concepts introduced a day
apart, and the sweep is the outer one.

**What this does not settle.** It does not follow that a `[sweep.*]` section
must be its own file, or that inline is wrong. Inline versus nested is a layout
question and the evidence here does not decide it. What the evidence does decide
is that the premise offered for the inline choice ("sweeps carry almost nothing,
so they cost one line each") is false: a sweep in this workspace carries a named
axis with values, a regime, a setup entry point, per-arm feature selection, and
between two and four packed parameter fields. A design that sizes the sweep
section at one line is sizing it against the subset that happens to be
expressible today.

## 5. What a sweep should carry, if the concept is doing real work

The framework runs one comparison across several configurations. For results
from two configurations of one bench to be **comparable**, the framework has to
know which things it is allowed to let differ, and record them. For a result to
be **trustworthy**, it has to pin the rest. The dividing test is one question:
*if two sweeps of one bench differ in this, are their numbers still comparable
cell-for-cell?* If yes, it is a sweep setting and must be recorded. If no, it
belongs to the harness and a sweep must not be able to move it.

Observed in code today, marked as such; the rest is my reading and is not
measured.

**Belongs to the sweep, and is observed in Rust today.**

1. **A parameter vector rather than a scalar point.** Named fields with declared
   ranges, so a row's identity is `W=13, NC=0, OP=0, D=3` rather than `130003`.
   This is the single largest gap and the one every other item leans on: it lets
   the artifact name its axes, lets history key on a stable tuple, and lets the
   framework check that a comparison moved one field. Five arvo crates and the
   `bench-matrix` TODO independently demonstrate the need.
2. **The regime.** Warm, cold-cycle(m), stream. It decides whether the predictor
   is aliased and whether the state is rebuilt, which routinely moves a dispatch
   measurement by more than the effect under test, and `coldcycle.rs` exists
   solely because it could not be varied per sweep.
3. **The arm set and per-arm feature selection.** `CellDecl.features`
   (`decl.rs:51`) has no config equivalent, so an arm needing a cargo feature is
   inexpressible outside Rust.
4. **The baseline and the floor.** Already config (`config.rs:170-186`), used
   157 times by vehje. Correct as it stands, and correct to be per-sweep rather
   than per-bench, because adding a floor arm is a property of the comparison
   being run.
5. **The validation policy**, `may_differ` and `required`. Per-bench in the
   schema, in Rust in hilavitkutin, and per-sweep is the right granularity: a
   sweep that adds a deliberately divergent arm changes the policy for that
   sweep alone.

**Belongs to the sweep, my reading, not observed.**

6. **The workload.** Per-bench today (`config.rs:89`). A bench comparing the
   same arms under an empty workload and under a realistic one is precisely a
   sweep over surrounding context, and today that costs two bench names.
7. **Timing.** `TimingOverride` exists per-bench (`config.rs:205-211`) and is
   used by nobody. A sweep whose points span 16 KiB to 32 MiB needs different
   `runs_per_pass` at its ends or it wastes wall-clock at one end and
   undersamples at the other.
8. **Thread count as a declared field, not a bool.** `threaded`
   (`config.rs:132`) says only "do not pin"; arvo sweeps actual thread counts
   inside the packed key. A parallel axis needs the count in the parameter
   vector and in the meta, or a reader cannot tell a four-thread row from a
   one-thread row without decoding an integer.

**Belongs to the harness, and a sweep must not be able to move it.** The shared
seed table (`scaffold.rs:24-41`), the anti-hoist chain, the setup-versus-
iteration split, the fidelity digest, the calibration floor, the fat-LTO
one-codegen-unit profile. `bench-matrix` already made these non-negotiable and
that is correct: they are what makes two sweeps of one bench comparable at all.
The distinction is worth stating explicitly in whatever vocabulary lands,
because "a sweep carries its own configuration" read without it invites a knob
for the seed table.

**And one thing that must be recorded whether or not it is configurable:** which
regime, which workload and which timing produced a row. `EnvMeta`
(`bench-harness/src/env.rs:15-33`) records cpu, os, rustc, commit, timestamp and
counter frequency, and none of the six. A committed CSV therefore cannot be
attributed to the measurement conditions that produced it beyond the hardware.

## 6. Adjacent findings, outside the assigned question

Reported under the standing instruction to name unlicensed mechanisms. Each is
a fact about the code on `dev` at 6ab55ee.

**The fidelity digest is computed, transported, and never compared.** The
scaffold computes a reps-invariant digest specifically so cross-arm validation
means something under calibration: "Reps-invariant so cross-validating on it is
meaningful under calibration, where the reps-variant `output` is not (panel
finding 6)" (`bench-matrix/src/scaffold.rs:129-131`). `Sample.digest` documents
itself as "Reps-invariant fidelity digest for cross-variant validation"
(`bench-harness/src/sample.rs:73-77`). It is carried through the FFI struct, the
worker line, both CSV writers and the parser. **The string `digest` does not
appear anywhere in `bench-harness/src/validation.rs`.** The commit that
introduced it claims otherwise ("the cross-variant fidelity check reads `digest`
instead of the reps-variant output bytes", `2471643`). Validation still compares
the reps-variant `output` bytes (`validation.rs:501`), which
`timed_calibrated!` makes dependent on the calibrated rep count
(`bench-core/src/lib.rs:596-604`), which differs per arm because it is derived
from how fast that arm ran.

**A cross-arm mismatch drops nothing.** `validate` returns `Err` on any byte
mismatch (`validation.rs:525-537`), never a partial survivor list. The driver's
`Err` arm prints `VALIDATION ERROR`, pushes a marker into `dropped`, and
**leaves `config.variant_paths` untouched** (`driver/mod.rs:415-418`), so every
arm is timed and reported anyway; only `required = true` turns it into an exit
code, and no consumer sets `required`. The survivor-dropping path
(`driver/mod.rs:396-414`) is reachable only when `validate` returns `Ok`, which
it does only when there were no mismatches. PR #21's description states that
validation "drops non-agreeing arms"; on `dev` it does not.

The observable consequence is committed:
`vehje/mock/benches/results/carrier_entgrid/*.csv` contains all nine entgrid
arms at all five points, although those nine run nine deliberately different
programs and the bench sets neither `may_differ` nor `required`. Whether the
run printed a validation error is not recoverable from the artifacts, so what is
established is that non-agreement does not remove an arm from the report, not
that this particular run mismatched.

**`render_bench_section` cannot emit three of the schema's keys.** It writes
title, workload, master_seed, normalise and the size rows
(`matrix.rs:285-308`). It has no path to `may_differ`, `required`, `threaded` or
a timing override, so a generated bench takes the defaults regardless of what
the family needs. This is the mechanical reason 135 vehje sections carry no
`may_differ`.

**arvo's driver contains six dead match arms**, listed in §3e, that no test or
gate reports.

## 7. What I did not establish

- Whether the design's proposed *layout* (per-bench directories, `[sweep.*]`
  inline in a per-bench `bench.toml`) is right. This file bears on the premise
  offered for it, not on the choice.
- Whether `carrier_entgrid`'s recorded runs actually failed cross-arm
  validation. The code path that would let them be reported anyway is
  established; the run's stderr is not committed.
- The exact wording of the "roughly 240 sweeps, only two carry configuration"
  claim. It appears in no committed document on any branch of this repository
  (`git grep -i sweep` over all branches, `*.md`). What is committed and closest
  is the 257-of-258 arm-set claim, which is reproduced in §3a. If the relayed
  sentence counted something else, §3a states precisely what I counted and
  §1 of `p1` prints the full key frequency so any other reading can be checked
  against it.
- Anything about performance. No measurement was taken and none was needed; this
  is a question about where declarations live.
