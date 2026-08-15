# Where the bench framework fights you

A read of the bench side of mockspace as a usability question: where the friction actually sits, what
authoring and running and diagnosing a bench costs a person today, and what would have to be true for it
to stop costing that. Derived blind, in parallel with a second expert on the same subject; the
reconciliation is appended below after this file was committed.

The lens is the tool's architecture rather than its feature list, so the method is the one that method
implies: pick concrete operations a person actually performs, walk them end to end at the data level, and
count what happens. Seven probes are committed beside this file at `202608151700_probes/`, each with the
negative control that had to fail before its number counted.

## Gate outcomes, first, because they change how the rest reads

**Canon gate: passed.** There is no `mock/canon/` in this repository, so there is no ratified canon to
defend and nothing here can conflict with one. What is fixed is the list under "Settled by op, not
reopened here" at `mock/design_rounds/202608151545_changelist.doc.md:31-38` and this workspace's own
discipline. Nothing below reopens any of the seven settled items, and section 8 states where each of my
proposals sits relative to them.

**Test gate: passed, with one named defect and one false claim found while running it.**

The whole workspace suite is green: 235 in `mockspace`, 126 in `lint-rules`, 79 plus 6 in
`bench-harness`, 47 in `cargo-mock`, 21 in `mockspace-manifest`, 15 in `bench-core`, 6 plus 4 in
`bench-matrix`, 4 in `bench-macro`, plus 8 integration tests in the root crate. I scanned all 152
`#[test]` bodies in the four bench crates for assertion-free bodies and read the ones the scan flagged plus every test in the
files I make claims about.

Three tests carry no assertion. Two of them are honest compile tests that say so in their own comments
(`bench-core/src/lib.rs:676` `timed_calibrated_expands_and_runs`, `bench-core/src/byte_routine.rs:171`
`timed_accepts_plain_and_wrapped_setup`); a compile test is a real test of a real claim and neither is
inflating a count. The third fails the gate:

```rust
// bench-core/src/byte_routine.rs:192-196
#[test]
fn small_sizes_work() {
    let _ = ByteRoutine::<1, 1, false>::build_input(0);
    let _ = ByteRoutine::<8, 8, false>::build_input(0);
    let _ = ByteRoutine::<16, 8, false>::build_input(0);
}
```

It asserts nothing, and its name claims something ("work") that nothing in it checks. Worse, it is
precisely the place where the sampled law would have been caught: `build_input`'s two fundamental
properties, determinism per seed and difference across seeds, are asserted at `byte_routine.rs:109-121`
at exactly one instantiation, `<64, 8, false>`. `small_sizes_work` is the one test that reaches
`IN` of 1, 8 and 16, and it is the one test that asserts nothing about them. The remedy is one edit
rather than a deletion: fold the three shapes into the determinism and difference laws and let the law
run over the matrix. That is a defect in one test out of 152, with the fundamentals present, so the gate
passes rather than refusing.

**And running the gate turned up a checkably false claim in the round's own doc changelist.** At
`mock/design_rounds/202608151545_changelist.doc.md:49-51`, change 1 includes "the deletion of the
tautology at `bench-core/src/lib.rs:637`, **which nothing else in flight performs**." PR #21 performs it,
in commit `ca58832`, whose subject is literally "fix: delete the abi hash tautology, record the profile
actually passed", and whose diff removes `abi_hash_reflects_four_field_layout` and leaves a comment
explaining the reasoning. So the clause is wrong, and it is wrong in exactly the way the same changelist
diagnoses six lines earlier: "PR #21 is open on the identical surfaces and nobody designed against it."
The correction was made at the top of the file and one clause below it was not updated. I cannot edit
`design_rounds/`, so it is reported here.

While checking the brief's own numbers: on the round branch every figure it gave reproduces (96
`eprintln!`, 107 `println!`, 37 `unwrap()`, 16 `expect(`, 3 `panic!`, 9 files at or over 500 lines,
`NotImplemented` at exactly two lines). The "11 `InvalidConfig` construction sites" figure is the grep
count for the identifier; the construction sites are 9, with the other two being the declaration and the
`Display` arm. That does not change anything the brief concluded.

## 1. The finding I would put in front of the maintainer first

**The two keys that decide what every number in a report *means* are not checked against anything, and a
one-character typo in one of them silently changes the answer by a third with nothing in the artifact
saying so.**

`baseline` names the arm every delta and every ratio is taken against. `floor` names the arm the ratios
are differenced against before the division. Neither is validated against the arm set.

```rust
// bench-harness/src/analysis.rs:276-281
pub fn with_baseline(mut self, name: &str) -> Self {
    if let Some(idx) = self.variants.iter().position(|v| v.name == name) {
        self.baseline_idx = idx;
    }
    self
}
```

A name that matches nothing leaves `baseline_idx` at its default of `0` (`analysis.rs:260`), which is
whichever arm sorted first. `with_floor` (`analysis.rs:295-298`) stores any string at all, `floor_mean`
(`analysis.rs:302-310`) returns `None` when it resolves to nothing, and `report::generate` then renders
raw ratios (`report.rs:212-215`).

Probe 02 renders the same three arms four ways. Reading the `× base` column:

| case | header | switch | threaded | nullfloor | floor note |
|---|---|---|---|---|---|
| correct | `Baseline: **switch**` | 1.00× | **0.38×** | 0.00× | present |
| `floor = "nulfloor"` | `Baseline: **switch**` | 1.00× | **0.50×** | 0.20× | ABSENT |
| `baseline = "swtich"` | `Baseline: **nullfloor**` | 0.00× | 0.00× | 1.00× | present |
| no floor declared | `Baseline: **switch**` | 1.00× | **0.50×** | 0.20× | ABSENT |

The typo'd-floor report is **byte-identical to the no-floor report**. The explanatory footnote at
`report.rs:227-239` is emitted only when the floor resolved, so the entire observable difference between
"floor applied" and "floor silently dropped" is the absence of one paragraph a reader would have to know
to look for. The number moved from 0.38 to 0.50, which is a 32 percent change in the quantity the bench
exists to produce.

The baseline typo is less severe because `report.rs:26` prints `Baseline: **{name}**`, so the effective
baseline is named. That is a real mitigation and it should be kept. It is still a report a person has to
read carefully to notice is answering a different question.

**And the fail-open is doubled in the generator.** `render_bench_section` resolves `baseline_contains`
by substring and falls back to `names.first()` when it matches nothing (`matrix.rs:279-282`), and the
floor path emits no `floor` line at all when its tag matches nothing (`matrix.rs:294-299`). The second
of those has a test pinning it as intended:

```rust
// bench-harness/src/matrix.rs:428-430
spec.floor_contains = Some("does_not_exist".into());
let section2 = render_bench_section(&spec, &comps);
assert!(!section2.contains("floor ="), "unmatched floor tag emits nothing");
```

The comment names the alternative it rejected: "a floor tag matching no variant emits no floor line
(rather than a dangling ref)". Both options in that dichotomy are wrong, and the third was never
considered: **refuse**. Emitting a dangling reference is bad, emitting silence is worse, and refusing at
generation time costs nothing because the generator has the full name list in hand two lines earlier. The
matching `baseline_contains` fallback has no test at all.

**Two readings, and what would distinguish them.** The generous reading is that this is deliberate
tolerance so a partially-written manifest still runs, which is a real value during authoring. The
ungenerous one is that nobody asked what happens when it does not match. The tell that decides it: a
deliberate tolerance would say so in the artifact, and the artifact says nothing. If the tolerance is
wanted, the fix is not to refuse but to record: a `floor` that did not resolve becomes a line in the
report and a field in the record, and the tolerance becomes visible rather than silent. If it is not
wanted, refuse at load. **Both are better than today and the choice is op's**; my own weight is on
refusing, because a bench that runs against a silently different normalisation has produced a number
whose meaning nobody can recover from the artifact.

### The fix is three lines in a function that already exists

This does not want a new mechanism. `BenchManifest::validate_roles` (`config.rs:564-584`) already loops
every bench section, already checks two role-consistency properties, already returns the right shape of
message, and is already called from both load paths (`tree.rs:180`, `tree.rs:355`, `config.rs:552`). It
checks that roles are not declared twice and that `floor`/`delta` do not appear without a `baseline`. It
does not check that any of the three names an arm that exists.

The addition is the same shape as what is there:

```
bench `warm-container-width-l1` declares baseline = "swtich", which is not
one of its arms. Declared arms: [switch, threaded, nullfloor]. Every delta
and ratio in this bench's report is taken against the baseline.
```

`validate_roles` is new in PR #21 and did not exist on `dev`, and it arrived with a test I would hold up
as the standard for the rest: `load_itself_refuses_double_roles_not_only_validate_roles`
(`config.rs:1110-1113`), whose comment says it "guards the call site: `validate_roles` existing is not
the [same as it running], while the direct `validate_roles` tests stay green." That is a test pinning
that a check is *wired*, which is the failure mode the validation topic file found four instances of.
Whoever wrote it had the right instinct and it should be the pattern for every check added below.

**What would close this option:** a decision from op between refuse-at-load and record-the-non-resolution.
Nothing else is unknown; the code location, the message shape and the test shape are all settled.

## 2. The diagnostics are good where somebody wrote them and absent where nobody did

This is the part I most want to say plainly, because the easy version of a usability review is that error
messages are bad, and here they largely are not.

Probe 01 builds a minimal valid benchspace, applies exactly one plausible authoring mistake, and prints
what the loader says. Ten of the fifteen cases produce a diagnostic I would call finished: it names the
object, names the defect, points at the line, and names the remedy.

```
invalid bench config: .../widths/bench.toml: TOML parse error at line 4, column 2
  |
4 | [sweeps.width]
  |  ^^^^^^
unknown field `sweeps`, expected one of `title`, `workload`, `master_seed`, `arms`,
`variants`, `points`, `sizes`, `may_differ`, `required`, `threaded`, `baseline`,
`floor`, `delta`, `timing`, `sweep`
```

```
invalid bench config: bench `widths` names arm `dense`, but widths/arms/dense/ does
not exist. Discovered arms: [densee, packed]. An arm outside arms/ is referenced by path.
```

The first comes free from `#[serde(deny_unknown_fields)]` on `ComposedBench` (`tree.rs:99`) plus toml's
spanned errors, and it is worth naming as a decision that paid: the attribute is one line and it converts
every misspelled key in the whole schema into a pointed diagnostic with the valid set enumerated. The
second is hand-written at `tree.rs:624-630` and is the house style at its best. `resolve_routine`
(`driver/mod.rs:151-166`) and the missing-dylib preflight (`driver/mod.rs:347-369`) are the same
standard, and `select_names` (`driver/mod.rs:194-218`) refuses an unmatched `--only` and lists what is
available.

**So the diagnostic surface is not the problem. The gap is which properties get checked at all.** Five
of the fifteen cases pass silently, and every one of them is a property nobody wrote a check for rather
than a check that produced a bad message:

| what a person did | what happened |
|---|---|
| `baseline` names an arm that is not in `arms` | accepted; section 1 |
| added an arm directory and did not list it | accepted, and the arm is **built** |
| deleted a member's `bench.toml`, leaving the directory | benchspace loads with **zero cells**, `Ok` |
| `master_seed = "0x1234"` as a string | accepted, and correctly so: `de_seed` (`config.rs:355-375`) takes the string form because TOML has no hex integers, and names the key in its error on a bad one |

The zero-cell case deserves its own line. A benchspace that resolves to nothing at all returns `Ok` with
an empty manifest, and the only thing standing between that and a silent success is
`driver/mod.rs:342-345`, `error: nothing selected (no bench/size entries)`. That message fires and is
correct, but it fires at the driver, after the tool has already decided there was nothing to build. A
person who moved a directory and expected a run gets a sentence about selection rather than about the
directory they moved.

One smaller instance of the same class, found while checking that row rather than looked for: `de_seed`
returns `Ok(v as u64)` for the integer arm (`config.rs:364`), so `master_seed = -1` becomes
`0xFFFF_FFFF_FFFF_FFFF` without comment. The run is still deterministic and still replayable, so this is
an annoyance rather than a defect, and it is one `try_into` away from being a named refusal.

The orphan-arm case costs build time rather than correctness: `TreeManifest.arms` collects every
directory under `<bench>/arms/` regardless of whether the manifest names it, and `run_generated` builds
every entry of `plan.arms` (`src/bench.rs:584-618`). An arm the manifest does not name is compiled on
every run and never measured. That is cheap to detect (the two sets are both in hand at load) and the
right response is a warning naming the directory rather than a refusal, because an arm parked while
someone works on it is a legitimate state.

### Where the diagnostics genuinely do fail

Three places, and they are the ones nobody wrote because they are not in the loader.

**A report regenerated from a slightly damaged CSV panics with an index out of bounds.**
`report::generate` opens `let base = ds.baseline();` (`report.rs:17`), `DataSet::baseline` is
`&self.variants[self.baseline_idx]` (`analysis.rs:312-314`), and the two emptiness guards in the function
(`report.rs:36`, `report.rs:42`) are `len() > 1` and sit *after* the index. Probe 07 confirms it panics
on a mode mismatch, on no samples, and on the garbled mode field probe 03 produces. This is reachable
from `mock bench report`, which is the cheap command a person runs to re-render from a committed CSV
without re-measuring, so the failure lands on the one operation whose whole appeal is that it is fast and
safe.

**A manifest point an arm was not compiled for kills the process.** The arm ABI is exactly three symbols,
`bench_entry`, `bench_name`, `bench_abi_hash` (`harness.rs:122-138`, declared at
`bench-macro/src/lib.rs:321-345`), and **none of them reports which points the arm implements.** The
macro's dispatch table falls through to a `panic!` with an excellent message
(`bench-macro/src/lib.rs:328-333`), but `bench_entry` is `extern "C"`, so probe 06 observes:

```
bench_entry(only64): unsupported n=128, declared sizes: [64]. Add the size to the
#[bench_variant(... sizes = [...])] attribute, or pick an existing one in your bench.toml.
...
panic in a function that cannot unwind
stack backtrace:
   ... 17 frames ...
thread caused non-unwinding panic. aborting.
exit=134
```

The useful sentence is there and it is buried under a SIGABRT and a seventeen-frame backtrace, in a
subprocess, at the innermost point of a run, after every arm has been built. **And the harness could not
have caught it earlier, because there is no symbol to ask.** That makes it a missing export rather than a
missing check, which is the whole reason it is worth naming.

**A malformed `--seed` is discarded and the run continues.** `parse_seed` (`driver/mod.rs:124-131`)
returns `Option`, and `parse_cli` assigns it straight into `seed_override` (`driver/mod.rs:105-110`), so
`--seed 0xZZ` silently leaves the manifest seed in place. A person replaying a run with a mistyped seed
gets a different run and no signal. The same `_ => {}` arm at `driver/mod.rs:113` drops any unrecognised
`--flag` without comment.

### The proposals

**A. Add `bench_points` to the arm ABI.** One `extern "C"` symbol emitted by the same macro that already
knows the list, returning the declared points. The driver's existing preflight then compares each cell's
declared points against each arm's supported set in the same loop that already collects missing dylibs,
and reports every mismatch at once instead of aborting on the first. This composes with, rather than
competes with, the earlier expert's proposal to stop asking for `sizes` in the attribute at all
(`202608151234_what-the-consumer-should-write.md:379-390`): if the tool generates the attribute from the
manifest the drift cannot occur, and if a consumer hand-writes an arm the export catches it. Both paths
want the symbol. Cost: one function in `bench-macro`, one lookup in `harness.rs`, one loop in the
preflight, and an `abi_hash` bump so old arms are refused with the existing ABI-mismatch message rather
than by missing-symbol failure.

*What would close it:* whether the abi-hash fold should cover the new symbol, which is a decision the
round already touches under change 2's "abi-hash version fold". If the fold lands, this rides it.

**B. Guard `report::generate` at its entry.** `if ds.variants.is_empty() { return <a report that says
so> }`, naming the CSV path and the mode it filtered on. Four lines. The alternative reading is that
`generate` should take a non-empty type so the state is unrepresentable, which is better and costs a
constructor change through `dataset_for_routine`; I would take the four lines now and note the type as
the shape to climb to when `DataSet` is next touched, because the round is not opening `analysis.rs`.

**C. Make `parse_cli` refuse what it does not understand.** A `--flag` that matches nothing, and a
`--seed` that does not parse, both become errors naming the flag and the accepted forms. Roughly ten
lines at `driver/mod.rs:92-122`. There is no reading under which silently ignoring a flag a person typed
is the right behaviour for a measurement tool.

## 3. The loader stops at the first mistake, and the tool already knows better

Probe 01's fifteen cases each carry exactly one defect, so each produced exactly one message, and that
looked fine until I broke my own scaffold in two places at once. It reported the first and stopped, and
the second stayed invisible until the first was fixed.

`tree::load` (`tree.rs:167-183`) is a per-member loop with `?`:

```rust
for member in resolve_members(benches_dir, &space)? {
    compose_member(benches_dir, &member, &mut manifest, &mut tree)?;
}
```

Every member is a separate file with a separate `bench.toml`, so the errors are independent by
construction. A person adopting the composed form across arvo's 49 sections, or migrating vehje's 180,
fixes one typo per invocation. The root file's failure is genuinely fatal (no root, nothing to do); the
member failures are not.

**And the tool already does the right thing one layer down.** The driver's preflight collects *every*
missing variant dylib and reports them together (`driver/mod.rs:347-369`), with a message that names the
count and lists each with its bench and point. That is the shape. The loader should have it, the same
accumulator, the same message form.

**Two readings.** Collecting everything is right for a migration and can be noise for a single edit,
where the first error is the one you want. The distinguishing question is which the tool is used for
more, and the answer is visible in the changelist: consumer migrations, per repo, on their own rounds
(`changelist.doc.md:68`). During a migration a one-error-per-run loader is the dominant cost of the
whole exercise. If the noise worry is real the mitigation is a cap ("and 34 more"), not a return to one.

**What would close it:** nothing external. It is a `Vec<BenchError>` accumulator in one loop plus a
`BenchError::Several(Vec<BenchError>)` variant, and the `Display` for that variant is a join. The one
design question it raises is whether `BenchError` should carry many, and the answer the enum already
suggests is yes, because `NotImplemented` occupies a variant slot for a state that occurs nowhere.

### And while looking at `BenchError`

`NotImplemented { what: &'static str }` appears at exactly two lines, its declaration
(`error.rs:19-21`) and its `Display` arm (`error.rs:70-72`), and nowhere else in the workspace. Its own
doc comment describes a scaffolding phase that ended: "Round 1 returns this from the entry point;
subsequent rounds remove individual call sites as the underlying implementation lands." Every call site
was removed and the variant was not. Deleting it is a two-line change with no consumer impact, since
nothing can construct it, and it frees the slot the multi-error variant wants. This is small and I raise
it because a public error enum is a contract a reader believes, and a reader today believes some part of
this pipeline is unimplemented.

`InvalidConfig { reason: String }` is the opposite shape and carries 9 construction sites on the round
branch and 22 on PR #21. One `String` variant carrying every configuration failure means a caller cannot
distinguish "you misspelled a key" from "the arm you named does not exist" from "this file is not TOML",
and the strings are the only structure. I am **not** proposing an enum of twenty variants; the messages
are good and turning good prose into variant names would lose it. What I would add is one field: the
`PathBuf` of the file the failure is about, which is already interpolated into most of the strings by
hand (`config.rs:549`, `tree.rs:319`, `tree.rs:352`, `tree.rs:411`) and is absent from the rest
(`tree.rs:427`, `tree.rs:457`, `tree.rs:466`, `tree.rs:624`). A structured path is what an editor
integration or a `bench check` would need, and it is what makes the multi-error join renderable grouped
by file.

*What would close it:* whether anything is ever going to consume these errors programmatically. If the
answer is no, the `String` is fine and only the path field is worth adding.

## 4. Every arm rebuilds every dependency it shares, and one parameter fixes it

`bench_tree::arm_target_dir` (`tree.rs:590-595`) gives every arm its own
`target/mock-arms/<bench>/<arm>`, and `cargo_build_at` (`src/bench.rs:508-534`) passes it as
`--target-dir` on a per-arm cargo invocation.

On disk in arvo today, under the pre-PR21 equivalent shape: **90 per-variant target directories totalling
2.4 GB**, each carrying the same six rlibs. Three variants sampled, and the sets are identical:
`mockspace_bench_core`, `syn`, `quote`, `proc_macro2`, `unicode_ident`, and the bench's support crate.
`syn` is compiled 90 times in one repository. The four consumer trees hold 1088 arm crates between them
(arvo 94, hilavitkutin 92, vehje 900, kirjo 2).

**The obvious fix is wrong and probe 04 is what killed it.** Putting the arms in one cargo workspace
compiles the shared crate once, and that single compile *is* the contamination: with `arm-a` requesting a
support crate's feature and `arm-b` and `arm-c` not, feature unification hands all three the same rlib,
and `arm-b` is then measured against code it did not ask for. That is the real reason per-arm isolation
exists, and vehje's per-cell feature gates make it a live case rather than a hypothetical
(`202608151339_topic.where-the-bench-configuration-lives.md`, the five categories with no `bench.toml`
expression).

**The fix that survives it is one parameter.** Keep one cargo invocation per arm. Give them one shared
`--target-dir`. Cargo shares an artifact only when the fingerprint matches, so arms with identical
feature resolution share and arms that differ get their own:

| shape | compilations of the shared crate, 3 arms, 2 feature sets |
|---|---|
| one workspace, one invocation | 1 (wrong: b and c get a's feature) |
| per-arm invocation, one shared target dir | **2** (correct and shared) |
| per-arm invocation, per-arm target dir (today) | 3 |

Isolation is unaffected: `nm` shows no arm resolving the shared symbol across the artifact boundary under
fat LTO and one codegen unit. Each cdylib stays a closed linkage unit.

`holds for: cargo 1.9x, macOS aarch64, crate-type cdylib, profile release with lto=fat and
codegen-units=1, dependency graph a path dependency with a feature axis, arms any, invocation
one-per-arm.` Not varied: linker, target triple, registry dependencies, build scripts, parallelism.

**Two readings of why it is per-arm today.** The generous one is that the comment says what it is for:
"Per-arm target directories cannot collide however the arms are named across benches" (`tree.rs:591-593`).
Collision avoidance is a real requirement and it is satisfied by cargo's own crate-name hashing inside one
shared directory, which is what the probe demonstrates. The ungenerous one is that per-arm was chosen
because it obviously cannot collide, without asking whether the shared directory can. **Either way the
requirement is met by the cheaper shape**, and the change is the value of one argument.

The residual risk I did not price: whether concurrent per-arm invocations into one target directory
serialise on cargo's package lock. They will, and today's shape does not, so a parallel build loses
concurrency it may or may not currently have. `run_generated` builds arms in a sequential `for` loop
(`src/bench.rs:584-618`), so there is nothing to lose today, but if the round ever parallelises arm
builds this trade reverses and the answer becomes per-bench target directories: shared within a bench,
separate across benches, which keeps most of the sharing and all of the parallelism.

**What would close it:** a run on the mockspace bench harness measuring build wall time for the three
shapes at a realistic arm count. I did not take that measurement and will not guess at it; the counts
above are counts of compilation units, not timings, and under this workspace's rules a wall-clock number
taken anywhere but `mock/benches/` could not be called a measurement. **The question of how much this
costs is unpriced.** What is established is that the shared shape performs strictly fewer compilations
and preserves isolation.

## 5. The samples codec exists twice, and neither copy can tell absent from zero

Two findings that turned out to be one.

**The codec is duplicated.** `cache.rs:400-431` `load_csv` is line-for-line `sample.rs:108-141`
`load_samples_csv` apart from the signature, an empty-line guard and an inverted length condition; the
seventeen field-parse lines are byte-identical. `cache.rs:433` duplicates `harness.rs:772` `write_csv`.
The seventeen-column header literal is written at `cache.rs:436`, `harness.rs:774`, and three times more
in tests. That is five places one fact lives, and the changelist lists the samples CSV's columns under
"Not changed" (`changelist.doc.md:96`), so the schema is stable and the extraction is safe: one
`Sample::HEADER`, one `Sample::from_row`, one `Sample::to_row`, in `sample.rs`, with `cache.rs` and
`harness.rs` calling them. This is compression from evidence rather than architecture invented ahead of
it: the repetition is already there, five times, and it has already drifted once.

**Neither copy can distinguish a missing field from a zero.** Every column parses with
`unwrap_or(<zero>)`, and zero is meaningful in each of them. Probe 03, all four confirmed:

| input | result |
|---|---|
| one garbled `algo_ns` cell | that arm reads **0.0 ns**, which the report calls the fastest |
| a pre-digest CSV | every `digest` reads **0**, so every arm's digest agrees |
| an arm name containing a comma | every column shears by one; `e2e_ns` reads 0.0 |
| a row truncated mid-write | kept, with the missing tail zeroed |

Rows shorter than ten fields are dropped without a count (`sample.rs:117`), so a partial write loses rows
silently as well.

**The pre-digest row is the one this round needs to hear.** Change 1 makes the digest load-bearing for
the first time, adding "the digest's post-cell comparison" (`changelist.doc.md:46-47`). A comparison of
zeros agrees. Any path that reaches the comparison through loaded samples rather than fresh ones will
report every arm as agreeing on a CSV written before the column existed, and the comment at
`sample.rs:138` says such CSVs are expected: "appended columns; absent in older CSVs, default 0."

The remedy in the extracted codec is to make absence and zero different at the type level:
`digest: Option<u64>` in the parsed row, with the comparison refusing rather than agreeing when it is
`None`, and a header-driven parse so a shear is detected instead of absorbed. The header is written on
every file and read by nobody: `load_samples_csv` skips the header line (`sample.rs:113`) rather than
using it to resolve column positions. Parsing it costs one split and turns the comma-in-a-name case from
silent shear into a named refusal.

**Two readings on the timing columns.** Treating a garbled `algo_ns` as zero is defensible if the goal is
that one bad row never loses a whole run. It is indefensible that the bad row then reads as the fastest
arm in the report. The version I would ship keeps the tolerance and moves the reporting: drop the row,
count the drops, and put the count in the record the round is already building, so a run that quietly
discarded 4 percent of its samples says so.

## 6. `cache.rs` is 463 lines that nothing calls, and it holds the second copy

Probe 05 classifies every module-level public item in the four bench crates by whether anything outside
its declaring file references it, searching the framework and the four consumer trees. Its limitations
are real and stated in its README: it cannot see a type used through method calls or inference, so
`ProgramBuilder` classifies as unreferenced while kirjo genuinely uses it
(`kirjo/mock/benches/src/main.rs:19-31`), and a serde section reachable only as a field of an exported
struct classifies the same way and is likewise fine. **The 38 non-called rows are not 38 dead items and
the probe must not be cited as though they were.**

The one row I checked by hand and that holds: **all seven of `cache.rs`'s public items are referenced
only by the `pub use` at `lib.rs:66`.** A direct grep for `cache::` across the framework and all four
consumers returns a doc comment at `config.rs:779` and nothing else. Nothing constructs a `Cache`, calls
`dylib_hash`, or calls `consensus_drift`.

That is not a call for deletion, and the round already says so: "wiring the cache's skip-rerun system
into the driver" is deferred (`changelist.doc.md:81`). The module is intended. The finding is what its
being unreached costs while it waits: it carries the second copy of the samples codec, so the codec
drifts in a module nothing exercises, and it holds `dylib_hash`, which the record topic names as "the
only copy of a fact the record needs"
(`202608151340_topic.one-record-and-every-writer-a-projection.md`, the inventory row for `.bench_cache`).

So the ordering that falls out: **extract the codec first, from `sample.rs`, and have `cache.rs` call
it.** That removes the drift risk before the module is wired, and it does not depend on the wiring
landing. `dylib_hash` moving out of `cache.rs` to somewhere the record can reach without depending on an
unwired subsystem is the record topic's question rather than mine, and I note only that it is currently
gated behind a module with no callers.

## 7. The file sizes are not holding steady, they are growing, and PR #21 grows them

The brief measured nine files at or over 500 lines on the round branch, against
`file-size-limit.md`'s threshold. On the state the round says it will build on, which is after PR #21
merges, it is ten, and the top of the list moves substantially:

| file | round branch | after PR #21 |
|---|---|---|
| `bench-harness/src/config.rs` | 935 | **1322** |
| `bench-harness/src/tree.rs` | absent | **1049** |
| `bench-harness/src/driver/mod.rs` | 603 | **973** |
| `bench-harness/src/harness.rs` | 848 | 887 |
| total, `bench-*/src` | 13,919 | 15,349 |
| `unwrap()` outside `tests.rs` | 37 | **86** |

I am not proposing that PR #21 be held; the changelist's reasoning for landing it first is sound and I
have nothing to add to it. What I am saying is that "the round's first act triples the count of the thing
the workspace rule calls a smell" is a fact the round should carry rather than discover later, and that a
prior round already ran exactly this pass (`mock/design_rounds/202607191401/202607191400_topic.split-large-modules.md`),
so the mechanism and the taste for it exist.

The seams are visible from the outside without opening the files, which is usually the sign they are
real. `config.rs` carries the serde schema, the role validation, the timing composition and the
`for_size` projection. `tree.rs` carries member resolution, glob matching, composed-form parsing and the
arm and support inventory; `glob_match` alone is a self-contained pure function with its own tests.
`driver/mod.rs` carries CLI parsing, routine resolution, output paths, history keying, the run loop and
the summary, and already has a `driver/` directory with `hooks.rs`, `index.rs`, `staging.rs` and
`worker.rs` beside it, so the pattern for splitting it is established and the remaining pieces have
obvious homes.

**This is deliberately the weakest recommendation in this file.** The rule calls 500 a smell and not a
gate, the code is not hard to read, and a split done for a line count rather than a seam is churn. What I
would actually do is narrower: pull `glob_match` and its tests out of `tree.rs`, and pull `parse_cli` and
`select_names` and `parse_seed` out of `driver/mod.rs` into a `driver/cli.rs`, because those are the two
places where the seam is a pure function with its own test block and no dependency on the rest. Both are
mechanical, both reduce the largest files, and neither requires a judgement about the module's shape.
The rest can wait for a round that has a reason to open those files.

**What would close the larger question:** somebody deciding whether the round wants a split pass at all.
It is not on the changelist and I am not arguing it onto it.

## 8. What is missing that a person would expect

The **additions** half of the brief. Each of these is a thing I reached for while working and did not
find, ordered by how often I wanted it.

**`mock bench check`.** Parse the whole benchspace, run every checkable property, print every failure,
exit nonzero, build nothing. Today the closest thing is `mock bench list`, which loads the tree as a side
effect of printing it (`src/bench.rs:859-883`) and so does function as a "does this parse" command. What
it does not do is run the checks that section 1 and section 2 are about, or collect more than one
failure, or exit nonzero. The value is highest exactly where the round is heading: a consumer migration
that has to move 180 sections wants to know what is wrong with all of them, once, before building
anything.

The checks it would run are all config-only and need no build: unknown keys and parse errors (already
free), every arm named exists, every arm present is named, `baseline`/`floor`/`delta` name declared arms,
no bench resolves to zero cells, the benchspace resolves to at least one member. The dylib-dependent
check (points supported) belongs in the existing preflight, where the dylibs exist.

*What would close it:* whether it is a new subcommand or a `--check` flag on `run`. I lean subcommand
because it composes into a pre-commit hook, which is a shape this repository already ships for other
gates.

**`mock bench list --effective`.** `list` prints the manifest's declared shape. What a person actually
wants before a long run is the resolved shape: which arms will be built, which points will be measured,
what the effective baseline is after resolution, what the effective timing knobs are after the three-way
override composition (`tree.rs:33-34`, section override then member then root), which cells will be
skipped. The three-way timing composition in particular is invisible today: a person reading a member's
`bench.toml` cannot tell what `passes` will actually be without reading two other files and knowing the
precedence. Everything needed is in `TreeManifest` at load, so this is a rendering rather than a
mechanism.

**A way to ask why a number changed.** History exists (`history.rs`), regression detection exists
(`detect_regressions_window`), `INDEX.md` exists. There is no command that takes two runs and says what
differs, and after this round's record lands there will be a `CellRecord` per cell carrying seed, profile,
toolchain, environment and dylib hashes, which is exactly the input such a command needs. A `mock bench
diff <runid> <runid>` that prints the record fields that differ alongside the medians that differ would
turn "this got 8 percent slower" from a question into an answer, and the reason I raise it here rather
than leaving it to the record topic is that **it is the strongest argument for the record being one type
rather than seven writers**, and the record topic argues that on other grounds. It is a consumer of the
thing the round is already building, and naming the consumer is how a design avoids building a record
nobody reads.

*What would close it:* nothing. It is downstream of change 1 and should be filed rather than designed
now.

**A dry-run for the generated driver.** PR #21 generates the driver crate from `bench.toml`
(`src/bench_gen.rs`). When generation produces something that does not compile, the error is in generated
code the person did not write and cannot see without knowing where it landed. `custom_lints.rs` has the
same shape and the same problem, so this is not new, but the bench path will hit it far more often
because `routine` strings become Rust paths under substitution
(`202608151234_what-the-consumer-should-write.md:352-360` names this trade and accepts it, correctly).
The cheap mitigation is that a compile failure in the generated crate prints the path to the generated
file and the `bench.toml` key that produced the offending line. The expensive one is the
`const _: fn() = || { ... }` assertion the earlier expert sketched, which turns the diagnostic into one
naming the config key. I would ship the cheap one and keep the expensive one on the list.

**Nothing about progress.** The driver prints `[3/47] name n=1024 (elapsed 12s, eta 200s)`
(`driver/mod.rs:421-433`), which is genuinely good and I would not touch it. What is missing is the layer
above: `mock bench run` builds 90 arms before the driver starts, printing `building arm x/arms/y...` per
arm with no count and no total (`src/bench.rs:613`). For arvo that is 90 unnumbered lines before anything
measurable happens. A count is one variable.

## 9. What I carry forward unchanged, and from whom

Eleven things, and the count is exact.

From `202608151234_what-the-consumer-should-write.md`, four, all of which I re-derived or re-checked
rather than adopted on reading:

1. **The correctness checks are the library's job, not each consumer's** (its section 3, category four,
   and section 7). I reached the same place from a different direction: `validate_roles` exists, is
   wired, and the test that pins its wiring is the model. Independently derived.
2. **Generation over a macro for the typed dispatch table**, on the monomorphisation argument (its
   section 6). Adopted on reading; I did not test it and have nothing to add.
3. **Splitting `results/` from `history/` so `rm -rf results/` stays safe** (its section 8, final
   paragraph). Adopted on reading, and I would defend it for the same reason: it is a small decision with
   a real failure mode behind it.
4. **`generate_all` has no test** (its section 9). Confirmed by grep; still true on PR #21.

From `202608151339_topic.where-the-bench-configuration-lives.md`, two:

5. **A cell is one arm and a sweep is the outer axis**, and the vocabulary collision that follows.
   Adopted; my probes use `arm` throughout on that basis.
6. **The five categories with no `bench.toml` expression**, one of which is per-arm cargo features.
   Adopted, and load-bearing for section 4: the feature axis is the reason the one-workspace fix fails.

From `202608151338_topic.the-validation-that-does-not-validate.md`, two:

7. **A cross-arm mismatch drops nothing.** Re-checked against PR #21, which is a different tree from the
   one the topic read: `driver/mod.rs:519-522` still has the `Err` arm printing and pushing into `dropped`
   without touching `variant_paths`. The topic's finding survives the PR.
8. **No consumer sets `required`, `may_differ` or `timing`.** Not re-measured; carried on the topic's
   count.

From `202608151340_topic.one-record-and-every-writer-a-projection.md`, one:

9. **`build_profile` was a hardcoded literal.** PR #21's commit `ca58832` fixes it, so the topic's
   finding is correct and already addressed by the PR it was written before. Carried as resolved.

From PR #21 itself, two things I would defend against a future reviewer:

10. **`#[serde(deny_unknown_fields)]` on the composed form** (`tree.rs:99`). One line, and it is what
    makes ten of probe 01's fifteen cases produce a finished diagnostic. Keep.
11. **`load_itself_refuses_double_roles_not_only_validate_roles`** (`config.rs:1110`). A test that pins a
    check is *wired* rather than that it works. This is the pattern every check in section 1 and 2 should
    follow, and it should be said out loud so it is copied rather than rediscovered.

## 10. What I did not settle, and where I could not reach

**The build cost is unpriced.** Section 4 establishes that the shared target directory performs strictly
fewer compilations and preserves isolation. It does not establish how much time that is worth, at any arm
count, on any machine. That needs the mockspace bench harness and I did not run it.

**I did not read `validation.rs` (852 lines) or `analysis.rs` (726 lines) closely**, because the
validation topic owns the first and the statistical content of the second is outside my lens. My claims
about `analysis.rs` are confined to `with_baseline`, `with_floor`, `floor_mean` and `baseline`, all of
which I read in full and probed.

**I did not test the routine form of `#[bench_variant]`**, only the typed form. Probe 06's predicate says
so. Whether the routine form's dispatch table has the same undeclared-point behaviour is untested and I
would expect it to, since both forms build the same `match n` with the same fallback arm
(`bench-macro/src/lib.rs:260-335`, where both arms of the `match &args.algo` feed one
`match n` with one shared fallback), but expectation is not a measurement.

**I could not determine whether the `floor` silent-drop is deliberate policy or an oversight.** The test
at `matrix.rs:428-430` pins it, which reads as deliberate, and the test's comment names only two options
of the three that exist, which reads as an oversight in the framing rather than a decision about the
behaviour. I state both readings in section 1 and the call is op's.

**One thing I attacked and failed to improve.** I wanted to say whether the per-arm cargo invocation could
be replaced by a single invocation over a generated workspace, because that would have removed the loop's
per-arm process cost as well as the rebuild cost. Probe 04 killed it on feature unification and I found
no way to keep the single invocation while keeping per-arm feature resolution: `-p a -p b` in one
invocation unifies the same way, and cargo has no per-package feature isolation within one resolve. The
residue, marked as a residue rather than as a proposal, is per-bench target directories rather than
per-arm, which keeps most of the sharing and would survive a future parallel build; I did not test it and
it should not be adopted on this file's word.

## Dimensions this file's instruments varied, and the ones they could not reach

Stated so a later reader can tell what any agreement with the parallel expert actually covers.

**Varied:** authoring mistake kind (fifteen), baseline and floor resolution states (four), CSV row damage
kind (four), cargo build shape (three), feature-set count per shared dependency (two), arm count per
build (three), declared-versus-requested point (two), dataset emptiness cause (three), framework branch
(round branch and PR #21, every count taken on both where it differs).

**Not reached, so nothing here claims anything about them:** platform other than macOS aarch64; any
target triple other than the host; thread count, since every probe is single-threaded and no probe
involves concurrency at all; parallel arm builds; the routine form of `#[bench_variant]`; registry
dependencies as opposed to path dependencies; consumers other than the four in this workspace; wall-clock
time anywhere; the `perf-counters` feature; the `boundary` feature of `bench-matrix`; Windows path
separators anywhere a path is interpolated into a message.
