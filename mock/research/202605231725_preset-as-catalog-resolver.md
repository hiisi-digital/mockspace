# Preset-as-catalog resolver (#611) design memo

Date: 2026-05-23
Phase: research
Source topic: #611, follow-up gap from PR #189 (#568 removal)

## The problem

After PR #189 dropped the 15 preset-replaced `inventory::submit!` catalog
entries (`no-alloc`, `no-std`, `no-dyn-dispatch`, `no-runtime-spawn`,
`no-runtime-registration`, `no-bare-numeric`, `no-bare-string`,
`no-bare-option`, `no-bare-result`, `no-public-raw-field`,
`no-vec-in-trait-sig`, `strategy-marker-required`,
`trait-first-signatures`, `writing-style`,
`lint-allow-requires-task-id`), these lints are unreachable from
consumer `mockspace.toml`.

The intended consumer-facing migration path is:

```toml
[lints.no-bare-numeric]
extends = "mockspace::no-bare-numeric"
```

But today the `extends` mechanism (config_loader.rs Layer 2) only
overlays a preset's config and scope onto an existing catalog entry.
It cannot construct a NEW catalog entry from a preset file. So the
above block resolves to a `ConfigError`: lint `no-bare-numeric` is
not in the registered catalog.

`v2 ready-to-go` requires the 15 lints to be reachable from consumer
TOML. #611 ships that resolver.

## What today's cascade does

`config_loader.rs::from_inputs_with_source` walks
`catalog::catalog_entries()` (the `inventory::collect!`'d set) and
runs `instantiate_with_cascade` for each. The cascade walks five
layers per entry:

1. **Catalog defaults** `CatalogEntry::default_config` /
   `default_scope` parsed from raw `&'static str`.
2. **Preset chain** `extends = "<host>::<name>"` resolved via
   `PresetSource`, then innermost-first overlay of each
   `PresetFile::{config, scope}`.
3. **Workspace defaults** `[defaults]` block in `lints.toml`.
4. **Per-lint user TOML** `[lints.<name>] {config, scope, gate}`.
5. **CLI overrides** `scope_intersection`, `severity_overrides`.

The cascade is entry-centric: every layer applies to one
`CatalogEntry`. The construction call at the bottom is
`(entry.instantiate)(merged_config, merged_scope)` plus a separate
severity-cascade walk over `entry.default_severity` and the
resolved chain.

## What #611 needs to change

Add a second walk after the catalog-entry walk: for every
`[lints.<X>]` block in user TOML whose name `X` is NOT in the
catalog set:

1. Parse the block's `extends = "<host>::<name>"`. If absent, emit
   `ConfigError::UnknownLint` (the existing error path).
2. Resolve the preset chain via `PresetSource`.
3. Materialise a synthetic instantiation by combining:
   - The preset's `primitive` field (selects the constructor).
   - The preset's `config` (cascade Layer 2 the deepest layer for
     the synthesised case, since no catalog default exists below).
   - The user's per-lint config (Layer 4).
   - Workspace defaults (Layer 3).
   - CLI overrides (Layer 5).
   - The preset's `severity` (Layer 1-substitute fallback for the
     severity cascade).
4. Push the resulting `InstantiatedLint` into the entry vector.

## The primitive-name → constructor lookup

The 15 removed lints map to ~10 distinct primitives. Each primitive
has an `instantiate_with(config: &toml::Table, scope: &toml::Table,
default_severity: GateSeverity) -> Result<Box<dyn Lint>, ConfigError>`
shape (the same shape the catalog's `instantiate` fn pointer wraps).

To support preset-as-catalog construction without re-declaring every
lint's catalog entry, mockspace-rs needs a primitive registry:

```rust
struct PrimitiveDescriptor {
    name: &'static str,
    instantiate: fn(&toml::Table, &toml::Table, GateSeverity)
        -> Result<Box<dyn Lint>, ConfigError>,
    mode: LintMode,
    staging_aware: bool,
    editor_skip: bool,
    finding_kinds: &'static [&'static str],
}

pub fn primitive_descriptors() -> &'static [PrimitiveDescriptor];
```

Initial population mirrors the existing `KNOWN_PRIMITIVES` test
constant in `preset_source.rs` (which already enumerates the 18
primitives shipping today). Each entry's `instantiate` field points
at the primitive module's `instantiate_with` (or a small adapter if
the signature shape differs).

This is a NEW abstraction; it does not replace `CatalogEntry`. The
catalog continues to register lints whose policy IS bespoke (the
seven kept-after-#189 lints). The primitive registry exists for
synthesised lints whose policy is in the preset file.

## Architectural choices

### Why not `&'static CatalogEntry` for synthesised lints?

`CatalogEntry` uses `&'static str` everywhere (name, kind,
default_config, etc.). Strings from TOML preset files are owned
`String`. A synthesised entry would need either a parallel
`OwnedCatalogEntry` type or a Cow-based variant. Both add API
surface and complicate `find_entry`.

Cleaner: synthesise the `InstantiatedLint` directly without going
through `CatalogEntry`. The cascade math is the same; the data
source differs (preset.config instead of entry.default_config). A
small refactor of `instantiate_with_cascade` to take its inputs as
separate parameters rather than a `&CatalogEntry` makes the same
function serve both paths.

### Why not require the preset file to declare a full CatalogEntry-equivalent?

The preset file already names `primitive`; the primitive descriptor
provides `mode`, `staging_aware`, `editor_skip`, `finding_kinds`.
Asking the preset author to redeclare those fields invites drift.
Looking them up via the primitive registry keeps the preset file
focused on what's preset-specific (config overlay + severity +
scope).

### Severity cascade for synthesised case

The catalog-entry case uses `entry.default_severity` as Layer 1.
The synthesised case has no `entry.default_severity`; the preset's
own `severity` field substitutes. Subsequent layers walk the
remaining presets in the chain plus user TOML plus CLI overrides
unchanged. The `resolve_severity_cascade` helper extends to take an
override base when invoked from the synthesised path.

### What happens if two presets in a chain disagree on primitive?

The chain `extends = "<host>::<a>"` where `<a>` itself has
`extends = "<host>::<b>"` resolves both. Each preset's `primitive`
field must match (a preset chain represents one lint's policy, not
a multi-primitive composition). Mismatching primitives is a
`ConfigError::PresetChainMismatch`. Same-primitive chains overlay
config and scope per the existing innermost-first walk.

## Implementation plan

Multi-PR. Each PR self-contained, mergeable independently.

### PR-1: primitive descriptor registry (foundation)

- New file `mock/crates/mockspace-rs/src/builtins/primitives.rs`
  declaring `PrimitiveDescriptor` + a static slice.
- Populate the slice with the ~18 entries currently in
  `KNOWN_PRIMITIVES`. Each entry's `instantiate` field wraps the
  corresponding primitive's `instantiate_with` (adapter shim if
  arity differs).
- Internal-only API at this PR; not yet consumed.
- Test: registry covers every name in `KNOWN_PRIMITIVES`.

### PR-2: refactor `instantiate_with_cascade` for shared use

- Split `instantiate_with_cascade` into:
  - `compute_cascade(layer1_config, layer1_scope, layer1_severity,
    preset_chain, workspace_defaults, user_block, overrides)
    -> (merged_config, merged_scope, resolved_severity,
        resolved_chain, only_staged, scope_filter)`
  - Existing entry path: pass `entry.default_config`,
    `entry.default_scope`, `entry.default_severity` as layer-1
    inputs.
- No behavioural change. Existing tests must continue to pass.

### PR-3: synthesised path through preset-as-catalog

- After the existing catalog-walk in `from_inputs_with_source`,
  walk `user_toml.lints` keys not in the catalog name set.
- For each, parse `extends` (must be present; error if absent).
- Resolve the chain. The deepest preset in the chain is the
  "anchoring" preset; its `primitive` selects the descriptor.
- Walk the chain for primitive consistency. Mismatches surface
  `ConfigError::PresetChainMismatch`.
- Call `compute_cascade` with:
  - `layer1_config = anchor_preset.config` (as `toml::Table`)
  - `layer1_scope = anchor_preset.scope`
  - `layer1_severity = anchor_preset.severity`
  - Remaining args same as catalog path.
- Look up the descriptor's `instantiate` fn; call it with the
  cascaded inputs.
- Push `InstantiatedLint` into entries.
- Tests:
  - `extends_to_unregistered_preset_synthesises_lint`
  - `extends_chain_with_inconsistent_primitive_errors`
  - `extends_to_unknown_preset_emits_config_error`
  - `block_without_extends_for_unregistered_lint_emits_unknown_lint_error`

### PR-4: re-enable the 15 lints in e2e tests + restore migration story

- Walk e2e fixtures and tests that previously used the 15 lints.
- Add a fixture variant exercising `extends = "mockspace::<name>"`
  for at least three of the 15.
- Update MIGRATION-v1-to-v2-lints.md to reflect the resolver is
  live; consumers can now opt back into any of the 15.

## Out-of-scope for #611

- Cross-pack extends (e.g. `extends = "stack-lints::<name>"`).
  Already works for overlay-only; same-primitive constraint
  applies if synthesising.
- Preset versioning. The cascade walks live trees of presets
  shipped in the embedded table; versioning a preset would touch
  `mockspace_config::PresetFile` and is its own scope.
- Override of primitive descriptor fields per-preset
  (e.g. preset saying `mode = "PerDocument"` to switch).
  Primitives' execution shape is part of the impl, not consumer
  policy.

## Why this design is right

The cascade math is unchanged; only the data source for Layer 1
differs. Splitting the cascade computation out from the
entry-centric loop is the smallest possible change for the largest
behaviour-set unlock. The primitive descriptor registry is a
read-only static table, no runtime registration mechanism, no
inventory dependency on a new collection.

After #611 ships, the 15 first-party preset-replaced lints are
opt-in via one line each. v2 is then end-to-end usable; downstream
consumer repos can migrate to v2 cleanly.

## Cross-references

- PR #189 (closed #568) the removal that created the gap.
- `mock/research/202605220500_lint-preset-infrastructure.md`,
  preset infrastructure design (the foundation #611 extends).
- `mock/crates/mockspace-rs/src/config_loader.rs` current
  cascade implementation.
- `mock/crates/mockspace-rs/src/preset_source.rs`,
  `KNOWN_PRIMITIVES` discipline list.
- `docs/MIGRATION-v1-to-v2-lints.md` current state of the 15-out
  story.
