---
date: 2026-05-19
phase: v2 design (Phase 0)
scope: mockspace-template lib crate API readiness for v2 render pipeline
status: audit complete; lib API sufficient at the substrate level; one extension filed as Phase 3 sub-task
related_tasks: [#489, feeds #482]
---

# `mockspace-template` API audit against v2 §47 render pipeline

## Verdict

The existing `mockspace-template` lib crate (shipped via #444, currently 167 lines of tests + ~250 lines of source across `lib.rs`, `template.rs`, `renderer.rs`, `error.rs`, `platform.rs`, `platforms/`) exposes a substrate-level API sufficient for v2 §47's render pipeline. **No Phase 0-blocking gaps.** One extension worth a Phase 3 follow-up task (atomic multi-file rendering helper); everything else is caller-side concerns that the v2 render pipeline owns and the lib doesn't need to know about.

This audit closes #489. Phase 0 (#479) is unblocked on the template front.

## What v2 §47 asks for

Reduced from §47 of the spec:

1. **Multi-target rendering**: same template can render to multiple targets (source_tree, local_only, forge API). Per `[[render.targets]]` in `mockspace.toml`.
2. **Substitution shapes**: `{{var}}`, `{% for %}`, `{% if %}`, `{% include %}`, `{{crate_summaries}}` (the last is a special caller-supplied composition).
3. **Data model exposure**: `project`, `crates`, `deps`, `round`, `lints`, `tools` (caller serializes a struct).
4. **Atomic multi-file rendering**: write to staging dir, swap by directory rename (POSIX `rename(2)` atomicity). Per-target. Includes power-loss recovery via `mock sync` discovering orphaned staging dirs.
5. **Deterministic rendering**: sorted-by-key map iteration; commit-time timestamps not wall-clock; external-tool output snapshotted.
6. **Render failure handling**: abort before ref update on transition renders; no partial overwrite on regen renders.
7. **Round artefacts never render**: filtered at caller level.
8. **Template loading from in-memory bytes** (so ref-tree contents can render without filesystem materialisation first).

## What the lib currently provides

`TemplateEnv` (template.rs):
- Wraps `minijinja::Environment<'static>`.
- Strict undefined-variable behavior on by default.
- Autoescape disabled (plain text, not HTML).
- `add_template(name, source)`: register from in-memory string.
- `get_template(name) -> Template`: retrieve registered.
- `render_str(source, ctx) -> String`: one-off from string.
- `inner_mut()`: minijinja access for custom filter/function registration.

`Template` (template.rs):
- `render(ctx) -> String` for compiled templates.

`AgentRenderer<'env, P: Platform>` (renderer.rs):
- Walks `src_root` for `.tmpl` files.
- Renders each to `platform.output_path(dst_root, logical)`.
- Returns a `RenderReport` with per-file metadata (source path, destination path, bytes written, duration).

`walk_template_tree(env, src_root, dst_root, ctx)`:
- Same walk shape, no platform-specific path rewriting, mirrors src tree under dst.

`RenderError` (error.rs): minijinja error, io error, missing-template error.

Tests cover: simple substitution, registered-template render, missing-template error, strict-undefined error, loop + conditional, agent-render output paths per platform, frontmatter shapes.

## v2 needs vs lib provides

| §47 need | Lib provides | Status |
|---|---|---|
| Substitution shapes (`{{}}`, `{% for %}`, `{% if %}`) | minijinja covers these out of the box | **fit** |
| `{% include %}` directive | minijinja loads from registered templates; caller pre-registers includes before rendering top-level | **fit, caller pattern** |
| `{{crate_summaries}}` special | Just a key in the serde ctx; caller composes per-crate readme content and injects | **fit** |
| Data model exposure (`project`, `crates`, etc.) | `render_str<C: Serialize>` takes any caller-shaped ctx | **fit** |
| In-memory template bytes (ref-tree source) | `add_template`, `render_str` both take strings | **fit** |
| Strict undefined behavior | Already configured in `TemplateEnv::new()` | **fit** |
| Deterministic map iteration | Caller-side (serialize `BTreeMap`, not `HashMap`); minijinja honors serde order | **caller responsibility** |
| Deterministic timestamps | Caller injects `ref_committer_time` into ctx; minijinja's `now()` filter is not exposed by default | **fit (caller responsibility)** |
| Round artefacts excluded | Caller decides what's in src tree / passed to renderer | **caller responsibility** |
| Multi-target rendering | Caller loops over targets, calls render per (target, path) tuple | **caller pattern, no lib extension needed** |
| Atomic multi-file rendering (staging + rename) | Current renderer writes each file directly; partial failure leaves partial state | **gap, file as Phase 3 extension** |
| Render failure handling (no partial overwrite) | Caller wraps render in staging dir, renames on success | **fit via staging pattern; lib helper would simplify (see gap above)** |

## The one gap worth filing

`mockspace-template` lacks a render-with-staging helper. Currently `walk_template_tree` and `AgentRenderer::render_all` write each file to its final destination directly, in a loop. On mid-loop failure, files written before the failing one remain on disk in the target tree. To get §47's atomic guarantee, the caller must:

1. Pass a staging directory as `dst_root`.
2. Check the returned `Result<RenderReport, RenderError>`.
3. On success: `rename(staging_dir, final_root)`.
4. On failure: `rm -rf staging_dir`.

This works, and the v2 render pipeline (Phase 3, #482) can do it. But every caller who needs the atomicity will reinvent the same wrapper. A lib helper would centralise:

```rust
pub fn render_atomic<C: Serialize>(
    env: &mut TemplateEnv,
    src_root: &Path,
    target_root: &Path,
    ctx: &C,
) -> Result<RenderReport, RenderError> {
    let staging = staging_path_for(target_root); // sibling .mock.staging.<pid>.<ts>
    let report = walk_template_tree(env, src_root, &staging, ctx);
    match report {
        Ok(r) => {
            atomic_rename(&staging, target_root)?;
            Ok(r)
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}
```

Plus a Windows fallback path (per-file atomic write + `.render-manifest` marker file) per §58.

Filing as a new Phase 3 sub-task; not blocking Phase 0.

## What's NOT a lib concern

Several §47 mechanisms live in the v2 render pipeline (Phase 3, #482), not in this lib:

- **Target taxonomy** (`source_tree`, `local_only`, `forge_api`). The lib renders to a path; the caller decides which paths feed which targets.
- **Forge API target**. The lib produces strings; the caller (or a forge module) takes the string and POSTs it.
- **Power-loss recovery for orphaned staging dirs**. `mock sync` (§44) handles this by scanning for staging-prefixed dirs.
- **Ref-tree iteration**. Caller reads orphan-ref tree entries via gix and calls `add_template` / `render_str` per template.
- **`mockspace-managed` HTML-comment delimited sections in PR bodies**. Forge integration concern; the lib renders the body string and the forge module manages the delimited zone.
- **Round artefact filtering**. Caller's responsibility to decide what's input.

These are all natural callers of the lib, not lib internals.

## Recommendations for the v2 render pipeline (Phase 3)

When implementing #482 (Phase 3, Render pipeline), the pattern is:

1. Read `mockspace.toml` `[[render.targets]]` entries (via mockspace-config).
2. Build the data model by composing fields from project, crates, deps, round, lints, tools sources.
3. For each unique target_root:
   a. Construct a fresh `TemplateEnv` (or share one across targets for caching).
   b. Pre-register `{% include %}` fragments by reading from ref-tree.
   c. Call `render_atomic(env, src_root, target_root, &ctx)` once `render_atomic` ships per the gap-followup task.
4. On any failure: surface a structured diagnostic; transition renders abort the phase transition (caller checks the result before stepping the state machine).

Phase 3 work includes the `render_atomic` helper if it isn't shipped earlier as a lib polish round.

## Cross-references

- Spec §47 (the render pipeline target the lib serves)
- Spec §58 (Windows platform notes; affects atomic-rename fallback)
- Task #444 (the original mockspace lib extraction that shipped this crate)
- Task #482 (Phase 3, consumes this lib + needs the gap helper)
- Task created as a follow-up: `render_atomic` helper in `mockspace-template`

## Recorded

2026-05-19, Phase 0 audit. Closes #489.
