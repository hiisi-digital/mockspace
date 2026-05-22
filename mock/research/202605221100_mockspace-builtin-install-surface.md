# Mockspace builtin install surface

> **Superseded by `202605221200_mockspace-builtin-install-surface-revised.md`.**
> The path layout proposed here (`mock/agent/{builtin,consumer}/` as
> tracked content) was reversed shortly after this memo merged.
> Builtins now live at gitignored `mock/target/agent/`, reusing the
> existing `mock/target/hooks/` precedent. The principle (split
> between mockspace-managed and consumer-authored content) carries
> forward; the path layout, marker convention, and migration story
> in this memo are obsolete. Read the revised memo for the current
> design. This file stays as audit trail.

Companion memo to task #581. Captures the architectural split between
mockspace-managed content (ships builtin with the tool, updates on refresh)
and consumer-authored content (the project owns it; mockspace never edits).
Lands before any code so the layout and refresh contract can be reviewed
before content slices follow.

## What ships builtin

Anything whose subject matter is mockspace itself.

- Agent rules describing the six-phase state machine, the four transition
  verbs, the side vocabulary (doc / src), what a manifest is, what an
  anchor is, what `cargo mock` subcommands do, the round / task identity
  shapes, the suppression directives.
- Hook scripts that drive `cargo mock check` at the commit / build / push
  gates. The hook bodies are mechanical (set up env, invoke the binary,
  exit on the result); they exist to wire git's hook contract to the
  mockspace runtime.
- Hook helpers that mockspace itself needs (e.g., the wrapper that
  detects the active workspace root, the script that fans out across
  multi-mockspace layouts when one repo carries more than one).
- The canonical phrasing of the AI-responsibility notice that injects
  into rendered docs (per #245).

These have one source of truth: the version of mockspace installed in
the consumer's `Cargo.toml`. The consumer cannot meaningfully diverge
from this content. If they edit `mock/agent/builtin/phases.md`, the next
refresh overwrites their edits, and they lose nothing real because the
file describes mockspace's own phase machine which the consumer has
no authority over.

## What stays consumer-authored

Anything whose subject matter is the consumer's project.

- Domain-specific lints, lint configuration, severity choices.
- Repo-specific workflows (release cadence, branch protection, CI
  shapes that consumers care about).
- The consumer's own agent rules covering project conventions: type
  surface decisions, primitive choices, no-alloc policies, naming, etc.
- The consumer's own hook scripts that wrap mockspace's hooks (e.g.,
  a pre-commit that runs `cargo mock check` plus the consumer's own
  formatter check).
- Round content, manifests, design rounds, research notes.

These have no source of truth in mockspace. The consumer authors them,
edits them, owns them. Mockspace must never touch this content.

## Path layout

```
mock/
  agent/
    builtin/               # mockspace-managed; refresh overwrites
      phases.md            # what the six phases are
      verbs.md             # what the four verbs do
      sides.md             # doc/src vocabulary, what a manifest is
      anchors.md           # what an anchor captures
      suppressions.md      # comment directives + Rust attr aliases
      commands.md          # `cargo mock` subcommand surface
      identity.md          # round/task slug shapes
      INDEX.md             # generated index of files in this dir
    consumer/              # consumer-authored; refresh never touches
      <whatever-the-consumer-writes>
  target/
    hooks/
      builtin/             # mockspace-managed; refresh overwrites
        pre-commit
        pre-push
        commit-msg
        _helpers.sh        # wrapper functions shared across hooks
      consumer/            # consumer-authored; refresh never touches
        <whatever-the-consumer-writes>
```

Two top-level discriminators per surface: `builtin/` and `consumer/`.
Refresh walks `builtin/` and re-derives every file. Refresh ignores
`consumer/` entirely. Files outside either tree (e.g., a stray file at
`mock/agent/something.md`) are flagged by `cargo mock status` as
unclassified and the consumer is told to move them into the right
subtree.

## Refresh contract

`cargo mock refresh` (and the install-time equivalent in `cargo mock
install`):

1. Walks every file mockspace knows about under `<root>/builtin/`.
2. Writes the current canonical content. Existing files are overwritten
   atomically. Files mockspace no longer ships (e.g., a doc retired in
   a later version) are deleted.
3. Skips `<root>/consumer/` entirely. The consumer's working tree there
   is preserved bit-for-bit.
4. Reports a structured summary: files written, files deleted, total
   bytes, refresh duration. No content shown; the consumer reads the
   files if they want to see what changed.

Atomicity: the install writes a fresh `<root>/builtin/` to a sibling
tempdir and renames at the end. On POSIX this is a true atomic swap
via `rename(2)`; crash during refresh leaves the prior `<root>/builtin/`
intact. On Windows `std::fs::rename` over an existing directory fails,
so the implementation does delete-then-rename which is not atomic; a
crash in the window between delete and rename can leave the consumer
without a `<root>/builtin/` directory until the next refresh recreates
it. Windows atomicity hardening (e.g., `MoveFileExReplace`) is
deferred to a follow-up; the failure mode is recoverable by re-running
refresh, so the gap is documented but not blocking.

Permissions: hook scripts under `target/hooks/builtin/` are written
mode 0755 explicitly. Agent rule markdown stays 0644.

## File-marker convention

Every file mockspace writes carries a top-of-file marker that names it
as builtin. The marker survives content updates and exists so:

- A consumer who copy-pastes a builtin file out of curiosity finds the
  marker and learns the file is mockspace-managed.
- Future tooling (`cargo mock doctor`) can scan for files-with-builtin-marker
  appearing outside `builtin/` directories and flag the drift.
- The refresh logic uses the marker as a sanity check: if a file under
  `builtin/` lacks the marker, refresh refuses to overwrite (something
  went wrong; the consumer should investigate before re-running).

Marker shape per file syntax:

```markdown
<!-- mockspace-builtin: do not edit. file generated by `cargo mock install`. -->
```

```sh
# mockspace-builtin: do not edit. file generated by `cargo mock install`.
```

```toml
# mockspace-builtin: do not edit. file generated by `cargo mock install`.
```

```yaml
# mockspace-builtin: do not edit. file generated by `cargo mock install`.
```

JSON has no comment syntax. JSON files carry a sentinel root key instead:

```json
{
  "_mockspace_builtin": "do not edit. file generated by `cargo mock install`.",
  ...
}
```

Marker is the first non-blank line (or sentinel key) in the file. The
rendering pipeline injects it.

## Migration story

Consumers running pre-#581 mockspace will have hand-authored agent rules
mentioning phases / verbs / hooks under `mock/agent/` (today's flat layout
with no `builtin/` `consumer/` split). The first refresh after upgrading
to #581-aware mockspace handles this:

1. Detect existing files under `mock/agent/` that match mockspace's
   builtin file inventory by filename. For each match, diff against the
   canonical builtin content.
2. If the file is byte-identical (consumer hadn't customized): move it
   under `mock/agent/builtin/<name>` and inject the marker. No content
   change.
3. If the file diverges: do not move. Print a per-file note saying the
   file looks like an old builtin but has been edited. The consumer
   picks one of three resolutions and the steady-state is explicit:

   - **Discard edits:** `rm mock/agent/<name>`, re-run refresh. The
     canonical file lands at `mock/agent/builtin/<name>`. The consumer's
     edits are gone.
   - **Keep edits as consumer content:** `mv mock/agent/<name>
     mock/agent/consumer/<name>`, re-run refresh. The canonical file
     lands at `mock/agent/builtin/<name>` alongside the consumer's
     copy. Both coexist with different paths.
   - **Do nothing:** leave the file at `mock/agent/<name>`. Refresh
     still writes the canonical file to `mock/agent/builtin/<name>`;
     the old file at the flat path stays where it was. `cargo mock
     status` continues to flag it as unclassified on every run until
     the consumer resolves it. The file is harmless but noisy.
4. Files under `mock/agent/` that do not match any builtin inventory
   entry: move them under `mock/agent/consumer/`. They are consumer
   content; mockspace doesn't claim them.

The same logic applies to `mock/target/hooks/`. Existing hook scripts
that match builtin filenames are diffed and either moved or flagged.

## Refresh in v2 commands

`cargo mock refresh` already exists. Today it re-derives the cargo
alias and re-installs hook scripts via `bootstrap::install`. After
#581 it also walks the builtin agent rule surface. Same idempotent
contract, broader scope.

`cargo mock status` extends to report the install-surface state. The
per-file outcomes are:

- **current:** file under `builtin/<name>` exists, has the marker, and
  matches the canonical content for the installed mockspace version
  byte-for-byte. Nothing to do.
- **outdated:** file under `builtin/<name>` exists and has the marker
  but the content does not match the installed mockspace version. The
  fix is `cargo mock refresh`. This is the normal state after a
  mockspace version bump and is not an error.
- **drifted:** file under `builtin/<name>` exists but the marker is
  missing or malformed. Someone edited the file in a way that stripped
  the marker. Refresh will refuse to overwrite (per the marker-as-sanity
  rule); the consumer must restore the marker or delete the file.
- **unclassified:** file lives outside both `builtin/` and `consumer/`
  but its name matches a builtin inventory entry. Migration logic kicks
  in on next refresh.

`cargo mock migrate` (the v1->v2 round-state guide shipped in PR #123)
is not affected; round migration is orthogonal to install surface
upgrades.

## What this memo does NOT cover

- The actual canonical content of the agent rule files. That's the
  next slice: author the markdown describing phases / verbs / sides /
  anchors / suppressions / commands / identity.
- The actual hook script bodies. Those exist today under
  `mockspace-rs::bootstrap`; the slice that wires them into the
  `builtin/` directory inherits them.
- The marker injection mechanism. Implementation detail of the render
  pipeline; lands alongside the content slice.
- Cross-language hook scripts (PowerShell on Windows, etc.). Out of
  scope for now; consumers on Windows handle hooks via WSL or git-bash
  as today.

## Cross-references

- Task #581: ship canonical mockspace agent rules + hooks as builtin
  install surface. Umbrella for this work.
- Task #245: mockspace built-in auto-inject AI responsibility notice
  into rendered WORKFLOW.md + PRINCIPLES.md. Sub-slice of #581.
- Task #246: mockspace built-in canonical workflow description in
  rendered WORKFLOW.md. Sub-slice of #581.
- `mock/research/202605181400_mockspace-v2-spec.md` §57: bootstrap
  invocation contract. The surface this work extends.
- PR #123 (mock migrate): postscript points consumers at `cargo mock
  refresh` for builtin updates. This work makes that pointer real.

## Recorded

2026-05-22 after the migrate-command iteration surfaced that
mockspace's install surface currently covers only cargo alias + git
hooks. Canonical mockspace agent rules describing the phase/verb
surface need a home too. The user explicitly directed shipping them
as builtin so consumers stop hand-maintaining content that's
mockspace-internal.
