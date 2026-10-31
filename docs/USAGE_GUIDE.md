# mockspace usage guide

This is the deep reference. Start with the [README](../README.md) for the conceptual overview; come here for subcommand surface, configuration, lint authoring, and template structure.

## Bootstrap

A mockspace lives in `mock/` at the repo root. The launcher is the sole entry, installed once per machine:

```bash
cargo install --git https://github.com/hiisi-digital/mockspace.git cargo-mock
```

Then run it from the repo root. On any normal invocation the engine:

1. Writes the generated validator under `mock/target/hooks/`.
2. Ensures the durable hooks in the user config home, which delegate to that validator; the repository's own `.git/hooks/*` run after it, on a pass.
3. Points `core.hooksPath` at them.

It is idempotent and cheap on the common path. There is no `build.rs` bootstrap, no cargo alias in `.cargo/config.toml`, and no generated proxy crate; a repo still calling `bootstrap_from_buildscript` from a build script gets a hard error naming the migration steps.

Activate the hooks once per clone:

```bash
cargo mock activate
```

This sets `git config core.hooksPath` to the generated hooks dir. The generated hooks always run the repository's own `.git/hooks/<name>` once their validation has passed, with the same arguments and the same stdin, so a hook another tool installed there keeps running and its refusal still counts; nothing runs past a refusal of mockspace's own. Deactivate with `cargo mock deactivate`; git then falls back to the repository's hooks unchanged.

## Directory layout

Everything outside `mock/` is yours. Mockspace owns:

```
<repo_root>/
├── mockspace.toml                            [authored: config]
├── docs/                                     [generated; regenerated every build]
│   ├── DESIGN.md, DESIGN-DEEP-DIVES.md
│   ├── STRUCTURE.md, STRUCTURE.GRAPH.{dot,png,svg}
│   └── <per-crate overviews>
└── mock/
    ├── Cargo.toml + Cargo.lock               [authored: mock workspace]
    ├── DESIGN.md.tmpl                        [authored: top-level design template]
    ├── PRINCIPLES.md.tmpl                    [optional authored invariants]
    ├── WORKFLOW.md.tmpl                      [optional authored workflow note]
    ├── crates/<name>/                        [authored: real code, plus per-crate
    │                                          DESIGN.md.tmpl, optional BACKLOG.md.tmpl,
    │                                          optional SHAME.md.tmpl]
    ├── design_rounds/                        [authored: round state machine]
    │   ├── <round-id>_topic.<name>.md
    │   ├── <round-id>_research.<name>.md
    │   ├── <round-id>_changelist.doc[.lock|.deprecated].md
    │   ├── <round-id>_changelist.src[.lock|.deprecated].md
    │   └── <archived-round>/                 [after `cargo mock close`]
    ├── research/                             [optional: round-independent material]
    ├── agent/                                [optional: agent template files]
    ├── lints/                                [optional: custom lint sources]
    └── target/                               [build artefact, gitignored]
```

## Doc layers per crate

A crate under `mock/crates/<name>/` carries up to four documentation layers. Only `DESIGN.md.tmpl` is required; the others are optional and serve specific purposes.

| File                 | Purpose                                                                                              |
|----------------------|------------------------------------------------------------------------------------------------------|
| `DESIGN.md.tmpl`     | Shipping claim. Every type, trait, macro, or signal named here is expected to exist in source.       |
| `BACKLOG.md.tmpl`    | Designed-but-deferred promissory notes. Items decided but deliberately out of the current shipping scope. Names here are *not* checked against source. Move entries into `DESIGN.md.tmpl` when they ship. |
| `SHAME.md.tmpl`      | Known gaps. Escape hatch for lints (`design-doc-source-mismatch`, `deprecation-comparison`, `undocumented-type`): a `## <key>` header with a 50+ word explanation silences the specific violation keyed by `<key>`. |
| `DEEPDIVE_*.md.tmpl` | Optional topic-specific deep dives. Rendered alongside DESIGN.md.                                    |

The split matters because `DESIGN.md.tmpl` is walked for type names by the `design-doc-source-mismatch` lint, and every backticked identifier is expected to correspond to a source item. Putting "designed, not yet shipped" material into DESIGN forces either a fake source stub or a SHAME entry per name. `BACKLOG.md.tmpl` is the clean home for those notes; promote a BACKLOG item into DESIGN in the same doc changelist that ships its source.

## Design-round state machine

Every round walks five phases. Each phase is detected from the presence and status-suffix of files in `mock/design_rounds/`.

| Phase       | Filename state                                                  |
|-------------|-----------------------------------------------------------------|
| `TOPIC`     | only `*_topic.*.md` or `*_research.*.md` files                  |
| `DOC`       | `*_changelist.doc.md` exists                                    |
| `DRAFT`     | `*_changelist.doc.lock.md` exists, no `*_changelist.src.md`     |
| `IMPL`      | `*_changelist.doc.lock.md` and `*_changelist.src.md` both exist |
| `CLOSED`    | `*_changelist.doc.lock.md` and `*_changelist.src.lock.md`       |

Filenames start with the round id:

```
<id>_{topic|research|changelist}.{name|doc|src}[.lock|.deprecated].md
```

An id is `YYYYMMDDHHMM-tttt`, the minute and a four character tag (a lowercase letter, then letters or digits). `cargo mock round-id` prints a fresh one and is how a round is opened: the tag is drawn from the branch, host and time, and the id is checked against the working tree, the `round/*` tags and `origin/dev` and `origin/main`, so two sessions that open a round in the same minute on separate branches do not meet in a collision when both close. A bare `YYYYMMDDHHMM` from an older round is still an id; the minute stays first, so ids sort by time across old and new. Two closed rounds that still share one id fail the merge of their `.meta` with `round id collision`, and `cargo mock rename-round <id>` renames one side (directory, files, the `round:` field, the local tags) on its own branch before the merge is tried again. An abandoned round keeps `-abandoned` through the rename and loses its `-N` count. A round already on `origin/dev` or `origin/main` exactly as it is committed has landed and is not renamed.

Transitions are subcommands, not manual renames:

| Command                   | Effect                                                            | Valid phases            |
|---------------------------|-------------------------------------------------------------------|-------------------------|
| `cargo mock lock`         | Lock the active changelist (`DOC -> DRAFT`, `IMPL -> CLOSED`)     | `DOC`, `IMPL`           |
| `cargo mock unlock`       | Deprecate src CL and unlock doc CL (destructive; source unchanged) | `DRAFT`, `IMPL`, `CLOSED` |
| `cargo mock deprecate`    | Deprecate the active CL; in `IMPL` also unlocks doc CL; a stub changelist (bare title) beside a locked one of its kind is retired instead, in any phase | `DOC`, `IMPL`, or any with a stub |
| `cargo mock close`        | Archive the round into a `<round-id>/` subdir with `.meta` + `.history`; takes the changelists, the files carrying the id of one, the files one names, any flat file given as `--take <file>`, and lists the flat files it leaves | `CLOSED` only       |
| `cargo mock archive`      | Archive an abandoned round into a `<round-id>-abandoned/` subdir; with changelists it takes what `close` takes (without `--take`) and is named by their id, and in the TOPIC phase, with none, it takes every flat file and is named by the earliest | any                     |
| `cargo mock migrate`      | Rename legacy `YYYY-MM-DD_*.md` files to the compact format       | any                     |
| `cargo mock round-id`     | Print a fresh round id, `YYYYMMDDHHMM-tttt`                       | any                     |
| `cargo mock rename-round <old> [<new>]` | Give a round a new id: directory, files, `.meta`, local tags | any                     |

All transition subcommands accept `--auto-commit`, which commits only the renamed files. A temporary `GIT_INDEX_FILE` is used, so whatever is staged stays staged and is not swept into the commit.

**That commit is written with git plumbing, so no hook runs and it is not signed.** `commit-tree` plus `update-ref` is what builds it, which means `core.hooksPath`, `pre-commit`, `commit-msg` and `commit.gpgsign` are all bypassed. If a repository relies on any of those, do the commit by hand: without the flag the subcommand prints the exact command to run.

The "## Comparison to deprecated changelist" section of the active CL plus `BACKLOG.md.tmpl` together are the repository's memory of decisions not yet implemented.

### Merging inside a round

The phase gates judge what somebody wrote, so a merge commit is judged by what it adds beyond its parents. A file the merge took whole from one parent, a file git combined from both, and a conflict settled by keeping lines of both sides add nothing, and they pass in every phase, designs and source together. A file with a line neither parent carries, or without a line every parent carries, was written or cut by hand and stays gated as an edit. The test is on lines, counted and in sequence: every staged line is a line some parent has, no line appears more often than the merge base has it plus what each parent added to it (so two sides that each add a blank separator both stand, and a second copy of a line both parents carry once does not), every line all the parents have is still there as often as the parent with the fewest, and each parent's own order holds. The order test runs for every file whatever its size; the time is capped per comparison (a file can need several: one per parent for each ordering tried), and a file where one comparison passes the cap is reported with a message naming it and gated as an authored change. A merge whose parents have no merge base (unrelated histories, or a shallow clone that cut it off) cannot bound repeats, so its files are not excused by the line test either, with a message saying so, and are gated as authored; a file taken whole from one parent or reproduced by git's own merge still passes. The panel-canon gate takes the same merge filter as the phase gates: canon staged by a merge is judged by what it adds beyond its parents, and only the canon paths are tested. Putting the two sides' sections one after the other, in either order, is not an edit; typing a new line, repeating one, or trading two lines of one parent is.

Merge drivers cut conflicts off before they reach that point: the generated documents, the registry rows and the test history each have one, and so do the files that only ever grow. Of those, the files that only grow, a list of `mod` lines, a table of strings, a log, a package's `SHAME.md.tmpl` and `BACKLOG.md.tmpl`, go to the `mockspace-append` driver (`cargo mock merge-additions`). When neither side changed or removed a line of the base, both sides' additions stay, each where it was made and ours before theirs where both added at one spot. When either side edited a base line, or both added the same line at one spot, the file is merged as text, with conflict markers labelled `ours` and `theirs`: the driver never selects a side. A project names its own files in `mockspace.toml`:

```toml
append_only = ["content/strings/en.toml", "mock/crates/*/src/tests/mod.rs"]
```

Each entry is a `.gitattributes` pattern relative to the repository root, kept as narrow as the file it names. `cargo mock` writes the lines and registers the driver in the clone's git config (and, with `cargo mock register-drivers --global`, in the user's).

### The rest of the surface

Those are the transitions. The subcommands that are not transitions:

| Command | Effect |
|---|---|
| `cargo mock status` | The current round, its phase, and what may be edited right now |
| `cargo mock check` | Readiness report: git state, phase, build, tests, lints |
| `cargo mock check-message` | Lint one commit message, pull request body or comment against the configured policy, and for a commit with its author and committer (`--author`, `--committer`) |
| `cargo mock check-edit` | Ask one per-crate lint (`--lint`) whether a Rust file (`--file`) would be clean once an edit is made: stdin is the new text, or with `--replace <file>` the text that replaces the one in that file. Takes `--edits <file>`, a JSON list of replacements, for a multi-edit tool. Exits 0 clean, 1 with the lint's blocking findings, 2 when the request cannot be answered (an unknown lint or gate, a lint judging the whole crate, a relative `--file`). The generated `forbidden-imports` hook calls it |
| `cargo mock query` | Query the registry |
| `cargo mock bench` | Run the bench harness |
| `cargo mock test` | Run the tests of every tree mockspace owns, the members and the benches only where something they depend on changed, or the benches when `--benches`, `--full` or `--no-cache` asks (`[test]` below) |
| `cargo mock panel` | Mint or consolidate a panel seat, or report a panel's state |
| `cargo mock pdf` | Render the design documents to PDF |
| `cargo mock clean` | Remove generated output |
| `cargo mock activate` / `deactivate` | Point `core.hooksPath` at the gate, or hand it back |
| `cargo mock tools` | Every subcommand and project tool, with usage, and each tool's kind: a check that cannot be a lint, or a maker that writes files |
| `cargo mock help <name>` | One subcommand or tool in full, including what a tool is held to: a check's reason, or the paths a maker writes with the root each argument a path starts with must fall under |
| `cargo mock lints` | Every lint that runs here, builtin and pack alike, the severity this project gave it, and where the two sides disagree |

**`cargo mock tools` and `cargo mock lints` are the lists that cannot go stale**, since they read what the binary actually ships and what the engine and the loaded pack actually register between them, none of which this table can know about. Prefer them over this table wherever the two might disagree.

## `cargo mock` pipeline

Running `cargo mock` with no subcommand:

1. Runs the bootstrap health-check and regenerates anything stale.
2. Runs `cargo check` inside `mock/`.
3. Parses every crate under the directories `src_dirs` declares (`mock/crates/` when it declares none) via tree-sitter.
4. Runs the lint pipeline with per-gate severity for the current mode.
5. Runs dylib ABI checks for crates listed under `module_crates`.
6. Regenerates everything in `docs/` from templates and parsed crate data.
7. Regenerates agent integration files if `mock/agent/` is populated.

Common flags:

| Flag                | Effect                                                       |
|---------------------|--------------------------------------------------------------|
| `--lint-only`       | Run lints only; skip generation and dylib checks             |
| `--doc-only`        | Skip source lints; run only doc-related checks               |
| `--scope <csv>`     | Restrict lints to named crates (comma-separated, by directory name under any `src_dirs` entry) |
| `--scope infra`     | Infrastructure-only mode (no crate check, no crate lints)    |
| `--commit`          | Lint mode: `Commit` (typically most permissive gate)         |
| `--strict`          | Lint mode: `Push` (strictest gate)                           |
| `--nuke`            | Take the source down, leaving the designs. Names every file, then asks. |
| `--nuke=docs`       | Take the designs and the source under them, which is the order the chain wants. |
| `--y`               | Answer a `--nuke` in advance, for a second run and for a script. |
| `--dir <path>`      | Override auto-discovered mock dir                            |

The generated `pre-commit` hook runs `cargo mock --lint-only --commit` scoped to the crates it finds staged, where a crate is the directory directly under any directory the project declares in `src_dirs`. The generated `pre-push` hook runs `cargo mock --lint-only --strict`, scoped to the crates the push changed under the same directories. A staged or pushed shared input is never bounded to the crates beside it, so the hook passes no `--scope`: every crate and every repo lint run. The shared inputs are a file under `<mock>/lints/`, a file under the directory of a `[lint-crates]` pack declared by `path` (a pack is code the run compiles, and a pack sitting under a source directory is not scoped as a crate), and a `mockspace.toml` at the repo root or in the mock directory. The one flag a shared input can still carry is `--doc-only`, and the pre-commit hook adds it only when nothing staged is source: no `.rs` file and no crate `Cargo.toml` or `Cargo.lock` under a source directory, no workspace manifest or lockfile, and no file of a project lint or a path pack. Anything else staged beside it leaves the flag off, and the engine checks the claim against the same layout. The pre-push hook passes `--strict` without `--doc-only` whenever a shared input is among the pushed changes. A file under `<mock>/registry/` is not a shared input: the repo lints read the registry and every run makes them, and a crate's lints are handed no registry, so a commit that stages registry rows beside a crate is scoped to that crate, and a commit or push of registry rows alone runs `--scope infra`, the repo lints and no crate's. The commit adds `--doc-only` there, as a staged registry row always carried it, and the engine checks that against the staged index; the push never carries it, since the engine checks it against whatever is staged at push time and not against the pushed commits, and the repo lints that read the registry are skipped by it. The pre-commit `cargo clippy --fix` runs only in the packages that own a staged `.rs` file. A crate's own `Cargo.toml` and `Cargo.lock` are source of that crate: they scope the commit to it with the source lints on, so a `[features]` or `[lints]` edit is linted. A staged or pushed `Cargo.toml` or `Cargo.lock` reaches the crates of its own workspace and no others: the nearest directory at or above a source directory, up to the mock directory, whose `Cargo.toml` has a `[workspace]` table (the mock directory when none does). A nested workspace under the mock directory, such as a console with its own lockfile, is scoped by its own manifest and lockfile and not by the mock workspace's. The nuked-workspace shortcut on push applies only when every crate under the source directories carries the configured nuke marker, as the engine decides it. The hooks are regenerated by the next `cargo mock` run without `--lint-only`, which is when `src_dirs` changes reach them: the hooks themselves run only under `--lint-only`, which does not write hooks, so a commit that adds a source directory with source under it meets the old hook, and the engine refuses its `--scope infra` as omitting staged crates until `cargo mock` has been run once.

This table (and the one above it) is not the exhaustive command reference; run `mock tools` for that, or `mock tools --long` for full usage and declared arguments per command. It never goes stale the way a hand-written list does, because it is computed from the same declarations the engine dispatches against.

## Panels

`mock panel {seat,consolidate,status}` mints panel seats against a formalised inventory at `<mock>/panel/<slug>.toml`, capped at 99 seats with an enforced consolidation cadence (`panel_consolidate_every` below, default 10). `mock check` refuses a change touching a configured `canon_paths` glob while any panel is open. See `mock tools --long` for the full contract (usage, declared arguments, and the longer help body), and `mock/agent/config.toml`'s `agent_panel_discipline` to ship the generated rule describing the discipline to an agent.

## Configuration: `mockspace.toml`

Placed at `mockspace.toml` in the repository root. A copy inside the mock directory also resolves, and the root is the one to write. Common fields:

```toml
project_name = "<your-project>"
crate_prefix = "<your-project>"       # defaults to project_name
abi_version = 1
proc_macro_crates = ["<name>-derive"]
module_crates = []                    # dylib-loaded modules get dylib checks
unprefixed_crates = ["core"]          # crates that don't use the prefix
primary_domain_macro = "<macro>"      # tracked per-crate in STRUCTURE.md
primary_domain_label = "<Label>"
layers = ["Layer0", "Layer1", ...]    # labels by depth index for the graph
append_only = ["<path-or-pattern>"]   # files that only grow, merged by keeping both sides' additions
lockfiles = ["<path/Cargo.lock>"]     # Cargo locks settled from the merged manifests (the mock directory's is always included)

install_git_hooks = "replace"
install_cargo_config = "merge-append"
install_agent_files = "replace"

auto_fmt = true                       # rustfmt the staged files pre-commit
auto_clippy_fix = true                # cargo clippy --fix staged workspace roots pre-commit
deny_check = true                     # cargo deny check on push (needs a deny.toml)

canon_paths = ["mock/canon/**"]       # what `mock check` protects from an open panel
panel_consolidate_every = 10          # seats a panel mints before a consolidation is due
```

Install modes (applied when generated content overwrites existing files):

- `replace`: always overwrite.
- `merge-append` / `merge-prepend`: preserve other sections.
- `skip` / `skip-if-exists`: never overwrite an existing file.

### Pre-commit auto-fix

Before a commit is linted, mockspace can run the repo's own formatter and clippy
fixer, re-staging the results so the commit lands already-fixed:

- `auto_fmt` (default `true`): runs `rustfmt` over the staged `.rs` files. Uses the
  repo's own `rustfmt.toml` (rustfmt resolves it by walking the directory tree
  upward, so one config at the repo root also governs a nested `mock/` workspace),
  and the edition each file's own package declares. That last part is passed
  rather than left to rustfmt: it reads the edition from a `rustfmt.toml` in scope
  and assumes 2015 where none up the tree declares one, which refuses anything
  newer. A config declaring only `style_edition` is one of those. Passing the
  edition overrides the config, so the answer is the same either way.
- `auto_clippy_fix` (default `true`): runs `cargo clippy --fix` in the same packages,
  respecting the clippy lints the entrypoint crates declare.

The fixers are scoped to the packages a commit actually touches (each staged `.rs` file's
nearest `Cargo.toml`), not the whole workspace, so the cost stays proportional to the
change: `cargo clippy --fix` only compiles the changed packages.

Both keys are the project's and every clone reads them. A single clone can turn
either fixer off for itself, on a machine that should not compile the project at
commit time, and that setting stays in the clone's own git config rather than in
the repository:

```bash
git config mockspace.autoClippyFix false
git config mockspace.autoFmt false
```

Where the clone and `mockspace.toml` both say something, the clone wins. Only the
clone's own config is read, so the same key in `~/.gitconfig` or the system file
does nothing, and a worktree reads the clone it hangs off. A value git cannot read
as a boolean is refused at load rather than ignored.

Both are best-effort: a fixer that fails (unparseable source, code that does not yet
compile) is skipped and never blocks the commit. Files staged partially (`git add -p`)
are left untouched so a re-stage never sweeps in withheld edits. A fixer may still
rewrite other files within a changed package (fmt and clippy operate per package, not
per file); only the staged files are re-staged, so the commit records just those. Set
either key to `false` to opt out per-repo.

### Dependency gate (cargo-deny)

On push, `deny_check` (default `true`) runs `cargo deny check` against a `deny.toml` at
the repo root, in every workspace root the repo contains (each pointed at the one config
via `--config`, so a nested `mock/` workspace and its transitive graph are covered too).
It gates advisories (RustSec), license compatibility across the whole transitive
dependency graph, dependency bans, and source registries, and blocks the push on a
violation.

It is skipped (never blocking) in four cases: there is no `deny.toml`; cargo-deny is
not installed (`cargo install cargo-deny`); a workspace root's path carries a whole
`research` path component, since that is a spike tree such as a panel's probe or a
feasibility sketch, and nothing it depends on reaches a consumer; or a workspace
root's `cargo metadata` resolves to no packages at all, the shape a repository has
when every crate carries its own `[workspace]`. A non-spike root whose package graph
cannot be read at all is the one case that still blocks rather than skips, since a
broken manifest says nothing about whether it is clean. Set `deny_check = false` to
opt out.

### Tests: `[test]`

`cargo mock test` runs the workspace members under cargo-nextest where it is installed,
and selects which to run rather than running them all:

- **The bench tree runs when its inputs moved, or when asked.** Compiling every arm is
  the most expensive thing the command does, so it is left out when its inputs match
  the tree as it stood when the benches last passed, which is recorded under
  `<mock>/target/`. The inputs are the files git tracks or would track under
  `<mock>/benches/` (less `results/` and `history/`, which `mock bench run` writes,
  build output, and a lockfile git does not track, which is the build's own; a committed
  lockfile is an input, since it picks the framework commit the arms are built at), the
  files of every local crate a bench manifest depends on by `path`, the support crates
  included, the text of the arm manifests and the driver that `mock bench` generates for
  a tree that has none, and the workspace manifest, lockfile, toolchain pin, and cargo
  and nextest configuration every build reads. A tree that never passed counts as moved.
  It is asked for with `--benches`, which holds whatever else does, and with `--full`
  or `--no-cache`, which run every member whatever its record says, so that `--full
  --no-cache` is the whole suite. `-p` names crates and so leaves it out, unless
  `--benches` is given too. A run that leaves it out says so on its last line, a failed
  run included.
- **A member runs when its fingerprint moved since its suite last passed.** The
  fingerprint is its own files (what git tracks or would track under its directory,
  shaders and fixtures kept there included), every path outside it that its sources
  name as a `../` string literal (an `include_str!` of a content file, a directory joined
  to `CARGO_MANIFEST_DIR`), the shared inputs of every member compiled into its tests
  (their libraries, never their `tests/`), and the workspace's lockfile, manifest,
  toolchain pin and cargo and nextest configuration, and the cargo config `mock test`
  generates from `[test.opt_level]`. A change to a member's library reaches that member
  and everything depending on it, and nothing else; a change to a workspace input, that
  table included, reruns every member.
- **Within a member that runs, an integration-test binary runs when its own inputs
  moved.** `tests/<name>.rs` and `tests/<name>/main.rs` are each a binary, built from
  their own files (the modules they declare, the files their string literals name inside
  the member, a directory named whole, a `trybuild` glob cut at its first wildcard), what
  their sources read from outside, the member's library and its dependencies. A binary
  that passed at inputs that have not moved is left out and listed as `reuse`, so a
  change confined to one `tests/<name>.rs` runs that binary, and a member that failed in
  one binary reruns only that one. The library's own tests, the binaries and the examples
  move with the member's shared files. What cannot be placed widens rather than being
  missed: a file under `tests/` that no binary claims, the manifest, the build script and
  the library move every binary; a binary that reads the environment or `file!()`, declares
  a module inside a macro or an inline module, or does not parse, is rebuilt on every file
  any binary claims; a manifest naming a test outside `tests/` narrows nothing. The values
  of environment variables are in no fingerprint, a path built at run time from pieces
  none of which names it is not seen, and `--full` and `--no-cache` run every binary.
- **A test is heavy when its last run took `heavy_after_secs` or more.** Heavy tests run
  `heavy_threads` at a time in nextest's `@tool:mockspace:heavy` group, and a heavy test
  that passed at its binary's current fingerprint is skipped as cached.
- **Every test is killed after `timeout_secs`**, and each run ends with its slowest tests.
- The members are built without debug info, and a crate gets symbols only where one is
  needed to trace a problem. Debug info is the larger part of what a test build writes,
  and a failing test needs it only in the crate it fails in. A member needs symbols when its tests failed
  the last time they ran, or when the run names it with `--debug=<crate,...>` (which also
  chooses the crate, as `-p` does, and runs it whether or not it passed), or with
  `-p x --debug`. A bare `--debug` with no `-p` gives symbols to every member the run
  builds, and `--no-debug` to none, a failed one included. A crate that needs symbols gets
  `[test] debug` (`line-tables-only` by default, enough for a backtrace to name lines;
  `"full"` or `2` for a debugger); every other crate, dependencies from a registry
  included, gets none. The plan prints a `debug` line saying which crates got symbols and
  why.

  The record of what failed is `<mock>/target/mockspace-test-state/failed.json`, per
  flavour, and is not part of the tracked `test-history/`: a failure belongs to one working
  copy, and committed it would hand every other clone symbols for a crate that is red on
  one machine. A target directory that is gone costs one run without symbols for a crate
  that fails again. A `--plain` run reads it, and writes only to take off the members it passed whole.

  It reaches cargo as the same generated `--config` file the opt levels do, and no manifest
  or cargo config is edited: a profile-wide `debug = 0` under `dev` and `test`; the same
  for `"*"`, which is every dependency (a `"*"` in a manifest or a cargo config beats a
  profile-wide setting, and this one beats theirs); a `debug` entry for each member and for
  each package outside the workspace that a manifest or a cargo config names for itself,
  lean for the ones that need nothing, since a name beats both a profile-wide setting
  and a `"*"`; and the level for the members that need symbols. A `--config` on the command line beats the file, and a `--release`
  or `--profile` run is the profile you chose, so only the crates that need symbols are
  touched there. The environment has no spelling for one package
  (`CARGO_PROFILE_<P>_PACKAGE_<NAME>_DEBUG` is read as nothing), which is why a file is
  used. A cargo config file (`.cargo/config.toml` up the tree, or `$CARGO_HOME/config.toml`)
  or a `CARGO_PROFILE_DEV_DEBUG` that sets debug info for every build is reported by name,
  once for each file, and overridden for the run: cargo gives a `--config` file precedence
  over a config file, the environment and the manifest alike, which is also why a helper
  container's global `debug = "line-tables-only"` used to beat a manifest's `debug = 0`
  without a word. `[test] lean = false` lets the manifest and those settings stand and
  gives symbols only to the crates named.

  What no profile setting reaches is rustflags: `-g` or `-C debuginfo=` above none in
  `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS` or a `rustflags` of
  `[build]` or `[target.*]` in a cargo config (cargo takes one of those sources, and the
  configs are read only where no variable is set) give every crate debug info. They are
  reported by name, never overridden, and the plan line then stops saying every other
  crate is built without debug info.

  Building a crate with symbols compiles that crate and the crates that depend on it once,
  since a dependency's profile is part of a dependent's fingerprint; its dependencies and
  every crate that is neither are not compiled again, and the lean build stays in the
  target directory beside the other, so going back compiles nothing. A run restricted to
  the crates that failed builds no dependents. None of this is an input of a member's
  fingerprint: a test passes or fails the same with or without symbols, so what passed
  under one setting stands under the other. The first lean run after a manifest that asked
  for debug info compiles the members once more, and a `cargo build` over the same target
  directory keeps its own, so the directory holds both.

`--full` runs every member asked for, every binary of it, `--no-cache` runs the cached
heavy tests and the binaries it would leave out too, and
`--cheap` defers every heavy test that has passed before (a new one, or one that failed
last time, still runs). `--full --no-cache` is the whole suite, for the end of a piece of
work and after merging the trunk; `--plain` is `cargo test` as before, with the opt
levels of `[test.opt_level]` and the debug info above applied the same way. Doc tests do not
run under nextest, a known gap: `--plain` is the way to them for now.

The record is tracked: `<mock>/test-history/<flavour>/<member>.json`, committed with the
work that produced it, so a fresh clone or a new container starts from the last green
run rather than from nothing. The fingerprints are digests of contents and
repository-relative paths, so a pass recorded on one machine stands on another. Only
tests near or over the heavy threshold are kept, with each test binary that passed and the
fingerprint it passed at, and a file is rewritten only when what
it says changed, so a rerun of a settled suite leaves the tree clean. A test or binary
that no longer exists is dropped from the generated nextest config and pruned from the
record on the next run that runs something, found by asking nextest what it lists (the
build flags you passed, and none that only shape a run). Where that listing cannot be
had, a failing build for one, the run says so and drops nothing, and a record naming a
deleted binary can still stop nextest until the build is mended. A `--features` run
keeps its own flavour directory. A run writes and removes only the files of the members
it ran, in the flavour it ran in, plus those of members the workspace no longer has, so
a run for one member leaves every other file as it was, whatever it holds.

Two branches that both moved a member's file merge without a conflict: `.gitattributes`
sends `<mock>/test-history/**/*.json` to the `mockspace-test-history` merge driver
(`cargo mock merge-test-history`), which `cargo mock` registers in the clone's git config (and, with `cargo mock register-drivers --global`, in the user's) beside the
registry's. A member's green record, and a test binary's recorded pass, stays where both
sides hold the same one or only one side moved it from the base, and is dropped where
both moved it to different values, because neither fingerprint is the merged tree's; the
next run records it. The driver never picks between two greens that both moved.
Per-test results and timings are unioned, and where both sides timed a test the later
measurement is kept: the side that moved it, else the side that last changed the file later,
else the longer timing. Two files of different versions, members or flavours, or one that
is not a record, are refused with a message and the path stays conflicted.

The pre-commit hook runs `cargo mock check-test-history` for a commit that stages one of
these files. A commit that nulls a member's green record while touching none of its
inputs (its directory, the directories of the members compiled into its tests, the paths
its sources read from outside, the workspace-wide files) is refused, naming the member:
a run cut short, by a container restart or a full disk, leaves that. A merge commit whose
nulled green is one the driver's rule drops is not refused.

```toml
[test]
heavy_after_secs = 10    # a test this slow or slower is heavy
heavy_threads = 1        # how many heavy tests run at once
timeout_secs = 900       # a test still running after this long fails
commit = "off"           # a lint level: warn runs `mock test --cheap` on commit, error also blocks
lean = true              # build every crate without debug info unless it needs symbols; false leaves the profile alone
debug = "line-tables-only"  # what a crate that needs symbols gets: 1, 2, "limited", "full", "line-tables-only" or "line-directives-only"

[test.opt_level]         # optional: the opt level a member is built at under `mock test`,
                         # nextest or `--plain`; a name that is no member is ignored
sim-core = 3             # 0 to 3, "s" or "z"; members not named keep their manifest's
```

### Lints

Per-lint severity per gate:

```toml
[lints.no-todo]
commit = "off"
build = "warn"
push = "error"
```

Which files a lint sees, on the same table:

```toml
[lints.no-todo]
include = ["src/**"]
exclude = ["**/generated/**", "*.pb.rs"]
```

Paths are the ones a lint would report, so crate-relative (`src/lib.rs`) rather than repo-relative. `?` matches one character inside a segment, `*` a run of them without crossing a `/`, and `**` any number of whole segments including none; a pattern with no `/` in it matches the basename at any depth, as gitignore does. Character classes, brace expansion and negation are not implemented. Naming a lint here is enough to ask for it, so a lint that is off by default runs if the only thing said about it is a path filter.

`include` and `exclude` apply to per-package and cross-package lints. **They are read and then ignored for repository lints**, whose dispatch has no file list to filter.

A `[lints]` section that cannot be read stops the load with a message naming the file, and the lint and the field at fault; a `lints` value that is not a table names only `lints`, since no lint is reached. It never loads partly. That covers a string where `exclude` wants an array, a param, `rule` or `rules` value of a type it does not take (the message names the key and the types it takes), a `rules` entry without `scope`, `forbidden` or `reason`, a severity or gate value that is not a name the engine knows (`file-size = "eror"`, `severity = "eror"`, `commit = "eror"`, a value under `findings`), a `lints` key that is not a table, and `[[lints.<name>]]`. The message for a misspelt value lists the accepted names. A severity takes `off`, `error`, `hard-error`, `hard_error`, `build-gate`, `build_gate`, `push-gate`, `push_gate`, `advisory`, `warn`, `warning`, `info`, `info-only` or `info_only`; a gate key takes `pass`, `off`, `info`, `warn`, `warning` or `error`. Case and surrounding space do not matter. Skipping one would run that lint on its defaults, or switch it off, and report success over a configuration nobody applied.

Which crates a lint binds, on the same table again:

```toml
[lints.no-std]
crates = ["{prefix}-dirs", "{prefix}-config"]
exempt_crates = ["{prefix}"]
```

Globs over the crate name, with the same syntax as the path patterns and `{prefix}` standing for the project's crate prefix. `crates` admits, `exempt_crates` removes and wins, and a lint naming neither binds every crate. A path filter cannot do this job, since a package lint's paths are relative to its crate and every crate has a `src/lib.rs`; this is what lets a workspace hold a `no_std` crate beside a `std` one and bind the no-std lints to the first alone. As with the path keys, naming a lint here is enough to ask for it.

The two keys reach per-package lints, which are skipped for a crate they do not bind, and cross-package lints, which are handed only the crates they bind and are skipped when that is none. **They are read and then ignored for repository lints**, the same carve-out as the path keys, and for the same reason.

One line at a time, `// lint:allow(<name>) reason: <why>` keeps a finding of that lint from that line, with `<name>` the name the lint is configured under. The runner honours it for every per-package and cross-package lint, whether or not the lint knows markers exist: on the line itself; on the plain `//` comment lines directly above the item the line belongs to, where a line inside a bracket a line above opened, such as a parameter of a wrapped signature, belongs to the item that opened it; or, where the line opens a block, on the plain comment lines directly below it, which is where rustfmt puts a marker written after a signature's `{`. A line with balanced brackets above it is its own item, so a marker over one struct field, match arm or chained call does not reach the next, and a blank line, an attribute, a doc comment or a finished statement ends the reach. The comma form `lint:allow(a, b)` names several lints at once.

Two things a marker does not reach. A finding about the crate as a whole, such as `file-size` naming a file in its message, has no line a marker could sit on and stands; the way to change that is the lint's own configuration. And a lint that reads the marker itself as an input, such as `no-self-define`, which demands an explanation of a certain length beside it, decides on its own terms: a bare marker there is a finding, not an allow.

Levels: `off`, `info`, `warn`, `error`. The five built-in design-round lints (`changelist-required`, `changelist-doc-gate`, `changelist-lock`, `changelist-immutability`, `changelist-seal`) are on at `error` by default, and a project turns one down only by saying so in its own config. `changelist-seal` refuses, in the active round, a locked changelist holding nothing but a bare title and an unlocked changelist beside a locked one of its kind; in a closed round directory, any unlocked changelist and any bare-title lock not yet committed at that path. Files a closed round already committed are not read. `lock` refuses to lock a bare title and `close` refuses a round the lint would, since `--auto-commit` commits without running a hook.

The v2 source-level directive vocabulary (`lint:allow`, `lint:scope-add`, `lint:defer`, `lint:file-disable`, `lint:prop`) and the `[primitive-introductions]` retirement are covered in [`MIGRATION-v1-to-v2-lints.md`](MIGRATION-v1-to-v2-lints.md). Consumers picking up the v2 engine should read it once per repo.

Forbidden-imports scope rules:

```toml
[lints.forbidden-imports]
commit = "warn"
build = "error"
push = "error"
rules = [
    { scope = "my-core", forbidden = "std::*, alloc::*", reason = "no_std, zero deps" },
    { scope = "*",       forbidden = "f32, f64",         reason = "use fixed-point" },
]
```

The `{prefix}` placeholder in `scope` / `forbidden` / `reason` expands to `crate_prefix`.

### Graph styling

```toml
[crate_colors]
primitives = "#E8EEF7 | #3A6EA5"    # "bg | fg"

[macro_styles]
define_thing = "thing | ⚙ | #FFF | #000"
```

## Lint pipeline reference

Three sources contribute rules:

1. **Built-in lints** from `mockspace_lint_rules` (sibling crate under `lint-rules/`). Universal quality lints (`no-empty-crate`, `file-size`, `undocumented-type`, etc.) and design-round state-machine lints.
2. **Custom lints** in `mock/lints/<name>.rs`. A file defines any of four entry points, and may define more than one:

   | Function | Kind | Handed |
   |---|---|---|
   | `pub fn lint()` | per-package | one package's sources |
   | `pub fn cross_lint()` | cross-package | every package at once |
   | `pub fn repo_lint()` | repository | paths, and no packages |
   | `pub fn message_lint()` | message | an authored commit message or PR body |

   `repo_lint()` is the one to reach for in a repository that has no packages, since it is the only kind whose input does not come from the package list. The engine discovers whichever are present and compiles them into one cdylib alongside the built-in lints. A run of one project tool (`mock <tool>` where `<mock>/tools/<tool>` holds a `Cargo.toml`) compiles only that tool's crate: a tool is handed paths and arguments and no lint pack, so the lints and the other tools are not built for it. That library is keyed by the tool's name and never moves `shared-dir`, so `mock test` still runs the lints of the whole build. The speedup rests on a shared cargo target directory (`CARGO_TARGET_DIR`): without one, each tool is its own key directory with its own release target, a cold dependency build for every tool run for the first time. A name that a `[lint-crates]` pack also registers is found by the whole runs, which record such names in `<mock>/target/mockspace-lints/duplicate-tools`; a run of the one tool reads that file and exits 2 when its name is in it, and takes one whole build when the file is absent.

   The generated crate and its build are not under the worktree. They live in a store shared by every worktree of the repository: `$CARGO_TARGET_DIR/mockspace-lints/` when that variable is set (a relative value is read against the repository root), else `$XDG_CACHE_HOME/mockspace/lints/`, else `~/.cache/mockspace/lints/`. A machine where none of those can be named gets an error and not a shared temporary directory. One directory per key holds one crate, where the key is a hash of the origin url, the mock directory below the repository root, the engine's lint-rules dependency and each pack at its resolved revision and, for a run of one project tool, that tool's name. Worktrees with the same key share the dependency build and take a lock for the write, the build and the load, so two runs at once never load each other's lints; a run rewrites the crate to its own lint sources, which recompiles that one crate. `<mock>/target/mockspace-lints/shared-dir` records which directory the worktree last wrote, and `cargo mock test` tests the lint crate only while that worktree is still the one that wrote it, and fails saying so when another has rewritten it since.

   `cargo mock clean` does not touch the store. Each run removes the directories whose recorded worktree no longer exists, and those unused for fourteen days; a directory a run holds the lock of is never removed. To free one by hand, delete the directory `shared-dir` names.
3. **Config-driven rules** under `[lints.forbidden-imports]` and friends in `mockspace.toml`.

Each lint has a `commit` / `build` / `push` level. Violations at `error` fail the pipeline; `warn` prints without failing; `info` is purely informational.

### Reach and saved verdicts

A lint says how much it reads by overriding `Lint::reach`: `File` (one source file and the settings it was configured with), `Crate` (one package's sources), `Workspace` (every package's sources) or `Repository`, which is the default and means anything in the repository. A run saves what each lint found under a digest of exactly what its reach covers, its configured parameters and path filter, and the engine and every loaded pack by content. The next run replays a saved verdict whose digest has not moved and runs the lint only where it has. Severity is applied after the replay, so changing it never invalidates a verdict. A `lint:allow` marker is applied after the replay too, but it sits in the file text, so adding or removing one changes that file's digest and the file is judged again. A lint that declares nothing is `Repository` and runs every time, so declaring a reach is an opt in. A declaration that is narrower than what the lint reads can replay a stale answer; the check below exists to catch that, and it only moves the inputs it knows about.

The declaration is checked. `mockspace_lint_rules::reach_check` runs a lint over a corpus, changes what it claims not to read (sibling files, a design document, the workspace directory, another package, all at once, against one reference run) and fails if the findings move. It is a sample, not a proof, and a lint that finds nothing on the corpus fails the built-in test, since it would pass trivially. Every built-in lint that declares a reach goes through it, and a pack should put its own declarations through the same two functions in its tests. A lint declaring `File` must also return `true` from `per_file`, and findings must come out in a stable order, since a replay returns them in the order they were saved.

A repository lint is never saved whole, because its reach is the widest there is. One that spends its time on separable items (a shader compiled, a file read against a table) calls `RepoContext::remembered` per item with everything that item's verdict depends on, and `compute` runs only on a miss. The lint's configured parameters, path filter and origin are mixed into every key by the engine. Leave any other dependency out of the parts and a stale verdict replays as fresh, so parts that are shared by every item (a table of replaced rows, a tool version) belong in each item's parts.

A lint whose items are slow and independent (one external process each) calls `RepoContext::remembered_batch` instead, with one list of parts per item and a `compute` that judges item `n`. Every key is asked of the store first, each distinct missing key is computed once on the machine's threads, and the findings come back per item in the order given, so `compute` must be safe to run at the same time as itself. Its keys are `remembered`'s, so the two forms replay each other's verdicts.

The registry schema check (`taplo check`) is replayed the same way, counted as `registry-schema-check` in the timing report. Its key holds taplo's version output, the generated config, the generated schemas and every `.toml` under the repository root, `target/` and `.git/` and symlinked directories included, since taplo examines all of them. Only a pass is saved, and only if the inputs are unchanged when taplo has finished. There is no key, so taplo runs, when the root `.taplo.toml` is not exactly the generated one, a `taplo.toml` sits beside it, `TAPLO_CONFIG` or `RUST_LOG` is set, or a `.toml` file has a `#:` comment, does not parse, or has a root `$schema` key.

The store is `$MOCKSPACE_VERDICTS_DIR`, else `$XDG_CACHE_HOME/mockspace/verdicts`, else `~/.cache/mockspace/verdicts`, one store for the machine, shared by every repository. It is content addressed and never committed, deleting it costs one full run, and entries not accessed for fourteen days are swept (on a mount that keeps no access times that means written fourteen days ago, which costs a rerun and nothing else). `MOCKSPACE_NO_VERDICTS=1` turns it off for a run, which is a full run, and `MOCKSPACE_TIMINGS=1` prints where a run spent its time and how many verdicts each lint replayed (`hit:<lint>`) or computed (`miss:<lint>`). `cargo mock lints` lists each lint's reach.

`single-source` is the cross-package lint for one concept defined in more than one package. It reads every source file of every package, following `mod` declarations from the package roots, so a definition in a module file, a nested or `#[path]` module or a `cfg`-gated module is found; test code is exempt. Only items that are plain `pub` are compared across packages; a private, `pub(crate)`, `pub(super)` or `pub(in ..)` item belongs to its own package. It reports two finding kinds, `type` (the same struct, enum, trait or union name in two packages) and `constant` (the same `const` or `static` name, type and value in two packages), and `[lints.single-source.findings]` can set their severities apart. A constant is compared only when its value is built from literals, so a value that names something is never reported. The module doc of `lint-rules/src/single_source.rs` lists what it cannot see.

## Generated documentation

Every `cargo mock` run regenerates:

- `docs/STRUCTURE.md` plus `docs/STRUCTURE.GRAPH.{dot,png,svg}`: per-crate item index and dependency graph. SVG / PNG rendering requires `dot` from graphviz; a warning is printed if it is not installed.
- `docs/DESIGN.md`: rendered from `mock/DESIGN.md.tmpl`.
- `docs/DESIGN-DEEP-DIVES.md`: aggregated per-crate deep-dive content.
- `docs/<crate>/overview.md` plus `docs/<crate>/deep-dive.md`: per-crate.
- Any other top-level `mock/*.md.tmpl` is rendered to `docs/` with a generation header.

## Git hooks

Mockspace never touches `.git/hooks/`. It generates parallel hooks under `mock/target/hooks/` that source the user's existing `.git/hooks/<name>` first, then run their own validation. Activation is explicit (`cargo mock activate`) and reversible (`cargo mock deactivate`); when deactivated, git falls back to whatever the user already had.

## Agent template generation (optional)

> ## A note on coding agents
>
> We do not recommend using coding agents with mockspace-managed codebases. Mockspace exists because design discipline is hard to enforce mechanically, and that discipline does not transfer cleanly to a system that has been trained primarily on patterns where source is the authority and design is post-hoc. Models default to writing source first and treating docs as documentation; mockspace inverts that. Expect friction.
>
> If you still choose to use a coding agent:
>
> - Be aware of the environmental and social impact of large-scale model inference. Minimise agent use where it is not needed. Be responsible.
> - Only use an agent if you yourself understand the architecture. Do not use an agent because you do not understand; you will waste time and energy, both yours and the planet's.
> - The agent template surface this section describes lets you encode your project's actual rules in one place that emits to every supported assistant. It helps but does not eliminate the problem. You will still need to correct the agent frequently.
>
> The recommendation stands: do this work yourself unless you know what you are doing and why.

If `mock/agent/` is populated, `cargo mock` generates configuration for common AI coding-assistant integrations from the templates it contains.

Source templates:

```
mock/agent/
├── MAIN.md.tmpl                  [main instructions]
├── PREAMBLE.md.tmpl              [optional, prepended to main]
├── POSTAMBLE.md.tmpl             [optional, appended to main]
├── rules/<name>.md.tmpl          [scoped rules]
├── skills/<name>/SKILL.md.tmpl   [named skills]
├── agents/<name>.md.tmpl         [sub-agent personas, Claude only]
└── hooks/<name>.sh.tmpl          [pre-tool-use guards]
```

Personas under `agents/` render to `.claude/agents/<name>.md`. They are the one output with no cross-platform counterpart, because no other supported integration has an equivalent concept. Two properties are specific to them:

- **Their frontmatter passes through untouched.** Rules and skills declare mockspace's own field names and get their frontmatter rebuilt on the way out, which is what keeps one source emitting matching output to several platforms. A persona's frontmatter is already the target's own schema, and that schema is not mockspace's to define, so it is copied verbatim. A field mockspace has never heard of survives and works.
- **Bookends are not applied.** A persona is a character definition read whole; wrapping it in the project preamble and postamble would dilute it. Variable substitution still runs across the whole template, frontmatter included, so a persona can reference `{{project_name}}` like any other template. Pass-through means mockspace does not rename or drop fields, not that it refuses to expand variables inside them.

A template with no frontmatter is skipped with a message, since a persona needs at least `name:` and `description:` to be registered at all.

Each template may use `{{HOOK_HELPERS}}` to get platform-appropriate helper functions substituted in (so a single template produces semantically equivalent output for each supported integration). Hook templates may declare matcher frontmatter:

```bash
#!/usr/bin/env bash
# @matchers: Bash, Write, Edit
```

Five built-in hooks are always generated, to both `.claude/hooks/` and `.github/hooks/`: `check-message.sh` (submits authored messages, commit and pull request text among them, to the configured message lints), `mockspace-write-guard.sh` (gates writes under `mock/` by the phase of the design round), `mockspace-reminder.sh` (prints the rules reminder before a Bash command that touches mockspace paths, never blocking), `no-yagni-guard.sh` (flags YAGNI reasoning in commit messages) and `mockspace-gate.sh` (blocks `git commit` and `git push` while the gate is broken). The write guard lets a Write or Edit through to a path git lists as unmerged while a merge is in progress, in any phase; the match is on the whole path, so a directory holding a conflict stays gated, and so does every Bash command that writes.

A repository with `[lints.forbidden-imports]` rules also gets one `forbidden-imports.sh` hook beside them, registered under `Write`, `Edit` and `MultiEdit` in the generated Claude settings and under `create|edit` in `.github/hooks/hooks.json`. It hands the text a Write or Edit is about to put in a Rust source file to `mock check-edit`, which asks the `forbidden-imports` lint about the file as it would read afterwards and refuses when the lint would block it, so the verdict is the lint's, scope and all. A finding elsewhere in the crate is that crate's and not the edit's; the file itself is judged whole, as the commit gate judges it, so the same text gets the same answer at the editor and at the commit gate (the hook never passes `--gate`, so it answers by the commit severities). The comparison is with the unscoped `mock --lint-only --commit` run. The installed pre-commit hook narrows that run to the crates it finds staged, git stages the real path of a file kept in a linked directory, and a nuked workspace is linted as documentation only, so in those cases the pre-commit run can pass what the editor refused. On Copilot the hook reaches `create` and `edit` only: an `apply_patch` or `str_replace_editor` edit is not judged at the editor, and the commit gate still judges it. A crate the gate never reads gets no verdict from the hook either: the gate takes a real directory holding `src/lib.rs`, so a crate with only a binary root, or only `src/bin`, a directory with no root yet, and a crate directory that is a link are all passed, and the edit that writes the first `src/lib.rs` is judged on its text. A file reached by several crates through links is refused when any of them would block it, as the gate lints it in each. A hook named after a lint runs that lint; whatever else a hook judges is a hook under its own name.

### Agent config: `mock/agent/config.toml`

Optional. Configures agent-integration behaviour. Empty defaults when the file is absent.

```toml
[attribution]
# Byline policy when ${PROJECT_UPPER}_AGENT_MODE is unset or "assistant".
# Empty (default): NO Co-Authored-By lines permitted; human is sole author.
# Non-empty glob pattern: bylines matching this pattern are accepted.
non_autonomous = ""

# Byline policy when ${PROJECT_UPPER}_AGENT_MODE=autonomous.
# Empty: autonomous mode errors with a configuration message at hook time.
# Non-empty glob pattern: commits must carry at least one matching byline.
autonomous = ""
```

Glob patterns use bash `[[ == ]]` matching semantics: `*`, `?`, `[...]`. Patterns without wildcards are literal equality. Mockspace has no hardcoded defaults for agent names, emails, or byline formats; consumers configure what their workflow expects. Every value in `config.toml` applies equally to every supported agent platform.

The `commit-msg` and `pre-push` hooks hand a pack's message lints the commit's author and committer as well as its message, read from `git var` at the commit and from each pushed commit at the push. A lint that judges bylines can then judge an agent identity under the same pattern, so a mode that permits no byline permits no agent author or committer either. When git cannot name either, the `commit-msg` hook blocks with the reason, and the engine refuses a pushed record whose header does not parse, so neither half of the gate falls open alone. A pull request body has neither and is handed none. How a lint pack recognises an agent is its own. A review sweep that reads an author, a committer or a `Co-Authored-By` out of a repository names an agent by the same rules as the pack that judges the commit at the gate, and both are held to one table of people and agents, `lint-rules/data/agent_identity_conformance.tsv`, so an identity is an agent to both or to neither.

## What mockspace does not do

- It is not a project scaffolder. It assumes `mock/` already exists with a `mockspace.toml` and at least one crate.
- It does not touch `.git/hooks/`. User hooks are preserved in every mode.
- It does not push to remote or create commits, except opt-in via `--auto-commit` on transition subcommands, and even then only the renamed files, leaving the working index alone. That commit skips hooks and signing; see the note above the pipeline section.
- It does not impose content opinions. Crate layouts, naming, lint rules specific to a domain: all yours.
