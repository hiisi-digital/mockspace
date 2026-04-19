# mockspace

**A design-round workflow engine for Rust projects. Opinion-free about what you build; opinionated about how the design converges.**

Mockspace lives alongside your project in a `mock/` directory. It enforces a file-based state machine that walks a round from *topic discussion* through *doc changelist* and *src changelist* to *closed and archived*. It generates documentation and dependency graphs from your design templates and crate source, runs a configurable lint pipeline on every build, and installs git hooks that validate commits against the current design-round phase.

## Why

Small projects drift. Large ones fragment. Mockspace exists because:

- **Design belongs before code, and stays as the authority.** When source and design disagree, the source is wrong. The tool makes the design the canonical artifact and makes code regenerable from it (see `cargo mock --nuke`).
- **One source, many outputs.** Templates under `mock/` become a rendered documentation tree under `docs/`, a crate dependency graph, and a consistent agent-tooling integration — all from the same edits.
- **Transitions are the only way forward.** Changelist status is encoded in filenames. State changes go through subcommands that rename files, validate invariants, and can commit surgically on request. Hand-editing file suffixes is how state machines rot.
- **Invariants are linted, not advised.** Mockspace ships a lint framework with per-gate severity (commit / build / push) so a rule can be a warning at commit and a hard error at push. Consumers add their own rules in `mock/lints/`.

None of this is strongly opinionated about your project's *content* — crate structure, naming, numeric discipline, framework choice are all yours. Mockspace only cares that whatever you decide is captured, reviewed, locked, and regenerable.

## Install

Add a one-liner `build.rs` to any crate inside your mock workspace:

```rust
fn main() { mockspace::bootstrap_from_buildscript(); }
```

and the matching dependency to that crate's `Cargo.toml`:

```toml
[build-dependencies]
mockspace = { git = "ssh://git@github.com/hiisi-digital/mockspace.git" }
```

On the first `cargo check` inside the mock workspace, `bootstrap_from_buildscript()` writes a cargo alias (`cargo mock`) into `.cargo/config.toml`, generates a proxy crate under `target/mockspace-proxy/`, and materialises git hooks into `mock/target/hooks/`. It is idempotent — subsequent builds re-run the same checks but do no work if everything is current.

Activate hooks explicitly:

```bash
cargo mock activate
```

This sets `core.hooksPath` to the generated hooks dir. Hooks installed this way always source the user's existing `.git/hooks/` first, so your personal setup keeps working. Deactivate with `cargo mock deactivate`.

## The design round

A *round* is the unit of design work. Every round passes through five phases, and each phase is detected from the presence and status-suffix of files in `mock/design_rounds/`:

| Phase       | Meaning                                        | Filename state                                                  |
|-------------|------------------------------------------------|-----------------------------------------------------------------|
| `TOPIC`     | Exploration; no changelist yet                 | only `*_topic.*.md` or `*_research.*.md` files                  |
| `DOC`       | A doc changelist is active; docs are changing  | `*_changelist.doc.md` exists                                    |
| `SRC-PLAN`  | Doc changelist locked; src changelist not made | `*_changelist.doc.lock.md` exists, no `*_changelist.src.md`     |
| `SRC`       | Src changelist active; source is being written | `*_changelist.doc.lock.md` and `*_changelist.src.md` both exist |
| `DONE`      | Both changelists locked; ready to archive      | `*_changelist.doc.lock.md` and `*_changelist.src.lock.md`       |

Filenames use a compact timestamp prefix:

```
YYYYMMDDHHMM_{topic|research|changelist}.{name|doc|src}[.lock|.deprecated].md
```

Transitions are subcommands, not manual renames:

| Command                   | Effect                                                            | Valid phases            |
|---------------------------|-------------------------------------------------------------------|-------------------------|
| `cargo mock lock`         | Lock the active changelist (`DOC → SRC-PLAN`, `SRC → DONE`)       | `DOC`, `SRC`            |
| `cargo mock unlock`       | Deprecate src CL and unlock doc CL (destructive; source not reverted) | `SRC-PLAN`, `SRC`, `DONE` |
| `cargo mock deprecate`    | Deprecate the active CL; in `SRC` also unlocks doc CL             | `DOC`, `SRC`            |
| `cargo mock close`        | Archive the round into a timestamped subdir with `.meta` + `.history` | `DONE` only         |
| `cargo mock migrate`      | Rename legacy `YYYY-MM-DD_*.md` files to the compact format       | any                     |

All transition subcommands accept `--auto-commit` to create a surgical git commit of only the renamed files, using a temporary `GIT_INDEX_FILE` so any staged changes remain untouched.

## Directory layout

A mockspace lives in `mock/` at the repo root. Everything outside `mock/` is yours.

```
<repo_root>/
├── .cargo/config.toml                        [generated by bootstrap]
├── docs/                                     [generated; regenerated every build]
│   ├── DESIGN.md, DESIGN-DEEP-DIVES.md
│   ├── STRUCTURE.md, STRUCTURE.GRAPH.{dot,png,svg}
│   └── <per-crate overviews>
└── mock/
    ├── mockspace.toml                        [authored — config]
    ├── Cargo.toml + Cargo.lock               [authored — mock workspace]
    ├── DESIGN.md.tmpl                        [authored — top-level design template]
    ├── PRINCIPLES.md.tmpl                    [optional authored invariants]
    ├── WORKFLOW.md.tmpl                      [optional authored workflow note]
    ├── crates/<name>/                        [authored — real code, plus per-crate DESIGN.md.tmpl]
    ├── design_rounds/                        [authored — round state machine]
    │   ├── <timestamp>_topic.<name>.md
    │   ├── <timestamp>_research.<name>.md
    │   ├── <timestamp>_changelist.doc[.lock|.deprecated].md
    │   ├── <timestamp>_changelist.src[.lock|.deprecated].md
    │   └── <archived-round>/                 [after `cargo mock close`]
    ├── research/                             [optional — round-independent curated artifacts]
    ├── agent/                                [optional — agent integration templates, see below]
    ├── lints/                                [optional — custom lint sources]
    └── target/                               [build artifact, gitignored]
```

## `cargo mock` — the default pipeline

Running `cargo mock` with no subcommand:

1. Runs the bootstrap health-check and regenerates anything stale.
2. Runs `cargo check` inside `mock/`.
3. Parses every crate under `mock/crates/` via tree-sitter.
4. Runs the lint pipeline with per-gate severity applied for the current mode.
5. Runs dylib ABI checks for crates listed under `module_crates`.
6. Regenerates everything in `docs/` — top-level design, structure, dependency graph, per-crate overviews — from templates and parsed crate data.
7. Regenerates agent integration files if `mock/agent/` is populated (see below).

Common flags:

| Flag                | Effect                                                       |
|---------------------|--------------------------------------------------------------|
| `--lint-only`       | Run lints only; skip generation and dylib checks             |
| `--doc-only`        | Skip source lints; run only doc-related checks               |
| `--scope <csv>`     | Restrict lints to named crates (comma-separated)             |
| `--scope infra`     | Infrastructure-only mode (no crate check, no crate lints)    |
| `--commit`          | Lint mode: `Commit` (typically most permissive gate)         |
| `--strict`          | Lint mode: `Push` (strictest gate)                           |
| `--nuke`            | Wipe all crate source; leave stub `lib.rs` files. Design preserved. Reproducibility test. |
| `--dir <path>`      | Override auto-discovered mock dir                            |

The generated `pre-commit` hook runs `cargo mock` scoped to changed crates with `--commit`. The generated `pre-push` hook runs `cargo mock --lint-only --strict`.

## Configuration: `mockspace.toml`

Placed at `mock/mockspace.toml`. The common fields:

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
```

Install modes (applied when generated content overwrites existing files):

- `replace` — always overwrite
- `merge-append` / `merge-prepend` — preserve other sections
- `skip` / `skip-if-exists` — never overwrite an existing file

### Lints

Per-lint severity per gate:

```toml
[lints.no-todo]
commit = "off"
build = "warn"
push = "error"
```

Levels: `off`, `info`, `warn`, `error`. The four built-in design-round lints (`changelist-required`, `changelist-doc-gate`, `changelist-lock`, `changelist-immutability`) are always on and non-negotiable. Consumers add project-specific lints either via `mock/lints/*.rs` (full Rust control) or `[lints.forbidden-imports]` rules in config:

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

The `{prefix}` placeholder in scope/forbidden/reason expands to `crate_prefix`.

### Graph styling

```toml
[crate_colors]
primitives = "#E8EEF7 | #3A6EA5"    # "bg | fg"

[macro_styles]
define_thing = "thing | ⚙ | #FFF | #000"
```

## Linting system

Three sources contribute rules:

1. **Built-in lints** from `mockspace_lint_rules` (sibling crate under `lint-rules/`). Universal quality lints (`no-empty-crate`, `file-size`, `undocumented-type`, etc.) and design-round state-machine lints.
2. **Custom lints** in `mock/lints/<name>.rs`. Each file defines `pub fn lint() -> Box<dyn mockspace_lint_rules::Lint>` (per-crate) or `pub fn cross_lint() -> Box<dyn mockspace_lint_rules::CrossCrateLint>` (cross-crate). Files suffixed `_cross` are treated as cross-crate. Bootstrap discovers them and wires them into the generated proxy crate.
3. **Config-driven rules** under `[lints.forbidden-imports]` and friends in `mockspace.toml`.

Each lint has a `commit` / `build` / `push` level. Violations at `error` fail the pipeline; `warn` prints without failing; `info` is purely informational.

## Generated documentation

Every `cargo mock` run regenerates:

- `docs/STRUCTURE.md` + `docs/STRUCTURE.GRAPH.{dot,png,svg}` — per-crate item index and dependency graph (SVG requires `dot` from graphviz; a warning is printed if it's not installed).
- `docs/DESIGN.md` — rendered from `mock/DESIGN.md.tmpl`.
- `docs/DESIGN-DEEP-DIVES.md` — aggregated per-crate deep-dive content.
- `docs/<crate>/overview.md` + `docs/<crate>/deep-dive.md` — per-crate.
- Any other top-level `mock/*.md.tmpl` is rendered to `docs/` with a generation header.

## Git hooks

Mockspace never touches `.git/hooks/`. Instead it generates parallel hooks under `mock/target/hooks/` that *source* `.git/hooks/<name>` first, then run their own validation. Activation (setting `core.hooksPath`) is explicit via `cargo mock activate` and reversible via `cargo mock deactivate`.

When deactivated, git falls back to `.git/hooks/` and behaves exactly as if mockspace were not installed.

## Agent artifact generation (optional)

If `mock/agent/` is populated, `cargo mock` generates configuration for common AI coding-assistant integrations from the templates it contains. This is one capability among several, not the primary framing.

Source templates:

```
mock/agent/
├── MAIN.md.tmpl                  [main instructions]
├── PREAMBLE.md.tmpl              [optional — prepended to main]
├── POSTAMBLE.md.tmpl             [optional — appended to main]
├── rules/<name>.md.tmpl          [scoped rules]
├── skills/<name>/SKILL.md.tmpl   [named skills]
└── hooks/<name>.sh.tmpl          [pre-tool-use guards]
```

Each template may use `{{HOOK_HELPERS}}` to get platform-appropriate helper functions substituted in (so a single template produces semantically equivalent output for each supported integration). Hook templates may declare matcher frontmatter:

```bash
#!/usr/bin/env bash
# @matchers: Bash, Write, Edit
```

Two built-in hooks are always generated: `check-byline.sh` (commit authorship policy, mode-controlled by a `${PROJECT_UPPER}_AGENT_MODE` env var) and `mockspace-write-guard.sh` (blocks writes to generated files from outside `mock/`).

### Agent config: `mock/agent/config.toml`

Optional. Configures agent-integration behavior. Empty defaults when the file is absent.

```toml
[attribution]
# Byline policy when ${PROJECT_UPPER}_AGENT_MODE is unset or "assistant".
# Empty (default): NO Co-Authored-By lines permitted — human is sole author.
# Non-empty glob pattern: bylines matching this pattern are accepted.
#   e.g. "Jane Doe <jane@example.com>"
#   e.g. "* <*@example.com>"
non_autonomous = ""

# Byline policy when ${PROJECT_UPPER}_AGENT_MODE=autonomous.
# Empty: autonomous mode errors with a configuration message at hook time.
# Non-empty glob pattern: commits must carry at least one matching byline.
#   e.g. "Claude Opus 4.7 <noreply@anthropic.com>"     (exact; locks model)
#   e.g. "Claude * <noreply@anthropic.com>"            (any Claude model)
#   e.g. "*"                                           (any byline — loosest)
autonomous = ""
```

Glob patterns use bash `[[ == ]]` matching semantics: `*`, `?`, `[...]`. Patterns without wildcards are literal equality. Mockspace has no hardcoded defaults for agent names, emails, or byline formats — consumers configure what their workflow expects. Every value in `config.toml` applies equally to every supported agent platform.

## What mockspace does not do

- It is not a project scaffolder. It assumes `mock/` already exists with a `mockspace.toml` and at least one crate.
- It does not touch `.git/hooks/`. User hooks are preserved in every mode.
- It does not push to remote or create commits, except opt-in via `--auto-commit` on transition subcommands (and even then only surgically, without touching the working index).
- It does not impose content opinions. Crate layouts, naming, lint rules specific to a domain — all yours.

## When to use mockspace

Good fit:

- Projects where design decisions are consequential and need a paper trail.
- Multi-crate Rust workspaces that benefit from regenerable structure documentation.
- Teams that want invariants enforced mechanically rather than socially.
- Setups where agent integration is desirable but needs to be consistent across Claude, Copilot, and future assistants from a single source.

Bad fit:

- Single-file scripts, one-crate demos, quick prototypes that don't need the ceremony.
- Projects where design evolves purely through code without a docs dimension.

## License

See `LICENSE` at the repo root.
