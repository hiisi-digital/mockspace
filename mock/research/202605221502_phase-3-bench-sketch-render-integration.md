# Phase 3 bench + sketch render-pipeline integration

Scoping memo for task #558. The v2 spec describes two first-party
frameworks that live alongside the render pipeline:

- **Bench framework** (spec §9): structured performance-measurement
  bundles under `mock/benches/<bundle>/` with statistical analysis
  (bootstrap CIs, paired tests) and `findings.md` artefacts.
- **Sketch protocol** (spec §10): minimal compile-probe code under
  `mock/research/sketches/<round-slug>/` with live and design probe
  variants, README per directory.

Both are full subsystems with their own CLI surfaces (`mock bench
run`, `mock sketch run`, etc.), their own ref namespaces
(`refs/mock/bench/<slug>`, `refs/mock/sketch/<slug>`), and their
own validation logic (bench findings parser, sketch design-probe
verifier).

This memo scopes only the **render-pipeline-side** integration. The
framework implementations themselves are larger work and remain
out of scope for #558.

## Scope cut: render side vs framework side

| Layer | Scope for #558 |
|---|---|
| bench / sketch frameworks (CLI, statistics, ref archival, validation, runner config) | OUT |
| render pipeline (`mockspace_rs::render`) | IN |

The frameworks produce on-disk artefacts (bench `findings.md`,
sketch `README.md`) and consumers reference them via `mock://`
URIs. The render pipeline ingests both surfaces. #558 covers only
the render pipeline's awareness of those artefacts; the frameworks
producing them are separate tasks tracked under #482 at the phase
level, with no per-framework breakdown yet.

## Render-pipeline integration points

Three distinct integration shapes exist:

1. **Bench-findings render**: each `mock/benches/<bundle>/findings.md`
   ships to `docs/benches/<bundle>.md` (or `docs/benches/<bundle>/findings.md`
   for multi-size bundles). The render pipeline walks the directory,
   each bundle becomes one output. No templating; bench-findings are
   already markdown produced by the bench harness.

2. **Sketch-index render**: each `mock/research/sketches/<round-slug>/README.md`
   ships to `docs/sketches/<round-slug>.md` (the round-level sketch
   index). The per-sketch `.rs` files are not rendered to docs; they
   are source artefacts. Only the README's status table is the
   public surface.

3. **mock:// URI cross-reference resolution**: topic files and locked
   manifests reference benches and sketches via `mock://bench/<slug>`
   and `mock://sketch/<slug>` URIs. When a template embeds one of
   these (e.g. via `{{ bench("structural-decomposition") }}` or
   similar), the render pipeline needs to resolve the URI to the
   rendered docs path and substitute a link or transclude the
   content.

Integration point 3 is the load-bearing one. Without it, the rendered
docs tree contains dead links to internal bench/sketch slugs. With
it, the rendered docs are self-contained: a reader landing on
`docs/DESIGN.md` can follow links to bench findings and sketch
indices without leaving the docs tree.

## Slice plan

Each slice is one PR. Slices ship in order.

### Slice 1: bench-findings render

Owner crate: `mockspace-rs::render`. New private helper:

```rust
fn regenerate_benches(
    project: &MockspaceProject,
    out_dir: &Path,
    out: &mut Vec<RenderedFile>,
) -> Result<(), RegenerateError>;
```

Walks `<project.root()>/mock/benches/<bundle>/`, copies (or
transcludes) each `findings.md` to `out_dir/benches/<bundle>.md`
via `write_atomic`. Bundle directories without a `findings.md` are
skipped silently (the bundle is mid-authoring).

Multi-size bundles (per-size `<size>_findings.md`) emit to
`out_dir/benches/<bundle>/<size>.md`. The detection rule: if the
bundle directory contains a `findings.md` AND no `<N>_findings.md`,
single-size mode; if it contains `<N>_findings.md` files (one or
more) AND no top-level `findings.md`, multi-size mode; if both,
single-size wins and the per-size files are ignored with a
warning.

Tests: bundles with findings.md only, with size-suffixed only,
with neither (skipped silently), with both (warning).

Defers: bench bundle schema validation (the bench harness owns it);
bench freshness check (CI-side concern).

### Slice 2: sketch-index render

Walks `<project.root()>/mock/research/sketches/<round-slug>/`. Each
directory's `README.md` ships to `out_dir/sketches/<round-slug>.md`.
Per-sketch `.rs` files are not copied to docs (source artefacts only).

Sketch directories without a `README.md` are skipped silently (the
sketch group is mid-authoring; the spec says the README describes
the sketches as a group, so its absence means the group hasn't been
documented yet).

Tests: directory with README, directory without README (skipped),
multiple round-slug directories rendered in alphabetical order.

### Slice 3: mock:// URI resolver

The biggest of the three; needs design choices that the prior slices
do not. Options:

- **Pre-render substitution**: before passing templates to minijinja,
  scan for `mock://...` URIs and rewrite them to relative docs paths.
  Pros: works for any template that embeds URIs as plain text.
  Cons: regex-based scan is fragile; minijinja-context-aware approach
  is preferable.

- **Template filter / function**: register a minijinja filter
  `{{ "mock://bench/foo" | mock_link }}` that maps URIs to rendered
  paths. Pros: explicit in the template, type-checked by the
  template engine. Cons: requires templates to opt in by calling
  the filter; raw URI references won't resolve.

- **Hybrid**: register the filter for explicit conversions, plus a
  pre-render pass that converts `[text](mock://...)` markdown link
  forms automatically (since those are the most common reader-
  facing references). Cons: split-brain.

Resolution: defer the choice to slice 3 PR time. The slices 1 + 2
infrastructure does not require this decision; slice 3 can pick
the approach with the actual call-site evidence in hand.

The slice 3 implementation also needs:

- A URI scheme parser for `mock://bench/<slug>`, `mock://sketch/<slug>`,
  `mock://round/<slug>`, etc. (the full scheme per spec).
- A resolver that maps `mock://bench/<slug>` to the correct rendered
  output path under `docs/`. The mapping is straightforward (`<slug>`
  → `docs/benches/<slug>.md`) but needs centralisation so future
  link-form changes touch one site.
- Failure-mode: unresolvable URIs (no matching bundle / sketch /
  round) surface as `RegenerateError::UnresolvableUri` or similar.
  Templates that reference non-existent slugs are author errors
  worth catching at render time.

## Out of scope for #558 entirely

- **Bench harness implementation**: the statistical analysis,
  bootstrap CI computation, paired-seed contract, per-iteration
  sensor recording. Spec §9.722 explicitly defers to "the
  bench-harness crate's own contract". A separate task owns this.

- **Sketch runner implementation**: the `mock sketch run <path>`
  command body, the project-configurable runner (`[sketch.runner]`
  in `mockspace.toml`), the WORKS / FAILS / INCONCLUSIVE result
  parser. Separate task.

- **`refs/mock/bench/<slug>` and `refs/mock/sketch/<slug>` archival**:
  the orphan-ref storage that captures bench bundles and sketches
  for cross-round reference. Separate task, part of the broader v2
  storage work.

- **Topic-file seal-time citation verification**: at PLAN seal,
  mockspace verifies cited benches exist with non-empty conclusions
  (spec §9.724). Separate task; lives in the manifest verifier, not
  the render pipeline.

- **`mock://round/<slug>` resolution**: round-level URIs reference
  closed rounds in the archive. The resolver in slice 3 handles
  the URI scheme but the round-archive read path is separate work.

## Open questions

1. **Multi-size bench bundle output path**: `docs/benches/<bundle>/<size>.md`
   vs `docs/benches/<bundle>-<size>.md`. The directory shape is closer
   to the on-disk source layout; the flat shape avoids creating
   per-bundle subdirs in `docs/`. My preference: the directory
   shape, since multi-size bundles often have aggregate writeups
   that would land at `docs/benches/<bundle>/index.md`. Defer to
   slice 1 PR with the actual examples.

2. **mock:// URI for `bench/<slug>/<size>`**: when a topic file
   references one size of a multi-size bench bundle, what URI form?
   `mock://bench/<slug>#size=1024`? `mock://bench/<slug>/1024`? The
   spec at §9 does not nail this. Defer to slice 3 PR.

3. **Sketch design-probe rendering**: design probes by spec do not
   compile in isolation, so the runner cannot produce a result file
   for them. The README status table notes them as
   `DESIGN-PROBE-DURING-SRC`. Does that label render the sketch
   subsection differently in `docs/sketches/<round-slug>.md`? My
   read: no. The README is markdown produced by the author, the
   render pipeline copies it verbatim. The author decides how to
   present the design-probe distinction.

4. **Round-slug overlap**: a sketch round-slug and a bench bundle
   slug must not collide. Both share the surface name `<slug>`. The
   spec at §19 names them as separate ref namespaces, so the source
   is correct; the render output puts them under separate
   directories (`docs/benches/` vs `docs/sketches/`) so the user-
   facing rendered tree is clean. No conflict in practice.

## Cross-references

- `mock/research/202605181400_mockspace-v2-spec.md` §9 (bench framework),
  §10 (sketch protocol), the "Sketches and benches" subsection of the
  topic-file structure (which sits inside the topic-file part), §19
  (ref namespaces for bench/sketch storage).
- `mock/research/202605222000_phase-3-render-pipeline-orchestration.md`
  (the slice plan that #557 followed; #558 is the next phase of the
  same module).
- Task #482 (v2 Phase 3 parent task covering render pipeline, agent
  integration, and bench/sketch frameworks): the umbrella at the
  phase level. #558 is one of the children.

## Status

Memo only; no implementation yet. Files claim no source changes.
Next firing can pick up slice 1 (bench-findings render) as the
shortest unblocked path.
