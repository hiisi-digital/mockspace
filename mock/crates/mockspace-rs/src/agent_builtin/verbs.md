# Transition verbs

Four verbs, three forward and one back.

**Not shipped as a `cargo mock phase <verb>` subcommand.** That form is in doc
comments and in older descriptions of this design; `cargo mock tools` is the
list of what actually runs. Read this for what each transition means and what it
costs, not as an invocation.

## The validity matrix

| From \ verb | plan | apply | finish | replan |
|---|---|---|---|---|
| `Topic` | `PlanDoc` | - | - | - |
| `PlanDoc` | - | `ApplyDoc` | - | - |
| `ApplyDoc` | - | - | `PlanSrc` | `PlanDoc` |
| `PlanSrc` | - | `ApplySrc` | - | - |
| `ApplySrc` | - | - | `Done` | `PlanSrc` |
| `Done` | - | - | - | - |

Anything else is refused with `InvalidFromPhase { current, verb, allowed_from }`.
**`Done` has no outgoing verb**; the exit is `mock close <slug>`.

## plan

Opens the planning surface. Writes `.phase = plan_doc`.

**Scaffolds no manifest.** The consumer authors `manifest.doc.toml` and pushes
it as a separate edit before the next apply. No anchor, no verifier, idempotent.

## apply

Seals the authoring manifest for its side.

- Validates `manifest.<side>.toml` structurally and against the verifier catalog.
- Hashes it and writes `manifest.<side>.locked.toml`, read-only from here.
- **Captures an anchor** of the files it claims. `anchors.md`.
- Writes `.phase = apply_<side>` and pushes via the atomicity protocol.

Takes `--source-tip <hex>`, the source branch tip at seal, which the anchor
records so restoration has a stable reference.

## finish

Advances bookkeeping. `ApplyDoc` to `PlanSrc` leaves the locked doc manifest in
place; `ApplySrc` to `Done` leaves both locked. No anchor, no verifier.

## replan

The backward verb, and **it always deprecates.** The sealed manifest does not
unseal: it is renamed to the next free `manifest.<side>.deprecated.<n>.toml`,
zero-indexed, and stays as audit trail, so the chain shows what the round
committed to before each replan.

Then it restores the source-side surface per `--mode` and writes
`.phase = plan_<side>`.

### Modes, and what each costs

- `destructive` (default): overwrite from the anchor blobs. **Refuses outright
  if any post-apply commit touched a claimed file.**
- `additive-by-commit`: commit the restoration on top. Cluttered history, no
  work lost.
- `accept-loss --accept-loss-path <PATH>`: lose post-apply work on those paths.
  Others refuse as in `destructive`. Repeat the flag per file.

**The rename and phase flip are identical across modes.** The mode informs the
restoration step running alongside, not the local ref work.
