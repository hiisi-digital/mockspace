---
date: 2026-05-22
phase: research
scope: mockspace-rs lint preset infrastructure; mockspace-config schema; mockspace-rs config_loader
status: design-locked
supersedes:
  - The "move 14 stack-lints to mockspace-rs or duplicate" architectural question
related:
  - mock/research/202605181400_mockspace-v2-spec.md
  - mock/research/202605211200_lint-schema-design.md
  - mock/research/202605220000_canonical-directive-vocabulary.md
  - mock/research/202605220030_auto-fix-and-structured-diagnostics.md
---

# Lint preset infrastructure

Mockspace's v2 lint engine ships catalog primitives (generic detection logic) separate from preset bundles (policy: forbidden patterns, severities, reason text). Both mockspace itself and lint-pack crates can ship presets. Consumers extend presets in their own `mockspace.toml` with surgical overrides via `extends = "<host>::<name>"`.

This memo records the design as locked. Implementation tasks are #537-#542 (schema, resolution, embedding, explain command, migration of 14 duplicates, publish stack-lints as preset pack).

## What problem this solves

The v2 catalog currently registers 21 entries in `mockspace-rs/src/builtins/registry.rs` via `inventory::submit!`. Fourteen of those are stack-specific lints (no-alloc, no-std, no-bare-numeric, no-bare-string, no-bare-option, no-bare-result, no-public-raw-field, no-vec-in-trait-sig, strategy-marker-required, trait-first-signatures, writing-style, lint-allow-requires-task-id, no-runtime-spawn, no-dyn-dispatch, no-runtime-registration). The same 14 lints also exist in the separate `mockspace-hilavitkutin-stack-lints` crate using the v1 API.

The duplication is the visible symptom. The deeper problem: the engine bundles three things into one `CatalogEntry`: the primitive (what pattern to detect), the policy (which patterns are forbidden, what reason, what severity), and the defaults. Presets split these cleanly. Primitives stay in mockspace as catalog entries; policies become named, composable, overrideable bundles; lint-packs become preset packs.

Consequence: the question "should the 14 stack lints live in mockspace or stack-lints?" dissolves. The primitives stay in mockspace. The stack-specific policy bundles move to stack-lints as presets. No duplication, no move-out vs duplicate choice.

## The architecture

### Presets are inert config exports

A preset is a TOML file packaged as an export under `refs/mock/export/<package-name>` per §29 of the v2 spec. Externally-published presets live at the URI `mock://ext/<pkg>/export/lint-preset/<name>`; first-party (mockspace-shipped) presets live at `mock://@/export/lint-preset/<name>` and are embedded into the mockspace binary at build time.

The preset TOML shape:

```toml
schema_version = "1.0"
name = "no-heap"
primitive = "forbidden_imports"
description = "Forbid alloc::* and std::vec usage in no-heap codebases."

# Optional: chain to another preset before applying these overrides
extends = "mockspace::no-bare-numeric"

# Configuration applied as an overlay over the primitive's defaults
[config]
forbidden = ["alloc::*", "std::vec::*", "std::collections::*"]
reason = "no-heap discipline; use stack-typed primitives"

[severity]
commit = "warn"
build = "error"
push = "error"

[scope]
exempt_categories = ["string-foundation"]
```

The `primitive` field names which catalog primitive the preset configures. The `[config]`, `[severity]`, and `[scope]` tables overlay onto the primitive's defaults. The optional `extends` field chains to another preset for inherited overrides (see Cascade below).

### Trust tier split

Existing mock:// imports have three trust tiers (per §30 of the v2 spec): local (no fetch), first-party `@/` (binary-trust-rooted, signature verified against hardcoded fingerprint), external `ext/<host>/` (lockfile-pinned, signature verified, TOFU on first contact).

The third tier exists because executable imports (hooks, lint plugins, runners) run code at gate time. Presets do not run code; they are inert TOML overlays. Applying signature verification + TOFU ceremony to a file that sets `severity = "warn"` would be theatre and would block the bootstrap case where a fresh checkout needs presets resolved before any `cargo mock` operation can proceed.

The architectural decision: extend the `[imports]` model with an explicit `kind = "config"` tier that gets SHA-pinned (for reproducibility) but NOT signature-verified, NOT TOFU-prompted. The lockfile records the resolved SHA so `cargo mock` is deterministic across machines; trust ceremony stops there.

```toml
[imports]
import = [
  { uri = "mock://ext/stack-lints/export/lint-preset/no-heap", kind = "config" },
  { uri = "mock://ext/stack-lints/export/lint-preset/arvo-strict", kind = "config" },
]
```

### Shorthand at lint config sites

Full URI form is verbose. The shorthand expands at config-load time:

```toml
[[lints]]
name = "my-no-heap"
extends = "stack-lints::no-heap"

[lints.config]
# extras layered on top of the stack-lints/no-heap preset
forbidden.add = ["my::custom::path::*"]
```

`stack-lints::no-heap` expands to `mock://ext/stack-lints/export/lint-preset/no-heap` with any pinned SHA from the lockfile. First-party shorthand: `mockspace::no-bare-numeric` expands to `mock://@/export/lint-preset/no-bare-numeric` and reads from the embedded mockspace binary's preset tree.

Why `::` and not `/`: `::` is the existing identifier-path separator across the v2 spec (e.g. `mock://task/compiler::ir::structural-robust-ir`). Catalog entry names match `[a-z][a-z0-9-]*`, which contains no `::`, so the namespace separator is unambiguous. `/` is URI path infrastructure and should not surface at config sites.

### Cascade ordering

The four-level cascade that shipped in Phase 2 (catalog defaults → workspace defaults → per-lint TOML → CLI overrides) gains a preset chain inserted between catalog defaults and workspace defaults. The new five-level cascade:

```
catalog defaults
  → preset chain (innermost extends-target first, then outer presets in reverse, finally the consumer's referenced preset)
  → workspace defaults
  → per-lint TOML
  → CLI overrides
```

This ordering is load-bearing. Workspace defaults and per-lint TOML always win over presets. Presets are a convenience floor that consumers can refine with their own policy. The consumer retains full override authority; a preset cannot dictate behaviour the consumer's TOML has already pinned.

### List-field merge semantics

Scalar fields override straight: `severity.commit = "error"` in the consumer's per-lint TOML wins over `severity.commit = "warn"` from a preset.

List and map fields default to **replace** semantics: if the consumer sets `forbidden = [...]`, the preset's `forbidden` list is discarded entirely. For surgical add/remove, the consumer uses opt-in sub-keys:

```toml
[lints.config]
# add to the preset's inherited list
forbidden.add = ["my::extra::path::*"]
# remove an inherited entry
forbidden.remove = ["std::vec::*"]
```

The `.add` and `.remove` operators are parsed by the config loader after the base list resolves through the cascade. Order at any single layer: inherited list, then `.add` extends, then `.remove` filters out matches. Both are no-ops if the field is empty.

**Chain semantics**: when a preset chain includes mid-chain `.add` / `.remove` directives, each preset's operators apply against the list resolved by deeper layers, before the next outer overlay sees it. A chain `consumer extends outer extends inner`, where each level uses `.add` or `.remove`, resolves as: start with the catalog primitive's default list; apply `inner`'s base list (replacing) plus its `.add` / `.remove`; pass the result up to `outer`, which sees that result as its inherited list and applies its own `.add` / `.remove`; pass that up to `consumer`, same shape. Each preset in the chain applies its own operators against the list resolved by deeper layers, in extends-chain order. The `cargo mock explain` output should show this stepwise resolution so the layered effect is visible.

### Cycle detection

The `extends` chain can self-reference (`a extends b extends a`) or otherwise loop. The config loader walks the chain depth-first with a `HashSet<(host, preset_name)>` visited set. On re-entry, the loader emits a hard `LoadError` with the full cycle path:

```
LoadError: preset cycle detected:
  stack-lints::no-heap
    extends mockspace::no-bare-numeric
    extends stack-lints::no-heap (cycle)
```

Cycle detection is mandatory at instantiate time, not deferred to a follow-up. Silent infinite recursion at config load is the worst possible failure mode.

### Missing extends target

If `extends = "stack-lints::no-heap"` and that preset has not been imported (no `mock://ext/stack-lints/...` entry in `[imports]`, or the SHA-pinned ref is unreachable), the config loader emits a hard `LoadError`, not an accumulating `ConfigError`. A preset that silently fails to apply is worse than a check that refuses to run.

The lockfile records the resolved SHA at install time (`cargo mock install` or equivalent). At check time, the resolution is a lookup against the lockfile; missing entries are install-time problems surfaced at check-time.

## Integration point

The existing `config_loader.rs:232` `instantiate_with_cascade` function is where this lands. The current cascade calls overlay in order: catalog defaults → workspace defaults → per-lint TOML → CLI. The new shape inserts preset chain resolution as a pre-pass:

```rust
fn instantiate_with_cascade(...) -> Result<InstantiatedLint, ConfigError> {
    // 1. Resolve the extends chain into a flat overlay table
    let preset_overlay = resolve_preset_chain(per_lint_config.extends.as_deref(), &lockfile, &imports)?;

    // 2. Compose the four-level cascade with the preset overlay inserted
    let mut config = catalog_entry.default_config.clone();
    overlay(&mut config, &preset_overlay);
    overlay(&mut config, &workspace_defaults);
    overlay(&mut config, &per_lint_config);
    overlay(&mut config, &cli_overrides);

    Ok(InstantiatedLint { ... })
}

fn resolve_preset_chain(
    starting_ref: Option<&str>,
    lockfile: &Lockfile,
    imports: &ImportsMap,
) -> Result<TomlTable, LoadError> {
    let mut visited = HashSet::new();
    let mut overlays = Vec::new();
    let mut current = starting_ref;
    while let Some(ref_str) = current {
        let (host, name) = parse_shorthand(ref_str)?;
        if !visited.insert((host.clone(), name.clone())) {
            return Err(LoadError::PresetCycle { path: visited.into_iter().collect() });
        }
        let preset = load_preset(&host, &name, lockfile, imports)?;
        overlays.push(preset.config_overlay);
        current = preset.extends.as_deref();
    }
    // Compose overlays innermost-first (deepest extends-target wins last in this list,
    // so when applied as a single merged table the outermost preset overrides the innermost).
    Ok(merge_overlays_in_reverse(overlays))
}
```

First-party preset loading reads from the embedded tree (a `phf` map or similar, populated by `build.rs` from `mock/presets/*.toml` in the mockspace repo). External preset loading reads from the on-disk cache populated by `cargo mock install`.

## `cargo mock explain <lint-name>`

Once the cascade has five levels, debugging "why is this severity error here but warn there?" needs tooling. A new CLI command walks the resolved cascade for a named lint and prints each layer's contribution:

```
$ cargo mock explain no-heap

Lint: no-heap (primitive: forbidden_imports)
  Layer 1: catalog defaults
    severity.commit = "off"
    severity.build = "off"
    severity.push = "off"
    forbidden = []

  Layer 2: preset chain
    stack-lints::no-heap (mock://ext/stack-lints/export/lint-preset/no-heap@<sha>)
      forbidden = ["alloc::*", "std::vec::*", "std::collections::*"]
      severity.build = "error"
      reason = "no-heap discipline; use stack-typed primitives"
    extends mockspace::no-bare-numeric (mock://@/export/lint-preset/no-bare-numeric)
      forbidden = (empty)  -- not applicable to this primitive

  Layer 3: workspace defaults
    (no overrides for no-heap)

  Layer 4: per-lint TOML
    (mockspace.toml:42)
    forbidden.add = ["my::custom::path::*"]

  Layer 5: CLI overrides
    (no overrides)

Final:
  severity.commit = "off" (catalog default)
  severity.build = "error" (preset stack-lints::no-heap)
  severity.push = "off" (catalog default)
  forbidden = ["alloc::*", "std::vec::*", "std::collections::*", "my::custom::path::*"]
    (preset stack-lints::no-heap + add from mockspace.toml:42)
```

Without this command, preset debugging becomes a maze. With it, every value points back to the layer that set it.

## Architectural consequences

The preset infrastructure dissolves three previously-separate questions:

1. **The 14 stack-lint duplicates** (architectural). Primitives stay as catalog entries in mockspace-rs. Stack-specific policy bundles move to first-party embedded presets (`mock://@/export/lint-preset/no-alloc` etc.), then are removed from the catalog registrations in `builtins/registry.rs`. Consumers extending `mockspace::no-alloc` get the same behaviour they get today via the inline registration. See #541.

2. **The stack-lints crate** (architectural). It becomes a preset pack. Each preset is a TOML file published at `mock://ext/stack-lints/export/lint-preset/<name>`. The Rust crate stays only as a publishing entry point; the actual content is the preset tree. See #542.

3. **Severity profiles** (feature). `stack-lints::arvo-strict`, `stack-lints::hilavitkutin-strict`, etc become preset chains. A consumer that wants strict mode adds one `extends = "stack-lints::arvo-strict"` reference. Custom severity profiles per consumer are also one-liner extends chains.

## What this design is NOT

The preset infrastructure is not a way to ship new lint detection logic. New primitives still ship as catalog entries with `inventory::submit!` registration (Rust code). Presets only configure existing primitives. A lint pack that wants to add a new lint primitive must ship a Rust crate the consumer adds as a dep; the preset mechanism is purely for policy bundles.

The preset infrastructure is also not a way to override catalog primitives. If `mockspace::no-bare-numeric` has a bug in its detection logic, the fix lives in the catalog entry's code, not in a preset. Presets cannot patch the primitive.

## Cross-references

- `mock/research/202605181400_mockspace-v2-spec.md` §27 (mock:// URIs), §29 (Exports), §30 (Imports), §31 (Signing and TOFU): the existing trust + URI machinery this builds on.
- `mock/research/202605211200_lint-schema-design.md`: the catalog work this extends.
- `mock/research/202605220000_canonical-directive-vocabulary.md`: companion design covering source-level directives; orthogonal to presets but lands together as part of the v2 finishing pass.
- `mock/research/202605220030_auto-fix-and-structured-diagnostics.md`: companion design covering Fix recipes; orthogonal to presets.
- `mock/crates/mockspace-rs/src/config_loader.rs:232`: `instantiate_with_cascade`, the integration point.
- `mock/crates/mockspace-config/`: the schema extension target for the new `[presets]` and `[imports]` shapes.

## Implementation tasks

- #537: extend mockspace-config schema with preset shape + `kind = "config"` import tier
- #538: implement resolution + cycle detection in config_loader
- #539: first-party preset embedding via `mock/presets/*.toml` + `build.rs` codegen
- #540: `cargo mock explain <lint-name>` cascade visualizer
- #541: migrate 14 duplicate catalog registrations to first-party presets
- #542: publish stack-lints as preset pack at `mock://ext/stack-lints/export/lint-preset/*`
