<!-- mockspace:workflow-reference -->
## Mockspace workflow at a glance

The canonical reference for mockspace's own workflow vocabulary lives
alongside the binary. After `cargo mock install` (or `refresh`), the
full per-topic breakdown lands at `mock/target/agent/`:

| File | Subject |
|---|---|
| `phases.md` | The six-phase state machine. |
| `verbs.md` | The four transition verbs (`plan`, `apply`, `finish`, `replan`). |
| `sides.md` | Doc and src manifests; what each side carries. |
| `anchors.md` | Content-addressed snapshots of surface files. |
| `commands.md` | The `cargo mock` subcommand surface. |
| `suppressions.md` | The five comment-canonical lint directives. |
| `identity.md` | Slugs, task IDs, ref paths, content hashes. |

That tree is gitignored and rewritten on every `cargo mock refresh`,
keyed to the mockspace binary version. Treat it as the source of
truth.

The summary below names the load-bearing shapes for readers who land
on this document without the agent extract handy.

### Six phases

A round carries one phase at a time. Forward progression:

```
Topic
  | mock phase plan
  v
PlanDoc
  | mock phase apply
  v
ApplyDoc
  | mock phase finish
  v
PlanSrc
  | mock phase apply
  v
ApplySrc
  | mock phase finish
  v
Done
```

Topic is the entry phase: the round exists but no manifest is sealed.
PlanDoc and PlanSrc are the two planning phases (one per side). ApplyDoc
and ApplySrc are the locked states after the matching manifest seals.
Done is the exit phase; `mock close` archives a Done round.

### Four verbs

- `plan` opens the planning surface for the first time. Valid only
  from Topic. Cheap and idempotent.
- `apply` seals the current authoring manifest and transitions to the
  matching APPLY phase. Captures an anchor of the surface files the
  manifest names. Valid from PlanDoc or PlanSrc.
- `finish` advances past APPLY. From ApplyDoc, transitions to PlanSrc
  (opens the source-side planning surface). From ApplySrc, transitions
  to Done.
- `replan` is the backward verb. Returns an APPLY phase to its matching
  PLAN phase, deprecating the locked manifest with a numbered suffix
  so the audit trail survives the rollback. Valid from ApplyDoc or
  ApplySrc.

### Two sides, one manifest each

The "doc" side describes what design lands; the "src" side describes
what source change implements it. Each side carries its own manifest
(`manifest.doc.toml`, `manifest.src.toml`) which the consumer authors
during the matching PLAN phase. `apply` seals it to
`manifest.<side>.locked.toml` and captures the anchor.

The two manifests share the same round but seal independently. The
doc side seals first; the src side seals second. Re-planning either
side is independent of the other.

### Commands surface

`cargo mock` ships these subcommands:

- `status`: adoption signals and bootstrap drift.
- `install` / `refresh` / `uninstall`: bootstrap the v2 surface.
- `check`: run the lint engine. `--gate {commit|build|push}` picks
  the severity gate; `--fix` applies suggested fixes inline; `--json`
  emits machine-readable output.
- `explain <lint>`: render the per-layer cascade resolution.
- `phase {plan|apply|finish|replan} <slug>`: drive the state machine.
- `close <slug>`: archive a Done round.
- `regenerate` / `regenerate --check`: render `mock/*.md.tmpl` into
  `docs/`.
- `migrate`: print a v1-to-v2 migration plan for legacy round
  directories.

Each subcommand's full flag set lives in `cargo mock <cmd> --help`.
