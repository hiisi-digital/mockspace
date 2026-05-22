# Transition verbs

Four verbs move a round between phases. Three are forward-moving;
one is backward. The verb names map directly to `cargo mock phase
<verb>` invocations.

## plan

Opens the planning surface for the first time. Valid only from Topic.

```
Topic ---(plan)---> PlanDoc
```

Effect: writes `.phase = plan_doc` on the round ref. The
doc-side manifest is not scaffolded by `plan` itself; the consumer
authors `manifest.doc.toml` and pushes it as a separate edit before
the next `apply`.

No anchor capture. No verifier run. Cheap and idempotent.

## apply

Seals the current authoring manifest and transitions to the
matching APPLY phase. Valid from PlanDoc or PlanSrc.

```
PlanDoc ---(apply)---> ApplyDoc
PlanSrc ---(apply)---> ApplySrc
```

Effect (per side):
- Reads `manifest.<side>.toml`, validates it structurally and against
  the verifier catalog.
- Hashes the manifest content; writes it back as
  `manifest.<side>.locked.toml` (read-only from this point).
- Captures an anchor of the surface files the manifest claims will
  change. Anchor blobs land at `.anchor.<side>.blobs/<sha-prefix>/<rest>`
  for content-addressed storage.
- Writes `.phase = apply_<side>`.
- Pushes the new round-ref tip via the atomicity protocol.

`apply` takes one required argument: `--source-tip <hex>`. This is
the OID of the source-side branch tip at the moment of seal. The
anchor records this OID so the round has a stable reference for
restoration on replan.

## finish

Advances bookkeeping past APPLY. Valid from ApplyDoc or ApplySrc.

```
ApplyDoc ---(finish)---> PlanSrc
ApplySrc ---(finish)---> Done
```

Effect:
- From ApplyDoc: the locked doc manifest stays in place; phase
  rewrites to `.phase = plan_src`. The consumer authors
  `manifest.src.toml` next.
- From ApplySrc: phase rewrites to `.phase = done`. Both manifests
  remain locked. The round is closed but not yet archived.

No anchor capture. No verifier run. Cheap.

## replan

Deprecates the current sealed manifest and returns to PLAN on the
same side. Valid from ApplyDoc or ApplySrc.

```
ApplyDoc ---(replan)---> PlanDoc
ApplySrc ---(replan)---> PlanSrc
```

Effect:
- Renames `manifest.<side>.locked.toml` to the next free
  `manifest.<side>.deprecated.<n>.toml` slot, where `<n>` is
  zero-indexed (first replan writes `deprecated.0`).
- Restores the source-side surface from the anchor, per the
  `--mode` flag.
- Writes `.phase = plan_<side>`.

`replan` is the backward verb. It is **always deprecating**: the
sealed manifest does not just unseal; it joins the deprecated chain
as audit trail. The consumer can read prior deprecated manifests to
see what the round committed to before each replan.

### Replan modes

`--mode <mode>` controls how restoration handles post-APPLY work
that touches files the locked manifest claimed.

- `destructive` (default): overwrites source-side files at
  restoration time. Refuses (hard error) if any post-APPLY commits
  touched claimed files. Use when no post-APPLY work has been done.
- `additive-by-commit`: commits the restoration on top of post-APPLY
  state rather than overwriting. History gets cluttered (post-APPLY
  work + additive replan commit) but no work is lost.
- `accept-loss --accept-loss-path <PATH>`: accepts post-APPLY work
  loss for the named paths. Other claimed files refuse as in
  `destructive`. Repeat `--accept-loss-path` per file.

The local-ref portion of replan does not branch on mode (the rename
+ phase flip is identical across modes); the mode informs the
higher-orchestration restoration step that runs alongside.

## Validity matrix

Putting all four verbs in one table:

| From \ Verb | plan | apply | finish | replan |
|---|---|---|---|---|
| Topic | -> PlanDoc | invalid | invalid | invalid |
| PlanDoc | invalid | -> ApplyDoc | invalid | invalid |
| ApplyDoc | invalid | invalid | -> PlanSrc | -> PlanDoc |
| PlanSrc | invalid | -> ApplySrc | invalid | invalid |
| ApplySrc | invalid | invalid | -> Done | -> PlanSrc |
| Done | invalid | invalid | invalid | invalid |

Done has no outgoing verb. The exit is `mock close <slug>`, which
archives the round.
