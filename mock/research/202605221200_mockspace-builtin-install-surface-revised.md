# Mockspace builtin install surface, revised

Supersedes `202605221100_mockspace-builtin-install-surface.md`. Same
problem (task #581): give canonical mockspace agent rules + hooks a
home that scales beyond the cargo alias + git hooks already shipped
by `bootstrap::install`. Different answer to where the files live.

The earlier memo proposed `mock/agent/builtin/` as a tracked,
version-controlled directory carrying the builtin content alongside
`mock/agent/consumer/`. Workspace direction received after that memo
merged: **builtins should be invisible to the end user**. They are
runtime artifacts the tool extracts from itself on install; they do
not belong in the consumer's tracked working tree.

This memo lands the corrected layout. The earlier memo stays as audit
trail showing the iteration.

## What changes

The builtin content moves out of `mock/agent/builtin/` (tracked, the
earlier memo's proposal) into a gitignored cache under
`mock/target/`. This matches the existing hooks precedent: hooks land
at `mock/target/hooks/` (see `mockspace-rs::bootstrap::HOOKS_DIR`).
Agent rules join them in the same target subtree.

```
Tracked (visible to the consumer):
  mock/agent/                # consumer-authored agent rules
    <whatever the consumer writes>

Gitignored (runtime cache, extracted on install/refresh):
  mock/target/
    hooks/                   # already exists
      pre-commit
      pre-push
      ...
    agent/                   # NEW: extracted canonical agent rules
      phases.md
      verbs.md
      sides.md
      anchors.md
      suppressions.md
      commands.md
      identity.md
      INDEX.md
```

The mockspace binary embeds the canonical content via `include_str!`
or equivalent. `cargo mock install` (and `refresh`) writes the
embedded content out to `mock/target/agent/`. `cargo clean` blows
that directory away; the next `cargo mock` invocation rewrites it.
The consumer's git status never shows the files because `target/` is
gitignored under Cargo's normal convention.

## What stays the same

- Consumer-authored content lives at `mock/agent/<whatever>` and
  mockspace never touches it. (Same as the earlier memo.)
- The boundary principle is unchanged: anything whose subject matter
  is mockspace itself ships builtin; anything project-specific stays
  consumer-authored.
- `cargo mock refresh` is still the way to pull current builtin
  content. Today refresh handles the cargo alias and the hooks; with
  this work it extends to the agent subtree.

## What drops out

The earlier memo's apparatus existed because the builtins were
tracked content. With builtins moved to a gitignored runtime cache,
several pieces become unnecessary:

- **The `builtin/` / `consumer/` two-discriminator split.** Consumer
  content lives at `mock/agent/<name>`; builtin content lives at
  `mock/target/agent/<name>`. The directory itself is the
  discriminator.
- **The file-marker convention.** Marker existed to recover the
  "this is builtin" property if a file showed up in the wrong place.
  With builtin content confined to gitignored `mock/target/`, a file
  showing up under `mock/agent/` is unambiguously consumer-authored.
  No marker needed.
- **The migration story for pre-#581 consumers.** Pre-#581 consumers
  who hand-authored agent rules under `mock/agent/` keep their files
  as-is; those are consumer content and the new builtin extraction
  does not collide with them. No file moves, no diffs, no per-file
  notes.
- **The atomic rename complexity.** A target subtree can be rewritten
  by deleting the directory and recreating it; if the rewrite is
  interrupted, the next `cargo mock` invocation just rewrites it
  again. The content is version-keyed (via the `VERSION` sidecar)
  against the mockspace binary's version constant, so there is no
  "consumer state" inside `mock/target/agent/` to corrupt. Worst
  case: a half-written file during a crash, fixed by re-running
  install.

The earlier memo's `cargo mock status` state machine (current /
outdated / drifted / unclassified) also simplifies. With builtin
content confined to `mock/target/agent/`, status reduces to two
states:

- **present:** `mock/target/agent/` exists and its `VERSION` sidecar
  matches the binary's version constant.
- **missing or stale:** `mock/target/agent/` either does not exist
  (e.g., after `cargo clean`) or its `VERSION` sidecar disagrees
  with the binary. The fix is `cargo mock refresh` (or any other
  `cargo mock` invocation that triggers the lazy-extract path).

Version-keying caveat: the `VERSION` sidecar stores the binary's
version constant (e.g. `0.1.0`). Two local builds of mockspace that
share the same `Cargo.toml` version but ship different embedded
content (e.g., a developer iterating without bumping the version)
both produce the same `VERSION` string. Refresh will not detect that
the embedded content changed between builds; the consumer must
`cargo mock refresh --force` (or delete `mock/target/agent/`) to
re-extract. A content-hash sidecar derived at compile time from the
embedded blob (e.g. first 8 bytes of a hash) would close the gap
without runtime cost; flag as an option in slice 2 when the wiring
lands.

Drift is no longer a state. A consumer who edits a file under
`mock/target/agent/` has their edits blown away on next refresh,
same as edits to anything under `target/`. They can do it (the
filesystem permits it) but it does not survive.

## Lazy extraction vs explicit install

Two options for when the agent subtree gets written:

1. **Eager:** `cargo mock install` writes the files. Subsequent
   `cargo mock` invocations check for presence and re-write if
   missing or stale.
2. **Lazy:** every `cargo mock` invocation checks presence and writes
   if missing. No install-time step.

Eager is cleaner because install/refresh is the user-visible
"prepare for use" entry point. Lazy means the files appear without
the consumer asking, which is fine for a target subtree but feels
implicit.

Recommendation: eager during install/refresh, lazy fallback in every
other subcommand. If a consumer runs `cargo mock check` against a
fresh clone that has not been `cargo mock install`-ed yet, the
extract happens on demand so agents reading the rules find them.
Cost is a presence check + one-shot extract on each cold start; both
are cheap.

## Cross-references to other rules

`hilavitkutin-build`-style "embed the content, extract at runtime"
already exists for hook scripts in `mockspace-rs::bootstrap`. The
agent subtree reuses the same pattern: `include_str!` of the
markdown content, `std::fs::write` at the cache path. No new
infrastructure.

The renderer pipeline (#557) is unrelated to this work: that
renders templates from `mock/crates/*/...md.tmpl` sources into
`docs/`. The builtin agent extraction is a separate code path that
runs at install time, not at render time.

## Slice plan

With this layout, the implementation slices for #581 become:

1. **Author canonical content.** Write `phases.md`, `verbs.md`,
   `sides.md`, `anchors.md`, `suppressions.md`, `commands.md`,
   `identity.md`, `INDEX.md` as static source files in the mockspace
   repo at e.g. `crates/mockspace-rs/src/agent_builtin/`. These are
   the source of truth; `include_str!` pulls them into the binary.
2. **Wire extraction.** Extend `mockspace-rs::bootstrap::install` to
   write the embedded content to `<root>/mock/target/agent/`.
   Generate the `VERSION` sidecar.
3. **Lazy fallback.** Every `cargo mock` subcommand's entry point
   checks for `<root>/mock/target/agent/VERSION`; missing or stale
   triggers a one-shot extract before the subcommand proceeds. The
   check lives in a single helper (working name
   `ensure_agent_extracted(repo_root)` on `mockspace_rs::bootstrap`)
   so subcommand bodies call it as a one-liner. Avoids per-command
   reimplementation and keeps the staleness logic in one place.
4. **Status report.** Extend `cargo mock status` to report the
   agent subtree's present / missing-or-stale state alongside the
   existing alias / hooks lines.

Each slice is a PR. Content lands first because the wiring depends
on it.

## What this memo does NOT cover

- The actual canonical content of the agent rule files. Next slice
  (slice 1 above).
- The agent-side mechanism that lets an agent find
  `mock/target/agent/` and load the files. That is consumer-side
  agent configuration; if mockspace needs to expose a discovery
  command (e.g. `cargo mock agent-rules-path`), it lands as a
  separate slice once the content is in place.
- The skill-style aggregator path under `.claude/` or equivalent.
  Mockspace renders the agent files; whether the consumer's editor
  reads them from `mock/target/agent/` directly or via a workspace
  aggregator like homma is out of scope for this work.

## Cross-references

- Predecessor: `202605221100_mockspace-builtin-install-surface.md`.
  Path layout there is superseded by this memo; the principle (split
  between mockspace-managed and consumer-authored) carries forward.
- Task #581: ship canonical mockspace agent rules + hooks as builtin
  install surface. Umbrella for this work.
- `mockspace-rs::bootstrap::HOOKS_DIR = "mock/target/hooks"`: the
  precedent this memo extends to agent rules.
- PR #123 (mock migrate): postscript points users at `cargo mock
  refresh` for builtin updates. This memo makes the pointer real
  with concrete content + extraction wiring.

## Recorded

2026-05-22 after the earlier memo merged. Workspace direction:
builtins should be invisible to the end user. Use the existing
`mock/target/` precedent rather than introducing a new tracked
directory shape under `mock/agent/`.
