# Phases

A mockspace round carries exactly one current phase at a time. The phase
drives what every command does: `mock phase apply` seals different
manifests in PLAN(doc) vs PLAN(src), and `mock close` is only valid from
DONE.

There are six phases. Three pairs across two sides (doc and src), plus
the entry and exit phases that have no side.

```
Topic
  |  (mock phase plan)
  v
PlanDoc
  |  (mock phase apply)
  v
ApplyDoc
  |  (mock phase finish)
  v
PlanSrc
  |  (mock phase apply)
  v
ApplySrc
  |  (mock phase finish)
  v
Done
```

## Topic

Free-form exploration. The round directory carries topic files,
sketches, benches, research notes. No manifest exists. The verifier
does not run; lints scoped to authoring rounds are inactive.

Exit: `mock phase plan <slug>` opens PlanDoc.

## PlanDoc

Doc-side manifest authoring. The manifest at
`manifest.doc.toml` is mutable; the consumer edits it freely. No
template or doc-side file is sealed yet.

Exit: `mock phase apply <slug> --source-tip <hex>` seals the manifest
and transitions to ApplyDoc. The anchor captures the doc-side surface
state at apply entry.

## ApplyDoc

Doc execution. The doc manifest is sealed: read-only, content-hashed,
its claims drive the verifier. Doc templates listed in the manifest's
`change` block get edited per the manifest. Every commit re-runs the
verifier.

Exit: `mock phase finish <slug>` advances bookkeeping and opens
PlanSrc.

Backward exit: `mock phase replan <slug>` deprecates the sealed
manifest and returns to PlanDoc. The locked manifest is renamed to
`manifest.doc.deprecated.<n>.toml` and stays as audit trail.

## PlanSrc

Src-side manifest authoring. Same shape as PlanDoc; src side. The
locked doc manifest from ApplyDoc remains read-only.

Exit: `mock phase apply <slug> --source-tip <hex>` seals the src
manifest and transitions to ApplySrc.

## ApplySrc

Src execution. The src manifest is sealed; source files listed in its
`change` block get edited per the manifest. The verifier runs every
commit, plus `design-doc-source-mismatch` runs across the project.

Exit: `mock phase finish <slug>` advances to Done.

Backward exit: `mock phase replan <slug>` deprecates the sealed src
manifest and returns to PlanSrc. Restoration of source-side files
follows the replan mode (see `verbs.md`).

## Done

Round closed. Both sides sealed. PR comments may still ingest into
the round-meta document for audit. The round is not deleted; it lives
on `refs/mock/round/<slug>` until archived.

Exit: `mock close <slug>` archives the round into the unified
`refs/mock/round-archive` and deletes the source round ref.

## The four verbs

Each verb moves a round between exactly one phase pair. The full
matrix:

| Verb | Valid from | Lands in |
|---|---|---|
| `plan` | Topic | PlanDoc |
| `apply` | PlanDoc | ApplyDoc |
| `apply` | PlanSrc | ApplySrc |
| `finish` | ApplyDoc | PlanSrc |
| `finish` | ApplySrc | Done |
| `replan` | ApplyDoc | PlanDoc |
| `replan` | ApplySrc | PlanSrc |

Any other (verb, current-phase) combination is invalid and the CLI
refuses with an `InvalidFromPhase { current, verb, allowed_from }`
error.
