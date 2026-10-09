# `cargo mock test` on kaski: what it selects, what it caches, what it saves

kaski's landings spent most of their time in three test suites, `kaski-render`
and `kaski-web` without and with its `editor` feature, each rerun whole on
every `cargo test` whatever had changed. Op asked for one command that every
helper and the pre-commit call, which runs the members under nextest with
timeouts and a group holding the heavy tests to one or two at a time, selects
what to run from what changed and what depends on it, skips the heavy tests
nothing they depend on has touched, and reports what was slow, all inferred
from the repository and earlier runs with no list kept by hand. This is that
command, measured on kaski.

The numbers are wall-clock rows of whole suites on one 4-core cloud container,
taken by `202610091400_probes/suite-timings/measure` through `time-suite`,
with nothing else running while a row was taken. That is an ad-hoc timing, not
the bench harness: one run per row, no arms, no noise gate. It answers how much
of a landing's wait goes away and nothing finer.

## What it does

`src/suite/` in mockspace, called from `mock test` for the members tree. Without
cargo-nextest, or with `--plain`, the members run under `cargo test` as before.

**The runner is nextest**, given its settings as a tool config file
(`--tool-config-file mockspace:<path>`, nextest's own mechanism for a tool
wrapping it), written fresh each run under `<mock>/target/mockspace-test-state/`.
A repository's own `.config/nextest.toml` still wins over it. It sets the heavy
group `@tool:mockspace:heavy` with `max-threads = [test] heavy_threads`, a
`slow-timeout` that kills a test after `[test] timeout_secs`, `fail-fast = false`
and a JUnit report, which is how each test's time and result come back.

**Selection is by fingerprint, per member.** A member's own inputs are every
file git tracks or would track under its directory (sources, tests, manifest,
and in kaski's case `kaski-render/shaders/`), plus every path outside it that
its sources name as a string literal climbing out with `../`: the
`include_str!("../../../../../content/kaski/interface/theme.toml")` and
`CARGO_MANIFEST_DIR`-joined content directories kaski's tests read. The
literals are found with tree-sitter, so a path in a comment is not one. Its
fingerprint folds in the own inputs of every local package compiled into its
tests (its own dependencies of every kind, then only what those need to
build: a dependency's dev-dependencies are not compiled into anyone else's
tests), vendored path crates included, and the lockfile, workspace manifest,
toolchain pin and cargo and nextest configuration. A member runs when its
fingerprint differs from the one its suite last passed whole at; the report says
which inputs moved. The digests are sha256 and the record is kept per flavour,
the cargo arguments other than package selection, so a `--features editor` run
keeps its own.

**A test is heavy when its last run took `[test] heavy_after_secs` or more**,
default 10 seconds. Chosen over the two other ways named in the brief: kaski
marks nothing in its source that tells a drawn test from another (they share
helper modules with tests that draw nothing, and `-- --skip drawn` misses
`tests::hud::*` and others that start a device without the word in their name),
and what a test links is decided per test binary, which in kaski holds the quick
tests of `kaski-render` beside its drawn ones, all 292 in one library binary. The timing is in hand after
one run and is right by construction about the thing that matters, which is
time. Its cost is that the first run in a fresh tree knows nothing heavy, so it
runs without the group.

**The cache is the heavy tests' last pass.** A heavy test that passed when its
member's fingerprint was what it is now is skipped and named as cached. The
fingerprint is the cache key, so the key covers exactly what selection covers:
the member's sources and those of what it depends on, its shaders, and the
content its sources name. The controls in `src/suite/fingerprint_tests.rs`
touch each of those alone (a dependency's source, the member's own, a shader, an
`include_str!`-ed content file, a content directory read at run time, a
vendored crate, a crate outside the repository) and assert that exactly the
members reading it move; content nobody names, a path in a comment and ignored
build output move nothing. `tests/mock_test_selects_what_changed.rs` walks the
same through the real binary and nextest.

**Modes.** `cargo mock test` runs what changed, cache on. `--full` runs every
member asked for, cache still on. `--no-cache` runs cached heavy tests too, so
`--full --no-cache` is the whole suite. `--cheap` also defers every heavy test
that has passed before in this flavour, running only new ones and ones that
failed last time, and leaves the members it deferred owing them, so the next
ordinary run picks them up. `[test] on_commit = "cheap"` runs `mock test
--cheap` before a commit is linted.

**What is recorded.** A member is recorded green at its fingerprint only when
nextest ran to the end (success, or test failures, not a build failure or an
interrupt) with no filter of the caller's narrowing it, none of its tests
failed and none of its heavy tests was deferred. Each test's time and result is
recorded whatever happened. Everything lives in
`<mock>/target/mockspace-test-state/history.json`; a `cargo clean` or a fresh
clone runs everything once.

**What it cannot see**, and so where a test can be skipped that should have run:
a path built at run time from pieces none of which climbs out alone
(`join("..").join("content")`), an environment variable, a file outside the
repository that no local package contains, a GPU driver or SwiftShader update.
kaski's tests name their content as whole literals, so none of these is a
case in kaski today; `--full --no-cache` is the answer to all of them.

## The numbers

Taken so far, at kaski `2ea49fc5`, with `202610091400_probes/suite-timings/measure`
(the first two phases ran through an equivalent script before `measure` was
written; the commands are the same):

| row | what | wall | tests |
|---|---|---|---|
| `before.build.render` | `cargo test -p kaski-render --no-run`, cold | 572s | |
| `before.build.web` | `cargo test -p kaski-web --no-run` | 1217s | |
| `before.build.web-editor` | the same with `--features editor` | 226s | |
| `before.none.render` | `cargo test -p kaski-render`, built, nothing changed | 1586s | 292 passed |

The build rows overlap mockspace compiling at `nice 19` on two jobs, so they
read high; the run row had the machine to itself. 17 of the 292 render tests
were still running after 60 seconds under libtest's default parallelism.

The rest of the sequence, the `kaski-web` baselines and every `after.*` row,
was stopped on op's word to land the work before spending hours on suites. It
is one command once this has landed:

    measure /home/user/kaski <mockspace checkout> before-none before-change \\
        after-first after-none after-change after-full after-cheap

What each `after` phase should show, from the end-to-end test rather than from
kaski: after none, nothing runs and the command takes the seconds `cargo
metadata` and the fingerprint take; after a change in `kaski-mesh`, both
suites run, their heavy tests in the group; a `--full` after that skips every
heavy test as cached; a `--cheap` pass defers them.

## What is still slow, and why

- **A change to anything a suite depends on still runs that whole suite**,
  drawn tests included, since the cache key is the member's fingerprint as
  the brief asks: one line in `kaski-mesh` reruns every drawn test of both
  suites. Between full runs `--cheap` is the way past that.
- **The first run in a fresh container knows nothing**: no history, so every
  member runs and no test is in the group yet. Helpers start in fresh
  containers, so each helper pays one full run. The history is a file under
  `mock/target/`; keeping it across containers is the question below.
- **Doc tests do not run under nextest**, which has no doc-test support.
  `kaski-render` has none; a crate that does keeps them only in `--plain`.
- **The build is untouched**: `cargo test --no-run` for `kaski-web` took 20
  minutes cold here, and selection only saves the builds of members it skips.

## Questions only op can answer

- Should the history survive a container, so a helper's first run is not a
  full one: a ref pushed beside the branch, an artifact the bootstrap fetches,
  or nothing?
- One heavy test at a time or two: the default is one, a stand-in until the
  `after-change` rows give a drawn test's time alone against two side by side.
- `[test] on_commit`: kaski's wiring branch sets `cheap`, so a commit runs the
  members it reaches without their drawn tests. Is a commit hook that can take
  minutes after a `kaski-mesh` edit what op wants, or should it stay `off`?
- Should `--full` also run doc tests through `cargo test --doc`?
