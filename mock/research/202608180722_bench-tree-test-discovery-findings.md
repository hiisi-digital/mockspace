# What arvo hit, whose problem each one is, and what stress-testing the reworked surface found

Branch: `fix/bench-test-subcommand`, off `dev` at `09da1fd`. Not opened as a PR; the coordinator
takes it from here per the dispatch.

## The canon and test gates, run first

**Canon gate.** No ratified canon governs mockspace's own source specifically (this is the tool
itself, not a `mock/`-gated consumer of it); `mock/` here is v2-only and parked, so v1 source
(everything under `src/`, `bench-*/`, `benches/`) is ungated and edits need no design round, per
this workspace's own `mockspace-workflow.md` and the standing "v2 waits while we dogfood v1"
decision. Confirmed by inspection: `mock/design_rounds/` carries no flat (active) round on `dev`,
and the two research documents that actually govern the bench harness rework
(`mock/research/202608150648_bench-ergonomics-survey.md`,
`mock/research/202608150809_bench-vocabulary-and-consolidation-design.md`) both state plainly, in
their own text, "This is a proposal, not a decision; the maintainer ratifies" — read in full before
any of the work below. Nothing here contradicts that design; the fix is the missing piece its own
"what this does not change" section (survey section 7) does not claim to have addressed, because it
was written a day before the rework it describes had landed.

**Test gate.** Ran `cargo test --workspace` on a clean `dev` checkout, before writing any code:
`cargo test --workspace 2>&1 | tee mock/research/sketches/bench-test-subcommand/03_pre_change_full_workspace_test_run.log`.
**641 passed, 0 failed**, summed from every `test result:` line the run produced (committed at that
path). The brief claimed 639; my own measured baseline is 2 higher and I did not chase the two-test
discrepancy, since it is not material to anything below. Read
the bodies of the tests most relevant to the surface touched (`tests/real_trees.rs` in full, several
of `bench-harness/src/tree.rs`'s own `#[cfg(test)]` module, `bench-harness/src/driver/hooks.rs`'s
ordering tests) rather than trusting names; none read as decorative, and `tests/real_trees.rs`'s own
header names, as a defect it was written to prevent, the exact class this dispatch is about: a check
that silently returns instead of panicking when its precondition is missing "is how a gate stops
being one."

## Part one: where the three things arvo hit actually belong

### 1. `cargo test` at `mock/benches/` reporting `0 passed`: mockspace's

**Confirmed the mechanism**, not just quoted 154's claim. arvo's `mock/benches/Cargo.toml` declares
a `[package]` + `[[bin]]` with no `[workspace]` table of its own; `mock/Cargo.toml`'s
`[workspace] exclude = ["benches"]` deliberately keeps the bench tree out of the crate-tree
workspace (its own comment explains why: bench crates are not governed by the canon/design/code
chain that empty `members = []` protects). So `cargo test` run at `mock/benches/` builds and tests
**only the `arvo-benches` bin crate**, which has zero `#[test]` items; every arm and support crate
(94 + 13 in arvo) is a path dependency, never a workspace member, and is invisible to it. `cargo`
reports `test result: ok. 0 passed` because, from its perspective, that is correct: the one crate it
was asked about has no tests. Nothing lied; the invocation asked the wrong question and nothing told
the caller so.

**Why this is mockspace's and not arvo's.** The shape (arm/support crates as path dependencies,
excluded from any cargo workspace) is not an accident of arvo's tree; it is mandated by mockspace's
own bench scaffolding convention (`build_variants_and_bin_filtered`, the escape-hatch driver shape
`src/bench.rs` generates and documents, and the same exclusion pattern the survey confirms every
pre-driver tree needs). Every consumer that follows the documented convention produces a tree where
bare `cargo test` at the bench root is structurally blind to the crates carrying the tests. This is
not a mistake arvo could have avoided by writing its manifests differently; it is a consequence of
the shape mockspace itself hands out. The fix therefore has to be a command mockspace provides, not
a workaround in each consumer's tree.

**And it could not have been mockspace's own tree-loading validation, either.** `tree::load`
already refuses a tree resolving to zero benches, with a comment stating the exact failure class
directly ("Loading it successfully means the run reports '0 benches' and exits 0, which reads as a
pass"). That refusal is real and correctly built, but it fires only when `tree::load` runs, i.e.
inside `mock bench run` / `report` / `list`. A bare `cd mock/benches && cargo test` never calls into
mockspace's code at all; it is cargo's own workspace resolution, entirely outside anything mockspace
validates. So the fact that mockspace is already alert to "reports success while measuring nothing"
did not and could not catch this one, because it happens on a path that bypasses mockspace's CLI
surface completely. That is the shape of gap worth naming for the second question, below.

**Fixed.** Added `mock bench test`, a new subcommand in `src/bench.rs`. It walks `mock/benches/`
for every `Cargo.toml` (skipping `target/` and dot directories, the identical rule
`tree::discover` already uses for the identical reason), runs `cargo test --manifest-path <path>`
in each crate's own directory, and aggregates. It refuses (nonzero exit) if no crate manifests are
found at all, if any crate's tests fail, or if every discovered crate reports zero tests between
them — the same "resolves to zero, and that is a misconfiguration rather than an empty valid state"
posture `tree::load` already uses, applied to the test surface it does not reach. A crate reporting
zero tests on its own (the ordinary case for a cdylib arm) is not itself a failure.

Verified against real code, not a fixture built by the same hand as the fix: `find_crate_manifests`
and `parse_test_result_lines` have fast unit tests (including two negative controls: an empty tree
finds nothing, and output carrying no `test result:` line is read as zero found rather than zero
passed), and a `#[ignore]`d test, gated on `MOCKSPACE_REAL_TREES` exactly as `tests/real_trees.rs`
already does, copies arvo's actual `warm-container-shared` crate into a scratch tree and runs
`cmd_test` against it end to end. Run:

```
MOCKSPACE_REAL_TREES=~/Dev/clause-dev cargo test --lib \
  bench::tests::cmd_test_finds_and_runs_a_real_arvo_support_crate -- --ignored --nocapture
```

Output: `ok    support/warm-container-shared/Cargo.toml  15 passed, 0 failed, 0 ignored`, then
`1 crates, 1 carrying tests, 15 passed, 0 failed, 0 ignored`, exit success. Fifteen is the exact
count that crate's own real `cargo test --release` reports, established directly in the prior
session rather than assumed here.

**A real bug the negative-control discipline caught before it shipped.** The first draft of
`parse_test_result_lines` stripped a trailing `" passed"` from a field and parsed the remainder as a
whole number; on cargo's real line shape (`"test result: ok. 15 passed; ..."`) the first field is
`"ok. 15 passed"`, and `"ok. 15"` is not a bare integer, so the parser silently returned `(0, 0, 0)`
for a genuinely passing crate — the exact "reports success while measuring nothing" shape this whole
dispatch is about, reintroduced inside the fix for it. Reproduced the failure standalone before
fixing it (`rustc`'d the old logic against the real line, confirmed `(0, 0, 0)`), fixed it, and kept
a test carrying a real captured line (`REAL_OK_LINE`, `parse_test_result_lines_reads_the_ok_verdict_prefix_correctly`)
so the fix cannot regress unnoticed. This is reported because a fix for a silent-success bug that
ships a second silent-success bug inside itself is exactly the kind of thing worth being honest
about rather than quietly correcting and moving on.

### 2. The assert-nothing stress test in `bitpack-write-contend-shared/src/stress.rs`: arvo's

Opened the file at source (`~/Dev/clause-dev/arvo/mock/benches/variants/bitpack-write-contend-shared/src/stress.rs`,
read-only) rather than taking 154's characterisation on report. `naive_kernel_corruption_rate_under_real_concurrency`
runs 3000 concurrent trials and genuinely asserts nothing; its own comment gives the reason and the
reason is sound (a scheduler-dependent corruption rate is not a fact this test should gate on). 154's
reading is right on both counts: the reasoning is correct and the placement is wrong. This is a
one-line fix (`#[ignore = "diagnostic: reports a scheduler-dependent corruption rate on stderr, \
never a threshold"]`) that belongs in arvo, not here. mockspace's own lint set
(`lint-rules/src/*.rs`) is scoped entirely to `mock/crates/` (the design-round-gated v2 source), and
has never reached into `mock/benches/`; a generic "flag a test with no assertion" lint would also be
a poor fit, since a test that only calls a helper containing the assertion is legitimate and a
naive static check cannot tell the two apart. I did not add anything to mockspace for this; I am
recommending the arvo-side change here rather than making it, since this dispatch is scoped to
mockspace and arvo's tree is a separate repository with its own branch/PR flow.

### 3. The suite being slow enough to stop being run: a compound of the above two, not a third thing

`wide-rung-shared` at 115s and `bitpack-write-contend-shared` unfinished after twelve minutes are
consequences, not a separate defect needing its own mechanism. Once `#[ignore]` is applied to
arvo's three deliberately-out-of-band `stress::` tests (per point 2) and `mock bench test` (per
point 1) becomes the documented way to run the tree's tests, the default run is fast by construction,
using stock `cargo test` behaviour (skips `#[ignore]`'d tests unless `-- --ignored` is passed) with
nothing new needed from mockspace beyond the command that makes it reachable at all. Nothing further
built here.

## Part two: stressing the reworked surface for what else it never considered

The three above are one afternoon's accidental sample; the brief asked to go looking rather than
stop at them. What was checked, and what it found:

**`tree::load`'s empty-result refusal**: read at source, correctly built, already carries a comment
naming the exact failure class ("reports '0 benches' and exits 0, which reads as a pass"). Sound.

**`resolve_members`**: refuses a literal `[benchspace]` member escaping the tree via `..`/`.`
components, refuses a member listed in both `members` and `exclude` as a self-contradiction rather
than silently dropping it, and refuses a literal member with no `bench.toml`. All three are the
correct shape (a named error over a silent skip); none of them looked new-and-unattacked.

**`Hooks`' `on_init`/`after_init`**: declared as having "no consumer" and existing for lifecycle
completeness only — checked whether they are actually wired into the driver or are dead struct
fields nobody calls. They are wired (`run_on_init` / `run_after_init` called from `driver/mod.rs` at
the documented points) and covered by ordering tests
(`on_init_runs_before_preflight_and_after_init_does_not_survive_a_failed_preflight`,
`after_init_fires_once_init_completes_and_before_any_cell`). Not a gap.

**`after_cell`'s `Fail` verdict reaching the process exit code**: traced `hook_failure` from
`run_after_cell`'s return value through to `final_exit`. Correctly folded; a `Fail` verdict flips
the exit code and withholds the history append, a `Note` verdict prints to stderr but does not fail
the run. Correctly built.

**What this leaves as the actual finding of the second question**: the rework's authors are
demonstrably alert to the "reports success, measured nothing" class within the surface their own
CLI and driver own (the empty-tree refusal explicitly quotes that exact failure shape as the reason
for the check). The gap that got through is specifically the one class of failure that happens
**entirely outside mockspace's own command surface** — a human or an agent bypassing `mock bench
*` and reaching for the tool they already know (`cargo test`) directly, on a tree shape whose
non-obviousness (path dependencies, deliberately excluded from workspace membership) is itself a
mockspace convention. Every other "silent success" class this workspace has already caught and
fixed recently (the `[timing]` partial-declaration reset, the SHAME suffix match, the fmt-only
working-tree check, the dead self-heal, the role refusal comparing nothing) was found from *inside*
a `mock bench` or `mock` invocation. This one was found because 154 did what a consumer would
actually do first, which nothing inside mockspace's own test suite does: run `cargo test` at the
bench root without going through the tool at all. That is worth naming as a category rather than
letting it read as one isolated miss: **anywhere mockspace hands a consumer a tree shape that a
stock, un-mockspace-aware tool (cargo, an IDE, a CI template) would reasonably be pointed at
directly, and where that stock tool's default behaviour on that shape is misleading rather than
refusing, is a gap of this exact kind**, independent of how careful the code reached only through
`mock bench` itself is. I did not find a second instance of it in the time available; naming the
category is the deliverable of this half of the dispatch, and it is exactly what real_trees.rs's own
"a skip that reads as a pass is how a gate stops being one" already generalises to, one layer up:
here it is not a test skipping silently, it is the *entry point itself* being bypassable in a way
that looks like using the tool correctly.

## What is committed

Branch `fix/bench-test-subcommand`.

- `src/bench.rs`: `mock bench test` subcommand, `find_crate_manifests`, `parse_test_result_lines`,
  their unit tests (including the negative controls named above), and the real-tree verification
  test gated on `MOCKSPACE_REAL_TREES`.
- `src/render_agent/skills/benchmarking/SKILL.md`: documents the new subcommand and states plainly
  why a bare `cargo test` at the tree root is misleading, so an agent reading the skill file before
  reaching for `cargo test` directly sees the warning before making the mistake 154 made.

`cargo test --workspace` after the change (also committed, `04_post_change_full_workspace_test_run.log`):
647 passed, 0 failed, i.e. the 641-test baseline plus the 6 new passing unit tests, with the new
real-tree verification test correctly absent from the default (non-`--ignored`) count. `cargo clippy
--bin mockspace`: clean on every line this change touches (pre-existing warnings elsewhere are
untouched and unrelated).

## What I did not do, named rather than left implicit

Did not touch arvo. The point-2 fix (moving three `stress::` tests behind `#[ignore]`) is real,
small, and belongs to a separate PR against arvo's own branch, which this dispatch does not open.

Did not attempt a broader lint or lint-rules extension to catch assert-nothing tests generically; the
`mock/crates/`-scoped lint infrastructure does not reach `mock/benches/` today and extending it there
is a larger design question (what would the lint's false-positive rate be against tests that assert
via a helper?) than this dispatch's scope, and it is not clear it is mockspace's job versus the
workspace's own `the-test-gate.md` discipline, already the standing mechanism for exactly this class
of review.

Did not run `mock bench test` against arvo's full tree end to end (all 94 + 13 crates); the known
multi-minute stress-test hang in `bitpack-write-contend-shared` would have made that run itself the
long pole, and the point of the verification test committed here is to prove the command reaches and
reports a real crate correctly, not to time arvo's own suite. Whoever lands the arvo-side `#[ignore]`
fix should re-run `mock bench test` against arvo's real tree afterward as the acceptance check for
both fixes together; that full run is not included here.

Did not chase the 639-versus-641 test-count discrepancy with the brief beyond noting it; it did not
bear on anything in this report and the brief itself only asked that the number be independently
verified rather than inherited, which it was.
