# Phase 3 render pipeline orchestration

Scoping memo for task #557. The v2 spec section "Assembly and rendering"
(spec lines 250-263) defines an 8-step pipeline that composes
`mock/*.tmpl` files into `docs/*.md` outputs. The template engine
itself ships in `mockspace-template`; the orchestrator that walks
templates, collects inputs, and writes outputs in the spec's order
does not yet exist.

## What ships today

- `mockspace-template` crate: minijinja-shaped renderer with
  `{{variable}}` interpolation, `{% for %}`, `{% if %}`, `{% include %}`.
  No filesystem walking, no orchestration.
- `mockspace-config`: parsed `mockspace.toml` with per-crate display
  metadata (`crate_colors`, `domain_kinds`, `known_macros`).
- `mockspace-rs::crate_graph`: cargo-metadata-driven crate set + deps.
- `mockspace-rs::design_rounds`: design-round directory parser.

## What is missing

A `regenerate` orchestrator that:

1. Resolves the project's mock root.
2. Collects per-crate `README.md.tmpl` summaries.
3. Renders the mock-root `DESIGN.md.tmpl` with `{{crate_summaries}}`
   interpolated.
4. Computes + renders the dependency graph (Graphviz default).
5. Writes the mock-root rendered file to `docs/DESIGN.md`.
6. Walks per-crate `DESIGN.md.tmpl` files, writes to
   `docs/<crate>/DESIGN.md`.
7. Walks `BACKLOG.md.tmpl`, writes to `docs/<crate>/BACKLOG.md`.
8. Walks per-crate `deepdives/*.md.tmpl`, writes under
   `docs/<crate>/deepdives/<topic>.md`.

Plus: `SHAME.md.tmpl` is read by the lint engine but is **not**
rendered to public output. The orchestrator must skip it.

The atomic-write helper `render_atomic` (task #496) is a separate
piece that lands inside `mockspace-template`; this orchestrator
consumes it.

## Slice plan

Each slice is a separate PR. Slices ship in order; later slices
depend on earlier ones.

### Slice 1: `render_atomic` helper (closes #496)

Owner crate: `mockspace-template`. New function:

```rust
pub fn render_atomic(
    template: &Template,
    context: &Value,
    dest: &Path,
) -> Result<(), RenderError>;
```

Renders the template + writes to `dest` via the standard
"write-to-temp, fsync, rename" sequence. Idempotent: a re-render
that produces identical bytes does not bump the destination's mtime.

Closes the precondition for the orchestrator.

### Slice 2: Mock-root regenerate

Owner crate: `mockspace-rs`. New module: `mockspace_rs::render`. Two
top-level functions (named once and reused across slices 2-5):

```rust
pub fn regenerate(
    project: &MockspaceProject,
    out_dir: &Path,
) -> Result<RegenerateReport, RegenerateError>;

pub fn check(
    project: &MockspaceProject,
    out_dir: &Path,
) -> Result<CheckReport, RegenerateError>;
```

The mock-root render lives behind an internal helper (e.g.
`render_mock_root`) called by `regenerate`. Slice 2 implements spec
steps 1, 2, 3, 5; slice 3 extends `regenerate` to cover steps 6-8.
The public surface is two fns; the internal helpers split per
concern.

Slice 2's `regenerate` function:

- Resolves `mock/DESIGN.md.tmpl`, `mock/PRINCIPLES.md.tmpl`,
  `mock/WORKFLOW.md.tmpl`.
- Collects per-crate summaries from each crate's `README.md.tmpl`.
- Renders the three mock-root templates with the collected summaries
  + dependency-graph rendering as context.
- Writes to `docs/DESIGN.md`, `docs/PRINCIPLES.md`, `docs/WORKFLOW.md`
  via `render_atomic`.

`check` is the read-only diff counterpart: render to memory, compare
against on-disk output, exit non-zero if drift. CI consumers use
`check`; user-driven regen uses `regenerate`.

### Slice 3: Per-crate regenerate

Extends `mockspace_rs::render::regenerate`:

- Walks every crate's per-crate templates (`DESIGN.md.tmpl`,
  `BACKLOG.md.tmpl`, `deepdives/*.md.tmpl`).
- Writes to `docs/<crate>/DESIGN.md`, `docs/<crate>/BACKLOG.md`,
  `docs/<crate>/deepdives/<topic>.md`.
- Skips `SHAME.md.tmpl` (lint-engine-only).

The per-crate walk needs a per-crate context shape that exposes the
crate's name, position in the dep graph, metadata from
`mockspace.toml`, and any per-crate data files.

### Slice 4: Dependency graph rendering

The default Graphviz renderer needs:

- A graph builder that walks `mockspace_rs::crate_graph::CrateGraph`
  and applies per-crate color + per-domain glyph metadata from
  `mockspace.toml`.
- Output as a `.dot` file plus a rendered `.svg` (where Graphviz is
  available on PATH) or `.dot` only (where it is not).

The render lives in `mockspace-template` by default. Split into a
new `mockspace-render` crate **only if** the graphviz adapter adds
more than ~5 transitive deps beyond the current `mockspace-template`
graph OR adds more than 2 seconds to `cargo check` on a warm cache.
Decided at slice 4 PR time with the actual dep tree in hand; the
threshold criterion exists so the decision is not subjective.

### Slice 5: CLI wiring

`cargo mock regenerate` + `cargo mock regenerate --check`.
Subcommand surface lands in `mockspace-cli/src/main.rs`. Calls into
`mockspace_rs::render::regenerate` (full) or
`mockspace_rs::render::check` (diff-only).

## Open questions

1. **Graphviz dependency**: does the orchestrator require Graphviz on
   PATH, or do we ship a pure-Rust `dot` renderer? My judgment:
   require Graphviz on PATH with a fallback to `.dot`-only output
   when missing. A pure-Rust SVG renderer is out of scope for a
   substrate tool.

2. **Atomic-write granularity**: per-file (slice 1) or whole-tree
   (one giant transaction)? Spec doesn't mandate either. Per-file is
   simpler + matches user mental model ("regen failed half-way? the
   completed files stayed"). Going with per-file.

3. **`{% include %}` fragment resolution**: where does the resolver
   look? Spec says `mock/fragments/*.md.tmpl`. The orchestrator must
   plumb that path into the template engine. Slice 1 includes the
   resolver wiring.

4. **`--check` semantic for additions**: if the orchestrator would
   write a new file that does not yet exist on disk, is that drift
   (fails `--check`) or expected (passes)? I argue: drift. A new
   rendered file means the user hasn't regen'd since adding a
   template. Failing `--check` surfaces that as honest CI signal.

5. **Bench/sketch framework integration** (task #558): out of scope
   for #557. The bench + sketch frameworks have their own rendering
   surfaces; integrating them is a follow-on PR after #557 lands.

## Cross-references

- v2 spec §4 (Assembly and rendering): `mock/research/202605181400_mockspace-v2-spec.md` line 240+
- `mockspace-template` crate: the renderer foundation.
- `mockspace_rs::crate_graph`: the dep-graph input.
- Task #496: `render_atomic` helper (slice 1 of this plan).
- Task #557: this memo's parent task.
- Task #558: bench + sketch framework integration (separate follow-on).

## Recorded

2026-05-22 night during overnight autonomous work, after the post-#581
janitor sweep cleared all known drift. Next cron firing picks up
slice 1 (`render_atomic` helper) as the implementation entry point.
