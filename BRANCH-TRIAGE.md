# Branch triage: five local branches in mockspace

Scope: `fix/type-harness/branch-name-trait`, `wip/attribution-rework-parked`,
`feat/proc-macro-exemption-in-builtin-lints`,
`fix/changelist-doc-gate-shame-exemption`, `fix/state-transitions-auto-commit-default`.
All five live only as local refs fetched from the central clone
(`/Users/orgrinrt/Dev/clause-dev/mockspace`, remote `central` here) and are on no
remote.

## Method and its limits

The repository's history has been rewritten (byline correction, then version
renumbering), so `origin/dev` and the central clone's `dev` are disjoint
histories: `git rev-parse origin/dev` is `d144cd64`, `git rev-parse central/dev`
is `09da1fd9`, and `git merge-base origin/dev central/<branch>` returns empty for
all five. Every patch-id instrument (`git cherry`, rebase detection,
`git log --cherry-pick`) is void here. Only content answers.

Each branch's own base is real inside the *old* history:
`git merge-base central/dev central/<branch>`. That number is used only to
compute each branch's own diff (`git diff <base> central/<branch>`), never to
compare against `origin/dev`.

An earlier automated pass on this same branch set produced line-added
percentages against `origin/dev`. Those numbers are reported here as a
provenance note, never as a verdict input: the pass that produced them
self-reported four prior wrong readings (a file-level "differs from dev" test
conflating "dev lacks this" with "dev has this and more"; a control branch that
was an ancestor of dev and so trivially read as clean; a `shuf` invocation that
does not exist on macOS and silently emptied every sample; a `head -40` sample
that biased toward one file's hunks). No claim below rests on that pass. Every
verdict here rests on reading each branch's own commits and diff, then grepping
`origin/dev` for the concrete symbol, behavior, or config key each branch
introduced, and opening what the grep returned.

---

## `fix/state-transitions-auto-commit-default`

**Base:** `bba3111` (2026-04-29, `central/dev`). One commit, `e834056`,
15 lines in `src/entry.rs`.

**What it was for.** `mock lock` / `deprecate` / `unlock` / `close` / `archive` /
`migrate` only committed their state-machine rename when the caller passed
`--auto-commit`. The commit flips the default: transitions auto-commit unless
`--no-commit` is passed, on the reasoning that an uncommitted rename in the
working tree is fragile (a later `git reset` resurrects the source path while
the renamed target is left an untracked orphan). It keeps `--auto-commit` as an
accepted no-op for backward compatibility.

**Substance searched:** the predicate the commit's `auto_commit` variable is
built from, and its default value under no flags.

**Where searched:** `git grep -n "auto_commit\|SubcmdOpts" origin/dev -- src/`,
then opened `src/entry/dispatch.rs` around the hit.

**What was found.** `src/entry/dispatch.rs:1431` still defines
`fn auto_commit_wanted(args) -> bool { args.iter().any(|a| a == "--auto-commit") }`,
i.e. the *old* default (opt-in, not opt-out) is what ships. But the same file
carries, at `:1416-1429`, a `FIXME` explaining why: the current commit path uses
`commit-tree` + `update-ref` directly rather than porcelain `git commit`, which
runs no hook and produces no signature. Flipping the default to on, as this
branch does, would make every transition write an unsigned, unhooked commit by
default, i.e. ship the `--no-verify` this workspace forbids, on by default, from
the tool that enforces the ban. The file then carries a catalogued red test at
`:1487-1497` (`#[ignore = "catalogue: a transition should commit by default,
and cannot until the commit path stops bypassing hooks and signing"]`) asserting
the exact intended behavior this branch's diff hard-codes, held red until the
commit path is fixed to go through porcelain.

**Verdict: SUPERSEDED.** `src/entry/dispatch.rs:1409-1497` on `origin/dev`.
Dev considered the identical question, found the branch's fix unsafe as
written (it silently reintroduces an unsigned/unhooked commit path on by
default), declined to ship it, and encoded the correct fix as a catalogued
red test blocked on a named prerequisite. Landing this branch as-is would
regress dev below its own documented safety bar.

**Confidence:** high. The FIXME and the ignored test name the exact mechanism
and the exact default-value question this branch's commit answers differently
and, on the evidence in the file, wrongly.

---

## `feat/proc-macro-exemption-in-builtin-lints`

**Base:** `20b02e0` (2026-04-23, `central/dev`). One commit, `a3f46e5`,
5 lines across three lints (`registrable_completeness.rs`,
`repr_c_abi_safety.rs`, `undocumented_type.rs`).

**What it was for.** Three lints hardcoded `crate::PROC_MACRO_CRATES.contains(&ctx.crate_name)`
to skip proc-macro crates. The commit switches all three to
`ctx.is_proc_macro_crate()`, a method presumably already added to `LintContext`
by an earlier commit in this same base history (the diff does not define the
method, only calls it).

**Substance searched:** `PROC_MACRO_CRATES` / `is_proc_macro_crate` in the three
target files, then the current shape of the exemption mechanism.

**Where searched:** `git grep -n "PROC_MACRO_CRATES\|is_proc_macro_crate"
origin/dev -- lint-rules/src/registrable_completeness.rs
lint-rules/src/repr_c_abi_safety.rs lint-rules/src/undocumented_type.rs`, then
`lint-rules/src/lib.rs`.

**What was found.** None of the three files call `ctx.is_proc_macro_crate()`
directly any more. All three (`registrable_completeness.rs:48`,
`repr_c_abi_safety.rs:55`, `undocumented_type.rs:63`) call
`ctx.should_skip_proc_macro_source_lint()`. `lint-rules/src/lib.rs:176-197`
defines both: `is_proc_macro_crate()` answers "is this a proc-macro crate"
(reading a runtime `proc_macro_crates` config list, falling back to the
now-empty `PROC_MACRO_CRATES` const); `should_skip_proc_macro_source_lint()`
wraps it with a second, independently configurable axis,
`lint_proc_macro_source` (`mockspace.toml` key), so a project can force source
lints to run against proc-macro crates anyway. This is a strict superset of
what the branch shipped: same runtime-configurable exemption, plus a second
knob the branch's version has no equivalent of.

**Verdict: SUPERSEDED.** `lint-rules/src/lib.rs:141-197`, and the three call
sites named above.

**Confidence:** high.

---

## `fix/changelist-doc-gate-shame-exemption`

**Base:** `3d99f84` (2026-04-25, `central/dev`). One commit, `51a98b6`, adds a
`SHAME.md.tmpl` carve-out to `changelist_doc_gate.rs`'s `is_doc_template`, plus
three unit tests, plus a one-line compile fix (`lint_proc_macro_source: false`)
backfilled into three unrelated test fixtures that had gone stale under a field
added by a different, already-merged PR.

**What it was for.** The changelist-doc-gate lint blocks `.md`/`.md.tmpl` edits
under `crates/` outside the DOC phase. The gate's own diagnostic text told
users to record gaps in `SHAME.md.tmpl` as the SRC-phase escape hatch, but the
gate then blocked writes to `SHAME.md.tmpl` under the same rule, so the
documented escape hatch was unreachable.

**Substance searched:** `SHAME` and `is_doc_template` in the current gate file.

**Where searched:** `git grep -n "SHAME\|is_doc_template" origin/dev --
lint-rules/src/changelist_doc_gate.rs`.

**What was found.** `lint-rules/src/changelist_doc_gate.rs:98-104` (doc comment
+ `fn is_doc_template`) excludes `SHAME.md.tmpl` via
`crate::is_shame_template(file)`, called from a now-shared helper rather than a
hand-rolled suffix check. Test coverage in the current file
(`:110-156`, module `is_doc_template_tests`) is broader than the branch's three
tests: it also asserts that `NOT_SHAME.md.tmpl` and `DESIGN_SHAME.md.tmpl` are
*not* falsely exempted, and exercises a `SrcLayout` parameter the branch's
version of the function did not take.

**Verdict: SUPERSEDED.** `lint-rules/src/changelist_doc_gate.rs:98-156`.

**Confidence:** high.

---

## `wip/attribution-rework-parked`

**Base:** `5f3d1ee` (2026-07-25, `central/dev`). Two commits: `300cb20`
("forgive the memberless-workspace cargo failure", introduces
`entry::cargo_gate`) and `9794710` ("wip: park attribution + durable rework for
branch split", the bulk of the diff: `src/attribution.rs` new, plus changes to
`src/bootstrap/durable.rs`, `src/bootstrap/hooks.rs`, `src/config.rs`,
`src/entry/{check,dispatch,mod}.rs`, `src/lib.rs`).

**What it was for.** Two separate pieces of work landed together, only the
first of which is what the branch name describes:

1. `cargo_gate`: cargo returns a hard error on a virtual workspace manifest
   with no members, which a repo mid-first-design-round legitimately has.
   `cargo_gate` distinguishes that specific, confirmed-benign failure from a
   real one so `mock check` and `cargo mock` do not hard-fail on it.
2. The attribution rework proper: the byline-rejecting `grep -E` pattern that
   decides whether a commit's `Co-Authored-By` is permitted was duplicated,
   hardcoded and out of sync across the generated `commit-msg`/`pre-push` hooks
   and the durable (machine-global) hook, so a repo configured for autonomous
   work (which *requires* a byline) could have its commit demanded by one layer
   and rejected by the other. The branch's fix: a new `src/attribution.rs` Rust
   module (`scan_function_body()`) shared verbatim by all three layers, plus a
   `Config`-driven `AttributionConfig` (two axes: agent co-authorship, judged
   per mode; advertising trailers, denied in every mode) and a new
   `uninitialised_blocks` (`GateScope::Surface`/`All`) config key controlling
   what the durable hook blocks when a repo is not yet initialised.

**Substance searched:** (a) `cargo_gate`, `is_memberless_virtual_workspace`,
`diagnostic_is_no_members`; (b) the byline-duplication-across-hook-layers
problem and its fix; (c) `AttributionConfig`'s `adverts` field;
(d) `uninitialised_blocks` / `GateScope`.

**Where searched:** `git grep -n "cargo_gate\|no workspace members" origin/dev
-- '*.rs'`; `git grep -n "attribution" origin/dev -- '*.rs' '*.sh'`; then opened
`src/bootstrap/hooks.rs`, `src/bootstrap/durable.rs`, `src/config.rs`,
`lib/attribution.sh`, `mockspace-manifest/src/gate.rs`.

**What was found.**

- **(a) `cargo_gate` landed unchanged.** `src/entry/mod.rs:27` (`mod
  cargo_gate;`), `src/entry/cargo_gate.rs:25` (`diagnostic_is_no_members`),
  `src/entry/check.rs:224-263` and `src/entry/dispatch.rs:675-686`
  (`is_memberless_virtual_workspace`, `forgives_failure`), same names, same
  shape as the branch. Fully in dev.
- **(b) the sync problem is fixed, but by elimination, not by the branch's
  shared-module approach.** `src/bootstrap/hooks.rs:68-77` (doc comment on
  `message_commit_msg_body`, dev's current commit-msg body generator): "Replaces
  a hardcoded `grep -E` that was baked into two hook layers under a comment
  conceding the copies 'MUST stay in sync'. They could not, and the baked
  pattern contradicted configuration outright, rejecting unconditionally what
  `[attribution] autonomous` was meant to require. Policy now lives in one place
  and every surface reaches it through the same command." The generated
  `commit-msg`/`pre-push` hooks (`src/bootstrap/hooks.rs:78-181` for the
  bodies, `:191-205` for `gen_commit_msg`) no longer bake any byline logic at
  all: they shell out to
  `mock check-message --domain commit-message`, i.e. the policy lives once, in
  the running binary, checked at runtime, rather than baked as duplicated bash
  text (the branch's `scan_function_body()` was still baked text, shared by
  source but still copied into three separate generated files). The same
  diagnosis is stated a second time, independently, as the module doc of
  `src/entry/message.rs:7-14` ("the same pattern was duplicated into two hook
  layers that a comment conceded 'MUST stay in sync', and it contradicted the
  configured policy outright"). The durable (machine-global) hook is generated
  by `mockspace-manifest/src/gate.rs:144-206` (`durable_hook`, called from
  `src/bootstrap/durable.rs:36-40`'s `ensure_durable_hooks` via
  `install_durable_hooks`): its rustdoc says "delegate to the generated per-repo
  hook when one exists, or block at the repo's configured
  `uninitialised_blocks` scope when it does not" (`:146-147`), and the emitted
  script itself says "Carries no policy: it delegates to the generated per-repo
  hook when mockspace is initialised, and blocks at the configured scope when
  it is not" (`:165`), the same delegate-or-block shape the branch's
  `durable_delegate_or_block` introduced.
  No commit or comment on dev names this branch, so whether dev's authors saw
  it is unknown; the mechanism converged on the same shape regardless.
- **(c) `AttributionConfig.adverts` landed.** `src/config.rs:329` (field),
  `:393`, `:731` (raw-config plumbing). Present, same name, same role.
- **(d) `uninitialised_blocks` landed, but as an untyped string key read at
  hook-runtime by the shell body, not as a Rust `GateScope` enum on `Config`.**
  `mockspace-manifest/src/gate.rs:147,178,199` (`ms_read_key "$cfg"
  uninitialised_blocks`, `scope=$(...)`, compared against the literal `"all"`
  string). `src/bootstrap/hooks.rs:668` shows the same key used in a test
  fixture (`uninitialised_blocks = "all"`). No `GateScope` type or
  `Config::uninitialised_blocks` field exists in `src/config.rs` on dev
  (`git grep -n "GateScope\|uninitialised_blocks" origin/dev --
  src/config.rs` returns nothing); the value is carried as a raw string read
  by the generated bash body instead of parsed into a typed Rust field.
- **Not the same job as `lib/attribution.sh` / `tests/attribution_test.sh`.**
  Those exist in dev and cover a different consumer (a `nutshell`-based bash
  library, "the engine" with "no policy of its own", intended for other tools,
  e.g. the clause-dev pr-review sweep, to import via `use attribution`; its own
  header names that use case explicitly). They are not what supersedes this
  branch; the Rust `bootstrap/hooks.rs` + `mockspace-manifest/src/gate.rs`
  mechanism above is.

**Verdict: SUPERSEDED.** `src/bootstrap/hooks.rs:59-205`,
`src/bootstrap/durable.rs:6-40`, `mockspace-manifest/src/gate.rs:1-231`,
`src/config.rs:255-337`. Dev's own doc comments name the identical bug this
branch was written to fix and describe fixing it, by a different and, on its
own account, more robust mechanism (delegation instead of a second copy) than
what the branch shipped (a shared-but-still-duplicated Rust function). The one
piece not fully superseded, `GateScope` as a typed `Config` field versus a
loose string read by a shell snippet, is a minor loss of type safety on a
value dev already threads through correctly; it is not something this branch
would need to contribute, since dev's shape works and the branch's own
`Config`-side plumbing for it predates dev's `mockspace-manifest` split and
would not apply cleanly against it.

**Confidence:** high on the core question (byline-sync bug: fixed differently
and more thoroughly in dev). Medium on whether `GateScope`'s missing typing is
worth a follow-up: that is a real, small gap, but it is a new, narrower piece
of work, not "land this branch."

---

## `fix/type-harness/branch-name-trait`

**Base:** `7b85bf2` (2026-05-22, `central/dev`). Seven commits, 19 files,
~1820 inserted / 460 deleted lines, entirely inside `mock/crates/mockspace-core`,
`mock/crates/mockspace-cli`, `mock/crates/mockspace-rs`.

**What it was for.** A retroactive audit (workspace task #591, referenced in
the workspace's own `harness-the-type-system.md`) walking back a violation of
the type-discipline ladder: `mockspace-core`'s task/round identity types
(`Slug`, `Namespace`, `TaskId`, `RefPath`) were concrete structs hardcoded
throughout the CLI and IO layers, rather than traits with a default impl. The
branch's seven commits, in order: park the CLI slice pending the audit; move
the CLI surface from raw `String` to concrete typed newtypes (rung 5 on the
ladder); then trait-ify `Slug`, `Namespace`, `TaskId`, `RefPath` one at a time
(each renaming the concrete struct to `DefaultSlug`/`DefaultNamespace`/etc,
introducing a `trait Slug { type Error; fn parse(...); }`-shaped abstraction,
and generalizing call sites to `S: Slug` and friends); finally add `trait
BranchName + DefaultBranchName`, cascading into `CloseMetadata`.

**Substance searched:** whether `mockspace-core` still has these types as
concrete-only structs (i.e. the audit never happened / was reverted), or as
`Slug`/`TaskId`/`Namespace`/`RefPath`/`BranchName` traits (i.e. the branch's
own shape landed), or as something else.

**Where searched:** `git log origin/dev --oneline -i --grep="slug"` and
`--grep="branch.name"` (restricted afterward to `origin/dev` specifically with
`git merge-base --is-ancestor`, since an unqualified `--all` search pulls in
every fetched remote-tracking ref including the branch itself); then
`mock/research/*.md` in `origin/dev` for "walkback" / "type harness"; then the
current `mock/crates/mockspace-core/src/{slug,namespace,ref_path,task,
branch_name,identity,entity}.rs`.

**What was found.** A single commit on `origin/dev`,
`5d7f5c5` ("fix: type-system harness audit (#591): RefTo<T> + NamedRefTo<T>
identity abstraction", squash-merged, one parent, 23 files / ~2045 inserted
lines), whose commit body is a literal, verbatim log of a PR stack that:

1. **Starts from this exact branch's seven commits**, quoted almost word for
   word as its first sub-entries: "wip: slice B parked pending type-system
   harness audit", "wip: CLI typed clap surface checkpoint (rung-5 only, trait
   refactor to follow)", "refactor: trait Slug + DefaultSlug + cascade IO
   executors to generic S: Slug", "refactor: trait Namespace + DefaultNamespace
   + generic RefPath::task", "refactor: trait TaskId + DefaultTaskId + generic
   RefPath/IO executors", "refactor: trait RefPath + DefaultRefPath + cascade".
   These land, tests green, reviewed across four PRs (#151-#154 named in the
   log).
2. **Then explicitly course-corrects past the branch's own shape**: "Course-
   corrects the first 4 stack frames (#597-#600) whose trait names were
   shape-named (trait Slug, trait Namespace, trait TaskId, trait RefPath).
   Those will be replaced by RefTo<T> + NamedRefTo<T> in one big PR." A design
   memo is locked first (`mock/research/202605222200_refto-trait-design.md`,
   present on dev, 231 lines) splitting identity into two function-named
   traits (`RefTo<T>`: a `#[marker]` anchor, "this type is a reference to
   entity T"; `NamedRefTo<T>: RefTo<T> + AsRef<str> + Display`, the
   human-readable-name role) parameterized over a new `entity` module (`Round`,
   `Task`, `Branch`, `GitRef`, `Instant`), rather than one bespoke trait per
   concrete type. `DefaultSlug` etc. rename back to bare `Slug` under the new
   scheme.
3. **Extends past the branch's scope**: adds `Iso8601Utc` /
   `NamedRefTo<Instant>` (task #602) and a nightly toolchain pin
   (`mock/rust-toolchain.toml`, needed for `#[marker]` / `marker_trait_attr`,
   already a WATCH-tier workspace-vetted feature per `unstable-features.md`).

Confirmed against present-day dev source, not only the commit message:
`mock/crates/mockspace-core/src/identity.rs:53,64` defines `trait RefTo<T>` and
`trait NamedRefTo<T>`; `src/branch_name.rs:1-20` defines a concrete
`struct BranchName` implementing `NamedRefTo<Branch>` (validated against
`git-check-ref-format`, matching the branch's own `BranchName` intent);
`src/slug.rs:1-30` likewise: concrete `struct Slug` under `impl NamedRefTo<Round>
`/`NamedRefTo<Task>`. No `trait Slug` / `trait TaskId` / `trait Namespace` /
`trait RefPath` (the branch's own trait names) exist anywhere in
`mockspace-core` on dev (`git grep -n "^pub trait\|^trait " origin/dev --
'mock/crates/mockspace-core/src/*.rs'` lists none of the four).

So the "type-harness walkback" this triage was asked to check for is real, but
it is not a *loss* of the branch's work: it is a second, better-considered pass
that fully absorbed the branch's seven commits, shipped them, reviewed them,
and then replaced their specific trait shape (one trait per concrete type) with
a more general one (two roles, `RefTo<T>`/`NamedRefTo<T>`, parameterized over an
entity marker) before locking. Nothing here suggests the branch was abandoned
or backed out for being wrong in its *goal*; the goal (kill the `String`-typed
identity surface per `harness-the-type-system.md`) is exactly what landed and
exceeds what the branch itself reached (it never got past the four
shape-named traits to a `BranchName` role split, and never touched
`Iso8601Utc`/`Instant`).

**Verdict: SUPERSEDED.** `mock/crates/mockspace-core/src/identity.rs`,
`entity.rs`, `branch_name.rs`, `slug.rs`, `namespace.rs`, `ref_path.rs`,
`task.rs`, `mock/research/202605222200_refto-trait-design.md`, all on
`origin/dev`, reached via commit `5d7f5c5`. This branch's diff does not apply
onto current dev (the trait names it introduces do not exist there and their
concrete types now mean something different), and rewriting it from its own
intent is unnecessary work: the intent already shipped, in a more general and
further-developed form, reviewed across four PRs and locked behind a design
memo.

**Confidence:** high. The evidence is not merely "dev has similar-sounding
types"; `5d7f5c5`'s own commit message quotes this branch's seven commit
subjects verbatim as its first six sub-entries, which puts the provenance
beyond a name-based coincidence.

---

## Summary

| Branch | Verdict | Reasoning |
|---|---|---|
| `fix/type-harness/branch-name-trait` | SUPERSEDED | `origin/dev@5d7f5c5` (#591 audit) absorbed this branch's seven commits verbatim, then replaced its per-type trait shape with a more general `RefTo<T>`/`NamedRefTo<T>` design across `mockspace-core/src/{identity,entity,branch_name,slug,namespace,ref_path,task}.rs`; not a loss, a further pass. |
| `wip/attribution-rework-parked` | SUPERSEDED | `cargo_gate` half landed unchanged (`src/entry/cargo_gate.rs`); the attribution-sync bug it fixed is fixed again in dev, by delegation instead of a shared function (`src/bootstrap/hooks.rs`, `mockspace-manifest/src/gate.rs`), whose own doc comments name the identical original bug. `GateScope` typing is the one small untyped gap, not worth reviving the branch for. |
| `feat/proc-macro-exemption-in-builtin-lints` | SUPERSEDED | The three lints call `ctx.should_skip_proc_macro_source_lint()` now, a strict superset of the branch's `ctx.is_proc_macro_crate()` swap, adding a second `lint_proc_macro_source` config axis (`lint-rules/src/lib.rs:141-197`). |
| `fix/changelist-doc-gate-shame-exemption` | SUPERSEDED | `is_doc_template` on dev excludes `SHAME.md.tmpl` via a shared `is_shame_template` helper with broader test coverage than the branch shipped (`lint-rules/src/changelist_doc_gate.rs:98-156`). |
| `fix/state-transitions-auto-commit-default` | SUPERSEDED | Dev kept the opt-in default on purpose: the branch's opt-out-by-default change would ship an unsigned, unhooked commit path by default, which dev's own `FIXME` and catalogued red test (`src/entry/dispatch.rs:1409-1497`) name as the reason it is blocked pending a commit-path fix. |

All five branches are safe to delete; none carries work `origin/dev` lacks.
