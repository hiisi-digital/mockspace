# Do the three compose

The seam review of the bench consolidation round: the three topic files
(`202608151338`, `202608151339`, `202608151340`) and the three proposals
(`202608151351_what-a-sweep-carries.md`, `202608151356_validation-semantics.md`,
`202608151419_the-cell-record.md`). The assignment: do they contradict each
other, what falls between them, is the whole implementable as one change, and
is it too much.

The short answers, expanded below with evidence: **the three do not contradict
each other**, and each honoured its scope with more care than the average round
manages. **What falls between them is larger than any disagreement among
them**, and the largest gap is not one the brief's three boundary lines could
have produced: all three proposals were designed and probed against `dev`
while a 3,501-line open PR (**#21, `feat/bench-consolidation`**) sits on the
identical surfaces and already implements several of the things the proposals
treat as open, absent, or future. Two of the three make checkably false
statements about that PR. **The composition is not implementable as one change
as written**, first because its relationship to PR #21 is undefined, and
second because the validation proposal defers its own precondition to another
round. **And yes, it is too much for one change**: the honest shape is two,
with a cut list, given in section 7.

## 0. Gates

**Canon gate.** No `mock/canon/` exists in this repository, confirmed by `ls`
as all three prior experts confirmed it. Nothing above the ratified topic
files to defend. The maintainer's settled points from the dispatch (declared
membership `["**"]`, no wrapper table in the per-file form, `[bench.<name>]`
supported indefinitely, one shared output tree, one `CellRecord` with every
writer a projection) are treated as fixed; nothing below reopens them.

**Test gate.** I ran the suite myself rather than inherit:
`cargo test --workspace --no-fail-fast` on `feat/bench-round-consolidation` at
`324911f`: **551 passed, 0 failed, 18 ignored, 28 suites.** That is the fourth
independent reproduction of this figure. The zero-test surfaces
(`driver/mod.rs`, `history.rs`, `cache.rs`, `env.rs`) are as the brief and the
prior experts state; re-confirmed by grep.

**The known tautology at `bench-core/src/lib.rs:637-651` is unowned, and the
one claim in this round about its fate is false.** The validation expert wrote
"PR #21's branch already deletes it"
(`202608151356_validation-semantics.md:90`). I ran the diff:

```
$ git diff origin/dev origin/feat/bench-consolidation -- bench-core/src/lib.rs
-    // The hash must be deterministic and must actually reflect the current
-    // four-field layout (not the retired single-field one). This is the value
-    // the harness compiles into every variant and checks on load.
     #[test]
     fn abi_hash_reflects_four_field_layout() {
```

PR #21 deletes **the three comment lines above the test and keeps the test
intact** (present on `origin/feat/bench-consolidation:bench-core/src/lib.rs:634`).
Deleting the comment over a tautology while keeping the tautology is the
purest possible form of making a fake test look less fake without making it
less fake. The expert's claim was one `git diff | grep` away from being
checked and was not, in the same file whose own section 0 lectures that "a
reproduction that agrees is real corroboration". This arc's whole pathology,
named in my dispatch, is instruments believed by reading instead of running,
and here it is again, in the round convened to end it. The deletion of that
test therefore belongs to **this** round (section 7, change 1), because
nothing else in flight does it. One mitigation in the test's favour, stated
so the deletion is done for the right reason: the literal `32` against
`size_of::<FfiBenchCall>()` would fail on a layout change, so it is not
structurally incapable of failing; but the sibling test at `:628-630` already
pins size and alignment, so this test's only *additional* content is a
transcription of `abi_hash()`'s own fold, which is the tautological part and
the reason it goes.

**What I re-ran or re-read rather than believed**, per the dispatch's warning:
the suite (above); `grep -c digest bench-harness/src/validation.rs` = 0; the
driver's `Err` arm and the `dropped` → `required_failure` trace
(`driver/mod.rs:415-422,598`); the `first_mismatch_reason` collapse
(`validation.rs:439-440,466,515-518`); `resolve_routine`'s hook-first ordering
and the byte-dispatch `may_differ` read (`driver/mod.rs:119-147`);
`DataSetMeta::default()` at `analysis.rs:262` and the dead Methodology
condition at `report.rs:116`; the `report.rs:37` vs `driver/mod.rs:506` seed
disagreement; `history.rs:97-116`'s header-skip and `p.len() >= 9`;
`.meta.json`'s zero readers; `Cache`'s zero driver call sites; `toml = "0.8"`
in `bench-harness/Cargo.toml`; `render_bench_section`'s exact emission set
(`matrix.rs:276-310`); `abi_hash`'s fold coverage (`bench-core/src/lib.rs:464-481`);
the staging runid (`staging.rs:87-99`); probe A re-compiled and re-run on the
pinned nightly (passes); the TOML u64 round-trip probe re-built and re-run
(passes, with its negative controls); and in the read-only consumer trees: 27
`bench_matrix!` files in vehje, `coldcycle`'s "compare cell-for-cell" doc,
`entgrid`'s empty sweep axis, arvo's hand-wired `harness::validate`
(now at `main.rs:162`; the topic's `:134-144` has drifted with the consumer
tree, substance unchanged), and zero `may_differ`/`required` in both
manifests. **Every load-bearing claim in the three proposals that I tested
held**, with exactly two exceptions, both about PR #21, both below.

## 1. The finding that reframes the round: PR #21 is the surface, and nobody designed against it

The round branch is based on `dev` (`6ab55ee`). PR #21
(`feat/bench-consolidation`, nine commits, +3,501/-370) is open against the
same crates and already implements, today, on a branch in this repository:

- **Two-level identity.** `BenchConfig` on that branch carries `bench`,
  `sweep`, `bench_name` as the `<bench>/<sweep>` composite, and a `nested`
  flag deciding output naming `<bench>/<sweep>_n<point>_report.md`
  (`origin/feat/bench-consolidation:bench-harness/src/config.rs:664-680`).
- **A generated driver.** `src/bench_gen.rs` (621 lines) generates the driver
  binary from `bench.toml`, emitting
  `byte_routine_dispatch!(out = .., sizes = [..])` from the manifest
  (`bench_gen.rs:257-261`), which answers the sweep topic's "whether a
  consumer driver exists at all" question in the affirmative-generated.
- **`routine_table!`** (`spec.rs:79`, commit `9a5bad3`), the exact mechanism
  the sweep proposal's §8 rejects.
- **Four named hooks with a verdict vocabulary.** `on_init`, `after_init`,
  `routine_for`, `after_cell`, with `CellVerdict::{Pass, Note, Fail}` and the
  documented rule that "a `Fail` verdict withholds the cell's history append
  outright ... The staged artifacts still promote: they are the evidence of
  what ran and why it was failed" (`driver/hooks.rs:17-23`, commit `e840e2e`).
  That is the history projection policy the record topic ratifies in prose,
  already half-implemented.
- **`[build]` overrides.** `BuildSection` in config, `profile_args_for(build)`
  in the build step (`src/bench.rs:469,548` on that branch). The
  "open PR" the record topic warns about is not hypothetical future work; it
  is nine commits of reviewable diff.
- **Part of the vocabulary migration**: "accept arms and points spellings"
  (`35a8e64`).
- And it does **not** touch `bench-harness/src/harness.rs` (zero-line diff),
  so the hardcoded `build_profile` literal at `harness.rs:54` becomes an
  actual lie the moment any `[build]` override is used. The record topic
  predicted exactly this (`202608151340...:29-31`); PR #21 confirms it live.

Against that, the two false claims:

**`202608151419_the-cell-record.md:97-101`**: "no `[build]` or per-bench
profile-override config surface exists anywhere in `config.rs` (grepped for
`BuildSection`, `[build]`, `opt_level`: zero hits), so the 'open PR' the topic
warns about is not yet buildable against anything in this repo; it is future
work." The grep was run on the wrong branch and the conclusion is wrong on
the branch that matters. The surface exists, in this repository, buildable,
one `git fetch` away, and the topic file had explicitly pointed at it. The
record expert then designed the `build.profile` handoff granularity as
"genuinely their unresolved surface, not mine" (`...:618-624`) when the
surface was sitting in the PR with its actual mechanism (`profile_args_for`,
per-tree `[build]`, persisting nothing) available to design against. The
handoff *requirement* the record states is correct and PR #21 makes it
urgent; the refusal to look at the one concrete implementation of the thing
being designed for is the defect.

**`202608151356_validation-semantics.md:90`**: the tautology claim, section 0
above.

**And the structural consequence, which is the real cost.** The record
expert's most careful piece of reasoning, the §3 argument that `manifest_key`
must absorb `bench` because "there is no shipped concept of a family/bench
grouping one level above that" and "a field with no way to populate it is
worse than no field" (`202608151419...:332-359`), is **reasoned correctly
from a surface that is about to stop existing**. On PR #21 the split the
record expert refused to pre-guess is already shipped: `bench` and `sweep`
are two populated fields and the composite is a third. The record's
`Identity` must be, from day one:

```
identity: { bench, sweep, manifest_key (= the composite / legacy section key),
            title, runid, axes }
```

with `bench` and `sweep` required, equal to each other for a flat-tree legacy
section exactly as PR #21's `for_size` already populates them
(`config.rs:608-616` on that branch: a non-nested section gets
`(bench_name, bench_name)`). No optional identity field is needed; PR #21
already gives every cell honest values for both. The record expert's own seam
note anticipated this shape; the update is mechanical and the deliberation
that produced the one-field answer is not wasted, merely overtaken.

Similarly, the validation expert's driver traces survive PR #21 semantically:
I checked the PR's driver, and the `Err` arm is byte-identical
(`eprintln!` + `dropped.push`, `variant_paths` untouched,
`origin/feat/bench-consolidation:bench-harness/src/driver/mod.rs:523-524`),
`routine_for` is still consulted before the byte dispatch (`hooks.rs:15-16`),
and the history withholding keys **only** on the `after_cell` hook's `Fail`,
not on a validation mismatch (`mod.rs:620,681`). So every semantic finding in
the validation file holds on the PR branch too; only the line numbers move by
roughly a hundred lines, which matters because the round's eventual src CLs
will otherwise be written against coordinates that no longer exist.

**The recommendation, plainly.** Land PR #21 first, then rebase the round's
three designs onto it. The PR implements, in code, several limbs of the
direction the round ratifies (nested identity, generated driver,
fail-withholds-ledger, arms/points spelling), and nuking it to rebuild the
same limbs from the round would discard working, reviewable mechanism to
recover a sequencing purity nobody needs. Where the round supersedes a PR #21
choice, and it does in exactly one place, `routine_table!` versus the sweep
proposal's generated index list (section 4d), the round's design wins at the
design tier and that macro is deleted or demoted in change 2; that is a
normal supersession, not a reason to hold the PR. What is *not* acceptable is
the current state: a round whose three proposals collectively cite the PR
twice, falsely both times, and design identity, handoffs and driver flow
against the branch it obsoletes.

## 2. Do the three contradict each other? No, and the scoping held.

I checked the three pressure points the dispatch named, plus the ones I found.

**A cell's identity.** The sweep proposal defines cell = one
(bench, sweep, point) over its resolved arms (`202608151351...:64`) and
forces the record to carry the full axis assignment, swept and held, by name
(`...:483-492`, the `width-l1`/`width-l2` collapse argument, which I verified
against the probe output: identical six widths, distinguishable only by the
held `nc`). The record proposal satisfies it with
`identity.axes: BTreeMap<String, AxisPoint>` holding every axis, open-shaped
so the multi-axis work lands without a record change
(`202608151419...:206-215`). The validation proposal keys its verdict per
(bench, size) cell and demands the tag attach to the cell, not the bench
(`202608151356...:668-672`). All three mean the same object. The one
correction needed is the `bench`/`sweep` split forced by PR #21, above,
which the record already declared as its seam.

**What a verdict means against where it is stored.** Topic 1338 forbade the
record topic from settling the verdict's meaning by choosing storage; topic
1340 forbade the reverse. Both complied: the validation file's §8 specifies
verdict *content* and explicitly does not place it; the record file's
`execution.validation: Option<ValidationOutcome>` places it and explicitly
does not shape it (`202608151419...:306-314`), citing the other file's §8 by
line range, which I checked and which is correct (`202608151356...:650-678`).
This is the cleanest seam of the three. The residue that fell between them is
the projection policy, section 3b.

**Can the record's fields be populated given the struct-typed const
parameter?** Yes, and I checked the mechanism rather than the assertion. The
sweep design moves the point across the FFI as an index into a generated
const table (`202608151351...:449-469`), which could have starved the record:
a record written by the driver cannot read a const table compiled into the
consumer's arms. It does not starve it, because the record's `identity.axes`
is populated from the **config side**, where the sweep design keeps the full
assignment as data (`[sweep.*]` `points` + `hold`), and the driver resolves
the tuple before dispatch exactly as it resolves `n` today. The FFI index
never needs to round-trip back into identity. The one hazard on this path is
the sweep expert's own: the index/value ambiguity on a stale dylib, closed by
the abi-hash version fold they require (`...:454-469`), which I verified is
currently absent from `abi_hash` (`bench-core/src/lib.rs:464-481` folds size,
count, widths, nothing about `n`'s meaning).

**Scope discipline.** Each was told not to settle a neighbour's half. Checked
against the deliverables: the sweep file states its cross-topic content as
constraints and labels them so (`...:479-517`); the validation file's one
excursion into storage is the §8 content list the topic itself demanded; the
record file takes both neighbours' outputs as inputs and shapes neither. No
violation found. The two earlier investigations (`202608151234`,
`202608151243`) are consistently cited for what they proved rather than how
they were written. This part of the round worked.

**One internal tension inside the validation file, not a cross-file
contradiction.** Its §7 first refusal ("require `may_differ` to be set
explicitly whenever `variants` has two or more entries and the routine is not
byte-dispatched with the key wired") coexists badly with its own §1d finding
that on the `routine_for` path the key is dead: enforced as written, every
arvo bench would be required to write a manifest key that §1d proves nothing
reads. A refusal that compels consumers to write dead configuration is worse
than the silence it replaces. Section 4e resolves this; section 7 cuts the
rest of §7's first refusal from the round.

## 3. What falls between the three

Seven items. For each: why no one owned it, and the resolution, built rather
than described where building it is possible in prose.

### 3a. The canonical string form of a point

Needed by artifact filenames, the history key, and report grouping. The sweep
expert flags it unresolved and defers it to "the layout topic"
(`202608151351...:536-539,590-596`), **and there is no layout topic in this
round**; the three subjects are validation, configuration, record. Topic 1338
scoped layout out to "its own topic file" (`202608151338...:9-11`) and no
topic file carries it. This is the boundary the coordinator drew badly, and
exactly as predicted, no expert reported the gap because each was told the
neighbouring subject was not its own. Meanwhile the record proposal silently
assumes the scalar form, writing its path as `<bench>_n<point>.meta.toml`
(`202608151419...:177-181`), which does not survive tuple points.

**Resolution: render the point as the ordered axis-value list, and retire the
packed integer entirely.** The rule: for a cell whose axes are
`{a1 = v1, a2 = v2, ...}` in `[axis.*]` declaration order, the point renders
as `a1v1_a2v2_...`, using the axis's symbolic label where one is declared
(`w8_ncsmall_opwrap-reduce_d3`). Properties, each load-bearing:

- **Injective by construction.** Two points differing in any axis value
  differ in the string, because the string carries every axis. This dissolves
  the sweep expert's open injectivity question about the packed rendering
  (`...:590-593`): no arithmetic rendering, no injectivity obligation, nothing
  to check per family.
- **Stable under point-list edits**, unlike the index (insert a point and
  every index-named file after it would be renamed).
- **Degenerates to today's convention.** A legacy single-axis bench renders
  `n1024`, and `<bench>_n1024.csv` is the current name, so the flat corpus's
  254 committed arvo triples and the whole `_n<point>` convention are the
  degenerate case of the general rule, not a casualty of it.
- **Composes with PR #21's nesting.** `<bench>/<sweep>_<pointstring>.csv`;
  the bench and sweep are directory and prefix, the point is the residue, and
  the residue is short because the sweep's held axes are constant within a
  sweep and *may be omitted from the filename* (they are recoverable from the
  sweep's `hold` in config and are in the record); only swept axes need
  appear. `width-l1_w8.csv` rather than `warm_container_n80003.csv`.

Constraint to enforce at schema load: axis names and labels match
`[a-z0-9-]+` (no underscore in names, since `_` is the separator), refused
otherwise. The sweep expert's worked example uses `wrap_reduce` as a label;
it becomes `wrap-reduce`. One line of validation, and the rendering needs no
decoder because nothing ever parses it back: the axes live as data in the
record and the config, and the filename is a projection like every other
writer.

### 3b. The history projection policy, and the verdict's absence from `schema_v2`

Three documents say three different things and none reconciles them. Topic
1340 ratifies "a failed cell projects no history row" (`202608151340...:53-56`).
The validation proposal requires history entries be "tagged as
unverified-agreement", hedged with "(or withheld, per §7)" that its §7 never
resolves (`202608151356...:386-394,641-648`). The record proposal's
`schema_v2` adds `runid`, `dylib_hash`, `build_profile` and **no verdict
column**, and says nothing about which cells project
(`202608151419...:531-544`). Left as is, a mismatched cell would project a
normal-looking row into the ledger that feeds `detect_regressions`'s rolling
window (`history.rs:119-135`), which is the laundering the validation file
spends its length forbidding.

**Resolution, and PR #21 already implements half of it:** `e840e2e`'s rule
for the `after_cell` hook, "a `Fail` verdict withholds the cell's history
append outright ... The staged artifacts still promote", generalised into the
projection policy the round needs:

> **A cell projects a history row iff its verdict is clean or consented
> (`outputs_may_differ`) and at least one arm was measured. Every other cell
> exists only as its record.** The record is written unconditionally (the
> ratified "nothing is lost"); the CSV and report are written and carry the
> taint by association with their cell's record; the history ledger, because
> it feeds an automated consumer that cannot read a tag, receives only clean
> rows.

This reconciles all three texts: the topic's sentence holds with "failed"
meaning any non-clean verdict; the validation file's "tagged" holds for the
artifacts a human reads (report, record) and its "(or withheld)" hedge
resolves to withheld for the one artifact a machine reads; the record's
`schema_v2` stays exactly as designed, needing no verdict column because
every row is clean by construction and the runid ties any gap in the series
back to the record that explains it. The distinction that makes this
principled rather than convenient: **tag the artifacts a human interprets,
withhold from the ledger a machine interprets.**

### 3c. The digest becomes load-bearing in the post-cell block, or topic 1338 is unanswered

Topic 1338 demands: "Either it becomes load-bearing or it stops being
carried" (`202608151338...:51-52`). The validation proposal, correctly,
refuses to wire the digest into pre-timing `validate()` (partial coverage,
`0 == 0` false passes, different call path; `202608151356...:459-474`) and
assigns the digest its real role, drift under actual repetition, then defers
the wiring: extending digest computation is "worth a task of its own"
(`...:505-509`). What it never notices is that **for the slice that already
computes a digest, the comparison costs nothing and has a natural home that
did not exist until the record proposal created it.**

The digest rides every sample row into the driver's memory: column 17 of the
samples CSV (`sample.rs:138,158`), populated per `FfiBenchCall`, per arm, per
sample, real for every `bench-matrix`-scaffolded arm. The driver's post-cell
block, the exact centralisation point the record topic ratifies, holds
`result.samples` with all of it. So:

> **In the post-cell block, group non-zero digests by arm. Within one cell,
> every arm's non-zero digest set must be a single value and all arms' values
> must agree. Zero is excluded per `FfiBenchCall`'s own convention
> ("a zero ... means 'not measured by this constructor', never a measured
> zero", `bench-core/src/lib.rs:437-438`). Disagreement is a cross-variant
> verdict like any other: recorded in `ValidationOutcome`, tainting the cell,
> withholding its history row per 3b. Coverage is recorded honestly: the
> verdict names which arms carried a digest and which carried none.**

This is a dozen lines in the block being built anyway, it makes the digest
load-bearing this round for the 594-of-902 vehje arm crates that already
compute it, it catches the under-repetition drift class the reps-1 validation
pass structurally cannot (the validation file's own analysis of why the two
mechanisms are complementary, `...:497-504`), and it manufactures no false
pass because zero never compares. The extension of digest computation to
hand-written arms remains a deferred task exactly as the validation file
says; what is not deferred any more is the comparison, because deferring the
comparison while carrying the column is the state topic 1338 exists to end.
Neither expert proposed this because it sits precisely on their seam: the
meaning is validation's, the block is the record's.

### 3d. The reps-1 forcing must land with the tainting, not in "whichever round touches bench-core"

The validation proposal makes mismatch consequences unconditional (§3:
mandatory tainting) and separately observes that the current byte comparison
is calibration-sensitive for accumulate-into-output routines, with a clean
fix, forcing `__reps = 1` under a validation-mode environment variable, that
it then hands off: "it belongs to whichever round actually touches
bench-core" (`202608151356...:487-495`). Composed, that is a trap: **land
tainting without the reps fix and every rep-sensitive routine becomes a
flaky taint generator**, at exactly the moment taints start withholding
history rows. The proposal's own §5 documents that at least one consumer
(entgrid) currently ships nine deliberately divergent arms relying on the
silence (`...:554-560`). The sequencing constraint is absolute and no file
states it: **the reps-1 forcing is part of change 1, same commit series as
the tainting, or the tainting does not land.** Topic 1338's scope already
names `bench-core/src/lib.rs`, so this is not even a scope stretch; it is the
validation round touching its own declared surface.

### 3e. `may_differ` on the `routine_for` path: the cross-check neither file reached

The validation file proves the manifest key is dead on the hook path
(`...:254-267`) and proposes, then defers, a `Routine`-contract change
(`...:629-639`). The sweep file makes `may_differ` a normal per-sweep key
(`202608151351...:205-210`) and defers the semantics. Composed as written:
the round ships a per-sweep key that is a silent no-op for one hundred
percent of arvo's routines. Nobody proposed the cheap move that requires no
contract change:

> **In `resolve_routine`, after the hook returns a spec, compare
> `config.may_differ` against the bridge's `outputs_may_differ` (already
> reachable: `validation_plan` reads `routine.bridge.outputs_may_differ`,
> `validation.rs:181`; the same field is on the spec the hook returned). On
> disagreement, refuse the run with an error naming both sources. And record,
> in the cell record's `ValidationOutcome`, the effective policy *and its
> source* (manifest key, impl override, impl default).**

This makes the manifest key meaningful on the hook path as a checked
assertion rather than a dead letter, catches exactly the hilavitkutin
failure shape (a hand-wired dispatch bound to the wrong input,
`...:269-277`) at load rather than never, costs one comparison, and touches
no trait. The `required`-style contract change stays deferred as the
validation file wants. The `source` field is a one-enum addition to the §8
verdict content and closes the gap where a reader of the record cannot tell
whether a policy was decided or defaulted, which is the round's originating
defect one level up.

### 3f. The profile handoff has a concrete locus now, and the record should name it

The record proposal correctly derives that the profile is a build-time fact
the harness process cannot re-derive, requires a handoff, and leaves the
granularity "to whoever designs that landing" (`202608151419...:87-116,618-624`).
PR #21 is that landing, it persists nothing, and it does not touch the
`env_meta_to_json` hardcode, so the field the topic calls a lie goes from
"accurate by coincidence" to false the first time `[build]` is used.
Resolution, one sentence the round can carry because the surface now exists:
**the build step writes the resolved profile values (the exact `--config`
arguments it passed) to one file at the tree root beside what it built,
per-invocation, since `PROFILE_ARGS` and PR #21's per-tree `[build]` are both
uniform within a tree; the driver reads it into `build.profile`; absent file
means honest `None`.** Per-arm granularity waits until a per-bench override
exists, which nothing in flight proposes. And `harness.rs:42-62`'s hardcoded
literal is deleted in the same change, since the record replaces `.meta.json`
wholesale.

### 3g. Line-coordinate rot across the whole round

Small but worth one line: every `file:line` in all three proposals is a `dev`
coordinate. Under PR #21 the driver's cited lines move by roughly a hundred.
The research files are the audit trail and stay as written; the round's
eventual doc and src CLs must be authored against the post-PR-#21 tree, or
they inherit citations to code that is not where they say it is.

## 4. Judgements on specific claims, with citations

**4a.** The sweep proposal's §8 rejection of `routine_table!` over
hand-written literals (`202608151351...:570-573`) is **correct and now
supersedes shipped code**, because PR #21 landed that macro (`9a5bad3`,
`spec.rs:79`). The probe-backed argument stands: probe B shows a macro cannot
iterate a const list, probe C shows the generated surface can be an index
list plus one const table, and the drift the values-restatement permits is
observed (six pairs, `p4_manifest_vs_table_drift.out`). One honest weakening
under PR #21: the generated driver regenerates its values-restatement from
the manifest (`bench_gen.rs:257-261`), so drift there requires stale
generation rather than a missed hand edit; the index shape is still strictly
better because a stale index table is refused by the abi version fold while
stale values are silently wrong. The supersession is normal design-tier
business; it just has to be *said* in change 2's CL, because deleting a
just-landed macro without naming the probe that killed it will read as
churn.

**4b.** The validation proposal's §1b, that `validate()` destroys per-arm
identity before the driver ever gets to discard it, is the sharpest single
finding in the round and I verified every line of it
(`validation.rs:439-440,459-468,501-520`). Its consequence, that the return
contract changes shape before the driver's `Err` arm is worth fixing, is the
correct dependency order for change 1.

**4c.** The record proposal's §1a (`DataSetMeta` permanently default, the
Methodology section never once rendered, `report.rs:116`'s condition always
false, and the stdout summary disagreeing with the findings report about the
seed, `report.rs:37` vs `driver/mod.rs:506`) verified in full. This is the
best available argument for the projection rule and it was found by the
expert whose topic the rule already belonged to, which is the round working
as intended.

**4d.** The record proposal's TOML/u64 probe re-ran clean with its negative
controls on the pinned toolchain. The `hex_u64` pattern matches the crate's
own precedent (`config.rs:90-96`). The `.meta.json` → `.meta.toml` extension
decision is safe: zero readers confirmed.

**4e.** The record's internal citation defect: §1c claims "Section 6 fixes
this" for the `--report-only` blank-environment bug
(`202608151419...:127`); section 6 is about failed-cell records and never
mentions report-only. The actual half-fix is §8's last two sentences, and
even there `report_from_csv_for_routine` gaining a record-reading path is
asserted, not designed (`lib.rs:222-238` builds `EnvMeta::default()` and
nothing in the file says what replaces that line). Small, real, worth one
line in the doc CL: the report-only path reads the record beside the CSV it
already reads, and refuses (rather than defaults) when the record is absent,
because rendering a report with a silently blank environment is the exact
field-that-lies class this round closes.

**4f.** The sweep proposal's arm-set frequency correction (25 of 49 arvo
sweeps sit in families with non-uniform arm sets, against the redesign's
"rare") reproduced from the committed probe output
(`g_arm_sets_per_family.out`: families 3/1/3/1/3/3/3 distinct arm sets, 25
total). The per-sweep `arms` override is a normal line, not an escape hatch;
the schema in change 2 should present it as such.

## 5. Is the whole implementable as one change? No, and here is the dependency structure

Something none of the three names, because each is individually coherent:
the composition has a strict internal order and one external dependency.

**External: PR #21 lands first** (section 1). Every alternative is worse:
holding it stales nine commits of mechanism the round's direction endorses;
folding it into the round makes one unreviewable mega-change; nuking it
rebuilds identity, hooks, generation and withholding from prose that was
written by looking at it.

**Then change 1, one PR, indivisible:** the `validate()` return-shape change
carrying per-arm structural failures and per-pair mismatches; the driver
consuming it (structural drop unconditional, mismatch tainting
unconditional); the reps-1 validation-mode forcing in `bench-core` (3d);
`CellRecord` with the PR-#21-shaped identity (section 1), written in the
post-cell block, including for pre-measurement failures with the record
expert's `output_paths` reordering; the digest post-cell comparison (3c); the
`resolve_routine` policy cross-check with the effective-policy-and-source
field (3e); the projection policy (3b) with `schema_v2`'s three appended
columns and the history-loader fixture test the record expert demands before
trusting the untested read path; the profile handoff file (3f) and the
deletion of `env_meta_to_json`'s hardcode; the `config.rs:120-122` doc-comment
fix; and the deletion of the tautology at `bench-core/src/lib.rs:637-651`,
which nothing else in flight performs. These are one change because every
split leaves a lie standing: semantics without storage is stderr again,
storage without semantics is an empty field, tainting without reps-1 is a
flake generator, a record without the projection policy launders the ledger.

**Then change 2, separable, framework-side sweep support:** the per-file
form's `[axis.*]`/`[sweep.*]` schema with `points`/`hold` (refusing an
undeclared held axis, resolving the sweep expert's §9 lean, which I endorse:
silence about an axis is how a comparison crosses two axes unnoticed); the
point rendering rule (3a); the index-over-FFI dispatch with the abi-hash
version fold, which are one atom (the sweep expert's stale-dylib probe shows
the split state is silently wrong for four of sixteen vehje indices);
`MatrixDecl`/`bench_matrix!`/`render_bench_section` gaining the four policy
keys (both layers, per the validation §1c two-gap finding); the cell→arm
rename in `bench-matrix`; and the `routine_table!` supersession (4a).
Change 2 leaves no trap if it never lands: change 1's record holds
`axes = {"n": ...}`, the filenames stay `_n<value>` as the degenerate
rendering, and entgrid's nine pseudo-arms get honestly tainted, with the
one-line consumer remedy (`may_differ = true`) available until the sweep
migration dissolves the category error properly.

**Then the consumer migrations**, per repo, on their own rounds: arvo's
packed keys to axes, vehje's forked coldcycle family to a regime key, the 27
`bench_matrix!` sites. Not this repository's change at all, and the abi
version fold is what makes the phase boundary safe.

## 6. Is it too much? What I would cut

**Cut from the round entirely (no successor owed):**

- The validation §7 manifest-layer hard refusal (explicit `may_differ` on
  every multi-arm bench). With tainting and the record, silence is no longer
  silent, which was the refusal's whole justification, and its interaction
  with the dead-key path is incoherent (section 2). The cross-check (3e)
  keeps the honest kernel.

**Cut from the round, named as deferred tasks (both experts already agree):**

- Digest computation extended to hand-written `timed!` arms
  (`202608151356...:702-709`). The post-cell comparison (3c) covers the
  populated slice honestly scoped.
- The `Routine`-contract explicit-declaration change (`...:711-715`).
- Wiring `Cache`'s skip-rerun system into the driver
  (`202608151419...:165-171`).
- `bootstrap_iterations` end-to-end wiring (the record carries the resolved
  value; honouring it is the pre-existing task the config comment names).

**Not cut despite temptation:** failed-cell records (topic-ratified, and the
record expert priced the reordering honestly); the four generator policy
keys (they look like change-2 sugar but are the only path by which 135 of
180 vehje sections ever get a validation policy at all, per topic 1338).

## 7. What I would not change

The three proposals' shared refusals, all verified and all correct: the
measurement core entire (scaffold seed table, S/I split, anti-hoist,
calibration floor, fat-LTO profile); cdylib-per-arm isolation and the
subprocess validation workers; `#[bench_variant]`'s one-const-parameter rule
(the struct-typed parameter answer is right and probe-backed); the samples
CSV's seventeen columns; the committed artifact trail; `bench-matrix`'s
three-layer split; the staging/promotion transaction; the warn-only disasm
precedent, which the validation expert correctly refuses to generalise to
correctness mismatches.

And the round's own working method: the probe discipline in these three
files (negative controls, committed outputs, pinned-toolchain reruns) is the
best this arc has produced, which is exactly why the two unchecked PR #21
claims stand out. The instruments were fine; they were pointed at one branch
too few.

## 8. What I could not settle

- **Whether PR #21 is mergeable as-is.** I audited it as a constraint
  surface, not as a review: its 175-line e2e test and the hook redesign got
  a read for shape, not a pass for correctness. It needs its own reviewer
  before the "land it first" recommendation executes; my recommendation is
  about ordering, not about its quality.
- **The report-table rendering of a tuple point** (the sweep expert's open
  item). The filename rule (3a) gives the history key and the artifact name;
  what a report column shows for a multi-axis sweep's row grouping is a
  presentation choice I did not design.
- **Whether the swept-axes-only filename (3a) should also omit single-valued
  swept axes.** I chose "swept axes appear, held axes do not" for
  determinism; a sweep with one point per axis renders long. Rare, harmless,
  unresolved.
- **The `ValidationOutcome` concrete type.** The validation file's §8
  content list plus the two additions from 3c (digest coverage) and 3e
  (policy source) is complete as content; the enum-versus-strings question
  the record expert deferred for drop reasons applies here too and belongs
  to change 1's src CL, written against the post-PR-#21 tree.
