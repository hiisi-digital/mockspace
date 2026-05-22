# The `cargo mock` command surface

Mockspace exposes its operations through a `cargo mock` alias. The
bootstrap installs `[alias] mock = "run --manifest-path
mock/Cargo.toml --bin mock --"` so `cargo mock <subcommand>` from
the repo root resolves to the mockspace binary.

This file covers what's shipping today. Newly added subcommands
land here; retired ones leave.

## Bootstrap commands

### `cargo mock status`

Read-only. Reports the bootstrap state of the current repo:
whether the cargo alias is installed, whether git hooks point at
`mock/target/hooks/`, whether `mock/` exists.

Exit 0 always; the report is the output.

### `cargo mock install`

Writes the bootstrap state into the current repo:
- Adds `[alias] mock = ...` to `.cargo/config.toml`.
- Sets `core.hooksPath` to `mock/target/hooks/`.
- Writes the canonical git hook scripts under `mock/target/hooks/`.
- Extracts the canonical agent rule files to `mock/target/agent/`.

Idempotent. Re-running on an already-installed repo prints
"already installed" and exits 0.

### `cargo mock uninstall`

Reverses `install`. Removes the alias, the `core.hooksPath` setting,
and the contents of `mock/target/hooks/` and `mock/target/agent/`.
The `mock/` directory itself stays (it carries the consumer's
round content).

### `cargo mock refresh`

Functionally identical to `install`. Named separately so
drift-repair has a distinct command-line affordance. Overwrites the
hooks and agent rule extracts with the canonical content for the
installed mockspace version.

## Engine commands

### `cargo mock check`

Runs the lint engine against the repo. Surfaces findings to stdout
in `<file>:<line>:<col>: [<severity>] <name>: <message>` format
(or `--json` for machine consumers).

Flags:
- `--gate <commit|build|push>`: which severity gate to evaluate at.
  Default `commit`. Pre-commit hooks use `commit`; CI uses `build`;
  pre-push uses `push`.
- `--json`: emit findings as a JSON array.
- `--surface <local|ci|editor>`: run surface. Default `local`. `ci`
  simulates a CI run; `editor` is LSP-shaped.

Exits non-zero if any finding is classified as error at the chosen
gate.

### `cargo mock explain <lint>`

Reports how the named lint resolves through the config cascade.
Prints catalog defaults plus any user TOML override seen for that
lint. Full per-layer breakdown (presets, workspace defaults, CLI
overrides) lands progressively as the cascade implementation
completes.

## Phase transition commands

### `cargo mock phase plan <slug>`

Opens PlanDoc on a Topic-phase round. See `phases.md` and
`verbs.md` for the full state machine.

### `cargo mock phase apply <slug> --source-tip <hex>`

Seals the current authoring manifest and transitions to the
matching APPLY phase. The transition validity matrix lives in
`mockspace-core::transition`; the I/O sequence (read manifest,
hash claimed files, write anchor TOML and blobs, rename manifest
to `.locked` form, bump phase marker, push with CAS) is the Phase 5
executor and lands progressively under
`mock/research/202605220843_phase-5-io-slice-plan.md`. Requires the
source-side branch tip OID as a hex SHA so the anchor records a
stable input for restoration.

### `cargo mock phase finish <slug>`

Advances past APPLY. From ApplyDoc lands in PlanSrc; from ApplySrc
lands in Done.

### `cargo mock phase replan <slug> [--mode <mode>] [--accept-loss-path <path>]...`

Deprecates the current sealed manifest and returns to PLAN on the
same side. See `verbs.md` for the three mode options.

### `cargo mock close <slug>`

Archives a Done-phase round into `refs/mock/round-archive` and
deletes the source round ref. Idempotent on partial-success
(source-ref delete failure can be retried; the archive write is
already content-addressed).

## Migration

### `cargo mock migrate`

Walks the v1 mockspace state under `mock/design_rounds/` (the
filesystem layout from before the v2 ref-based storage) and prints
a per-round migration report. Each round shows its v1 state, the
target v2 phase, and the action needed.

Print-only today. Auto-conversion is a future slice.

## What's not yet shipped

- `cargo mock task {create|list|close|move}`: task identity commands.
- `cargo mock check --fix`: auto-fix integration. The fix recipes
  exist in the catalog; the CLI wiring lands later.
- Network-side commands (push CAS for round refs).

Each is tracked under task #579 / #580 in the project's task tree.

## Exit codes

- `0`: success.
- `1`: command-level failure (verifier finding at gate, validation
  error, IO error, transition-validity refusal).
- `2`: usage error (bad flags, unknown subcommand).

The pre-commit and pre-push hook scripts treat exit code 1 from
`mock check` as commit-blocking; exit code 2 also blocks but
indicates a config error rather than a lint finding.
