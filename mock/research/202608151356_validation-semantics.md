# Validation semantics: what a mismatch means and what the framework does about it

## Gate note, read first

The dispatch named branch `feat/bench-round-consolidation`, carrying three
committed topic files under `mock/design_rounds/`, mine being
`202608151338_topic.the-validation-that-does-not-validate.md`. **At the point I
checked, neither the branch nor any of the three files existed anywhere.**
Checked against the GitHub API directly (`gh api
repos/hiisi-digital/mockspace/branches`), not a stale local clone: the eleven
branches on the remote at that moment were `dev`, `main`,
`docs/bench-ergonomics-survey`, `docs/dissolve-proxy-design`,
`docs/sweep-consumer-view`, `docs/sweep-investigation`,
`feat/bench-consolidation`, `feat/proc-macro-exemption-in-builtin-lints`,
`feat/proc-macro-lint-behavior-config`,
`fix/changelist-doc-gate-shame-exemption`,
`fix/state-transitions-auto-commit-default`,
`fix/type-harness/branch-name-trait`. I did not stop at one check: I fetched
every remote branch, diffed each against `dev` for `mock/design_rounds/` and
`mock/research/`, checked the open PR list, and read the local workspace clone
at `~/Dev/clause-dev/mockspace` (read-only, per standing rule) for the same
branch name. It was not anywhere, by any of those checks.

What did exist, on `docs/sweep-investigation` (`dev@6ab55ee` plus two research
files and their probes, confirmed by `git merge-base`), was
`mock/research/202608151243_the-matrix-and-the-sweep.md`, whose closing
"Adjacent findings" section (§6) states, almost verbatim, the four mechanisms my
brief handed me. I built the whole of §1-§8 below against source, independently,
using that memo only as a pointer to where to look, per my own standing
discipline about claims resting on someone else's read.

**The branch and its three topic files materialised on the remote while I was
mid-investigation**, evidently pushed by whoever else is working this round
concurrently: `git fetch` after I had already written this file and attempted
to push found `bb5c128` ("round: open the bench consolidation round with three
topics") sitting on `origin/feat/bench-round-consolidation`, carrying exactly
`202608151338_topic.the-validation-that-does-not-validate.md` alongside the
other two named topics. I read it. It is close to word-for-word what my brief
had already paraphrased, and it cites the same four mechanisms at line numbers
that match what I independently derived (its `validation.rs:525-537` is one
line short of the block's actual closing brace, `:538`, which I cite correctly
below; a trivial discrepancy, not mine to fix). I rebased my one commit onto
the real branch rather than discard the independent work: every citation
below was checked against source directly, not against this topic file, so the
rebase changes nothing about what I found, only what I am allowed to say about
whether the file existed when I started.

I am writing this to `mock/research/`, as instructed, alongside the topic file
rather than inside `mock/design_rounds/`, since the brief's naming (`mock/research/<timestamp>_validation-semantics.md`)
is unambiguous about where a design proposal belongs relative to a topic file.

## 0. Gates

**Canon gate.** No `mock/canon/` exists in this repository
(`where-the-canon-lives.md`: "no repo has migrated"). There is nothing above
the two redesign documents on `docs/bench-ergonomics-survey` to defend, and
nothing above this file either. Existing state (the four mechanisms) is
challengeable in full; I have challenged and sharpened it below rather than
transcribed it.

**Test gate.** `cargo test --workspace --no-fail-fast` on `dev` at `6ab55ee`:
**551 passed, 0 failed, 18 ignored**, reproduced myself (not copied from the
source memo) via `grep "test result:"` summed across all 28 suites. Matches
the source memo's own run exactly, which is worth stating plainly: a
reproduction that agrees is real corroboration, not assumed agreement.

I read the body of every test in the surface I am designing over.
`bench-harness/src/validation.rs:695-852` has nine tests; they establish that
`per_variant` runs regardless of `cross_variant` (`:714-733`), that the
validator is called for every live variant when agreement is required
(`:756-774`), that a skipped variant is excluded (`:775-786`), that consent
(`outputs_may_differ`) turns cross-variant off (`:787-794`), that a tolerance
selects approximate over byte-exact (`:795-806`), hex round-tripping
(`:807-818`), and duplicate-name detection (`:819-851`). All nine are real,
none tautological, and none of them touches the driver's response to a
`validate()` result. **`bench-harness/src/driver/mod.rs` has zero tests** (`grep
-n "mod tests\|#\[test\]"` returns nothing), and `bench-harness/tests/smoke.rs`
says explicitly it does not cover the orchestrator
(`:6-10`, "end-to-end orchestrator validation lives in consumer-adoption
[crates]"). So the exact function that discards a validation `Err` and leaves
`config.variant_paths` untouched (`driver/mod.rs:415-418`) is untested, not
merely under-tested. This is not a defect I am asked to fix, but it is worth
recording as fact: the behaviour this file is about was never pinned by
anything, which is one of the reasons it went unnoticed.

The one known defect in the surrounding suite, `bench-core/src/lib.rs:637-651`
(`abi_hash_reflects_four_field_layout`, transcribes `abi_hash()`'s body with
literals substituted and asserts the transcription equals the function), is
outside `validation.rs`/`driver/mod.rs` and does not bear on this design. I
read it to confirm it, not to fix it; the brief already named it and PR #21's
branch already deletes it.

## 1. The four mechanisms, independently re-verified, and what I found underneath them

Each of the following was checked against the source, not against the memo
that named it. I include `file:line` for everything, and I flag every place
where my own reading is more precise or different than what was handed to me.

### 1a. The digest is computed, carried, and never compared. Confirmed, and coverage is partial.

`grep -c digest bench-harness/src/validation.rs` returns 0. Confirmed exactly.

But I went further: `digest` appears in fifteen files across the workspace
(`grep -rln digest --include="*.rs" .`), and I read every one that is part of
this framework (excluding two unrelated hits in `mockspace-core`/`mockspace-rs`
that are a different "content digest" for lint caching, confirmed by reading
both: `mockspace-rs/src/scope.rs:167-168` is a BLAKE-family content hash for a
cache key, nothing to do with benches). Of the framework hits:

- `bench-core/src/lib.rs:439-445` declares the field on the FFI struct
  `FfiBenchCall`, documented at `:432-435`: "a reps-invariant fidelity witness.
  Under calibration the reps count is timing-dependent, so the run-block's
  output bytes are reps-variant and cannot cross-validate variants; the digest
  is computed on a fixed-seed, fixed-init single pass instead." This is the
  correct diagnosis of a real problem, stated by the people who built the fix,
  and I independently confirm the problem is real in §1b below.
- `timed!`/`timed_calibrated!` (`bench-core/src/lib.rs:507-619`) both leave
  `digest: 0` unconditionally (`:612`, `:522` for `timed!`; the doc at `:421-424`
  says outright that these constructors "measure only `run_ticks` and leave
  them zero"). **Any bench authored as a hand-written `#[bench_variant]`
  function using `timed!` or `timed_calibrated!` directly never populates a real
  digest, ever.** `bench-macro/src/lib.rs` (the `#[bench_variant]` proc macro)
  does not touch the function body at all (confirmed: no mention of `digest`,
  `timed_calibrated`, or `scaffold` anywhere in it), so this is not a gap the
  macro could close without the author opting in.
- Only `mockspace-bench-matrix`'s scaffold module computes a real digest:
  `bench-matrix/src/scaffold.rs:129-136` (the `warm` regime),
  `:179-211` (`cold_cycle`), `:241-261` (`stream`), each folding
  `DIGEST_INIT.rotate_left(7) ^ cell(...)` over a fixed `SEEDS` array, outside
  the calibrated loop, exactly as documented. And this scaffold is reached only
  from **generated** variant crates: `bench-matrix/src/generate.rs:32` renders
  `mockspace_bench_matrix::scaffold::{scaffold_fn}::<N,_,_,_>(...)` into the
  emitted `src/lib.rs`. A hand-written variant never calls it unless its author
  does so by hand, which none of the four consumer trees' hand-written arms do
  (checked by the earlier grep: digest appears in no consumer `main.rs` or
  hand-authored variant crate).
- I confirmed there is no read of `s.digest` anywhere that compares it to
  anything. `bench-harness/src/cache.rs:427,436,459` and
  `bench-harness/src/sample.rs` round-trip it through the CSV parser/writer and
  the history cache, faithfully, and never once diff it against a sibling
  arm's value. `bench-harness/src/report.rs:816` constructs a test fixture with
  `digest: 0`; not a real read.

**So the claim is confirmed and it understates the problem.** The digest is not
merely uncompared; it is only ever *populated* for the fraction of the corpus
generated by `mockspace-bench-matrix` (vehje: 594 of 902 arm crates, per the
source memo's §3b; hilavitkutin and arvo, which do not appear to use
`bench-matrix` at all in this repo's cited consumer paths, would carry `digest
= 0` on every single sample). Wiring the digest into `validate()` as written
today would make the check pass, silently and meaninglessly, for exactly the
consumers whose benches are hand-written `#[bench_variant]` functions, because
`0 == 0`. That is a worse failure than not checking at all: it looks like
coverage and is not. Any design that turns the digest on as a comparison must
close this before it ships, or it manufactures a new false-pass class to
replace the one it removes.

### 1b. A cross-arm mismatch returns `Err`, which the driver discards. Confirmed, and the loss happens one function earlier than the driver.

Confirmed at the cited lines: `validate()` returns `Err(BenchError::ValidationFailed
{ variant, reason })` on any byte mismatch (`validation.rs:525-538`) or any
non-deterministic pair (`:585-599`), and the driver's `Err` arm
(`driver/mod.rs:415-418`) does exactly one thing: `eprintln!` and
`dropped.push(format!("(validation error: {e})"))`, a string that is not a real
path and is never matched against `config.variant_paths`. `config.variant_paths`
is untouched on this arm, so every variant, including whichever one(s)
disagreed, proceeds to `harness::run_orchestrator` (`:438`) unfiltered, gets
timed, gets written to CSV (`:475`), gets a findings report (`:496`), and gets
appended to history (`:566`). The only place the failure shows at all is
transient stderr. I traced `dropped` through the rest of `driver/mod.rs`
(`grep -n dropped`): it feeds exactly one boolean, `required_failure`
(`:420-422`), and nothing else. `required_failure` is checked once, at the very
end of the whole manifest run, after every bench's artifacts are already
written (`:598-601`), and it changes only the process exit code.

**The driver's discard is real, but it is not the root cause; it is the second
failure in a chain that starts inside `validate()` itself.** `validate()`'s own
internal loop *does* know, per seed, exactly which variant index disagreed
(`validation.rs:459-468` for per-variant structural failures, `:501-520` for
cross-variant): both paths compute `(i, reason)` or `(names[i].clone(),
reason)` precisely. But both collapse into the same two accumulators,
`mismatches: usize` and `first_mismatch_reason: Option<(String, String)>`
(`:439-440`), shared across *both* the per-variant check and the cross-variant
check, across *every* seed. `first_mismatch_reason.get_or_insert(...)`
(`:466-467`, `:487-490`, `:515-518`) keeps only the first one it ever sees and
discards every subsequent identity. So even a driver that handled `Err`
correctly could not recover "which arm(s) disagreed" from what `validate()`
returns today, because that information is destroyed before the function
returns, not merely ignored by the caller. **Fixing the driver's `Err` arm is
necessary and not sufficient; `validate()`'s return contract has to change
shape first**, from `Result<Vec<String>, BenchError>` (survive-or-die) to
something that can carry a set of per-variant structural failures and a set of
cross-variant disagreements simultaneously, because both classes can be present
in the same call and today either one alone forces the same all-or-nothing
`Err`.

One more precision the source memo did not have: `Ok(safe_paths)`
(`validation.rs:617-629`) *never* excludes a variant for disagreeing or for
failing its own validator. It excludes only `slow_variants`, populated
exclusively by the pre-flight timeout probe (`:249-278`) and by a worker crash
or truncated output during collection (`:395-430`). A structural failure
(`check_each_variant` returning a non-empty list for some seed) and a
cross-variant mismatch both route through the *same* `mismatches > 0` check at
`:525` and both produce the *identical* whole-function `Err`. **The framework
currently treats "arm X's own validator rejected its output" identically to
"arm X disagrees with arm Y", at every level: same accumulator, same error
variant, same discard at the call site.** These are different claims about the
world (§3 below), and the fact that the code cannot currently tell them apart
by the time it reports anything is the sharper version of the finding than
"the driver discards the error".

### 1c. The generator cannot emit `may_differ`, `required`, `threaded`, or a timing override. Confirmed, and it is two gaps, not one.

`render_bench_section` (`matrix.rs:276-310`) emits exactly `title`, `workload`
(hardcoded `"realistic"`, `:287`), `master_seed`, the `[bench.*.normalise]`
block, and per-size `[[bench.*.sizes]]` rows. No path to `may_differ`,
`required`, `threaded`, or `[bench.*.timing]` exists in the function; confirmed
by reading it end to end, not by grepping for absence.

What the source memo did not check, and I did: **the declaration layer one step
upstream has no field for any of these four either.** `MatrixDecl`
(`bench-matrix/src/decl.rs:65-89`) carries `name`, `crate_path`, `crate_dep`,
`extra_deps`, `master_seed`, `sweep`, `sizes`, `baseline`, `floor`, `regime`,
`setup_path`, `cells`, and nothing else. `CellDecl` (`:43-52`) carries `tag`,
`op_path`, `setup_path`, `features`. **Neither struct has anywhere to put
"variants of this sweep may differ", "this sweep is required to agree", "this
sweep spawns its own threads", or a per-sweep timing override.** So "the
generator must be able to emit X" is the second half of a two-part gap: the
`bench_matrix!` macro and `MatrixDecl` would need the fields first, with
nothing for `render_bench_section` to render until they exist. Fixing
`render_bench_section` alone, without touching `decl.rs`, accomplishes nothing:
there is no data to thread through it. Whatever design lands on this, it names
both layers or it names neither usefully.

### 1d. Zero uses of `required` across 259 sections, confirmed and reproduced. For `may_differ`, the deeper reason is structural, not a consumer discipline gap.

I did not re-run the consumer-manifest count myself (the four consumer trees
are outside this repository and the standing rule is read-only on shared
clones; the source memo's probe, `202608151243_probes/p1_count_section_config.py`,
is committed and its output, `p1_count_section_config.out`, shows the 259/0/0/0
counts for `may_differ`/`required`/`timing` reproducibly). What I did verify,
inside this repository, is *why* the number is zero for `may_differ`
specifically, and it is not simply that nobody bothered.

`BenchConfig.may_differ` (`config.rs:521-522`) is real, resolved data, and it
*does* flow somewhere: `driver/mod.rs:128`,
`(registry.byte_dispatch.dispatch)(config.n, config.may_differ)`. The generated
dispatcher from `byte_routine_dispatch!` (`bench-core/src/lib.rs:383-401`)
genuinely branches on it at runtime (`if may_differ { ...true } else {
...false }`, `:386-393`) to pick between `ByteRoutine<N,OUT,true>` and
`ByteRoutine<N,OUT,false>` monomorphisations. For a bench dispatched this way,
setting `may_differ = true` in `bench.toml` works correctly, today, with no
changes needed.

But `resolve_routine` (`driver/mod.rs:119-147`) checks a **custom hook first**:
`if let Some(spec) = (registry.routine_for)(config) { return Ok(spec); }`
(`:125-126`), unconditionally, before ever reaching the byte-dispatch branch
that reads `config.may_differ`. Any bench served by a `routine_for` hook, i.e.
any hand-authored `Routine` impl (which the earlier "what the consumer should
write" memo established is how 100 percent of arvo's routine dispatch happens,
because arvo's structured, non-flat-byte types cannot use `ByteRoutine` at all)
**never reaches the code that reads `config.may_differ`.** For those benches,
`Routine::outputs_may_differ()` (`bench-core/src/lib.rs:110-115`, default
`false`) is whatever the impl hardcoded in Rust, permanently, with zero runtime
configurability from the manifest. Setting `may_differ = true` in arvo's
`bench.toml` would compile, deserialize, populate `BenchConfig.may_differ`, and
then be read by nothing, because `routine_for` already returned before
`config.may_differ` was consulted.

hilavitkutin's hand-rolled 13-name `matches!` list (cited by the source memo at
`hilavitkutin/mock/benches/src/main.rs:213-216`, outside this repo, not
independently re-checked by me since it is a read-only consumer tree) is, by
this trace, almost certainly *itself* a `routine_for` hook, choosing a
`ByteRoutine<N,8,MAY_DIFFER>` monomorphisation by a static name match rather
than by calling `byte_routine_dispatch!`'s generated dispatcher (which would
have read `config.may_differ` correctly for free). If so, hilavitkutin did not
merely fail to set the key; it reimplemented the dispatch mechanism by hand and
wired it to the wrong input.

**So "no consumer has ever set `may_differ`" is not one fact, it is two, and
they need different fixes.** For the sub-population served through
`byte_routine_dispatch!`, the key works and consumers simply have not needed it
yet (which is a weaker, more benign reading than the brief's framing invites).
For the sub-population served through any `routine_for` hook, which structurally
includes the entire richly-typed, highest-value-to-validate slice of the
corpus, **the manifest key cannot have any effect no matter what a consumer
writes in `bench.toml`**, and no schema-level fix (adding the key, changing its
default, generating it) touches this, because the manifest is never consulted
on this path. A refusal mechanism that lives in the manifest schema (a lint on
`bench.toml`) is structurally blind to this class; it would have to live at the
`Routine` impl or the `resolve_routine` call site instead. I return to this in
§7.

`required` does not have this split: `BenchConfig.required` is read in exactly
one place (`driver/mod.rs:420, 442, 598`), unconditionally, regardless of which
dispatch path served the routine. Its zero-use is a plain consumer-behaviour
fact, and given what `required = true` currently buys (a process exit code
after every artifact for the whole manifest is already written, per §1b), it is
a rational one: the mechanism it gates is barely worth configuring.

### 1e. A finding neither the brief nor the source memo names: the schema's own doc comment is false.

`config.rs:120-122`, on `required`: "Whether a validation failure on this bench
fails the whole run (process exit code). `false` (default): failures are
**recorded in findings** and the run continues." I traced `dropped` through
every use in `driver/mod.rs` (§1b): it feeds one `eprintln!` and one boolean.
It is never written to `findings_path` (`:497`), never enters `generate_report`
(`:496`), never touches `HistoryEntry` (`:522-533`), never appears in the CSV
row shape (`sample.rs:73-171`, no validation field at all). **Nothing is
recorded in findings, on either value of `required`.** This is not a design
choice I am pushing back on; it is a claim the schema makes about its own
behaviour that the code does not keep. Whatever the doc comment describes was
either never built or regressed silently, and it is worth fixing as part of
this design regardless of which way the semantic questions below land, because
right now the schema lies to the person reading it to decide whether `required`
matters.

## 2. The real shape of the problem

Three mechanisms exist, run at different times, on different data, through
different code paths, and none of them talks to the other two:

| | when | how | compares | coverage | on mismatch |
|---|---|---|---|---|---|
| `check_each_variant` (structural) | pre-timing | subprocess worker, direct `entry()` call | one arm's output against `Routine::validate_output` | every routine (default no-op) | folded into the same `Err` as cross-variant |
| cross-variant (`validate`) | pre-timing | subprocess worker, direct `entry()` call | raw output bytes, arm vs. baseline arm | every routine, unless `outputs_may_differ()` | `Err`, discarded by the driver, no artifact |
| digest | during timing | in-process, `bench-matrix` scaffold only | fixed-seed fold, never compared to anything | `bench-matrix`-generated arms only | not applicable, nothing reads it |

These are not three views of one check. They are three different
instruments, built at different times by different concerns, that happen to
share the word "validation". A design that treats them as one thing (as the
schema's `may_differ`/`required` pair implicitly does, by naming a single
policy for "does this bench validate") will keep missing the digest's
partial coverage and the structural/cross-variant conflation, because neither
is visible from the schema's vantage point.

## 3. What a validation failure means, and what happens

The brief asks for precision on the difference between "an arm disagrees with
its peers" and "an arm fails its own validator", because they are different
claims about the world. Having traced both code paths to where they currently
merge (§1b), I can now state the difference exactly and derive the responses
from it rather than asserting them.

**A structural failure is a claim about one arm, checkable against a fixed
oracle, with no reference to any other arm.** `Routine::validate_output`
(`bench-core/src/lib.rs:90-96`) takes one input and one output; it needs
nothing else to render a verdict. When it returns `Err`, the framework knows,
with the same certainty the routine author encoded in the check, that this
arm's number is not trustworthy: either it is fast because it is wrong, or its
wrongness happens to look fast, or both, but either way its presence in a
performance comparison misrepresents the thing being measured. There is no
ambiguity about which arm is at fault; the routine said so.

**A cross-variant mismatch is a claim about a relationship between two or more
arms, and by itself it does not say which one is wrong.** `collected[i][si].0
!= *baseline` (`validation.rs:501`) tells you arm `i` disagrees with whichever
arm happened to be `base_idx` (the first arm that survived the probe stage,
`:444`, an accident of ordering, not a designated ground truth). With exactly
two arms, "drop the disagreeing one" is undefined: there is no principled way
to know which one is correct without an oracle, which is precisely what
`validate_output` is for when the routine author bothers to write one, and
`validate_output`'s default is a no-op (`:94-96`) so most routines have none.
With three or more arms, a majority vote is *available* but it is a heuristic,
not a proof: a bug shared by the majority implementation strategy (the common
case when several arms are variations on the same approach, e.g. arvo's
"warm-container" family sharing one bridge type across 57 arms per the source
memo's §3d) would out-vote the one arm that is actually correct. Silently
trusting a majority here is exactly the kind of coincidentally-correct
inference this framework should not manufacture on a consumer's behalf.

So the three responses the brief asks about are not a menu to pick from freely.
They compose along this distinction, and the distinction decides which
response(s) apply to which class:

**Structural failure → drop the arm, unconditionally, and record why.** This is
sound because the oracle is per-arm and self-contained. There is no case where
keeping a structurally-invalid arm in a performance comparison is the right
call, so this does not need `required` or any other policy knob to gate it;
dropping is the correct behaviour regardless of what the bench author wrote in
the manifest. `check_each_variant` already computes exactly the `(index,
reason)` list needed (`:107-126`); it is discarded downstream, not missing.

**Cross-variant mismatch → the comparison at that (bench, size) cell is not
valid, and the framework must say so rather than present it as valid.** This is
not the same as "fail the whole process". It means: the CSV rows, the report
table, and the history entries for that cell are produced (or withheld,
per §7) *tagged* as unverified-agreement, because the framework genuinely does
not know which arm is right, and presenting timing numbers for two things that
were supposed to compute the same value but did not is presenting numbers for
"a bug" and "a correct implementation" as if they were "fast" and "slow". The
report must not launder that into an ordinary comparison table. This is
mandatory and unconditional in the same sense: it does not depend on
`required`, because it is not a decision about the process exit code, it is a
decision about what the artifact is allowed to claim.

**`required` is a real, separate, third knob, and it is about the process, not
the arm and not the artifact.** Once the two responses above are unconditional,
`required` stops being the only thing standing between "silent" and "loud" (it
currently is that, wrongly, per §1b/§1e) and becomes what its own doc comment
already claims it should be: whether an unverified comparison is tolerable for
this particular bench in this particular CI context, or whether it should hard-
stop the run. That is a legitimate per-consumer policy choice (a bench a human
reviews by eye in a findings.md wants advisory; a bench gating a merge wants a
hard stop), and it is exactly the kind of choice `may_differ`/`required`-style
booleans are good at expressing, once they are not also the only mechanism
keeping a mismatch from vanishing.

One more distinction the brief's three-way framing does not name and that
falls out of the structural/cross-variant split: **`outputs_may_differ() =
true` is not a failure response at all.** It is consent, decided in advance,
that turns cross-variant comparison off (`validation_plan`, `:85-86`). A
routine that legitimately has more than one valid output for the same input
(the module doc's own example, graph colouring, `validation.rs:18-19`) is not
"failing" when its arms disagree; the comparison was never meaningful to run.
This already works correctly in the code (`:787-794` tests it), and nothing
here should touch it. What needs fixing is that its manifest-facing cousin,
`may_differ`, has no reliable connection to it (§1d).

## 4. What the digest is for

The brief's stated complication is precise and I confirm it, with the sharper
mechanism underneath it. `run_worker_validate`
(`bench-harness/src/harness.rs:801-839`) calls the variant's exported
`entry()` function directly (`:829-830`), the *same* exported symbol the timed
run also calls. If a variant's `entry()` body was written with `timed_calibrated!`
(`bench-core/src/lib.rs:578-619`), that function *unconditionally* performs a
probe pass plus a calibrated number of repetitions of the `run{}` block
(`:590-604`), regardless of who is calling it or why: the FFI symbol carries no
"validation mode, run once" signal, because the ABI is `fn(input, output, n) ->
FfiBenchCall` (`bench-core/src/lib.rs:451-452`) with no such parameter. So a
call to `entry()` from inside the validation worker triggers the exact same
calibrate-and-repeat dance as a call from inside the timed harness, and the
number of repetitions it settles on (`calibrate_reps`, `:498-505`) depends on
how fast the *first probe pass* happened to run, which depends on the machine's
current state (cache, scheduler, thermal) at that moment. **For a correctly
implemented, idempotent routine (output is a pure function of input, freshly
assigned each call) this is harmless: any rep count produces byte-identical
output.** The exposure is specifically to a routine whose `run{}` block
accumulates into `output` across repetitions rather than overwriting it fresh
each time (a legitimate implementation shape for some algorithms, and exactly
the class of bug fidelity-checking exists to catch): such a routine's validated
output bytes would depend on an incidental, machine-state-dependent rep count,
making `validate()`'s byte-exact comparison at `:501` genuinely flaky for that
class, in exactly the direction that matters (it could both false-positive-fail
a correct-but-rep-sensitive implementation and false-negative-pass a broken one
whose accumulation bug happens to cancel out at the calibrated count).

The digest, as designed, is the correct fix for exactly this class of exposure:
one pass, fixed seeds, fixed init, outside the calibrated loop, deliberately
reps-invariant (`scaffold.rs:129-136`, and proven so by its own test,
`:271-300`, `assert_eq!(m1.digest, m2.digest, "digest must be reps-invariant
across calls")` after varying the reps between the two calls). **This is not a
decoration that happened to go unused; it is the fix sitting on the shelf,
unconnected to the check it was built to repair.**

But it cannot simply be swapped in as `validate()`'s comparison target, for two
independent reasons, both established in §1a:

1. **Coverage is partial and the two mechanisms run at different times through
   different call paths.** Digest is computed in-process, during the timed run,
   only by the `bench-matrix` scaffold. `validate()` runs pre-timing, out-of-
   process, via the FFI entry point, for every bench. There is no digest value
   available at the point `validate()` currently runs unless the scaffold's
   computation is either duplicated into the validation worker path or moved.
   For a hand-written `#[bench_variant]` arm, no digest is ever computed at all,
   so there is nothing to compare there regardless of when.
2. **A zero digest must never read as a passing comparison.** `FfiBenchCall`'s
   own doc convention already states this correctly for the field in general
   ("a zero in any of the three latter fields means 'not measured by this
   constructor', never a measured zero", `bench-core/src/lib.rs:437-438`); any
   design that wires digest into a cross-arm comparison has to honour that
   convention specifically for digest, or it manufactures a `0 == 0` false pass
   for every non-`bench-matrix` arm, which is worse than the current silence
   because it looks like coverage.

**My reading, weighing the two mechanisms against what each can actually
catch.** They are not redundant and I would not collapse one into the other.

`validate()`'s pre-timing check, run against a reps-invariant target, is the
right place to catch "this arm computes the wrong answer" in the general case:
it runs for every bench, before any time is spent timing a broken arm, and a
single-pass fixed-seed comparison is exactly as sound whether or not
calibration is involved, *provided the value it compares is not itself
calibration-sensitive*. The cleanest fix I can identify for that precondition
does not require the digest at all: `timed_calibrated!`'s expansion
(`bench-core/src/lib.rs:590-604`) can check an environment variable (something
like `MOCKSPACE_BENCH_VALIDATE=1`, cheap, read once per call, already
propagates naturally to a spawned worker subprocess since `run_worker_validate`
already runs in its own process per `harness.rs:801`) and force `__reps = 1`
when set, bypassing calibration entirely for the duration of a validation
worker's calls. That makes the *existing* byte-exact comparison provably
reps-invariant by construction, for every consumer, with universal coverage,
without introducing a second comparison target or a partial-coverage hazard.
I did not implement or bench this; it is a design proposal, not a claim I have
tested, and it belongs to whichever round actually touches `bench-core`.

The digest earns a genuinely different, narrower, complementary role once the
above closes the general case: it is the only mechanism positioned to catch
drift **under actual repetition**, inside the real timed run, which a
forced-reps-1 validation pass structurally cannot see (it never repeats
anything). That is a real, distinct failure class (state leaking across calls
in the same process, a cache or memoisation bug that only manifests after many
calls) and it is worth keeping for the slice of the corpus that already goes
through the scaffold, reported honestly as scoped to that slice rather than
implied to be universal. Extending digest computation to every `timed!`/
`timed_calibrated!` call site is a larger, separate piece of work (it touches
how every hand-written variant is authored, not just the validation layer) and
I would not fold it into this round; it is worth a task of its own, named as
such rather than assumed.

## 5. Should `required` default to `false`?

Argued both ways, because the brief asked for the argument and not just the
verdict.

**For keeping the default `false`.** A measurement tool that hard-fails CI by
default on every validation hiccup, including transient subprocess timing
noise or a routine that legitimately wants `outputs_may_differ` but has not had
that wired yet, punishes adoption and encourages exactly the workaround this
whole investigation is about: consumers routing around the framework's own
correctness checks (per the source memo's §3, arvo's driver hand-wires
`harness::validate` itself with a comment recording that the framework's
default path once produced "400 rows of ordinary-looking numbers and exit 0"
without it). `warn`-vs-`deny` is an established, comprehensible convention
(rustc's own lint levels) and mirroring it here is not unreasonable on its
face.

**For flipping the default to `true`.** The actual measured behaviour today
(§1b, §1e) is that `required = false` does not mean "advisory"; it means
"erased". Nothing about `false` as a value is the problem; the problem is that
`false` currently means the exact same thing as "there is no record this ever
ran". A default that quietly discards evidence of an unverified comparison is
a stronger default than "advisory" implies, and a reader of `config.rs:120-122`
is told the opposite of what happens.

**Resolution.** I think this question, argued in isolation, is under-
determined, and I said so rather than picking one side to look decisive. What
is *not* under-determined is §3's requirement: once cross-variant mismatch
unconditionally taints the artifact (report, CSV, history) regardless of
`required`'s value, the stakes of the boolean's default collapse. A
`required = false` bench with a mismatch still shows up, unmissably, in
`findings.md` and in the history ledger; the process merely does not exit
non-zero. At that point defaulting to `false` is a defensible, ordinary
"advisory by default, opt into a hard gate" choice, and it is genuinely
advisory rather than a euphemism for silent. **My recommendation is: fix the
recording first (§3, mandatory, independent of this question), keep the
default `false`, and treat that as the answer**, because it delivers everything
the "flip to true" argument wants (nothing goes unrecorded) without the
disruption the flip would cause.

Named for completeness, since the brief asked what changes for existing
consumers under either path: flipping the default to `true` without the
recording fix would immediately fail CI for every existing bench whose
cross-variant check has ever silently mismatched (which per the committed
artifact evidence at `vehje/mock/benches/results/carrier_entgrid/*.csv`,
cited by the source memo at §6, contains all nine deliberately-different
`entgrid` arms at every point with neither `may_differ` nor `required` set,
meaning at least one consumer's bench is currently relying on exactly the
silence a flipped default would remove), and it would do so for reasons ranging
from "this bench genuinely needs `outputs_may_differ = true` and nobody has set
it" to "this comparison is transiently flaky under calibration per §4". That
disruption is real and I would not recommend eating it as a side effect of a
default flip; if the framework wants tighter defaults later, it should follow
the recording fix by enough time that consumers can see, in their own history
ledgers, which of their benches would newly fail, and fix or annotate those
deliberately rather than discovering it via a broken CI run.

## 6. What the generator must be able to emit

Per §1c, this is two requirements, in order, and the first is not optional
scaffolding for the second, it is a hard prerequisite.

1. **`MatrixDecl` and `bench_matrix!` need fields for the four keys.** At
   minimum: a per-family (or, per §3 of the earlier "what the consumer should
   write" memo's finding #5, per-sweep-value override of a family default)
   `may_differ: bool`, `required: bool`, `threaded: bool`, and an optional
   timing override matching `TimingOverride`'s shape
   (`config.rs:205-211`: `passes`, `runs_per_pass`, `batch_size`,
   `harness_runs`, `cooldowns_ms`, all optional). Per-sweep-value granularity is
   the right default given that a `MatrixDecl` already expands into one
   `[bench.*]` section per sweep value (`generate.rs:113-124`), and each such
   section is exactly where a validation policy or timing budget legitimately
   differs (a sweep adding a deliberately divergent arm is a property of that
   sweep, not of the whole family).
2. **`render_bench_section` emits them when present.** Once the data exists in
   `Composition`/`MatrixSpec`, threading it into the `[bench.<name>]` block
   alongside `title`/`workload`/`master_seed` (`matrix.rs:285-288`) is
   mechanical, symmetric with how `normalise`/`floor` are already conditionally
   emitted (`:294-298`).

Neither of these, on its own, closes the gap identified in §1d for
`routine_for`-served benches. That gap is not a generator gap at all; it lives
entirely in `resolve_routine`'s hook-first ordering (`driver/mod.rs:125-126`)
and in each consumer's hand-authored `Routine` impl, and no change to
`bench.toml`'s schema or to what the generator writes into it can reach a code
path that never reads the manifest. I want this stated plainly rather than
implied, because a design that "fixes the generator" and stops there will look
complete and will have addressed only the `bench-matrix`-consumer slice of the
corpus, leaving arvo's entire routine population (100 percent `routine_for`,
per the earlier memo) exactly as unreachable as it is today.

## 7. What the framework should refuse to run

Two different refusals, at two different layers, because §1d established that
one layer cannot see the other's failure class.

**At the manifest/config layer: refuse a bench with two or more variants and no
declared validation policy at all**, rather than silently defaulting
`may_differ = false, required = false` and proceeding. Concretely: require
`may_differ` to be set explicitly (not merely defaulted) whenever `variants`
has two or more distinct entries and the routine is *not* dispatched via
`byte_routine_dispatch!` with the manifest key actually wired (which the
harness itself can determine, since `resolve_routine` already knows which path
served the routine, `driver/mod.rs:119-147`). This is a real refusal because it
is checkable mechanically and it targets exactly the ambiguity that currently
resolves itself silently to "byte-exact, non-fatal" without anyone having
decided that on purpose. It does not, and cannot, close the `routine_for`
sub-case (§1d): the manifest cannot refuse what it cannot see.

**At the `Routine` trait layer: a `routine_for`-served bench with the default,
unoverridden `validate_output` and the default `outputs_may_differ() = false`
is not something the manifest schema can refuse, but it is something worth
naming as a distinct, weaker guarantee rather than silence.** I would not
propose a compile-time refusal here (a blanket "every custom `Routine` must
override `validate_output`" is a real cost for routines that genuinely have no
structural invariant worth checking, and forcing an author to write a no-op
override to satisfy a lint is exactly the kind of policing this framework
should not do to its consumers). What I would refuse, at the point the
`Routine` is resolved, is running a *cross-variant* comparison at all for two
or more arms sharing a `routine_for`-served type that has never had
`outputs_may_differ` reasoned about, i.e. the same explicit-declaration
requirement as the manifest case, expressed as a `const` or associated item on
the trait rather than a manifest key, since that is the only place this
sub-population's decision can actually live (§1d). This is a larger, separate
change to `Routine`'s contract and I would not fold it into this round without
a second pair of eyes on whether it is worth the churn to every existing
`impl Routine`; I am naming it because the brief asked specifically what the
framework should refuse, and the honest answer includes a case the manifest
cannot reach.

**What I would not refuse:** a cross-variant mismatch itself. §3 already
settled that a mismatch is not an error the framework should reject running;
it is a fact about the arms that the framework must record and must not
launder into an ordinary comparison. Refusing to *run* on a mismatch would
throw away the very information (which arm, on which seed, by how much) that
makes the mismatch actionable; refusing to *report it as valid* is the correct
strength of refusal, and that is what §3's tagged-artifact requirement already
does.

## 8. What a verdict must record, for topic 3 to build storage against

Not my subject, and I am not proposing where this lives. For my semantics
above to be checkable later, a verdict needs to carry, per (bench, size) cell,
at minimum:

- which check ran (`per_variant` only, `cross_variant` byte-exact, approx with
  its `eps`, or none because of consent) and against what plan
  (`ValidationPlan`'s two fields, `validation.rs:63-68`, already name exactly
  the right shape for this);
- for a structural failure: which arm(s), which seed(s), and the routine's own
  reason string (`check_each_variant` already produces this, `:107-126`, and
  currently throws it away past the first one);
- for a cross-variant mismatch: which arm(s) disagreed with the baseline,
  which seed(s), and (for approx) the measured relative error, not merely "a
  mismatch occurred" (the same discard problem, `:481-490`);
- whether the cell's artifacts (CSV rows, report entry, history entry) are
  therefore tagged unverified, per §3, and this tag must be attached to the
  *specific* cell, not to the whole bench or the whole run, since a sweep with
  many size points can have some points agree and others not (arvo's own
  `bitpack-write-contend-race`, cited in the source memo's §3a, is exactly this
  shape: one arm dropped at four of its points and not the others);
- whether `required` escalated this to a process failure, kept separate from
  whether the comparison was valid (§3's third, independent knob).

Whatever storage topic 3 designs, a verdict that cannot express "arm A failed
structurally at seed 7" versus "arm A disagreed with arm B at seed 12" as two
different facts has re-created the exact collapse this file spent most of its
length undoing.

## 9. What I would not change

The subprocess-per-variant isolation for validation
(`run_worker_validate`/`validate`'s worker spawn, `harness.rs:801-839`,
`validation.rs:379-394`) is sound and I would not touch it: it is what keeps a
crashing or slow variant from taking down the whole validation pass, and it is
what makes the reps-1 environment-variable fix in §4 cheap (the variable
already crosses a process boundary that exists for other reasons). The
cdylib isolation, the shared workload, and the artifact trail generally, named
in the brief as load-bearing, I did not find any reason to question and did
not attempt to. The disassembly duplicate-detection warn-only precedent
(`driver/mod.rs:384-391`, `disasm::check_duplicates` returns `()`,
`disasm.rs:306-318`) is a good model for a *different* class of defect
(fairness: two arms compiling to identical code) and I would keep it warn-only,
deliberately, because unlike a correctness mismatch there are legitimate
reasons for two variants to compile identically (testing that an optimisation
barrier held). I would not generalise that precedent to cross-variant
correctness mismatches; §3 already explains why those are not a human-taste
call in the same way.

## 10. What I could not settle

Whether extending real digest computation to every `timed!`/`timed_calibrated!`
call site (closing the partial-coverage gap in §1a/§4 completely, rather than
scoping digest to its current `bench-matrix`-only reach) is worth the cost. It
would touch how every hand-written variant is authored across four consumer
trees, which is a larger blast radius than anything else in this file, and I
did not attempt to price it; it wants its own round with its own
`benches/`-harness measurement of what the extra fixed-seed pass costs per
variant, not a guess from me.

Whether the `Routine`-trait-level refusal I named in §7 (an explicit
declaration requirement for `routine_for`-served cross-variant comparisons) is
worth its churn against every existing `impl Routine`. I named the shape
because the manifest genuinely cannot reach this case, not because I have
weighed the migration cost against the benefit.

Whether `carrier_entgrid`'s committed nine-arms-at-five-points CSV
(`vehje/mock/benches/results/carrier_entgrid/*.csv`, cited by the source memo)
actually reflects a run that mismatched. The code path that would let it be
reported anyway regardless of a real mismatch is established in this file
(§1b); whether that specific run's stderr said `VALIDATION ERROR` is not
recoverable from the committed artifacts and I did not chase it further, since
it is outside this repository and the standing rule keeps me read-only there.

Exact wording or line-level precision for anything in the two other topics'
territory (driver-table restatement, point-list triplication, consumer
ergonomics); I read both source memos in full to place my subject correctly
and did not re-derive their conclusions.
