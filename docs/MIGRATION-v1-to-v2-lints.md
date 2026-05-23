# Migrating from v1 to v2 lints

This guide covers what changes for downstream consumers of mockspace when
they pick up the v2 lint engine. The v2 engine replaces inline TOML
declarations of policy with structured source-level directives. Both
surfaces are landed and shipping today.

The reconciled design memo is at
`mock/research/202605221700_directive-vocabulary-reconciled.md` (it
supersedes two earlier memos that described a different directive set;
those originals are preserved as research artifacts but carry status
pointers to the reconciled version). This guide is the consumer-facing
companion.

## What you have to change

In one sentence: replace the `[primitive-introductions]` TOML table
with per-site `lint:allow` directives (plus optional `lint:scope-add`
for transitive helpers), and use the v2 5-directive vocabulary in
source comments where you previously used freeform suppression
comments.

If a consumer crate's `mockspace.toml` does not carry the
`[primitive-introductions]` table and the consumer is not authoring
lints, no source changes are required. The v2 directive surface is
backwards-compatible for existing `// lint:allow(...)` comments; they
map to the v2 `lint:allow` directive without change.

## The directive vocabulary

Five directives, one parser, one canonical form per language. The
canonical form is comments; language-native attribute aliases are
parsed where the language has idiomatic decorator syntax (Rust
attributes today; TypeScript decorators planned).

### `lint:allow(<lint-name>)`

Per-site suppression of a specific lint, with `reason:`
(`SuppressionMetaLint` enforces a minimum word count) and
`tracked: #N`. Both fields are required by the meta-lint; the parser
accepts the directive without them but the meta-lint emits a finding
for missing fields.

```rust
// lint:allow(no-bare-numeric) reason: hardcoded slot count for the FNV-1a constant table per the IETF spec; substituting USize loses const-evaluability here. tracked: #427
const FNV_PRIME: u64 = 0x100000001b3;
```

Same shape as the v1 `// lint:allow(...)` comments; the v2 parser
produces structured `Directive::Allow` records the engine consumes.

### `lint:scope-add(<lint-name>, <axis>=<value>)`

At a module or file boundary, extends the scope of a lint along one
axis for the contained items. The axis set is bounded to
`ScopeConfig` fields: `paths`, `exempt_paths`, `crates`,
`exempt_crates`, `languages`, `proc_macro_exempt`. Lint packs cannot
invent new axes; the axis set is framework-fixed.

```rust
// lint:scope-add(no-bare-numeric, exempt_paths=src/ffi/**)
mod ffi {
    #[repr(C)]
    pub struct DescriptorV1 {
        pub version: u32,
        pub flags: u32,
    }
}
```

### `lint:defer(<name>, until: #N)`

Acknowledges a violation is known and will be fixed when the linked
task closes. Distinct from `lint:allow`: an allow is an intentional
exception, a defer is a known-bug-with-deadline.

```rust
// lint:defer(no-bare-string, until: #185) reason: clause test rehab pending API migration of String to Str across test fixtures
fn legacy_test_helper(name: String) { /* ... */ }
```

`SuppressionMetaLint`'s `forbid_expired` config controls whether
deferred suppressions whose `until: #N` task has closed flip from
permitted to forbidden. Allows do not have an expiry; defers do.

### `lint:file-disable(<name>)`

File-level disable for the named lint. Placed at the top of a file.
Requires the same `reason:` and `tracked:` fields as `lint:allow`.

```rust
// lint:file-disable(writing-style) reason: this is a generated FFI binding file; the formatter does not preserve the project's writing-style invariants. tracked: #207
```

Distinct from `lint:scope-add` in that it is a disable, not a scope
extension. Use `scope-add` to relax one axis of a lint; use
`file-disable` to silence the whole lint for the file.

### `lint:prop(<name>)` / `lint:prop(<name> = <value>)`

Marks a source item with a named property that prop-reading lints
pick up. Lints declare which props they read via the
`Lint::declared_props` trait method; the engine populates a `PropMap`
at scope time and the lints read from it during dispatch.

```rust
// lint:prop(audited)
pub fn critical_path() { /* ... */ }

// lint:prop(arena_size = 4096)
struct StaticBuffer { /* ... */ }

// lint:prop(audit_id = "A-2026-04")
pub fn export_descriptor() {}
```

Presence form (`lint:prop(audited)`) parses to
`PropValue::Bool(true)`. Key-value forms accept Bool / Integer /
String literals. An optional trailing `reason: "..."` clause attaches
to any variant for human notes.

The framework reserves the `mockspace::` prop namespace for
first-party prop names; collisions among `mockspace::`-prefixed names
are silent (one pack's coordinated namespace). Unqualified prop
collisions across two or more lints surface as a startup warning
(`StartupWarning::PropNameConflict`).

## Language-domain aliases (Rust attributes)

All five directives have a Rust attribute alias. Both surfaces (the
canonical `// lint:...` comment form and the `#[mockspace::...]`
attribute form) produce the identical internal record:

```rust
#[mockspace::allow(no-bare-numeric, reason = "...", tracked = "#427")]
const FNV_PRIME: u64 = 0x100000001b3;

// equivalent to the canonical comment form:

// lint:allow(no-bare-numeric) reason: ... tracked: #427
const FNV_PRIME: u64 = 0x100000001b3;
```

Both surfaces emit the same entry into the engine's internal directive
records. The lint engine downstream does not know which surface the
author used.

The `directive-style-consistency` built-in lint optionally enforces a
single style per crate. Default is `"mixed"` (both forms accepted);
opt-in to `"comments-only"` or `"attributes-when-available"` via a
one-line config in your `mockspace.toml`.

## Migrating `[primitive-introductions]` to per-site directives

The v1 `[primitive-introductions]` table in `mockspace.toml` is
retired. Mockspace v2 rejects the table with a clear error pointing
at this migration path.

Important note on what `lint:introduces` is and is not: an earlier
design memo described a `lint:introduces(<category>)` directive as the
TOML replacement. That directive was never implemented. The shipped
replacement is per-site `lint:allow` directives on each introducer
item, with optional `lint:scope-add` for transitive helpers. The
per-site form gives more granular control than the v1 table (the
table addressed a whole crate; the directive form names the precise
item).

Migration in four steps:

1. **Read the table** in your `mockspace.toml`. Each `[primitive-introductions.<category>]` block names a crate that introduces a category.
2. **Locate the introducer items** in source. Usually the obvious `pub struct` named after the category root: `string-foundation` → `pub struct Str`, `bit-foundation` → `pub struct Bits`. List the lint name(s) the v1 table was actually carving out (commonly `no-bare-string`, `no-bare-numeric`, etc.); the v2 form requires naming the specific lint per site.
3. **Add per-site `lint:allow`** on each introducer item. Carry a `reason:` referencing the introducer role and a `tracked: #N` for ongoing review:
   ```rust
   // lint:allow(no-bare-string) reason: introducer for string-foundation category; this is the only constructor of the typed Str primitive. tracked: #185
   pub struct Str(u32);
   ```
4. **Optionally add `lint:scope-add`** at a module head when several transitive helpers all need the same primitive carve-out. The scope-add extends the lint's scope for the contained items rather than repeating per-site allows.

Drop the `[primitive-introductions]` table in the same commit.

Verification: after migration, `cargo mock check` should produce the
same finding count it did before the v1 table existed. If new
findings appear, the introducer item has moved during migration (move
the directive), or the v1 table was carving out transitive helpers
the per-site form does not (add explicit `lint:allow` on the helpers
or a `lint:scope-add` at the containing module).

## What changes in your `mockspace.toml`

After migration, the table is gone:

```toml
# v1 (forbidden in v2)
[primitive-introductions]
"string-foundation" = "hilavitkutin-str"
"bit-foundation" = "arvo-bits"

# v2: the directives in source replace the table; no TOML changes needed.
```

The `[lints]` blocks and per-lint `[lints.<name>]` config tables stay
the same. The cascade (catalog defaults → preset chain → workspace
defaults → per-lint → CLI) is also unchanged; presets are an additive
opt-in via `extends = "<host>::<name>"`.

## Reading props from a lint (lint authors only)

If you author lints that need to consult source-level metadata, the
`lint:prop` directive provides the read surface. Two halves:

**Declare what your lint reads.** In your `Lint` impl, return the
prop names you consult:

```rust
impl Lint for AuditCheckLint {
    fn declared_props(&self) -> &'static [&'static str] {
        &["audited", "reviewed"]
    }
    /* ... */
}
```

**Read during dispatch.** The engine populates a `PropMap` per
project at scope time; the resolved map is exposed through the
project surface your `check_document` body already receives. Look up
the prop on the relevant item span and read its value (parsed as a
typed value where the prop's `=value` is present; presence-form props
read as `PropValue::Bool(true)`).

The `mockspace::` namespace prefix is reserved for first-party prop
names; consumer lint packs author their own names without prefix. The
engine emits a `StartupWarning::PropNameConflict` if two unqualified
lints declare the same prop name.

## Lints relocated to preset opt-in (post #568)

The following 15 lints were auto-registered catalog entries in v1 and
through the early v2 phases. They are now first-party presets only;
the catalog no longer registers them at engine startup.

- `no-alloc`
- `no-std`
- `no-dyn-dispatch`
- `no-runtime-spawn`
- `no-runtime-registration`
- `no-bare-numeric`
- `no-bare-string`
- `no-bare-option`
- `no-bare-result`
- `no-public-raw-field`
- `no-vec-in-trait-sig`
- `strategy-marker-required`
- `trait-first-signatures`
- `writing-style`
- `lint-allow-requires-task-id`

These lints' definitions still ship inside mockspace (under
`mock/crates/mockspace-rs/src/builtins/`) and their preset files live
at `mock/crates/mockspace-rs/presets/<name>.toml`. As of #611, the
`extends` mechanism resolves the preset into a synthesised catalog
entry at engine startup. Opt-in is one line per lint:

```toml
[lints.no-bare-numeric]
extends = "mockspace::no-bare-numeric"

[lints.writing-style]
extends = "mockspace::writing-style"
```

Additional `[lints.<name>.config]` or `[lints.<name>.scope]` blocks
overlay onto the preset's defaults, same shape as for any
catalog-registered lint. Per-gate `[lints.<name>.gate.commit]` and
similar overlays apply on top of the preset's severity floor. Chain
references (`extends` pointing at another preset that itself extends
a deeper one) compose innermost-first; every preset in the chain
must point at the same primitive (cross-primitive chains fail at
load time with a structured config error).

The seven bespoke lints that remain auto-registered carry domain
logic the preset surface does not capture today:

- `directive-style-consistency`
- `no-bare-vec`
- `no-manual-id`
- `no-manual-impl`
- `no-adhoc-framework`
- `registrable-completeness`
- `deprecation-comparison`

These keep their existing `[lints.<name>]` config shape in consumer
TOML; nothing changes for them.

## Per-consumer checklist

Walk this once per consumer repo (arvo, hilavitkutin, vehje, notko,
viola, mockspace-hilavitkutin-stack-lints):

1. Search `mock/mockspace.toml` for `[primitive-introductions]`. If present, migrate to per-site `lint:allow` directives in source per the steps above. Delete the TOML table.
2. Search source for existing `// lint:allow(...)` comments. If any lack `reason:` or `tracked: #N`, add them. `SuppressionMetaLint` flags missing fields as findings.
3. Search source for `// lint:allow(...) reason: "will fix when X closes"` patterns. Promote them to the explicit `lint:defer(<name>, until: #N)` shape if the intent is "known, will fix".
4. If the repo authors lints that consult source-level metadata, audit `Lint` impls for `declared_props`; the namespace-conflict detector runs at engine startup.

For repos that do not author lints and do not carry
`[primitive-introductions]`, the v2 engine works without changes;
existing `// lint:allow(...)` comments continue to parse.

(`homma` is not in the consumer list. It does not own a built-in lint
surface; its `mockspace.toml` does not carry the table.)

## Cross-references

- `mock/research/202605221700_directive-vocabulary-reconciled.md`: the truth-of-impl memo behind this guide.
- `mock/research/202605220000_canonical-directive-vocabulary.md`: original 5-directive design (preserved as research artifact; superseded).
- `mock/research/202605220600_lint-provided-marker-directive.md`: original 6th-directive proposal (preserved as research artifact; superseded).
- `mock/crates/mockspace-core/src/lint.rs`: `Directive` enum and `PropValue` (truth-of-impl).
- `mock/crates/mockspace-rs/src/preprocessor/comment.rs`: comment-based directive parser.
- `mock/crates/mockspace-rs/src/preprocessor/rust_attr.rs`: Rust attribute alias parser.
- `mock/crates/mockspace-rs/src/builtins/suppression_meta.rs`: `lint:allow` / `lint:defer` validation logic.
- `mock/crates/mockspace-rs/src/builtins/directive_style_consistency.rs`: the optional uniformity lint.
