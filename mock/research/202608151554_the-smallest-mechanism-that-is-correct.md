# The smallest mechanism that is correct: what the bench framework carries by convention

**Date:** 2026-08-15
**Phase:** research, cold derivation, phase one committed blind
**Branch:** `docs/the-smallest-mechanism-that-is-correct` off `feat/bench-round-consolidation`
**Probes:** `mock/research/202608151554_probes/` (seven, each with its negative controls and committed output)
**Read:** `feat/bench-consolidation` (PR #21) as the tree that lands first, per the round's own changelist

## Gates

**Canon gate: passed.** mockspace has no `mock/canon/`; this is the part-written situation. I checked
the assigned work against the fixed set at `mock/design_rounds/202608151545_changelist.doc.md` under
"Settled by op, not reopened here", and against the three topic files. Nothing below reopens a settled
item. Two proposals sit adjacent to settled ones and are written to preserve them: F6 keeps both role
spellings, because "explicit `[bench.<name>]` keying stays supported indefinitely; nothing is
deprecated"; F3 is addressed to change 1 rather than in place of it.

**Test gate: passed, and the suite is better than the brief's numbers suggest.**
`cargo test --workspace` on `feat/bench-consolidation`: **606 passed, 0 failed, 20 ignored, exit 0**,
657 test lines emitted. I read the bodies of every test in the four files I touch (`config.rs`,
`analysis.rs`, `report.rs`, `spec.rs`). None is tautological, none is a smoke test, and two are the
opposite of sampled:

- `config.rs:1297 every_field_of_every_denied_struct_still_parses` asserts every key of every
  `deny_unknown_fields` struct, and its doc comment states why a sampled version would be useless.
- `bench-core/src/lib.rs:628 ffi_bench_call_is_four_u64` ties a declaration to a real layout
  (`size_of == 32`, `align_of == 8`) rather than to another declaration.
- `report.rs:848 floor_differences_the_ratio_against_the_named_cell` carries its own negative control
  inline: the same data without the floor must give the raw `0.50x`.

Of the 20 ignored, 5 are catalogue-reds with a stated gap and a tracked id, 2 need sudo or a nested
cargo build, and 13 are `ignore`d doc examples. That last group is a finding in its own right and is
**F9** below.

**One thing the gate does not reach, and it is the shape of everything below.** The config suite has
four tests asserting that a typoed *key* is refused (`:1220`, `:1244`, `:1255`, `:1266`) and **zero**
asserting anything about a typoed *value*. That is not a weak suite. It is a suite faithfully covering
a guarantee whose perimeter stops exactly one level short of where the values live.

## Corrections to the brief, made before relying on it

Three of the brief's measured facts do not hold on the tree that lands first. Each took one command.

**1. The panic surface is a tenth of the stated size.** The brief gives "37 `unwrap()`, 16 `expect(`,
3 `panic!`" excluding files named `tests.rs`. Excluding inline `#[cfg(test)]` modules as well, across
all four crates on `feat/bench-consolidation`:

| pattern | all occurrences | outside `#[cfg(test)]` |
|---|---|---|
| `unwrap()` | 89 | **7** |
| `.expect(` | 20 | **0** |
| `panic!` | 9 | **3** |

Ten panic sites in shipped paths across 13,600 lines. **The panic surface is not a problem and I am
not proposing anything about it.** The one panic that matters is not in that count at all, because it
is a bounds index rather than a `panic!` (F2).

**2. The `println!` count is inflated about tenfold by the grep itself.** `grep 'println!'` matches
`eprintln!`. Corrected, outside test code: **101 `eprintln!` and 11 `println!`**, not "96 and 107".
Worth stating because the same grep will be re-run by whoever reads the round.

**3. The file sizes in the brief are pre-PR-21.** On the tree that lands first, `config.rs` is 1322
(not 935), `driver/mod.rs` is 973 (not 603), and `tree.rs` is 1049 and new. Nine files over 500 becomes
**ten**, and the two largest grew by 40% and 60%. I am not proposing a split pass; `file-size-limit.md`
calls 500 a smell and the round has larger problems. Recorded so the number is right.

**4. One item of change 1 is already performed by PR #21, which the same changelist says lands first.**
The changelist reads: "the deletion of the tautology at `bench-core/src/lib.rs:637`, **which nothing
else in flight performs**". PR #21's commit `ca58832` is titled "fix: delete the abi hash tautology,
record the profile actually passed", and the tautology is gone on that branch, replaced by a comment
explaining that a test recomputing `abi_hash`'s own fold "could only fail if the two copies of the
constants drifted, which certifies the copying, not the hash." The same commit also performs change 1's
"profile handoff and the deletion of the hardcoded literal": `harness.rs:44-70` reads the profile from
`MOCKSPACE_BENCH_PROFILE` and **omits the field entirely** when the tool did not drive the build, on
the stated reasoning that "no claim beats a wrong one".

This is not fatal, and after PR #21 merges the source changelist simply must not re-list them. It is
worth saying plainly because it is the changelist's own diagnosed failure ("believing by reading rather
than running") recurring one level up, inside the document that diagnoses it, about the very PR it
names.

## What I am carrying forward unchanged, and from whom

**Count: eight.** Each was examined and each survives. Keeping them is the result. The eighth is
`glob_match`'s own test suite and is stated with F12, where the examination that kept it lives.

1. **The four-crate split, from whoever drew it.** I set out to challenge it and it holds, on a
   checkable ground rather than a stylistic one: a variant cdylib links `bench-core` + `bench-macro`
   and **nothing else** (`arvo/mock/benches/variants/warm-clamp-accfit-dyn/Cargo.toml:14-16`;
   `vehje/mock/benches/variants/carrier_dispatch_real_nullfloor/Cargo.toml:11-14`). Across the four
   consumers that is **1088 arm crates** each rebuilding the ~1100-line ABI surface instead of the
   12,781-line orchestrator. `bench-macro` is forced separate by rustc. `bench-matrix` gates its
   `bench-harness` dependency behind a default-on `generate` feature
   (`bench-matrix/src/lib.rs:47-51`), so a consumer that only authors cells drops the whole tree. The
   boundaries buy compile time proportional to the arm count, which is the largest number in the
   system.

2. **`bench-matrix` layering on `bench-harness::matrix`, not duplicating it.** I expected two copies of
   one generator and found one. `bench-matrix/src/generate.rs:11` imports
   `mockspace_bench_harness::matrix::{generate, AxisSpec, AxisValue, MatrixSpec}` and maps `MatrixDecl`
   onto it. The three layers are stated at `bench-matrix/src/lib.rs:13-23` and the code matches the
   statement. The `bench-harness::matrix` / `bench-matrix` **name** collision is real and is the
   round's vocabulary work; I am not reopening it.

3. **`DriverRegistry` beside `DriverSpec`, from PR #21.** It looks like two registration surfaces and
   is a ten-line adapter honestly labelled "the hook-less compat surface"
   (`driver/mod.rs:73-74`, `:263-272`). Correct as it stands.

4. **PR #21's `--config profile.release.*` mechanism, derived independently.** See the next section: I
   found the defect it fixes before reading it, and its fix is stronger than its own doc claims.

5. **PR #21's `[workload.*]` declarative stage list**, which removes the `build_workload` duplication I
   was going to propose as an addition. `hilavitkutin/mock/benches/src/main.rs` and
   `vehje/mock/benches/src/main.rs` carry byte-identical six-stage `realistic` programs;
   `src/bench_gen.rs:151-166` now ships that program as a builtin. The consumer migration is deferred
   by the changelist and that is right.

6. **The `deny_unknown_fields` discipline and the reasoning at `config.rs:531-539`.** "A measurement
   tool that ignores a key its author wrote is the worst shape available: the run succeeds, the report
   looks ordinary, and the setting was never applied." That paragraph is correct and is the argument
   for every finding below; my whole contribution is that it stops at keys.

7. **The `de_sizes` hand-written deserializer** (`config.rs:388-442`) and its 8-line comment explaining
   why `#[serde(untagged)]` was rejected. That is a real diagnostic-quality decision, correctly
   reasoned, correctly recorded, and expensive to rediscover.

---

## The thread I entered first, and where it ended: a concession

I found, by grep across the four consumer trees, that **97 of 1088 arm crates declare no
`[profile.release]`** at all: all 94 of arvo's and three of hilavitkutin's (`ema_autovec`, `ema_neon`,
`ema_scalar`). `arvo/mock/Cargo.toml:32` excludes `benches`, so those arms are their own free-standing
packages taking cargo's defaults (`lto = false`, `codegen-units = 16`) while the framework's central
claim is fat LTO and one codegen unit. The remaining 991 declare a byte-identical block in two
whitespace spellings, 947 spaced and 54 unspaced, which is two generators emitting one literal.

**PR #21 already found this and fixed it**, and documents the measurement in the same words
(`src/bench.rs:300-316`, "a tree of ninety-four variant crates, none declaring a profile and none
declaring a workspace"). Its fix passes the profile on the cargo command line
(`src/bench.rs:317-324`, `:477-491`).

I did not stop at reporting that. The fix rests on an unstated precedence property, and 991 arm crates
declare a competing profile in their own manifests, so I checked which wins.
`mock/research/202608151554_probes/profile-precedence/`, on `cargo 1.98.0-nightly (fbb61be30)`:

```
C1  thin-declaring arm, no --config   : codegen-units=16 lto=thin opt-level=1
C2  nothing-declaring arm, --config   : codegen-units=1 lto=fat opt-level=3
Q   thin-declaring arm, --config      : codegen-units=1 lto=fat opt-level=3
```

Both controls pass, so: **`--config` wins over an arm's own `[profile.release]` table.** PR #21's fix
is complete for the tool path and stronger than its doc comment claims, which says only that a manifest
cannot silently *drop* the settings; it also cannot silently *weaken* them. That property is what the
991 declaring arms depend on and it was nowhere written down.

**Residue, and it is small: the hand path.** `cargo build --release` inside a variant directory, which
is what a person iterating does, still gets the manifest's own profile, so arvo's 94 hand-built arms are
`lto=false, codegen-units=16`. But the tool rebuilds every arm on every run
(`src/bench.rs:614`, `:799`) and a different `--config` is a different fingerprint, so no stale
artifact survives into a tool-driven measurement. The hand path produces no artifacts.

**This hole is exhausted and the honest result is a concession: PR #21 closed it.** What I add is the
committed precedence probe and the 97/991 census, neither of which existed.

---

## F1. The three declared analysis roles are free strings, and a wrong one is silent

**The finding.** `baseline`, `floor` and `delta`/`mode` are declared in `bench.toml`
(`config.rs:262-272` flattened, `:310-326` as the `[normalise]` table) and reach the report as
`Option<String>` / `String`. Every one of them is applied by a lookup that does nothing when it misses:

- `analysis.rs:276-281`: `with_baseline` is `if let Some(idx) = ...position(|v| v.name == name) { ... }`
  with no `else`.
- `analysis.rs:291-296`: `with_floor`, whose doc says outright "A name not among the variants is ignored
  by the reporter".
- `report.rs:196`: `let ratio_mode = ds.meta.normalise_mode == "ratio";` is the **only** read of
  `normalise_mode` in the rendering path. Of the four documented values, three are indistinguishable
  from each other and from any typo.

**The probe.** `mock/research/202608151554_probes/normalise-silence/`. Three arms at 100/200/400ns.
Controls: a real baseline, a real floor and `mode = "ratio"` must each change the rendered report.
All three pass. Then:

```
F1 baseline="charley" (typo) == no baseline at all: true
F2 floor="alfa" (typo) == no floor at all:           true
F3 mode="percent" == mode="subtract":              true
F3 mode="none"    == mode="subtract":              true
F3 mode="percnt"  == mode="subtract":              true
F3 mode=""        == mode="subtract":              true
F3 mode="RATIO"   == mode="subtract":              true
F3 mode="banana"  == mode="subtract":              true
```

**The cost, as a quantity.** One letter wrong in a baseline name, same data, same run:

```
-- declared baseline `charlie` --      -- typoed baseline `charley` --
| alpha   | 101ns | -74.81% |          | alpha   | 101ns | base    |
| charlie | 401ns | base    |          | charlie | 401ns | +297.03% |
```

Sign, magnitude and which arm is the reference all flip, and the two reports are distinguishable only
by reading the manifest that produced them. This is `mock/design_rounds/202608151338`'s stated property
("a wrong answer survives the pipeline and is indistinguishable in the artifacts from a right one") on
the **analysis** path, which that topic does not cover because it is scoped to cross-arm output
validation.

**What to build.** The mode is a closed set of four; make it one:

```
Delta ::= Subtract | Ratio | Percent | None      -- deserialised, unknown value refused by name
Role  ::= NoBaseline | Baseline(arm) | Baseline(arm) with Floor(arm)
```

Two properties matter and neither is about spelling. First, `Delta` as a deserialised enum makes
`delta = "percnt"` fail at manifest load with the same diagnostic quality
`deny_unknown_fields` already gives a typoed key, which is the guarantee's perimeter moved to where the
values are. Second, `Role` as a sum type dissolves the `floor`-requires-`baseline` invariant that
`validate_roles` (`config.rs:564-584`) currently enforces at manifest load **only**: `BenchConfig` is
documented as "Construct manually for ad-hoc runs" (`config.rs:701-702`) and every field is `pub`, so a
hand-built config with `normalise_floor: Some(x)` and `normalise_baseline: None` is constructible today
and refused by nobody. That is the perimeter rule in
`what-you-can-observe-is-what-you-guaranteed.md`: the invariant is established in one constructor and
the type has another door.

**And an arm name that is not an arm must be refused, not ignored.** That is separate from the type
work and is the larger half of the cost: `with_baseline`/`with_floor` become fallible, or the resolution
moves to where the arm list is known and returns a `BenchError::InvalidConfig` naming the declared name
and listing the arms that exist. The framework already writes exactly that error shape at
`config.rs:599-606`.

**Second reading, stated because I do not think it wins.** The role names could stay strings and gain a
validation pass in `validate_roles` that cross-checks them against the arm list. Cheaper, no type
change, no consumer impact. It loses because `validate_roles` runs on the manifest, and the manifest is
not where the arm list is: arms are resolved to paths at `for_size` and to *names* only at dlopen
(`harness.rs:138-140`, "the name the variant's cdylib exports through its `bench_name` symbol, not
anything derived from its path"). So a manifest-time cross-check cannot see the names it must check
against, and would compare declared roles to *path stems*, which is a fourth spelling of the same
identity. **What would close this option:** find a point before measurement where the resolved arm names
exist. The driver's preflight resolves every dylib; if it also read `bench_name`, both readings become
available and the string one becomes viable.

## F2. `Sample::mode` is a two-element closed set carried as a `String`, and selecting the wrong one is a bounds panic

**The finding.** `Sample::mode` (`sample.rs:31`) is compared against string literals at eleven sites
(`harness.rs:272,332,376,452,470,572,626`; `driver/mod.rs:241,590,646`). Twelve functions take
`mode: &str`, six of them public entry points (`lib.rs:166,179,209,229`; `analysis.rs:165,675,687`;
`cache.rs:296,381`). `DataSet::from_samples` filters by that string; `report::generate` opens with
`ds.baseline()` at `report.rs:17`, which is `&self.variants[self.baseline_idx]`
(`analysis.rs:312-314`) with no emptiness guard.

**The field's own doc comment names two values the code never writes.** `sample.rs:28-29` reads
`Mode label ("normal", "batched", etc.). Reserved for per-mode aggregation in Round 5.` The values are
`"warm"` and `"cold"`.

**The probe.** `mock/research/202608151554_probes/mode-is-a-string/`. Controls: the correct mode renders
both arms and does not panic; both pass. Then every other value, including both the doc comment names:

```
F  mode="wamr"    -> PANIC: index out of bounds: the len is 0 but the index is 0
F  mode="Warm"    -> PANIC: index out of bounds: the len is 0 but the index is 0
F  mode="cold"    -> PANIC: index out of bounds: the len is 0 but the index is 0
F  mode="normal"  -> PANIC: index out of bounds: the len is 0 but the index is 0
F  mode="batched" -> PANIC: index out of bounds: the len is 0 but the index is 0
F  mode=""        -> PANIC: index out of bounds: the len is 0 but the index is 0
```

`lib.rs:146-148` documents the usage that triggers it: "Mockspace consumers typically call this twice
(once per mode) and emit `findings_warm.md` + `findings_cold.md`". A run that produced no cold samples
terminates the process with a bounds index carrying no bench name, no mode, no path and no suggestion.
The driver itself never hits it (`driver/mod.rs:590` hardcodes `"warm"`); the library path a consumer
follows does.

**What to build.** `enum Mode { Warm, Cold }` with `#[serde(rename_all = "lowercase")]`, so the CSV
bytes are unchanged and v1 polka-dots caches still read. Eleven comparison sites become matches, twelve
signatures take `Mode`, and `dataset("wamr")` stops compiling. The rewrite is mechanical and the
serialised form does not move, which is the whole reason this one is cheap.

**Independently of the enum, `baseline()` must not index blind.** `report::generate` over an empty
`DataSet` has no correct output; the honest signature returns a `Result` or the caller is refused
earlier. A closed `Mode` removes the typo route into that state and does not remove the state: a bench
whose cold cohort genuinely produced nothing reaches it too.

**Second reading.** Keep `&str` and have `from_samples` refuse an empty result. That fixes the panic and
not the typo, and it leaves twelve `&str` parameters whose domain is documented wrongly at
`sample.rs:28`. **What would close it:** if any consumer or artifact uses a third mode value, the closed
set is wrong and the enum is premature. I grepped the four consumer trees and the committed CSVs and
found only `warm` and `cold`; I did not vary across mockspace's own git history, so my claim is about
the current tree.

## F3. The worker wire is a positional TSV parsed with `unwrap_or`, and a short line is a zero-nanosecond measurement

**This is the finding I would act on first, and it interacts directly with change 1.**

The worker-to-orchestrator wire is a 12-or-13-column tab-separated line, written by two `println!`
format strings at `harness.rs:490-499` and parsed positionally at `harness.rs:720-741`. Both ends are
in the same file, 230 lines apart, matched by hand-counted index. The comment above each end says the
same thing in prose ("The parser reads them positionally", "instructions/cycles at fixed 7,8; matrix
setup_ns/first_ns/digest at 9,10,11; optional score at 12") because nothing else makes them agree.

Two properties of the reader, and they compound:

**The guard is `if parts.len() >= 9`** while the writer emits 12 or 13. Three always-present columns
(`setup_ns`, `first_ns`, `digest`) and the optional `score` sit outside it.

**Every field is `.parse().unwrap_or(<default>)`.** A malformed or short line is not dropped. It becomes
a `Sample` with `e2e_ns = 0.0`, `algo_ns = 0.0`, `batch_count = 0`, `digest = 0`.

**A zero timing is not a missing sample. It is a fast sample.** Nothing downstream guards it: the only
zero checks in `analysis.rs` are divide-by-zero guards at `:351`, `:397`.

**The probe.** `mock/research/202608151554_probes/zero-sample-survives/`. Controls: a clean 20-sample
arm reads a mean of exactly 100ns, and dropping three of the twenty does not move it. Both pass. Then
the same three arriving as short lines:

```
F1 3 of 20 samples arriving as short lines:
   reported mean 85.00ns against the true 100.00ns  (-15.0%)
   the run reports 20 samples either way
F2 mean 85.00  best20% 25.00  worst20% 100.00  count 20
```

An arm reports **15% faster than it is**, with the sample count intact and a `best 20%` of 25ns that is
physically impossible for a 100ns routine, and nothing anywhere names it.

**The interaction with change 1, which is why this is urgent rather than merely ugly.** The changelist
puts in change 1 "the digest's post-cell comparison, **which makes it load-bearing for the first
time**". `digest` is column 11, outside the `>= 9` guard, defaulting to `0`. Two arms whose lines are
short both report `digest = 0`, compare equal, and the comparison change 1 is adding certifies agreement
between two arms **that never reported one**. The check that exists to catch a wrong answer would, on
that input, manufacture a right one.

**What to build, and it is small.** One type, one parse, at one place:

```
WorkerLine ::= Batch { arm, mode, batch_idx, e2e, algo, bridge, count,
                       instructions, cycles, setup, first, digest, score? }
             | Timeout { arm, mode, value }
```

with a `Display` and a `FromStr` that is the only reader. Three properties follow that the current shape
cannot have. The column order exists once instead of twice. A line that does not parse is a
`BenchError` naming the arm and the offending column, never a sample. And the wire format becomes
testable at all, which it is not today: the parser is an inline loop inside `run_orchestrator` with no
function boundary, so **no test anywhere names the format**, and the round is about to make one of its
columns decide a verdict.

**Do not reach for serde here.** The format is line-per-batch on a pipe, hot, and the current
hand-format is the right performance shape; what is missing is that the writer and reader are not one
definition. A `Display`/`FromStr` pair on one struct keeps the bytes identical and costs a rename.

**Second reading.** Change only the guard: `>= 12` instead of `>= 9`, and drop rather than default a
line that fails. Two lines, no type, closes the digest hazard before change 1. **This is the right
emergency fix and the wrong resting state**, because it leaves the column order duplicated and the
parser untestable. **What would decide between them:** whether change 1 lands before or after this
round's source changelists. If change 1 is imminent, take the guard now and the type after.

## F4. Seven public items with no caller anywhere, and one of them is the type the design already needed

I enumerated the 95 symbols `bench-harness/src/lib.rs` re-exports and counted references across the
mockspace repository and the four consumer trees. **31 appear in exactly one file, their own
definition, and in zero consumer files. Another 15 appear in exactly two.**

The count is a `\b` word grep, so it **over**-counts: generic names like `setup`, `read`, `load`,
`validate` are contaminated and I make no claim about them. A symbol at count 1 with a distinctive name
cannot be a false negative, and six of those are hard:

| symbol | site | references anywhere |
|---|---|---|
| `apply_drift` | `cache.rs:328` | declaration + the `pub use` line |
| `config_hash` | `cache.rs:62` | declaration + the `pub use` line |
| `domain_work` | `workload.rs:283` | declaration + the `pub use` line |
| `global_mean` | `cache.rs:394` | declaration + the `pub use` line |
| `global_mean_for_mode` | `cache.rs:381` | declaration + the `pub use` line |
| `VariantSpec` | `spec.rs:45` | declaration + the `pub use` line + its own module doc |

Plus `BenchError::NotImplemented` (`error.rs:19`), which the brief already names: declaration and
`Display` arm, nothing else. Its doc comment describes a porting process that finished
("Round 1 returns this from the entry point; subsequent rounds remove individual call sites"). Delete it.

**Two readings, and they split the list.**

*Reading A, dead scaffolding: delete.* `apply_drift`, `config_hash`, `global_mean`,
`global_mean_for_mode` and `NotImplemented` are public API the framework may not break, that nothing
calls, that a reader of the docs must skim past. Cost of deletion: nothing, since nothing calls them.

*Reading B, unreachable mechanism: expose it.* `domain_work` is the sharp case and it belongs in B, for
a reason F7 makes exact: it is a workload stage constructor that **cannot be named from the declarative
surface**, so its deadness is a consequence of a grammar gap rather than of nobody wanting it.

**`VariantSpec` is neither, and it is the real finding here.** It is the resolved triple
`(name, dylib_path, abi_hash)`. `spec.rs:8` names it as one of the two things the module exists for.
Nothing constructs it. What the harness does instead: `BenchConfig.variant_paths: Vec<PathBuf>`
(`config.rs:730`) carries paths only, and `load_variant` (`harness.rs:118-147`) dlopens, reads
`bench_abi_hash`, compares it, reads `bench_name`, and **returns `(String, BenchEntryFn)`, a bare
tuple, with the abi hash discarded after the comparison.**

So the type that makes the three facts one object exists, is documented as the mechanism, and is
bypassed by a tuple and a `Vec<PathBuf>` at the exact point where all three are in hand. The record
topic lists "per-arm dylib hashes and dep pins" among the facts nothing carries; the **ABI** hash is a
fourth such fact, read and thrown away twenty lines from where the record will want it.

**What to build.** `load_variant` returns `VariantSpec` plus the entry pointer. That is a signature
change to one private function, and it puts the abi hash where the `CellRecord` can project it without a
second dlopen. Small, and it deletes a documented lie rather than a symbol.

## F5. `TimingOverride` is `TimingSection` with every field wrapped in `Option`

`TimingSection` (`config.rs:479-497`) has five fields. `TimingOverride` (`config.rs:345-351`) has the
same five, each `Option`. `for_size` merges them by hand, one `ov.and_then(...).unwrap_or(...)` arm per
field (`config.rs:663-675`). A sixth timing knob is **four** edits: the field, the optional field, the
merge arm, and the `BenchConfig` field.

Nothing checks that the two lists agree. `every_field_of_every_denied_struct_still_parses` asserts both
parse, not that they have the same fields, so a knob added to one and forgotten in the other is a knob
that silently cannot be overridden per bench, which is the `deny_unknown_fields` failure mode inside the
schema rather than at its edge.

**What to build.** One definition with the optionality derived. `TimingOverride` becomes a generated or
macro-derived partial of `TimingSection`, with the merge derived alongside, so the five-way list exists
once. In this workspace's idiom that is a small declarative macro over the field list, not a dependency.

**Second reading: leave it.** Five fields, stable for the life of the crate, and a derive macro is a
mechanism to maintain. **What would close it:** whether the round's sweep work adds timing knobs. The
`202608151339` topic says a sweep "could move the seed table, the anti-hoist, the S/I split, the digest,
the calibration floor or the build profile" and rules those out for comparability. If the list stays at
five, leaving it is correct and I would leave it in isolation.

### But it does not stay in isolation: PR #21 makes it three copies of twelve fields

I first wrote this section against `config.rs` alone and left `tree.rs` unread as a concession. Reading
it changes the size of the finding by a factor of six. **Three structs now carry one bench vocabulary:**

| | `BenchSection` `config.rs:203` | `ComposedBench` `tree.rs:102` | `SweepSection` `tree.rs:138` |
|---|---|---|---|
| `title` | `String` | `String` | `Option<String>` |
| `workload` | `String` | `String`, defaulted | `Option<String>` |
| `master_seed` | `u64` | `Option<u64>` | `Option<u64>` |
| `arms` / `variants` | `Vec<String>` | `Vec<String>` | `Vec<String>` |
| `points` / `sizes` | `Vec<SizeSection>` | `Vec<SizeSection>` | `Vec<SizeSection>` |
| `may_differ` | `bool` | `bool` | `Option<bool>` |
| `required` | `bool` | `bool` | `Option<bool>` |
| `threaded` | `bool` | `bool` | `Option<bool>` |
| `baseline` / `floor` / `delta` | `Option<String>` x3 | `Option<String>` x3 | `Option<String>` x3 |
| `timing` | `Option<TimingOverride>` | `Option<TimingOverride>` | `Option<TimingOverride>` |
| `normalise` | `Option<NormaliseSection>` | **absent** | **absent** |

Twelve fields written three times, with `TimingSection`/`TimingOverride` making five of them a fourth
and fifth time, and the merges hand-written at `config.rs:663-675`, `tree.rs:401-527` (127 lines) and
`tree.rs:552`.

**The copies have already disagreed, and the probe shows where.**
`mock/research/202608151554_probes/per-file-form-asymmetry/`, on `feat/bench-consolidation`, with both
controls passing (the flattened roles load per-file; the `[normalise]` table loads in a root section):

```
F1 per-file + [normalise] table        REFUSED  unknown field `normalise`
F2 root section + flattened roles      LOADS
F3 [sweep.a] + flattened roles         LOADS
F4 [sweep.a] + [normalise] table       REFUSED  unknown field `normalise`
```

So `[normalise]` is writable in the `[bench.<name>]` form and refused in the per-file form and in a
sweep. **That may well be the right call**, since the flattened keys are the canonical spelling and not
carrying a legacy table into a new form is defensible. It is stated in no document I found, and a
consumer moving a section into a per-file bench meets it as a parse error rather than as a decision.

**The diagnostic is good and I am not criticising it.** It names the field and lists the accepted ones.
What it also does is print the duplication: the two accepted-field lists carry the same names **in
different orders**, because they are two separately hand-written struct declarations.

```
expected one of `title`, `workload`, `master_seed`, `arms`, `variants`, `points`, `sizes`, ...
expected one of `points`, `sizes`, `arms`, `variants`, `title`, `workload`, `master_seed`, ...
```

**This raises F5 from a maybe to a yes.** One field list, with the required, the optional and the merge
derived from it, and `normalise`'s presence or absence per form becomes a declared fact rather than an
emergent one. **What would close it the other way:** if the three forms are meant to diverge in more
than `normalise`, a derived partial is the wrong shape and the divergences should be enumerated
explicitly instead. Nothing I read says they are meant to diverge at all.

## F6. The two role spellings are two field sets, and only one of them needs to be

`BenchSection` carries `baseline`, `floor`, `delta` (`config.rs:262-272`) **and** an optional
`normalise: NormaliseSection` with `baseline`, `mode`, `floor` (`config.rs:310-326`). The same three
facts in two shapes, with different names for one of them (`delta` against `mode`). Reconciling them
costs `validate_roles` (21 lines, `config.rs:564-584`) plus a merge (15 lines, `config.rs:633-647`),
and the merge's own comment has to explain why there is no precedence question.

**Both surfaces stay.** "Explicit `[bench.<name>]` keying stays supported indefinitely; nothing is
deprecated" is settled, and I read the same intent as covering this. What need not stay is **two field
sets in the parsed type**: both spellings deserialise into one `Role` value (F1's sum type), and the
"declared twice" refusal becomes a property of that one deserialiser rather than a separate validation
pass over a struct that permits the contradictory state in the first place.

That is the general shape of every finding in this file. The contradictory state currently exists, is
constructed, and is then refused. Making it inexpressible deletes the refusal along with the state, and
36 lines with it.

## F7. The workload stage vocabulary is three lists in two crates

PR #21's `[workload.*]` (F-carried-forward 5) is the right mechanism. Its implementation keeps the
vocabulary in three places:

- `bench-harness/src/workload.rs:257-286`: **seven** `pub fn` stage constructors.
- `src/bench_gen.rs:124-125`: **six** name literals with their arities, in a `match` on `&str`.
- `src/bench_gen.rs:126-131`: the same six names again, in the error message that lists the builtins.

The generator emits `format!("harness::{name}({v})")` (`bench_gen.rs:135,142`), a string naming a
function it never resolves. Rename `scalar_work` and `parse_stage` still accepts `"scalar_work 48"`,
emitting a generated driver that fails to compile with an error pointing at generated code.

**And the seventh constructor explains F4's `domain_work`.** `domain_work(run_fn: fn(u64, &mut u64))`
takes a function pointer, so it has no form in the `name` / `name N` grammar
(`bench_gen.rs:124-125`). It is not dead because nobody wanted it; it is dead because the declarative
surface cannot name it, and the two lists being separate is why nobody noticed.

**What to build.** One definition of the stage set in `bench-harness::workload`: a closed `Stage` with
`FromStr` for the declarative grammar and a rendering for the generator. Adding a stage becomes one
variant, and the arity table, the builtin list and the error message all derive from it. That is the
"one rule, not a pile" move at its smallest, and it costs less than the three lists do.

**Then decide `domain_work` deliberately.** Either the grammar grows a form that can name a consumer
function path (which the matrix generator already does, via `op_path` in
`bench-matrix/src/generate.rs:71`), or the constructor is deleted. What is not acceptable is leaving a
seventh stage that only a hand-written driver can reach while the tool generates the driver.

## F8. 101 `eprintln!` and 11 `println!`: diagnostics are a channel, not a value

Outside test code, in the framework crates: `driver/mod.rs` 32, `validation.rs` 24, `harness.rs` 18+7,
and 20 more spread across seven files.

**`validation.rs` is the one that matters, because the round is rewriting it.** 24 stderr writes and
zero `println!`. Topic `202608151338` establishes that "Every verdict in the system is printed to
stderr and recorded nowhere", and change 1 gives the verdict a home in `CellRecord`. My addition is
narrow and composes: once `validate()` returns per-arm structural failures and per-pair mismatches as
data, those 24 sites are the **render** of that data and should become one, at the driver, from the
record. If change 1 lands with the structured return *and* the 24 writes still in place, the system has
two independent accounts of the same verdict, which is the duplication rule's exact prediction: they
will disagree on the inputs nobody compared them on.

I am **not** proposing a logging abstraction. The framework is a CLI, stderr is the right destination,
and 101 sites is not itself a defect. The defect is only where the printed thing is also a recorded
thing, which after change 1 is `validation.rs` and the driver's summary.

**What would close this:** read change 1's source changelist when it exists and count how many of
`validation.rs`'s 24 survive. If it is zero, this finding is already handled and should be dropped.

## F9. Every doc example in the framework is `ignore`. There are no compiled examples at all

Eleven doctests across the four crates, **all eleven `ignore`d**: `bench-core` 4
(`byte_routine`, `byte_routine_dispatch`, `timed`, `timed_calibrated`), `bench-harness` 4
(`driver::hooks::Hooks`, the `driver` module, `spec::RoutineSpec`, `spec::routine_table`),
`bench-macro` 2, `bench-matrix` 1 (`bench_matrix`). `ignore` means not compiled.

These are the ergonomics surface. They are the snippets a consumer copies, on the macros
(`timed!`, `byte_routine_dispatch!`, `bench_matrix!`) that are the hardest part of the API to get right,
and **not one of them is known to compile**. The suite reports them in its ignored count, so they read as
covered.

Most are `ignore` for a real reason: they need a consumer's `Routine` type that the framework does not
have. That reason is removable, and removing it is the addition: a `#[doc(hidden)]` example routine in
`bench-core` (the test modules already build several) turns most of the eleven into `no_run`, which
compiles them. The ones that genuinely cannot compile should say why in the fence's neighbourhood, so
`ignore` stops being the default.

**Two of them are wrong today, and the probe says which.**
`mock/research/202608151554_probes/doc-examples-compile/` transcribes six of the examples verbatim
between BEGIN/END markers, each as its own cargo bin, plus the minimum scaffolding the example's own
prose names but does not show. On `feat/bench-consolidation`, **four compile and two do not**:

- **`bench-core/src/byte_routine.rs:12`** documents
  `use mockspace_bench_core::{routine_bridge, ByteRoutine, RoutineSpec};` and `bench-core` **has no
  `RoutineSpec`**. It lives in `bench-harness/src/spec.rs:34`. `error[E0432]: unresolved import`. This
  is the module doc of `ByteRoutine`, the type the framework tells a consumer to reach for first, and
  it is in the one crate all 1088 arm crates link.
- **`bench-core/src/lib.rs:379`**, the trailing comment
  `// dispatch(n, may_differ) -> Option<RoutineBridge>`, describes something callable. The macro yields
  a `ByteDispatch` **struct**. The code line above it compiles (`ex2a`); the comment beside it does not
  (`ex2b`, `error[E0618]: expected function, found ByteDispatch`). Splitting those into two bins is
  what lets the finding be about the comment rather than the example.

**The probe's first run was wrong and the fix is the interesting part.** Its path dependencies are
relative, so it compiled against whichever branch the repository was checked out at. Run on this
round's base and attributed to PR #21's tree, `ex4` reported a failure that is entirely an artifact of
the checkout. The missing control was the tree's own identity, and `run.sh` now prints
`git rev-parse` before anything else and both runs are committed.

That artifact is also the finding's mechanism stated better than I could state it: **`ex4` genuinely
fails on one branch and compiles on the other**, because `Hooks` and `routine_table!` are PR #21
additions. An `ignore`d example can be correct on one branch and wrong on the next with nothing
anywhere to notice, which is what `ignore` buys and what the two live failures are instances of.

**What to build.** A `#[doc(hidden)]` example `Routine` in `bench-core` (its test modules already build
several) turns most of the eleven into `no_run`, which compiles them. Fix the two that are wrong first;
they are a one-line import correction and a one-line comment correction, and neither survives the day
`no_run` lands.

## F10. Four report entry points that are one function and two flags

`lib.rs` exports `write_report`, `write_report_for_routine`, `report_from_csv`,
`report_from_csv_for_routine`: the 2x2 of (source: `BenchResult` or CSV) x (throughput: off or on).

`write_report:167` and `write_report_for_routine:182` differ in exactly one line,
`result.dataset(mode)` against `result.dataset_for_routine(routine, mode)`.
`report_from_csv:212-220` and `report_from_csv_for_routine:233-241` are **nine lines duplicated
verbatim**, the same synthetic `BenchResult` literal, differing only in which of the first two they call.

The 2x2 is why: each axis was added by adding a function. A fifth entry point on either axis makes it
six.

**What to build.** One function taking the routine as `Option<&RoutineSpec>`, or `Into<DataSet>` on the
two sources so the CSV path is a conversion rather than a copy. The four names can stay as
one-line wrappers if consumers use them; three do (`load_samples_csv` appears in three consumer files).
The nine duplicated lines should not.

Low confidence on priority: this is the smallest thing in the file and it is here because it is the
clearest example of the shape. It is worth ten minutes and not worth a round.

## F11. Two public paths to every symbol, and a status doc describing a branch that does not exist

`bench-harness/src/lib.rs` declares 21 `pub mod` and re-exports 95 symbols flat. Every one is reachable
two ways: `mockspace_bench_harness::validate` and `::validation::validate`. Consumers use both
(`kirjo/mock/benches/src/main.rs:9-10` imports from `driver::` and from the crate root in adjacent lines).
Two paths per symbol is two things to keep stable and two things a reader must learn are one thing.

Not urgent, and the fix is a policy rather than a refactor: either the modules are private and the flat
re-export is the API, or the modules are the API and the flat list goes. Deciding is cheap; both are
better than both.

**And the crate doc is stale in a way that misdescribes the project.** `lib.rs:11-15` says v2 "is being
ported one round at a time on `feat/bench-harness-v2`. Round 1 defines the public API surface;
subsequent rounds fill in workload, cache, orchestrator, validation, analysis, report, sensors,
history." That branch does not exist on the remote, and every subsystem it lists as forthcoming is
shipped. A reader arriving at the crate is told it is a skeleton. One paragraph.

## F12. `glob_match` is correct where it is documented, and has a combinatorial wall

`glob_match` (`tree.rs:276-303`) decides benchspace membership, and `["**"]` is the settled default, so
it runs for every consumer that adopts the form. I named it as unexamined and then examined it.

**Its own suite is real and I am carrying it forward** (this is the eighth thing kept). `tree.rs:1031-1047`
is 17 assertions covering `**` at the root, `*` not crossing `/`, and prefix, suffix and infix
components, one of them carrying its reason inline ("a component `*` never crosses `/`"). For a grammar
this small that is close to the whole matrix rather than a sample.

**Every corner it does not name behaves as documented.**
`mock/research/202608151554_probes/glob-corners/`, both controls passing:

```
glob_match("a/**",   "a")     = true     as documented
glob_match("**",     "")      = true     as documented
glob_match("a/**/b", "a/b")   = true     as documented
```

The doc comment's zero-component claim holds and was unasserted. `**` as a prefix, `**` in the middle,
several `*` in one component, `*` matching an empty run, and the empty pattern all behave sensibly.
`glob_match("*", "")` is `false` while `glob_match("", "")` is `true`, which is harmless because
`split('/').filter(non-empty)` means a path component is never empty.

**The one real hazard is the backtracker.** `component` (`tree.rs:277-289`) recurses over every cut
point with no memoisation. Growth in the number of `*` in one component, on a 24-character component,
as an ad-hoc spike showing a shape and pricing nothing:

```
stars   4     5     6     7     8     9    10    11    12    13
ms    1.3   5.9  20.7  64.5 167.9 341.0 615.3 974.2 1405  1838
```

The ratios flatten past `k = n/2` because the cost is **combinatorial in (stars, component length)**
rather than exponential in stars alone. An earlier run at `k = 16` against `n = 40`, much nearer that
peak, had not returned after roughly 400 seconds and was killed; that is an existence claim about
non-termination in practice, and it is the only claim the run supports.

**This is a hazard, not a live defect, and I rank it last.** Patterns are consumer-authored and the
realistic ones (`**`, `bench-*`, `*-probe`) sit far from the wall. It is worth recording because
`members` and `exclude` are matched against **every discovered path** (`tree.rs:197`, `:261`), so the
cost is paid per path rather than once, and because the standard two-pointer greedy glob is linear and
is about the same number of lines. **What would close it:** if any consumer's `members` or `exclude`
ever carries more than about three stars in one component, replace the matcher; below that, leave it.

---

---

## Additions: mechanisms whose absence forces duplication

Three, in the order I would build them. Each is stated as an intent rather than a signature.

**A1. One definition of the worker wire (F3).** The largest correctness return of anything here, and it
is one type with two impls.

**A2. One definition of the stage vocabulary (F7).** Turns three lists into one and makes the seventh
stage a decision rather than an accident.

**A3. A declared-name resolution point (F1).** Somewhere between preflight and measurement, the arm
names exist and every declared role can be checked against them. The framework already dlopens every
arm in preflight; reading `bench_name` there costs nothing and makes six categories of silent
misdeclaration into named errors. This is the mechanism F1's cheaper reading needs and cannot have
today, so building it makes both readings available and lets whoever decides pick with the information.

**Not proposed, deliberately.** A logging abstraction (F8's boundary). A serde wire format (F3's
boundary). A file-size split pass. A `Cargo.toml`-emitting change to consumer arm crates, since PR #21's
`--config` makes the 991 declared profile blocks redundant on the path that produces artifacts and I
measured that rather than assumed it.

## Options opened, and what closes each

| # | option | what would close it |
|---|---|---|
| F1 | typed roles against a validation pass | whether a pre-measurement point knows the resolved arm names (A3 creates one) |
| F2 | `enum Mode` against an emptiness guard | whether any consumer or artifact uses a third mode value; I found only `warm`/`cold` in the current trees |
| F3 | the `WorkerLine` type against widening the guard to `>= 12` | whether change 1 lands before or after this round's source changelists |
| F4 | delete the six against exposing them | per symbol: is it unreachable from a documented path (expose) or unwanted (delete); `domain_work` is decided by F7 |
| F5 | derive one field list against leaving three | whether the three forms are *meant* to diverge beyond `normalise`; nothing I read says so |
| F8 | collapse `validation.rs`'s 24 writes against leaving them | count how many survive change 1's structured return |
| F9 | fix the two wrong examples against making all eleven `no_run` | do both; the second makes the first unrepeatable |
| F12 | replace the glob matcher against leaving it | whether any consumer's `members` or `exclude` carries more than about three stars in one component |

## Predicates: what my instruments varied, and what they could not reach

Every claim above holds in the region below and nowhere I did not measure.

```
holds for: repository = hiisi-digital/mockspace,
           branches = { feat/bench-round-consolidation, feat/bench-consolidation },
           consumers = { arvo, hilavitkutin, vehje, kirjo } at their current dev checkouts,
           toolchain = nightly-2026-05-28 (cargo 1.98.0-nightly fbb61be30),
           host = darwin/aarch64, threads = 1
```

**Dimensions my instruments did not reach, listed so nobody reads silence as coverage.** I ran no
benchmark and I priced nothing; every number here is a count, a grep or a rendered string, and no claim
in this file depends on how much. I did not vary the operating system, and F-carried-forward 1's
`DLL_PREFIX`/`DLL_SUFFIX` path resolution (`config.rs:451-474`) is platform-dependent by construction.
I did not vary the toolchain, which matters for the profile-precedence probe: cargo's `--config`
precedence is a cargo behaviour and I measured one cargo. I did not exercise the driver end to end
against a real bench tree, so every claim about `driver/mod.rs` is from reading and from its own tests,
not from running it. I did not read `tree.rs` (1049 lines, entirely new in PR #21), `cache.rs`,
`summary/`, `disasm/` or `perf.rs` beyond grepping them, so I make no claim about any of them, and the
F4 census counts them only as reference sites.

**And one shared-input warning about any agreement between me and the parallel expert.** We read the
same three topic files, the same changelist and the same six research files, and this workspace's rules
load into both our contexts automatically. Where we agree on something those documents state, that is
one instance wearing two hats. The parts of this file that are instrument-backed are F1, F2, F3 and the
profile precedence, each with a committed probe and stated negative controls; the census in F4 and the
counts in the corrections are greps anyone can re-run. Those are the parts where agreement would mean
something.

## Concessions

**The profile thread.** Entered first, pursued to a committed probe, and PR #21 had already closed it.
The residue is the hand-build path, which produces no artifacts. Reported as a concession rather than
dressed up as a finding.

**The `build_workload` duplication.** Real across three consumers, and PR #21's `[workload.*]` closes it
at the framework level. My contribution shrank to F7, which is a defect *in* that fix.

**F9's measurement: withdrawn as a concession.** I first wrote that I could not measure whether any of
the eleven examples is wrong. I could, by transcribing them into separate bins rather than by
un-ignoring the doctests, and the answer is two of the six I could transcribe. What remains genuinely
unmeasured is the other five: `timed!` and `timed_calibrated!` name `Input<N>` / `Output<N>`, which are
not types in `bench-core` and read as deliberate placeholders rather than as claims; `bench-macro`'s two
and `bench_matrix!` need a consumer crate to transcribe against. **Five of eleven unmeasured, five
compile, two are wrong.**

**`tree.rs`: withdrawn except for one part, which is now the honest gap.** I first conceded 1049 unread
lines, then read its type declarations and composition path (F5's second half, the per-file-form probe)
and then its glob (F12). What I have still **not** exercised is the composition merge itself,
`compose_composed_member` at `tree.rs:401-527`: 127 lines of hand-written precedence deciding which of a
sweep's, a member's and the root's value wins for each of twelve fields, with `merge_timing` at `:552`
doing the same for five more. It is the single place where F5's three copies actually meet, its
correctness is entirely in the ordering of hand-written `or_else` chains, and I read it without
exercising it. **That is where I would send the next person**, and the instrument is cheap: the
per-file-form probe already builds a real tree and calls `tree::load`, so asserting a full precedence
matrix over it is a few dozen lines on machinery that is committed and working.

---

## Phase two: reconciliation, owed and not performed

Phase one is committed and pushed: this file, seven probe directories with their sources, their
committed outputs and each one's negative controls.

**At the time of writing there is no sibling to reconcile against.** I fetched all refs repeatedly
across the run. The only `docs/*` branches carrying work dated today are
`docs/sweep-consumer-view` and `docs/sweep-investigation`, both from the round's earlier phase and both
already folded into the three topic files, and `docs/bench-ergonomics-survey`, whose measurement the
`202608151339` topic corrects. `feat/bench-hygiene-collection` moved during my run and carries PR #21
plus lint work, not a parallel derivation.

So the reconciliation section this file owes is **not omitted, it is outstanding**, and whoever finds the
sibling branch should append it here rather than assume it was skipped. Two things to check first when
it appears, because they are where agreement between us would mean least:

- **Shared inputs.** We read the same three topic files, the same changelist and the same six research
  files, and this workspace's rules load into both contexts automatically. Agreement on anything those
  documents state is one instance wearing two hats.
- **Which of my claims are instrument-backed.** F1, F2, F3, F9, F12 and the profile precedence each have
  a committed probe with stated negative controls, and each probe's `run.sh` prints the tree it built
  against, because the first run of two of them compiled against the wrong branch and I only caught it
  by adding that line. F4's census, F5's field table and the three corrections to the brief are greps
  anyone can re-run. Everything else in this file is reading, and reading is where two experts agree
  most easily and least usefully.
