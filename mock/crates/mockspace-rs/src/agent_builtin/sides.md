# Sides: doc and src

A round has two sides. The doc side is where design lives; the src
side is where source code matching that design lives. The two-sided
shape exists so design and implementation can each go through
manifest-sealed authoring without contaminating each other's audit
trail.

## What each side covers

**Doc side.** Templates, topic files, sketches, research notes,
design rounds, anything that describes what the round is about.
The doc-side manifest at `manifest.doc.toml` (mutable in PlanDoc,
sealed as `manifest.doc.locked.toml` in ApplyDoc) claims which
doc-side files the round will edit and what verifier checks them.

**Src side.** Source code files, tests, configuration. Edited
during the second authoring loop, after the doc side has sealed.
The src-side manifest at `manifest.src.toml` (mutable in PlanSrc,
sealed in ApplySrc) does the same job for source files.

## What's in a manifest

A manifest is structured TOML. The shape comes from
`mockspace-core::manifest::Manifest`:

- `scope`: paths the round will touch, with explicit include/exempt
  patterns and per-axis bounds.
- `acceptance`: what passing looks like, expressed as verifier
  catalog entries.
- `change`: the actual edits the round commits to. Per-file claims
  about what gets added / modified / removed.
- `deprecated_accounting`: when this manifest supersedes a prior
  deprecated manifest in the same round, this block records which
  claims carry over and which were dropped.

Manifests are TOML so the consumer can author them by hand and
machines can verify them mechanically. The structured form is the
contract; rendered prose versions live in templates the consumer
keeps separately.

## Lifecycle per side

Each side moves through its own PLAN -> APPLY pair:

```
(side = doc)               (side = src)
  PlanDoc                    PlanSrc
    |                          |
   apply                      apply
    |                          |
  ApplyDoc -- finish -> PlanSrc -- finish -> Done
```

The doc side completes before the src side begins. This ordering
exists because the src manifest can claim adherence to the
already-sealed doc manifest (verifiers check that the src side
matches what the doc side committed to).

## File layout per side

In the round-ref tree (read via `mock` IO, not the working tree):

```
manifest.doc.toml                # mutable in PlanDoc
manifest.doc.locked.toml         # sealed at ApplyDoc entry
manifest.doc.deprecated.0.toml   # first replan, zero-indexed
manifest.doc.deprecated.1.toml   # second replan
.anchor.doc.toml                 # anchor metadata captured at apply
.anchor.doc.blobs/<sha-prefix>/<sha-rest>  # content-addressed blobs

manifest.src.toml                # parallel surface for src
manifest.src.locked.toml
manifest.src.deprecated.0.toml
.anchor.src.toml
.anchor.src.blobs/<sha-prefix>/<sha-rest>
```

Both sides' surfaces live in the same round-ref tree. The side is
encoded in the filename. The `<side>.toml` -> `<side>.locked.toml`
rename is the seal; the `<side>.locked.toml` -> `<side>.deprecated.<n>.toml`
rename is the replan.

## When the verifier knows which side

Every verifier check declares which side it applies to. Doc-side
verifiers run during ApplyDoc (and on every commit during that
phase). Src-side verifiers run during ApplySrc. Verifiers that span
both sides (e.g., `design-doc-source-mismatch`, which checks that
src-side claims match the locked doc-side manifest) run during
ApplySrc only, since both sides are sealed by then.

## What's not split by side

Tasks. Anchors. The round-ref itself. Topic files. Sketches. These
belong to the round as a whole. The side split is specifically
about which manifest is in scope at each authoring loop.
