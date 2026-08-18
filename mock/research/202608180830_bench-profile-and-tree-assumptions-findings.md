# The profile dimension is missing everywhere it matters, and the flag that would supply it never arrives

Branch `fix/bench-profile-and-tree-assumptions`, off `155`'s `fix/bench-test-subcommand` at
`4ad6d34`. Four fixes, six probes, nine new tests. Not opened as a PR.

## The gates, run first

**Test gate.** `cargo test --workspace` on `155`'s branch before touching anything:

```
grep "^test result" <log> | sed -E 's/.*ok\. ([0-9]+) passed.*/\1/' | paste -sd+ - | bc
647
```

**647 passed, 0 failed, 22 ignored, across 30 test binaries, debug profile** (`cargo test`'s
default, and after what follows I am not going to quote a test figure without naming that again).
That confirms the coordinator's 647 independently rather than inheriting it. After my four fixes
the same command gives **656 passed, 0 failed**: the baseline plus nine tests I added.

I read the bodies of the tests in the surface I touched rather than their names:
`src/entry/dispatch.rs`'s test module in full, `src/bench.rs`'s `#[cfg(test)]` module in full
including everything `155` added, and `tests/real_trees.rs`. None is decorative. Two things are
worth naming as unusually good, because the gate obliges me to be as specific in praise as in
criticism: `the_build_argv_carries_the_profile_and_the_json_format` carries a comment saying the
test it replaced "asserted the constant against itself and never reached a command", and
`tests/real_trees.rs` panics rather than returning when its precondition is missing, with its
own header explaining that a skip which reads as a pass "is how a gate stops being one". Both are
the discipline this dispatch is about, applied by the authors to themselves.

**Canon gate: passed.** No ratified canon governs mockspace's own source; `mock/` here is v2-only
and parked, so v1 source is ungated and edits need no design round. I checked rather than
inherited `155`'s reading: `mock/design_rounds/` carries no flat active round, and the two
research documents governing the bench rework both say in their own text that they are proposals
the maintainer ratifies. Nothing I changed contradicts them.

## Part one: attacking `155`

### The locus calls are right, and one of them is right for a reason `155` does not give

**`0 passed` is mockspace's: agreed, and I verified the mechanism rather than the claim.** The
shape is mandated by the scaffolding, not chosen by arvo, so no consumer could have avoided it by
writing its manifests differently. `155` is correct and the fix belongs where it put it.

**The assert-nothing stress test is arvo's: agreed.** Its reasoning is sound and its placement is
wrong, and one `#[ignore]` fixes it in arvo.

**The pool soundness bug is arvo's, and I established it rather than assuming it.** The
coordinator asked whether anything in mockspace's scaffolding, generated driver, or documented
convention leads a consumer to the shape at
`bitpack-write-contend-shared/src/pool.rs:110-111`. It does not:

```
grep -rn "std::thread\|spawn(" --include='*.rs' src/  ->  three hits, all process spawns in render_agent
```

mockspace scaffolds a `lib.rs` with a run block and a manifest; it generates no threading, no
pool, and no worker. It has a `threaded = true` manifest knob and a documented threading contract
about pinning (`bench-harness/src/config.rs:250-254`), and neither describes a pool shape. arvo
wrote that pool unaided. **Locus: arvo.**

### But point 3 is wrong, and it is wrong in the way this whole dispatch is about

`155` section 3 says the slowness is "a compound of the above two, not a third thing", and that
once arvo's three `stress::` tests are `#[ignore]`d, "the default run is fast by construction".

**It is not.** `wide-rung-shared` has no stress tests, no threads, and no relationship to
`bitpack-write-contend-shared`. Measured directly, on a warmed target directory, one host, back
to back:

```
cargo test              30 passed, finished in 133.72s
cargo test --release    30 passed, finished in   4.99s
```

**27x, and the whole of it is the profile.** `#[ignore]` on arvo's stress tests changes this by
nothing. The slowness is a third thing, it is the default profile, and attributing it to the
other two is the same missing dimension that produced the incident this dispatch exists to chase.
`155` reached the right conclusion about two of three items and misattributed the third for
exactly the reason the third item is interesting.

### Three defects in the fix itself

**D1. `mock bench test --release` ran a debug pass and reported it as a success.** This is the
serious one and it is not `155`'s fault alone; `155` inherited it. The command builds an `extra`
list of `--`-prefixed arguments and forwards them to cargo, and **it can never receive one**. The
dispatcher builds `bench_args` from `positional_args` (`src/entry/dispatch.rs:416` at `4ad6d34`),
and `positional_args` drops every flag at `src/entry/dispatch.rs:193` at that same commit:

```rust
if arg.starts_with('-') { continue; }
```

Observed under a cargo shim, `mock bench test --release`:

```
cargo argv actually used: test --manifest-path /private/tmp/fxT/mock/benches/support/alpha/Cargo.toml
```

No `--release`. The flag is accepted, discarded, and the run reports a pass. The same is true of
`cmd_run`'s and `cmd_report`'s `extra` lists, which have been unreachable through the CLI for as
long as the dispatcher has filtered this way, so **three forwarding mechanisms exist and none of
them can fire.** Probe `04`, with a two-layer control: a positional argument is shown to arrive
(`mock bench run <unknown>` produces the "not found in bench.toml" refusal), so "flags are
dropped" is distinguishable from "nothing arrives".

Priced through the command on a real crate, warmed, probe `03` before the fix:

```
arm 1  mock bench test            127s   30 passed
arm 2  mock bench test --release  135s   30 passed
arm 3  cargo test --release         5s   30 passed
```

**Fixed.** `subcommand_args` in `src/entry/dispatch.rs` takes everything after the subcommand,
flags included, while still consuming the three value-taking globals and their values so a path
cannot leak into a subcommand's argv. Five tests, including one asserting that the positional
filter and the argument vector are genuinely different lists, and one asserting a flag *before*
the subcommand is still the tool's.

The same three arms after the fix, same crate, same host, warmed:

```
arm 1  mock bench test             92s   30 passed
arm 2  mock bench test --release    5s   30 passed
arm 3  cargo test --release         4s   30 passed
```

Arm 2 tracked arm 1 before and tracks arm 3 after, and the 23x between arm 1 and arm 3 is what
keeps that from being a vacuous assertion: the two are far enough apart that tracking one rather
than the other is unambiguous.

**D2. A crate name handed to `mock bench test` was silently discarded.** Probe `02`: naming a
crate that exists, naming one that does not, and naming nothing all produced byte-identical
output. So a typo or a stale name ran the whole tree and reported a clean pass over crates the
caller had not asked about. Its sibling ten lines away already refuses this:
`variant_dirs_for` returns ``bench `{name}` not found in bench.toml. Available: ...``. The new
command dropped a refusal the file already had. **Fixed**, with the same shape of refusal.

**D3. The acceptance check was never run, and it does not pass.** `155` says plainly that it did
not run `mock bench test` against arvo's real tree. I did. Probe `05`, on a copy of
`bitpack-write-contend-shared` and its path dependency:

```
arm A  mock bench test                          DID NOT COMPLETE in 240s
arm B  cargo test -- --test-threads=1           completed in 6s, 15 passed
```

The hang is arvo's bug. What was mockspace's is that the one-word workaround **could not be
expressed**, because of D1. After the fix:

```
arm C  mock bench test -- --test-threads=1      completed in 7s, 20 passed
```

So the command can now run the tree it was written for. I want to be exact about the credit here:
`155`'s command is the right mechanism and it was two defects away from working on its target.

**And one thing I did not fix.** `cmd_test` captures each crate's output with `.output()` and
prints only on completion, so during arm A's hang the consumer sees **nothing at all**, not even
which crate is stuck: probe `05` records `stalled-test notices: 0` while cargo was printing "has
been running for over 60 seconds" into a buffer nobody would ever read. Streaming is a larger
change than I wanted to make on someone else's branch and I am naming it rather than doing it.

## Part two: the profile dimension, upstream

The coordinator asked whether the harness records the profile, whether `mock bench test` inherits
the hole, and whether anything downstream can tell a debug number from a release one. The answers
are: partly, yes, and no.

### The harness has the mechanism and nothing consumes it

`bench-harness/src/harness.rs:45` defines `MOCKSPACE_BENCH_PROFILE`, and `env_meta_to_json` writes
a `build_profile` field into every `.meta.json` from it. Its doc is careful and its reasoning is
right: when the variable is absent, "the field is omitted: no claim beats a wrong one."

**But nothing ever reads it.**

```
grep -rn "build_profile" --include='*.rs' . | grep -v /target/
```

Six hits: the writer, its formatter, and two assertions in the writer's own unit test. No
consumer, no validation, no report surfaces it, and nothing refuses an artifact that lacks it. It
is a **write-only record**, which is the test gate's "declaration nothing constrains" moved out of
the type system and into the artifact: it could carry any value, or none, and every downstream
behaviour is identical.

### And in the field it is absent from every artifact

arvo's committed bench artifacts are the only thing in that repository that can price anything,
and they are what the numeral canon panel quoted for a year:

```
ls *.meta.json | wc -l                          254
grep -l "build_profile" *.meta.json | wc -l       0
grep -L "build_profile" *.meta.json | wc -l     254
```

**Zero of 254.** A sample:

```json
{"cpu":"Apple M1","os":"Darwin 25.5.0","rustc":"rustc 1.98.0-nightly (57d06900f 2026-05-27)",
 "git_commit":"f32abe4-dirty","timestamp":1786185196,"counter_freq":24000000,
 "framework":"mockspace-bench-harness"}
```

The chain that produces this is three links and each is individually defensible:

1. The env var is set only where the tool spawns the driver.
2. `cargo mock bench run` is known-broken for arvo (it hardcodes a binary name arvo does not use,
   recorded in arvo's `96` and still unfixed), so arvo runs its driver directly, which is the
   documented escape hatch rather than a misuse.
3. On that path the field is omitted, deliberately, and **nothing anywhere refuses or warns**.

So every bench number arvo has ever quoted is unpredicated on the dimension that, in the same
repository, retired a true finding. `OPTIONS.md` Q52 ends: *"Retired: the claim that
`wide-rung-shared` takes 107s. Three measurements now put it at 4.05s, 4.25s and under 5s. Dropped
rather than carried as contested."* All three were release, the 107s was debug, and the retirement
is wrong. **That is a fact about arvo's bookkeeping and this is the harness-side twin of it.**

**I did not fix this**, and I want to say why rather than leave it looking overlooked. Making
something refuse an artifact with no `build_profile` changes what a valid artifact is, and 254
existing ones would become invalid at a stroke. That is a design call about artifact semantics
with a migration attached, and it is not mine to make on a feature branch. The shape I would
propose, for whoever does make it: the field becomes mandatory in newly written artifacts by
having the harness record what it can determine on its own (`cfg!(debug_assertions)` at minimum)
rather than only what the tool tells it, so the escape-hatch path stops being silent; and the
report path names the profile of the artifacts it read. Both are small. The decision that they
should happen is not mine.

### `mock bench test` inherited the hole, and that half I did fix

The new command printed `N passed` with no profile anywhere in its output, and had no way to
select one (D1). Both are closed. The summary now reads:

```
1 crates, 1 carrying tests, 1 passed, 0 failed, 0 ignored  [cargo test, profile: debug (cargo's default)]
1 crates, 1 carrying tests, 1 passed, 0 failed, 0 ignored  [cargo test --release, profile: release]
```

reported from the flags actually forwarded rather than from a constant, so it cannot drift from
what ran, with a test and a control asserting that the default does not claim release and the
release form does not claim debug.

## Part three: the second instance of `155`'s category

`155` named the category and did not find a second instance. Here is one, and it is inside the
surface the rework just touched.

**A `[build]` override is honoured on the generated-driver path and silently dropped on the
consumer-owned one.**

`BuildSection` is a section of `bench.toml`, declarable by any consumer, and its own documentation
makes the promise this violates (`bench-harness/src/config.rs:147-150`):

> Release profile overrides. The tool passes the effective values on the command line
> (`--config`), **where a manifest cannot silently drop them**; these keys move the values, they
> do not relocate the mechanism.

The mechanism, at source. `profile_args_for` computes the effective flags from `[build]`, and had
exactly one production caller, at `src/bench.rs:764` as of `4ad6d34`, inside `run_generated`. Every build on the
consumer-owned path went through `build_argv`, which **took no config parameter at all** and
extended its argv from a `PROFILE_ARGS` constant. So the override could not reach that path; it
was not ignored by a branch, it was never parsed.

Probe `01`, two fixtures declaring an identical `[build] opt-level = 0`, differing only in whether
`mock/benches/Cargo.toml` exists, cargo shimmed to log its argv:

```
declared in both fixtures : profile.release.opt-level=0
fixture A (consumer-owned): profile.release.opt-level=3      <- the declaration dropped
fixture B (generated)     : profile.release.opt-level=0
```

**This is the `[timing]` defect exactly.** The brief lists, among what the rework already found
and fixed, "a member declaring part of `[timing]` silently resetting every undeclared knob to a
framework default". Same shape, different section, on the path the earlier fix did not cover, and
arvo's tree is on that path.

**Why it survived the tests.** `the_profile_env_value_mirrors_the_flags` asserts that
`profile_args_for` reflects an override, and it passes, and it is true. It calls the function
directly and never asks which build path calls it. The repo's own end-to-end test,
`tests/bench_generated_e2e.rs:99-103`, exercises a `[build]` section with `opt-level = 0` against a
real run,
**on the generated path only**. So the covered path is the one that works and the uncovered path
is the one that does not, which is not a coincidence: the fixture that exists is the one whose
shape the test author was building.

**Fixed.** `build_argv` and `cargo_build_json` take the effective profile; `consumer_tree_profile`
reads the tree's own `bench.toml` and reports rather than swallows a parse failure; both consumer
call sites use the same vector for the builds and for the driver's env, so the record cannot
diverge from what was built. `PROFILE_ARGS` is deleted, because there is now one source of the
profile instead of two that agreed until a consumer declared something; its reasoning is preserved
on `profile_args_for`. Two tests, one of which is the control asserting a tree with no `bench.toml`
still gets the framework defaults.

## Part four: smaller things, reported and not fixed

**`mock bench list` from a repo root fails with an error naming the wrong thing.**

```
$ cd ~/Dev/clause-dev/arvo && mock bench list
error: io error during reading bench.toml: No such file or directory (os error 2)   [exit 1]
$ mock --dir mock bench list
bitpack-carrier-width  [16384, ...]  arms:   Packed 13-bit against u16, u32 and u64 dense carriers...
```

`cmd_run`, `cmd_report` and `cmd_test` all check `bench_dir.exists()` and say "…/benches does not
exist. Run `mock bench init` first." `cmd_list` and `cmd_add` do not, so a wrong working directory
surfaces as a confusing io error about a file the user can see exists. Cheap to fix; I left it
because it is a different surface from the one I was sent to and it refuses rather than passing.

**`extra` in `cmd_run` and `cmd_report` is now reachable for the first time.** My dispatcher fix
makes flags arrive at all three subcommands. For `cmd_test` that is the point. For the other two it
means arguments that were silently discarded now reach a driver that has never received one, and
**I did not test that path**, because I have no generated-driver tree to run end to end and
building one that measures something real was out of scope. It is the one place my change could
surprise, it is named here rather than left for someone to find, and the conservative reading is
that a driver rejecting an unknown flag is better than the tool eating it.

## What I settled, moved, and could not

**Settled.** That `155`'s point 3 is wrong and the slowness is the profile, with a 27x direct
measurement. That the pool shape is arvo's unaided, by grep over everything mockspace generates.
That `build_profile` is written and never read, by exhaustive grep. That it is absent from 254 of
254 real artifacts, by count.

**Moved.** The category `155` named now has a second instance with a fix, a test and a control.
`mock bench test` can now run the tree it was written for, which it could not when it landed.

**Could not.** I could not close the artifact half of the profile problem, and I do not think it
should be closed by an agent on a feature branch: making `build_profile` mandatory invalidates 254
committed artifacts and changes what the report path may assume. I have proposed a shape and said
plainly that the call is not mine. I also could not test the newly-reachable `extra` on the
generated driver path.

## Coverage, bounded

**Read in full:** `155`'s findings document, `src/bench.rs`'s `cmd_test`, `cmd_run`, `cmd_report`,
`build_argv`, `cargo_build_json`, `profile_args_for`, `profile_env_value`, `variant_dirs_for` and
the whole `#[cfg(test)]` module; `src/entry/dispatch.rs`'s argument handling and test module;
`bench-harness/src/harness.rs`'s env-meta section; `bench-harness/src/config.rs`'s `BuildSection`
and the manifest fields around it; `tests/real_trees.rs`.

**Grepped, not read:** the rest of `bench-harness` and all of `bench-core`, `bench-macro`,
`bench-matrix`; `src/bench_gen.rs` beyond `plan`'s signature; every lint rule.

**Not opened:** `mock/` here (v2, parked), the two governing research documents beyond confirming
`155`'s quotations of them, `mockspace-manifest`, and the `feat/bench-round-consolidation` round
the brief named as in flight, which I stayed off deliberately.

**What would move if I am wrong about something I leaned on.** The `[build]` finding rests on
`profile_args_for` having one production caller, which is a grep I ran and can be re-run in one
command; if a second caller exists on the consumer path that I missed, probe `01`'s fixture A
would have shown `opt-level=0`, and it showed 3. The pool locus rests on a grep over `src/` for
generated concurrency; if mockspace ships a threading example somewhere I did not look, the answer
changes from "arvo unaided" to "arvo following a documented shape", and the fix moves repository.
The 254-of-254 count is a fact about arvo's committed tree at the commit I read it, and would
change the day anything runs through a tool-spawned driver.
