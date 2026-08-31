# Sides: doc and src

A round has two sides. **Doc is where the design lives, src is the code matching
it.** Each goes through its own sealed authoring loop so neither contaminates the
other's audit trail.

**Doc completes before src begins**, and that ordering is load-bearing: the src
manifest can claim adherence to an already-sealed doc manifest, and verifiers
check that it holds.

| Side | Covers | Authored in | Sealed in |
|---|---|---|---|
| doc | templates, topic files, sketches, research, design rounds | `PlanDoc` | `ApplyDoc` |
| src | source, tests, configuration | `PlanSrc` | `ApplySrc` |

## What a manifest carries

Structured TOML, `mockspace-core::manifest::Manifest`:

- `scope`, the paths the round may touch, with include and exempt patterns and
  per-axis bounds
- `acceptance`, what passing means, as verifier catalog entries
- `change`, the per-file claims: what is added, modified, removed
- `deprecated_accounting`, present when this manifest supersedes a deprecated one
  in the same round, recording which claims carried over and which were dropped

**TOML so a person can author it and a machine can verify it.** The structured
form is the contract; rendered prose lives in templates the consumer keeps
separately.

## Layout in the round-ref tree

Read through mock IO, not the working tree. The side is in the filename, and
both sides share one tree.

```
manifest.doc.toml                          mutable in PlanDoc
manifest.doc.locked.toml                   sealed at ApplyDoc entry
manifest.doc.deprecated.<n>.toml           one per replan, zero-indexed
.anchor.doc.toml                           captured at apply
.anchor.doc.blobs/<sha-prefix>/<sha-rest>  content-addressed bodies
```

`manifest.src.*` and `.anchor.src.*` are the parallel surface.

**Two renames are the whole state machine**: `<side>.toml` to
`<side>.locked.toml` is the seal, `<side>.locked.toml` to
`<side>.deprecated.<n>.toml` is the replan.

## Which verifiers run when

Every check declares its side. Doc-side checks run through `ApplyDoc`, src-side
through `ApplySrc`, on every commit in that phase.

**A check spanning both sides runs in `ApplySrc` only**, because that is the
first point where both manifests are sealed. The source-against-design check is
the example.

## Not split by side

Tasks, anchors, the round ref itself, topic files, sketches. Those belong to the
round. **The split is only about which manifest is in scope in which authoring
loop.**
