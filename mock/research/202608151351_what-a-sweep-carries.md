# What a Sweep Carries

The constructive half of the finding in
`202608151243_the-matrix-and-the-sweep.md`. That file established that bench
configuration migrated into Rust and that the redesign's premise was measured on
the wrong surface. This one says what replaces it: the vocabulary, the exact
content of a sweep, whether multi-axis or flattened, whether the single const
generic parameter is a real blocker, and what is generated versus written.

Every claim is either a `file:line` or a probe committed in
`202608151351_probes/` beside this file. The nine probes each print a negative
control before their result; five are compiled programs, four are counters over
the consumer manifests. The Rust probes ran on the workspace-pinned toolchain,
`rustc 1.98.0-nightly (57d06900f 2026-05-27)`
(`202608151351_probes/toolchain.txt`), which is `nightly-2026-05-28` per
`workspace.md`. Consumer clones were read only.

## 0. Two things about the brief, before the work

**The branch and the topic files do not exist on the remote, so I could not read
the topic file I was asked to check.** The brief names
`feat/bench-round-consolidation` with three topic files under
`mock/design_rounds/`, mine being
`202608151339_topic.where-the-bench-configuration-lives.md`. At 12:51 UTC and
again at 13:51 UTC, `git ls-remote --heads origin` returned no such branch, and
no branch in the repository carries any file matching `mock/design_rounds/2026*`
(all 80 tracked files under that directory are in archived subdirectories). The
round exists in the coordinator's local tree and is unpushed. **The compression
review is therefore not done and is still owed**; push the branch and it is a
short pass. This file does not depend on it: nothing below is derived from the
topic file, and the settled points quoted in the brief are honoured as given.

**One consequence worth naming.** The other expert's deliverable
(`202608151234_what-the-consumer-should-write.md`) reached me only because it was
committed into a clone I happened to share and became the parent of my own
commit on `docs/sweep-investigation`. `docs/sweep-consumer-view` points at `dev`
with no commits of its own. So that expert's work is currently reachable only
from my branch, by accident. Under
`evidence-lives-in-the-repo-or-it-never-happened.md` it is committed and citable,
which is why I cite it below; but it is one force-push of my branch away from
being unreachable, and it should be pushed where it belongs.

**Canon gate.** `mockspace` has no `mock/canon/`, so there is no canon to defend
and no canon this can contradict. The settled points listed in the brief
(declared membership with `members = ["**"]`, top-level fields in the per-file
bench form, `[bench.<name>]` supported indefinitely, one shared output tree) are
treated as fixed and nothing below reopens them.

**Test gate.** Re-run not repeated; the suite result and its one tautology are
recorded in `202608151243_probes/p0_test_suite_run.out` and section 0 of the
prior file. Nothing in this design changes that state.

## 1. The vocabulary, settled first because everything else uses it

Six concepts, five of which already have a name that is right. The collision is
real and one of the two colliding uses has to move; I say which and price it.

| term | what it is | what it is not |
|---|---|---|
| **bench** | one comparison: one question, one arm set, one or more sweeps | one `[bench.*]` section of today's manifests, which is usually a sweep |
| **sweep** | one instantiation of a bench: an axis assignment, some axes swept and the rest held | a bench, and not merely a point list |
| **arm** | one measured implementation, compiled to its own cdylib, timed in isolation | a support crate, and not a role |
| **point** | one assignment of every axis: the tuple a single measurement is taken at | an integer, and not a size except in the byte-shaped benches |
| **cell** | one measurement: one (bench, sweep, point) over its resolved arms, producing one samples file, one meta, one report | an arm |
| **product** | a cartesian construction over axes that generates arms or sweeps | a sweep |

**`cell` collides and `bench-matrix` is the use that moves.** In `bench-matrix`
a cell is one arm: `CellDecl` carries a tag, an op path, an optional setup
override and a feature list (`bench-matrix/src/decl.rs:43-52`); `to_spec` maps
each cell to one `AxisValue` of an axis literally named `"cell"`
(`bench-matrix/src/generate.rs:61-95`); the variant name is `"{bench}_{cell}"`
(`generate.rs:93`). One cell, one cdylib.

The rule that decides which use moves is the one the redesign itself invokes:
one term per concept. `bench-matrix`'s cell **is** the arm, exactly and with no
residue, so keeping both words for it is the defect. The measurement, by
contrast, has no other name. So rename `bench-matrix`'s cell to arm and reserve
cell for the measurement.

Cost, counted: `CellDecl` and its four fields, the `cell` keyword in
`bench_matrix!` (`bench-matrix/src/macros.rs`), the `cell_<tag>` generated
function names, the axis name at `generate.rs:95`, the `{cell}` slot in the name
template at `generate.rs:93`, and **27 consumer invocation sites in vehje**
(`vehje/mock/benches/carrier/src/bench/`, 27 files containing `bench_matrix!`).
All of it is Rust identifiers and macro keywords; none of it is data, none of it
is a committed artifact name, and no CSV column changes. This is a smaller blast
radius than the `variants` to `arms` rename the redesign already accepts, which
touches a manifest key.

**The alternative I rejected**: leave `bench-matrix` alone and call the
measurement something else. Every candidate is worse. "Run" is taken
(`harness_runs`, `runs_per_pass`). "Measurement" is four syllables in a word used
on every line of prose. "Point-run" invents a compound of the kind
`vocabulary.md` bans. And leaving the collision in place means the design's own
§2 entry is false about its own precedent, which is how this was found.

**`product` is the new word and it is needed.** The harness's N-axis machinery
(`MatrixSpec`, `AxisSpec`, `expand`, `bench-harness/src/matrix.rs:49-183`) is not
a sweep and it is not a bench. It is a construction: given axes, take the
cartesian product and emit one artifact per point. Today it emits arm crates and
`bench-matrix` uses exactly one axis of it, named `"cell"`, hand-looping the
sweep outside (`generate.rs:113-124`). Naming it `product` and saying what it
ranges over makes the two uses one mechanism:

- a product over **implementation axes** yields the **arm set** (dispatch shape
  x record layout x value representation);
- a product over **parameter axes** yields the **sweep's points** (width x
  density, or correlation x locality window).

That is the whole of the multi-axis answer in one sentence, and section 4
prices it. **Keep the crate name `bench-matrix`**; the word matrix is fine for
the crate and for prose, and `product` is what the construction is called when
precision is needed.

## 2. What a sweep carries, exactly

Twelve entries. For each: what it means, what the consumer writes, and whether
it is **observed** (something in the workspace expresses it today, somewhere) or
**proposed** (my reading, nothing expresses it).

### The identity

**1. The axis assignment.** *Observed.* Which axes this sweep varies and what
every other axis is held at. This is the entry that does not exist anywhere in
config today and it is the one everything else hangs from. Consumer writes
`sweep = "w"` (or a list) plus `points` for the swept axes and `hold` for the
rest.

**2. The points.** *Observed* as `sizes`. Under a named axis set, `points` is a
list per swept axis rather than a flat integer list, and the cell's point is the
tuple formed with `hold`.

### What is compared

**3. The arm set.** *Observed* as `variants`, and **overridden far more often
than the redesign states**. Measured over arvo: grouping the 49 sections into
families by shared arms reproduces the redesign's decomposition exactly, 18
families of which 7 are multi-section with 10, 8, 7, 5, 3, 3 and 2 sweeps
(`g_arm_sets_per_family.out`, matching design §1's table row for row, derived
independently from the manifest). Within those families, **five of seven have
more than one distinct arm set, and 25 of arvo's 49 sweeps sit in such a
family.** The warm-container family alone has three arm sets across its ten
sweeps: six arms, five, and four (`h_point_set_copies.out` context;
`f_warm_container_as_sweeps.out` prints the family). So the per-sweep `arms`
override is used by half the corpus, not by one bench. The redesign's line
"a sweep overrides only what differs, which the survey measured as rare (257 of
258 sweeps share their bench's arm set)" is right about the mechanism and wrong
about the frequency, and it is wrong because the 257-of-258 figure measures
variation *within* a section across its points, which I reproduced exactly
(`202608151243_probes/p5_reproduce_257_of_258.out`), while the new vocabulary
creates the different question of variation *between* the sweeps of one bench.
Nothing is broken by this; the override must simply be a normal thing to write
rather than an escape hatch, and the design should not size a sweep section on
the assumption that it is absent.

**4. The roles: baseline and floor.** *Observed*, `[bench.*.normalise]`
(`bench-harness/src/config.rs:168-186`), used 157 and 114 times by vehje.
Per-sweep is the right granularity, because adding a null-floor arm is a property
of the comparison being run.

### What is measured

**5. The regime.** *Observed* in Rust only: `Regime::Warm`,
`Regime::ColdCycle(m)`, `Regime::Stream` (`bench-matrix/src/decl.rs:12-27`). It
decides whether state is rebuilt, whether the branch predictor is aliased across
m distinct programs, and whether the measured op sweeps the harness input. It
changes what the number means more than any other entry here. It has no config
expression, and the cost of that is visible: `carrier_coldcycle`
(`vehje/mock/benches/carrier/src/bench/coldcycle.rs:24-33`) is an entire forked
family duplicating `carrier_predecode`'s sweep axis, seed, baseline and arm set,
differing in `regime: cold_cycle(16)`, whose own doc comment instructs the reader
to "Compare cell-for-cell against `carrier_predecode` (memorized): the delta is
the memorization the warm numbers hide" (`coldcycle.rs:10-11`). A comparison the
author tells the reader to make cell-for-cell across two benches is one bench
with two sweeps.

**6. The routine.** *Observed* in Rust: the monomorphised bridge, today a
`(name, n)` match arm (`arvo/mock/benches/src/main.rs:227-595`). Per bench rather
than per sweep in every case measured: 256 table rows over 47 bench names use 15
distinct bridge types, and **no bench name needs more than one**
(`202608151243_probes/p3_classify_drivers.out`). So `routine` is a bench-level
key and a sweep never overrides it.

**7. The setup entry point.** *Observed* in Rust: `MatrixDecl.setup_path`, plus
per-arm overrides (`decl.rs:50, 87`). Bench-level with a per-arm override, which
is the shape `bench_matrix!` already has and which the entgrid and cfg families
both use (`entgrid.rs:55-60`, `cfg.rs:48-60`).

**8. Per-arm cargo features.** *Observed* in Rust: `CellDecl.features`
(`decl.rs:51`), unioned into the generated crate's dependency line
(`bench-harness/src/matrix.rs:241-248`). An arm that reaches a feature-gated API
cannot be expressed in config at all today.

**9. The workload.** *Observed* as a bench-level config key
(`config.rs:89`); **proposed** as per-sweep. A bench comparing one arm set under
an empty workload and under a realistic one is a sweep over surrounding context,
and today it costs two bench names.

**10. Timing.** *Observed* as an optional per-bench override
(`config.rs:133-136, 203-211`) that **no consumer has ever set** (zero
occurrences across all four manifests, `202608151243_probes/p1_count_section_config.out`).
Per-sweep is the right granularity because a sweep whose points span 16 KiB to
32 MiB needs different `runs_per_pass` at its ends.

**11. The validation policy**, `may_differ` and `required`. *Observed* in the
schema (`config.rs:114-124`), never set by any consumer, and hardcoded in Rust
for 13 benches in hilavitkutin (`main.rs:213-216`). Per-sweep, because a sweep
that adds a deliberately divergent arm changes the policy for that sweep alone.
The semantics belong to the validation topic; what this design fixes is where the
flag is written.

**12. Threads.** *Observed* two ways, both wrong. `threaded` is a bool meaning
"do not P-core-pin" (`config.rs:125-132`), set by 6 arvo sections; the actual
thread count is packed into arvo's integer as `T` in `KEY = N*10 + T`
(`arvo/mock/benches/variants/bitpack-contend-shared/src/routine.rs:23-25`).
Thread count is a parameter axis and belongs in the axis set; the pinning policy
is a separate boolean and should keep its own key.

### What the sweep must not carry

The shared seed table (`bench-matrix/src/scaffold.rs:24-41`), the anti-hoist
chain, the setup-versus-iteration split, the fidelity digest, the calibration
floor (`bench-core/src/lib.rs:486`), and the fat-LTO one-codegen-unit profile
(`matrix.rs:263`). `bench-matrix` already made these non-negotiable and that is
correct: they are what makes two sweeps of one bench comparable at all. The test
that separates this group from the twelve above is one question: *if two sweeps
of one bench differ in this, are their numbers still comparable cell for cell?*
For the twelve the answer is yes provided the difference is recorded. For these
six it is no, silently.

### The worked example, measured rather than invented

arvo's warm-container family is ten sections that the redesign's own grouping
makes one bench. Decoded from its own key encoding
(`f_warm_container_as_sweeps.out`), it is this:

```
sweep                       axis  values                     held
width-l1                    w     [8,13,16,32,60,64]         d=3, nc=8192,    op=wrap-reduce
width-l2                    w     [8,13,16,32,60,64]         d=3, nc=1048576, op=wrap-reduce
density-w13                 d     [1,2,4,8,16]               w=13, nc=8192,   op=wrap-reduce
density-w64                 d     [1,2,4,8,16]               w=64, nc=8192,   op=wrap-reduce
precise-container-width-l1  w     [8,13,16,32,60,64]         d=3, nc=8192,    op=sat-reduce
warm-elementwise-width-l1   w     [8,13,16,32,60,64]         d=4, nc=8192,    op=wrap-elementwise
precise-elementwise-w-l1    w     [8,13,16,32,60,64]         d=4, nc=8192,    op=sat-elementwise
warm-affine-collapse-l1     w     [8,13,16,32,60,64]         d=3, nc=8192,    op=wrap-affine
precise-widening-thm-l1     w     [8,13,16,32,60,64]         d=1, nc=8192,    op=sat-widen
warm-affine-density-w13     d     [1,2,4,8,16]               w=13, nc=8192,   op=wrap-affine
```

Seven sweeps of `w` over the identical six widths, three of `d` over the
identical five densities, and **every difference between them is a held value**.
Written against the settled per-file form, with fields top-level and no wrapper
table:

```toml
title = "Warm/Precise container rule against deletion, rung(W+1), and primitives"
workload = "realistic"
master_seed = 0x1234_5678_9ABC_DEF0
routine = "bench_warm_container_shared::Case"
regime = "warm"
arms = ["kernel", "headroom", "minimum", "plusone", "native", "lanes-deferred"]
baseline = "native"

[axis.w]
doc = "declared width in bits"
[axis.nc]
doc = "element count"
values = { small = 8192, large = 1048576 }
[axis.op]
doc = "reduction semantics"
values = { wrap_reduce = 0, sat_reduce = 1, wrap_elementwise = 2,
           sat_elementwise = 3, wrap_affine = 4, sat_widen = 5 }
[axis.d]
doc = "arithmetic operations per element before the accumulation"

[sweep.width-l1]
sweep = "w"
points = [8, 13, 16, 32, 60, 64]
hold = { nc = "small", op = "wrap_reduce", d = 3 }

[sweep.width-l2]
sweep = "w"
points = [8, 13, 16, 32, 60, 64]
hold = { nc = "large", op = "wrap_reduce", d = 3 }
arms = ["kernel", "headroom", "minimum", "plusone"]   # measured: this sweep runs 4

[sweep.density-w13]
sweep = "d"
points = [1, 2, 4, 8, 16]
hold = { w = 13, nc = "small", op = "wrap_reduce" }
```

Three things to notice, each of which is the point of the exercise. The held
values are named rather than being digit positions. `width-l1` and `width-l2`
are visibly the same sweep at two residencies rather than two unrelated bench
names. And the arm override on `width-l2` is a normal line, because five of
seven arvo families need one.

## 3. What is generated and what is written, checked as a compile question

The other expert argues that the typed dispatch table must be generated rather
than macro-expanded, "because monomorphisation needs literal tokens, so
`macro_rules!` cannot iterate a const point list"
(`202608151234_what-the-consumer-should-write.md:341-346`). **The conclusion is
right, the stated reason is not, and the difference changes the design.**

**Probe A: the premise is false as stated.** `Case<{ POINTS[0] }>` compiles and
monomorphises, with no feature gate, where `POINTS` is an ordinary const array
(`a_const_index_in_const_arg.rs`, `a_const_index_in_const_arg.out`).
Monomorphisation needs a **const expression whose value is known**, not a literal
token; an indexed const item qualifies.

**Probe B: the conclusion is right for a different reason.** A `macro_rules!`
repetition requires a metavariable "matched as repeating at this depth"
(`b_macro_cannot_iterate_const.out`), which is a property of tokens. A const item
supplies no repetition depth, so a macro cannot emit one arm per element of
`POINTS`. Confirmed on the pinned nightly.

**Probe C: so what must be generated is the arity, not the values.** A dispatch
table over four named axes, where every axis value lives in one
`const PTS: [Point; K]` and the only generated tokens are the bare index list
`0, 1, 2`, compiles and produces distinct monomorphisations
(`c_multiaxis_table.rs`, with controls asserting that two points differing only
in two fields do not collapse and that the fn items are distinct).

That distinction is worth the probe, because it changes what can drift. A
generated table that restates **values** can disagree with the manifest, and in
arvo it already does: six `(bench, point)` pairs exist in the Rust table with no
manifest entry (`202608151243_probes/p4_manifest_vs_table_drift.out`). A
generated table that restates only the **count** cannot disagree about a value,
because there is only one place a value is written.

**Probe D: the arity cannot be eliminated, and the wall is a forbidden feature.**
A recursive const-generic trampoline would produce K monomorphisations from one
number, putting nothing about the point set in Rust. It needs `I - 1` in
const-argument position and is refused with "generic parameters may not be used
in const operations ... add `#![feature(generic_const_exprs)]`"
(`d_recursive_arity.out`). `unstable-features.md` lists `generic_const_exprs` as
**FORBIDDEN (op's call)**. Per
`a-refused-bound-wants-a-trait-not-a-feature.md` I looked for the trait
decomposition: a type-level successor chain carrying the index as an associated
const needs one impl per index, so it relocates the K tokens rather than removing
them. **K monomorphisations require K tokens, and the only escape is banned.**

**So the fork resolves as follows, and it is narrower than either side put it.**
The count of points per sweep must reach Rust as tokens, and the tool is the
only thing that knows it, so **generation is required and a `routine_table!`
macro over hand-written literals is not sufficient**. But the generated artifact
should be an **index list plus one const point table**, not a restatement of
values, which is a strictly smaller and non-drifting generated surface than
either the redesign's `routine_table!` or the other expert's substituted-string
table. The consumer's Rust, where it exists at all, holds the measured op and
the setup, and never a point.

**What this costs today, counted.** arvo currently writes its point set in four
places: `bench.toml` (256 rows), the driver's routine table (256 rows), each of
81 arms' `#[bench_variant(sizes = [...])]` (1328 literals), and three shared
crates' `pub const ALL_KEYS` (131). **1971 integer literals describing 256
distinct pairs, in four locations, kept in agreement by hand**
(`h_point_set_copies.out`). Copies 1 and 4 currently agree exactly for all three
families that have an ALL_KEYS table (57, 46 and 28, no difference either way);
copies 1 and 2 do not, by six pairs. Under the shape above the number is 256, in
one place.

## 4. Multi-axis, not flattened, and what it costs

**Decided: multi-axis.** Two consumers flattened independently, neither had a
choice, and both flattenings are measurably worse than the thing they replaced.

The evidence that this is a framework limitation rather than a consumer habit is
that `bench-matrix/TODO.md:60-63` names the fork and both options in the
framework's own words a month before the redesign: "**multi-sweep.** The entropy
grid is a 2D `op_correlation x locality_window` product; the macro's `sweep` is a
single axis. Either add cartesian multi-sweep, or keep it a single flattened
sweep with composite values." vehje's entgrid family took neither and did a
third thing, flattening the grid into nine arms
(`vehje/mock/benches/carrier/src/bench/entgrid.rs:34-60`, nine cells named
`c0_w4` through `c900_wmax`, all running the identical measured expression with
different setups, declared sweep axis empty at `entgrid.rs:40`). arvo took the
second, packing composite values into the integer.

**The price of multi-axis, on the 36 arvo sections that carry a packed key.**
Those 36 sections hold 40 varying axes and 87 constant settings inside the
integer (`202608151243_probes/p2_decode_packed_points.out`). Migrating them
means:

- **Deleted:** five families of `key_*` decode functions and their doc-comment
  encodings, roughly 50 lines each in `warm-container-shared`, `warm-clamp-shared`
  and `satfold-shared`, 15 in `wide-rung-shared`, plus the two-field split in
  `bitpack-contend-shared/src/routine.rs:23-25`. The three `ALL_KEYS` tables and
  their round-trip tests go with them, since there is no encoding left to round
  trip.
- **Changed:** `Case<KEY>` becomes `Case<POINT>` where `POINT` is a struct const
  parameter (section 5), and each `Routine` impl reads `P.w` instead of
  `key_w(KEY)`. That is a mechanical substitution at the roughly 30 call sites
  the decoders have inside those crates.
- **Unchanged:** every arm's measured code, the harness, the CSV schema, the
  statistics.
- **The artifact-name question is real and is section 7's**, because
  `<bench>_n<point>.csv` has no obvious tuple form and arvo has 254 committed
  triples at the flat root.

**The price of the alternative, keeping flattened.** The 87 held settings stay
digit positions, the `_n130003` filenames stay undecodable without a module doc,
the history ledger stays keyed on an integer whose meaning is a comment, and the
next family that needs a fifth axis packs it or forks. entgrid stays nine arms,
which section 6 shows is not merely ugly.

**On the forked vehje family.** With `regime` a per-sweep key,
`carrier_coldcycle` is deleted and becomes one more sweep of `carrier_predecode`:
one `bench_matrix!` block of 60-odd lines and its six generated sections go, and
the cell-for-cell comparison its doc comment asks for becomes two rows of one
bench rather than a manual join across two.

## 5. The single const generic parameter is not the blocker, and need not be lifted

The constraint is real and is stated twice: the proc macro finds the first const
generic parameter and errors if there is none
(`bench-macro/src/lib.rs:246-257`), the emitted call passes exactly one argument
`#fn_name::<#n_lit>` (`bench-macro/src/lib.rs:273, 301`), and the module doc says
"The function must have exactly one const generic parameter (the dispatched
size)" (`bench-macro/src/lib.rs:55-56`).

**It can be lifted**: probe C compiles a four-const-parameter measured function
and dispatches it. But **it should not be**, because there is a better answer
that leaves the rule untouched.

**Probe E: give the one const parameter a struct type.** With
`#![feature(adt_const_params)]` and a `#[derive(ConstParamTy)]` struct of `usize`
fields, `fn run<const P: Point>()` monomorphises per point and reads `P.w`,
`P.nc`, `P.op`, `P.d` by name (`e_point_as_one_const_param.rs`). Probe E2 is the
same file with `const_param_ty_trait` and `allow(incomplete_features)` removed:
**it compiles clean with `adt_const_params` alone and emits no warning**
(`e2_no_allow.out`). `unstable-features.md` lists `adt_const_params` (#95174) as
**ALLOWED**, "largely complete", a 2026 const-generics stabilisation target, with
the unsound reference-carrying part split out into `unsized_const_params`, which
this does not use because every field is a sized `usize`.

So the answer to "can the constraint be lifted, and if not where should
multi-axis identity live" is: **it does not need to be lifted, and multi-axis
identity lives in the type of the one const parameter.** `#[bench_variant]`'s
rule stands unchanged, the arm's signature keeps one const generic, and the four
axes arrive as named fields instead of as digits.

**Probe I: the shipping shape, end to end, with controls.**
`i_shipping_shape.rs` builds the arm side as it would ship: one struct const
parameter, an FFI entry taking a single `usize` that is an **index into the
generated point table** rather than a packed value, and a `macro_rules!` fed only
the index list. Its controls assert that arvo's real `width-l1` and `width-l2`
rows at `w = 8` do not collapse into one measurement, that distinct widths do not
collapse, and that an out-of-range index is refused rather than silently served.

**And the hazard that change introduces, which the design must close.**
`bench_entry(input_ptr, output_ptr, n: usize)` (`bench-macro/src/lib.rs:321-325`)
keeps its signature if `n` becomes an index, so nothing in the type system
notices. The load-time staleness check compares `bench_abi_hash` against
`abi_hash()` (`bench-harness/src/harness.rs:100-103`), and `abi_hash` folds only
`size_of::<FfiBenchCall>()`, the field count 4, and four 8-byte field widths
(`bench-core/src/lib.rs:464-481`). **It does not cover the meaning of `n`.** So a
stale dylib built under value semantics loads cleanly and is dispatched with an
index it reads as a value. That is not hypothetical: vehje's own declared point
list is `[1, 2, 4, 8, 16, 32, 64, ...]`
(`vehje/mock/benches/src/main.rs:46-48`), and indices `1, 2, 4, 8` are all
declared points in it, so four of sixteen indices would silently select the
**wrong monomorphisation** rather than hitting the unsupported-`n` panic
(`i_shipping_shape.out`). **Requirement: fold a point-encoding version into
`abi_hash`.** It is a one-line change to a const fn and it converts a silent
wrong answer into a refusal at load.

Two notes for whoever implements it. `Routine::build_input_bytes(seed)` takes
only a seed (`bench-core/src/lib.rs:170`) because the routine type is already
monomorphised over its parameters, so the input shape does not depend on the
`n` that crosses the FFI; only `max_call_us(_n)` (`bench-core/src/lib.rs:140`) and
the variant's own dispatch read it. And the byte-shaped benches, where `n`
genuinely is an input size, keep a one-axis point whose single field is that
size, so nothing about them changes except that the field has a name.

## 6. Where this constrains the other two topics

Stated as constraints, not as decisions. Both belong to their own experts.

**To the metadata topic: a cell's recorded identity must be the full axis
assignment, not the sweep name plus a scalar.** This is forced, and the argument
is measured. `warm-container-width-l1` and `warm-container-width-l2` sweep the
identical six widths and differ only in a held value, `nc = 8192` against
`nc = 1048576` (`f_warm_container_as_sweeps.out`). Today the packed integer
distinguishes them: `130003` against `131003`. If the record holds
`(bench, sweep, swept value)` then both rows read `(warm-container, width-*, 13)`
and **the axis that distinguishes them is nowhere in the record**, so the new
shape would lose information the packed key currently carries. The record must
hold every axis, swept and held, with its name.

Three further things belong in the record for the same reason, each being a
sweep entry from section 2 whose value changes what the number means:
**regime**, **workload**, and the **effective timing**. `EnvMeta` today records
cpu, os, rustc, git commit, timestamp and counter frequency
(`bench-harness/src/env.rs:15-33`) and none of the three, so a committed CSV
cannot currently be attributed to the measurement conditions that produced it
beyond the hardware. Whether they live in the meta JSON or as CSV columns is the
metadata topic's call.

**To the validation topic: fixing the arm/sweep category error changes what
cross-arm validation is being asked to do, before any semantics are decided.**
entgrid's nine arms are nine points of a 2D parameter sweep running the identical
measured expression on nine different programs (`entgrid.rs:34-60`). Cross-arm
byte-exact comparison over them is a category error: they are not competing
implementations and have no reason to agree. Once they are sweep points, the arms
within any one cell are a single implementation compared against itself, which is
exactly the case byte-exact comparison exists for. So part of what looks like a
validation-semantics problem dissolves into a vocabulary fix, and the validation
expert should decide their semantics against the corrected shape rather than
against today's. What remains theirs, and what I am not deciding: whether
`may_differ` is per-sweep or per-arm-pair, what a mismatch does to the run, and
what the `digest` column is for given nothing currently compares it
(`digest` appears nowhere in `bench-harness/src/validation.rs`; established in
the prior file's section 6).

## 7. What I would not change

**The measurement core, entire.** The scaffold's seed table, S-versus-I split,
first-touch pass, fold-one-keep-alive rule and anti-hoist chain
(`bench-matrix/src/scaffold.rs`), cdylib-per-arm isolation, the subprocess
driver, the calibration floor, the fat-LTO one-codegen-unit profile. This design
touches how a measurement is *identified*, never how it is *taken*.

**`#[bench_variant]`'s one-const-parameter rule**, per section 5.

**The CSV schema and the committed artifacts.** Nothing above requires a column
to change or a committed file to move.

**The `_n<point>` filename convention, provisionally, and I flag it as the one
place my design has an unresolved cost.** A tuple point has no obvious filename
form, and arvo has 254 committed csv/meta/findings triples at its flat root whose
names encode the packed integer. The cheapest answer is to keep the packed
integer as a *filename-only* rendering derived from the axis assignment, so
committed names survive and the axes still live in config and in the record. I
have not established that the rendering is injective for every family, and it
must be if it is used as a filename. Section 9 lists it.

**`bench-matrix`'s three-layer split** (transport, discipline, consumer domain).
It is the right decomposition and this design strengthens it: the discipline
layer gains the axis product, and the transport layer's `MatrixSpec` gains
nothing at all, because it already has N axes and only ever gets one
(`generate.rs:95`).

**The `routine_for` hook.** The other expert argues for keeping it as the escape
where a routine cannot be named by a path
(`202608151234_what-the-consumer-should-write.md:365-372`), and I agree, with one
addition from my own count: since no bench name needs more than one bridge type
(`202608151243_probes/p3_classify_drivers.out`), `routine` is a bench-level
config key and the hook's residual job is genuinely small rather than merely
believed to be.

## 8. The alternatives I did not take

**Lifting the single const generic parameter to N parameters** (probe C
compiles). Rejected because probe E gets the same expressiveness without
touching the proc macro's contract or the 1064 files carrying the attribute, and
because a struct const parameter gives the axes names inside the measured
function whereas four positional const parameters do not.

**Keeping the packed integer and adding a decoder to the config**, so
`bench.toml` declares `encoding = "W*10000 + NC*1000 + OP*100 + D"` and the tool
decodes for reports. Rejected: it keeps the injectivity obligation on the
consumer, keeps the artifact names undecodable without the config, and makes the
tool parse an arithmetic expression, which is the "no expression language" line
the harness already drew for itself (`bench-harness/src/matrix.rs:25-27`).

**A `routine_table!` macro over hand-written literals**, the redesign's §9 item.
Rejected on probe B plus the drift count: it moves 256 rows to 47 rows plus 256
integers, and those 256 integers are a fourth copy that can and does disagree.

**Making the sweep a product over parameter axes at the harness layer**, reusing
`MatrixSpec::expand` directly rather than adding a sweep concept above it. This
is tempting and I did not take it, because the product over parameter axes emits
*sweeps* and the product over implementation axes emits *arms*, and those two
artifacts have different lifetimes: an arm is a compiled cdylib, a sweep is a
row group in a report. Unifying them at the type level would make one mechanism
responsible for both, which is a bigger change than this round should carry. It
is a real option and I record it for whoever attacks this next: the machinery is
there and unused, and if the two artifacts can be reconciled the design gets
smaller.

## 9. What I could not settle

- **The topic-file review I was asked for.** The branch and the three topic
  files are not on the remote (section 0). Still owed.
- **Whether the packed-integer rendering is injective per family**, which decides
  whether it can serve as a filename after the axes move to config. Checkable
  mechanically over the 36 packed sections; I did not run it because the
  filename convention belongs to the layout topic and I did not want to decide
  it by arithmetic in passing.
- **How a tuple point renders in a report table column.** The CSV keeps one row
  per arm per sample and gains nothing, but the report's per-point grouping and
  the history TSV key both need a canonical string form. I have not proposed one.
- **Whether `hold` should accept an axis the bench declares but the sweep does
  not mention.** Defaulting a missing axis silently is how a sweep ends up
  comparing across two axes at once, which is the failure the whole design exists
  to make visible; refusing it is safer and costs a line per sweep. I lean to
  refusing and did not establish it.
- **Anything about performance.** No measurement was taken and none is needed;
  this is a question about where declarations live. The one place performance
  could enter is compile time, since a struct const parameter monomorphises per
  point exactly as the packed integer does, so the instantiation count is
  unchanged by construction.
