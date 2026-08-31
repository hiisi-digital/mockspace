# Phases

A round carries one phase. The phase decides what every command does.

Six phases: an entry, an exit, and two sides (doc and src) with a plan and an
apply each. `mockspace-core::phase::Phase`.

```
Topic -> PlanDoc -> ApplyDoc -> PlanSrc -> ApplySrc -> Done
```

| Phase | What it is | Manifest |
|---|---|---|
| `Topic` | free exploration: topic files, sketches, benches, research | none |
| `PlanDoc` | doc-manifest authoring | mutable |
| `ApplyDoc` | doc execution, templates edited per the claims | sealed, read-only, hashed |
| `PlanSrc` | src-manifest authoring, same shape | doc stays sealed |
| `ApplySrc` | src execution, source files edited per the claims | both sealed |
| `Done` | closed, not archived; lives on its round ref | both sealed |

- **The verifier runs on every commit in the two APPLY phases**, and in
  `ApplySrc` the source-against-design check runs across the project too.
- **In `Topic` no manifest exists** and lints scoped to authoring rounds are
  inactive.
- **`Done` is closed but still present.** `mock close <slug>` is what archives
  it and deletes the round ref.

## What reaches these phases

**`cargo mock tools` is the list of subcommands, and there is no `phase` verb in
it.** The six phases above are the model `mockspace-core` carries; the CLI that
ships moves a round through them with `lock`, `unlock`, `deprecate`, `close` and
`archive`, over changelist files whose names carry the state.

**A `cargo mock phase <verb>` form appears in doc comments and in older
descriptions of this design. It is not a shipped subcommand.** `verbs.md` says
what each intended verb does and marks the same gap; run `cargo mock tools`
before telling anybody to invoke one.
