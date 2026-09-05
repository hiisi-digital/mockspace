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
    │   ├── <timestamp>_topic.<name>.md
    │   ├── <timestamp>_research.<name>.md
    │   ├── <timestamp>_changelist.doc[.lock|.deprecated].md
    │   ├── <timestamp>_changelist.src[.lock|.deprecated].md
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

Filenames use a compact timestamp prefix:

```
YYYYMMDDHHMM_{topic|research|changelist}.{name|doc|src}[.lock|.deprecated].md
```

Transitions are subcommands, not manual renames:

| Command                   | Effect                                                            | Valid phases            |
|---------------------------|-------------------------------------------------------------------|-------------------------|
| `cargo mock lock`         | Lock the active changelist (`DOC -> DRAFT`, `IMPL -> CLOSED`)     | `DOC`, `IMPL`           |
| `cargo mock unlock`       | Deprecate src CL and unlock doc CL (destructive; source unchanged) | `DRAFT`, `IMPL`, `CLOSED` |
| `cargo mock deprecate`    | Deprecate the active CL; in `IMPL` also unlocks doc CL            | `DOC`, `IMPL`           |
| `cargo mock close`        | Archive the round into a timestamped subdir with `.meta` + `.history` | `CLOSED` only       |
| `cargo mock archive`      | Archive an abandoned round into a `<timestamp>-abandoned/` subdir | any                     |
| `cargo mock migrate`      | Rename legacy `YYYY-MM-DD_*.md` files to the compact format       | any                     |

All transition subcommands accept `--auto-commit`, which commits only the renamed files. A temporary `GIT_INDEX_FILE` is used, so whatever is staged stays staged and is not swept into the commit.

**That commit is written with git plumbing, so no hook runs and it is not signed.** `commit-tree` plus `update-ref` is what builds it, which means `core.hooksPath`, `pre-commit`, `commit-msg` and `commit.gpgsign` are all bypassed. If a repository relies on any of those, do the commit by hand: without the flag the subcommand prints the exact command to run.

The "## Comparison to deprecated changelist" section of the active CL plus `BACKLOG.md.tmpl` together are the repository's memory of decisions not yet implemented.

### The rest of the surface

Those are the transitions. The subcommands that are not transitions:

| Command | Effect |
|---|---|
| `cargo mock status` | The current round, its phase, and what may be edited right now |
| `cargo mock check` | Readiness report: git state, phase, build, tests, lints |
| `cargo mock check-message` | Lint one commit message, pull request body or comment against the configured policy |
| `cargo mock query` | Query the registry |
| `cargo mock bench` | Run the bench harness |
| `cargo mock test` | Run the tests of every tree mockspace owns, not only the workspace members |
| `cargo mock panel` | Mint or consolidate a panel seat, or report a panel's state |
| `cargo mock pdf` | Render the design documents to PDF |
| `cargo mock clean` | Remove generated output |
| `cargo mock activate` / `deactivate` | Point `core.hooksPath` at the gate, or hand it back |
| `cargo mock tools` | Every subcommand and project tool, with usage |
| `cargo mock lints` | Every lint that runs here, builtin and pack alike, the severity this project gave it, and where the two sides disagree |
| `cargo mock ask <question...>` | The passages of this project's own writing that answer a question in words, quoted under their file and lines, for a project that carries a `[corpus]` table |

**`cargo mock tools` and `cargo mock lints` are the lists that cannot go stale**, since they read what the binary actually ships and what the engine and the loaded pack actually register between them, none of which this table can know about. Prefer them over this table wherever the two might disagree.

## `cargo mock` pipeline

Running `cargo mock` with no subcommand:

1. Runs the bootstrap health-check and regenerates anything stale.
2. Runs `cargo check` inside `mock/`.
3. Parses every crate under `mock/crates/` via tree-sitter.
4. Runs the lint pipeline with per-gate severity for the current mode.
5. Runs dylib ABI checks for crates listed under `module_crates`.
6. Regenerates everything in `docs/` from templates and parsed crate data.
7. Regenerates agent integration files if `mock/agent/` is populated.
8. Builds the corpus index under `mock/.muisti/` if `mockspace.toml` carries a `[corpus]` table, incrementally, so the documents this run generated and the round it closed are in the index it leaves behind. A failure here, a machine that cannot reach the model hub being the usual one, is reported and does not fail the run.

Common flags:

| Flag                | Effect                                                       |
|---------------------|--------------------------------------------------------------|
| `--lint-only`       | Run lints only; skip generation and dylib checks             |
| `--doc-only`        | Skip source lints; run only doc-related checks               |
| `--scope <csv>`     | Restrict lints to named crates (comma-separated)             |
| `--scope infra`     | Infrastructure-only mode (no crate check, no crate lints)    |
| `--commit`          | Lint mode: `Commit` (typically most permissive gate)         |
| `--strict`          | Lint mode: `Push` (strictest gate)                           |
| `--nuke`            | Take the source down, leaving the designs. Names every file, then asks. |
| `--nuke=docs`       | Take the designs and the source under them, which is the order the chain wants. |
| `--y`               | Answer a `--nuke` in advance, for a second run and for a script. |
| `--dir <path>`      | Override auto-discovered mock dir                            |

The generated `pre-commit` hook runs `cargo mock` scoped to changed crates with `--commit`. The generated `pre-push` hook runs `cargo mock --lint-only --strict`.

This table (and the one above it) is not the exhaustive command reference; run `mock tools` for that, or `mock tools --long` for full usage and declared arguments per command. It never goes stale the way a hand-written list does, because it is computed from the same declarations the engine dispatches against.

## Panels

`mock ask <question...>` puts a question in words to an index over the mock dir: the rounds, the research, the registries, the templates and the source, lexical and dense retrieval fused, with every passage that clears the threshold quoted under the file and lines it came from, or a refusal with the near misses under it. It is for the question that does not know its key, where `mock query` is for the row that has one. The index lives at `<mock>/.muisti/`, ignores itself, and is rebuilt by every bare `cargo mock` run for a project whose `mockspace.toml` carries a `[corpus]` table; a project without one has no index directory at all. The embedder is `potion-base-8M`, fetched once into `~/.cache/muisti` and shared with anything else built on the same crate, and the answer is extractive: the passages themselves and never a paraphrase, since the engine carries no generator. A project that opts in also ships a generated agent rule saying to ask before grepping.

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

install_git_hooks = "replace"
install_cargo_config = "merge-append"
install_agent_files = "replace"

auto_fmt = true                       # rustfmt the staged files pre-commit
auto_clippy_fix = true                # cargo clippy --fix staged workspace roots pre-commit
deny_check = true                     # cargo deny check on push (needs a deny.toml)

canon_paths = ["mock/canon/**"]       # what `mock check` protects from an open panel
panel_consolidate_every = 10          # seats a panel mints before a consolidation is due

[corpus]                              # the table is the opt-in; every key under it is optional
threshold = 0.75                      # the fused score a passage reaches before `mock ask` quotes it
include = []                          # globs over the mock dir; with any, only what one matches is indexed
exclude = []                          # globs that win over the includes
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

The fixers are scoped to the packages a commit actually touches (each staged file's
nearest `Cargo.toml`), not the whole workspace, so the cost stays proportional to the
change: `cargo clippy --fix` only compiles the changed packages.

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

It is skipped (never blocking) when there is no `deny.toml` or cargo-deny is not
installed (`cargo install cargo-deny`). Set `deny_check = false` to opt out.

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

Levels: `off`, `info`, `warn`, `error`. The four built-in design-round lints (`changelist-required`, `changelist-doc-gate`, `changelist-lock`, `changelist-immutability`) are always on and non-negotiable.

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

   `repo_lint()` is the one to reach for in a repository that has no packages, since it is the only kind whose input does not come from the package list. The engine discovers whichever are present and compiles them into one cdylib alongside the built-in lints.
3. **Config-driven rules** under `[lints.forbidden-imports]` and friends in `mockspace.toml`.

Each lint has a `commit` / `build` / `push` level. Violations at `error` fail the pipeline; `warn` prints without failing; `info` is purely informational.

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

Two built-in hooks are always generated: `check-byline.sh` (commit authorship policy, controlled by `${PROJECT_UPPER}_AGENT_MODE`) and `mockspace-write-guard.sh` (blocks writes to generated files from outside `mock/`).

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

## What mockspace does not do

- It is not a project scaffolder. It assumes `mock/` already exists with a `mockspace.toml` and at least one crate.
- It does not touch `.git/hooks/`. User hooks are preserved in every mode.
- It does not push to remote or create commits, except opt-in via `--auto-commit` on transition subcommands, and even then only the renamed files, leaving the working index alone. That commit skips hooks and signing; see the note above the pipeline section.
- It does not impose content opinions. Crate layouts, naming, lint rules specific to a domain: all yours.
