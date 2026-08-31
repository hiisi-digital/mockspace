---
date: 2026-05-22
phase: research
scope: mockspace-rs preprocessor + cross-language directive surface
status: superseded-by-reconciliation
superseded_by: mock/research/202605221700_directive-vocabulary-reconciled.md
supersedes:
  - Concept of `[primitive-introductions]` per-crate TOML table
related:
  - mock/research/202605211200_lint-schema-design.md
  - mock/research/202605201700_engine-preprocessor-architecture.md
---

> **Status update 2026-05-22**: This memo is preserved as a research artifact. The implementation diverged: `lint:introduces` was never shipped and `IntroducerMap` does not exist in source. The truth-of-impl is documented in `mock/research/202605221700_directive-vocabulary-reconciled.md`. Per #570 user decision: option (b), update the memos to match the shipped 5-directive set (Allow, ScopeAdd, Defer, FileDisable, Prop). The retired `[primitive-introductions]` TOML now migrates to per-site `lint:allow` + optional `lint:scope-add` per the reconciled memo. Read the body below for design intent and history; do not treat it as the directive-vocabulary reference.


# Canonical directive vocabulary

Mockspace ships a single canonical vocabulary for source-level directives that processors (lint engine first, formatter and future tooling later) consume. The vocabulary unifies three mechanisms that had been growing in parallel: comment-based `lint:allow`, the per-crate `[primitive-introductions]` TOML table, and the per-lint scope-config exemption fields.

This memo records the design as locked. The implementation tasks live as #119 (retitled, now scoped to introducer directive + IntroducerMap), #186 (retitled, now scoped to the five-directive parser surface), and #543-#549 (new tasks; see end of memo).

## The five directives

The canonical set is five. Lint packs ship new categories within these directives. Lint packs cannot ship new directive keywords; new directive shapes are framework-side schema changes requiring a version bump.

### 1. `lint:allow(<name>)`

Per-site suppression of a specific lint, with required `reason:` (minimum word count enforced by the existing `SuppressionMetaLint`) and `tracked: #N`. Carries over from the existing comment-based suppression mechanism with no semantic change. Only the parsing surface unifies.

```rust
// lint:allow(no-bare-numeric) reason: hardcoded slot count for the FNV-1a constant table per the IETF spec; substituting USize loses const-evaluability here. tracked: #427
const FNV_PRIME: u64 = 0x100000001b3;
```

### 2. `lint:introduces(<category>)`

Marks the current item as the canonical introducer of the named primitive category. Carves out the marked item and its direct impl blocks (same file, same module, same type) from category-checked lints. Does NOT carve out transitive helpers or whole modules.

```rust
// lint:introduces(string-foundation)
pub struct Str(u32);

impl Str {
    pub fn from_static(s: &'static str) -> Self { /* ... */ }  // bare &str OK: same impl block
}

fn unrelated_helper(s: &str) -> Str { /* ... */ }  // bare &str NOT OK: outside impl, requires its own lint:introduces or lint:allow
```

Replaces `[primitive-introductions]` in TOML entirely. The TOML table is hacky precisely because it cannot distinguish "this struct, not that one"; the directive form names the precise item.

### 3. `lint:scope-add(<lint-name>, <axis>=<value>)`

At a module or file boundary, extends the scope of a lint along one axis for the contained items. The axis set is bounded to `ScopeConfig` fields (paths, exempt_paths, languages, exempt_categories, proc_macro_exempt). Lint packs cannot invent new axes through this mechanism; that would be a framework schema change.

```rust
// lint:scope-add(no-bare-numeric, exempt_categories=ffi-boundary)
mod ffi {
    // ffi descriptors get the ffi-boundary category carve-out for the whole module
    #[repr(C)]
    pub struct DescriptorV1 {
        pub version: u32,
        pub flags: u32,
    }
}
```

### 4. `lint:defer(<name>, until: #N)`

Acknowledges a violation is known and will be fixed when the linked task closes. Semantically similar to `lint:allow` but the intent is "known, will fix" rather than "intentionally excepted." The `SuppressionMetaLint` distinguishes the two: allows accumulate as a policy question; defers expire when the linked task closes.

The `forbid_expired` config in `SuppressionMetaConfig` already contemplates this distinction; promoting it to a first-class directive keyword makes the intent user-visible instead of implicit in the reason text.

```rust
// lint:defer(no-bare-string, until: #185) reason: clause test rehab pending API migration of String → Str across test fixtures
fn legacy_test_helper(name: String) { /* ... */ }
```

### 5. `lint:file-disable(<name>)`

File-level disable for the named lint. Placed at the top of a file. Requires the same reason + tracked fields as `lint:allow`. Distinct from `lint:scope-add` in that it is a disable, not a scope extension.

```rust
// lint:file-disable(writing-style) reason: this is a generated FFI binding file; the formatter does not preserve the project's writing-style invariants. tracked: #207
```

## Dual surface: comments are canonical, language-domain decorators are aliases

The directive vocabulary is **comment-based across all languages**. Each language's preprocessor knows its comment delimiter and parses the same five directive forms:

| Language   | Comment form                                     |
|------------|--------------------------------------------------|
| Rust       | `// lint:introduces(string-foundation)`          |
| Zig        | `// lint:introduces(string-foundation)`          |
| TypeScript | `// lint:introduces(string-foundation)`          |
| Markdown   | `<!-- lint:introduces(string-foundation) -->`    |
| TOML       | `# lint:introduces(string-foundation)`           |

This is the load-bearing portability guarantee. Comments work everywhere; the parser cost is one branch per language paid once.

In addition, **languages with idiomatic decorator syntax MAY also support attribute-based aliases**. The canonical internal record (what lands in `IntroducerMap`, `SuppressionMap`, `ScopeAddMap`) is the same regardless of which surface the author used. The alias is a parsing-time transformation, not a separate concept.

For Rust:

```rust
#[mockspace::introduces(string-foundation)]
pub struct Str(u32);

// equivalent to:

// lint:introduces(string-foundation)
pub struct Str(u32);
```

Both produce the same `IntroducerMap` entry. The Rust language-extension parser walks the syn AST for `#[mockspace::*]` attributes and emits the same directive records the comment parser emits. The engine downstream does not know which surface was used; it operates on the unified record.

### Why both forms

The architect's initial analysis argued comment-only on cross-language portability grounds. That argument addressed the wrong constraint. The right design separates:

- **Cross-language portability** is satisfied by the comment form being the canonical, language-agnostic surface.
- **Language idiomaticity, LSP discoverability, IDE tooling integration** are satisfied by allowing language-domain extensions to register *aliases* in the language's native decorator syntax.

The framework owns the canonical vocabulary; the language-domain extensions own the surface-syntax aliases. No information is lost in either direction. A future formatter or LSP completion provider sees the same directive records whether the author wrote comments or attributes.

### Language-extension trait surface

Each language preprocessor registers two parsers:

```rust
pub trait LanguagePreprocessor {
    fn parse_comment_directives(&self, source: &str) -> Vec<DirectiveRecord>;
    fn parse_native_directives(&self, source: &str) -> Vec<DirectiveRecord>;
}
```

The comment parser is shared logic (delimiter varies, body grammar is the same). The native parser is language-specific: for Rust it walks syn AST for `#[mockspace::*]`; for TypeScript it walks the TS AST for `@mockspace.*` decorators; for Python it would walk the AST for `@mockspace.*` decorators; for Zig (no idiomatic decorator syntax) the native parser is a no-op.

Both parsers emit `DirectiveRecord` values into the same vector; the engine treats them identically.

### Consistency lint

A new built-in lint (`directive-style-consistency`) enforces project-level uniformity. Project-level config:

```toml
[lint.directive-style-consistency]
style = "comments-only"  # | "attributes-when-available" | "mixed"
```

- `comments-only`: every directive must be in comment form; attribute usage is a finding
- `attributes-when-available`: in languages with native decorators, every directive must use the native form; comment usage in those languages is a finding (other languages still use comments)
- `mixed`: both forms accepted, no consistency check

Default: `attributes-when-available` for Rust (idiomatic) once the language extension lands; `comments-only` while only the comment parser exists.

## Carve-out scope for `lint:introduces`

The scope of the carve-out is **the marked item + its direct impl blocks** in the same file, same module, same type. Concretely:

- The item declaration itself
- Any `impl ... for <Marked>` block, or any `impl <Marked> { }` block, located within the same file (or, if the type is defined inside a `mod { }`, within that module's tree)
- Items with `pub(super)` or private visibility within the same module block as the marked item

Not included:
- Helper functions outside the impl blocks (even in the same file)
- Sub-modules that consume the type
- Free functions that construct the type without being inside an impl block

A helper that legitimately needs to handle the bare primitive must either (a) sit inside an impl block of the introducer type, or (b) carry its own `lint:introduces(<sub-category>)` or `lint:allow(<lint-name>)` with appropriate justification.

This strict scope is the right choice: the whole point of moving from the per-crate TOML mechanism to per-site directives is granularity. A module-wide or file-wide carve-out would have re-introduced the per-crate hackiness in a different shape.

## Extensibility boundary

The framework defines the directive grammar. Lint packs extend by registering new *categories* and new *lints*, both of which compose with the existing directive vocabulary:

- A lint pack registers category `bit-storage`; a consumer writes `lint:introduces(bit-storage)` on the introducer site
- A lint pack registers lint `no-bare-bit-primitive`; a consumer writes `lint:allow(no-bare-bit-primitive) reason: ...`

A lint pack **cannot** ship a new directive keyword. That would require a framework schema change with a version bump.

This asymmetry is correct: categories and lint names are data; directives are grammar. Data is additive and backwards-compatible; grammar changes require coordinated migration.

## Validation timing

Unknown lint names in directives, and unknown category names in `lint:introduces`, are hard errors at engine startup post-catalog-build, not at file-parse time. The preprocessor stays catalog-agnostic. It emits structured directive records without knowing which lints or categories are registered. The engine validates the records against the assembled catalog during its post-load step, before any dispatching begins.

This prevents silent pass-throughs (unknown lint name in `lint:allow(...)` was previously a no-op; now it fails loud) while keeping the preprocessor a pure parser.

## Auto-fix integration

This vocabulary integrates with the auto-fix domain (see `202605220030_auto-fix-and-structured-diagnostics.md`). Several directive-related issues are trivially auto-fixable:

- Migrating `// lint:introduces(...)` comment to `#[mockspace::introduces(...)]` attribute when project style is `attributes-when-available`
- Migrating attribute form back to comment when project style is `comments-only`
- Adding missing `tracked: #N` placeholder when a `lint:allow` lacks it (with `#NNN` placeholder requiring human resolution)
- Resolving `[primitive-introductions]` TOML migration to `lint:introduces` directives on the actual introducer sites

Each of these gets a `Fix` recipe attached to the corresponding finding so `cargo mock check --fix` can apply them automatically.

## Integration point

`RustPreprocessor::extract` at `mock/crates/mockspace-rs/src/preprocessor.rs:47` (currently a stub returning empty map) is where this lands. The method evolves to:

```rust
fn extract(&self, doc: &MockspaceDocument) -> DirectiveExtractionResult {
    let mut records = Vec::new();
    records.extend(self.parse_comment_directives(doc.source()));
    if let Some(native) = self.native_parser() {
        records.extend(native.parse_native_directives(doc.source()));
    }
    DirectiveExtractionResult::new(records)
}
```

`DirectiveExtractionResult` exposes three parallel structures on the project after extraction:

- `SuppressionMap` (existing, gains `defer` entries alongside `allow`)
- `IntroducerMap` (new, hangs off `MockspaceProject`)
- `ScopeAddMap` (new, hangs off `MockspaceProject`)

The existing `ScopeFilter::accepts` (signature at `mock/crates/mockspace-rs/src/scope_filter.rs:90`; the `project.introduced_categories(crate_name)` call inside it lives at line 123) reads from `project.introduced_categories`. The backing data shifts from the TOML table to the `IntroducerMap`; the call site does not change.

## Migration of `[primitive-introductions]` TOML

The TOML table is deprecated by this design. Migration tooling (#549) walks each consumer repo's mockspace.toml, identifies declared categories, and either:

- Finds the obvious introducer site for the category in source via heuristic search (e.g. "the only `pub struct` named after the category root in the declaring crate") and adds the `lint:introduces` directive there
- Or emits a diagnostic listing the unresolved categories and asks the human to place the directive

The TOML table itself is removed in the same migration commit. Mockspace versions that read the v2 schema reject the table with a clear "deprecated; use `lint:introduces` directives" error.

## Cross-references

- `mock/research/202605211200_lint-schema-design.md`: the catalog work this builds on.
- `mock/research/202605201700_engine-preprocessor-architecture.md`: the preprocessor design this extends.
- `mock/research/202605220030_auto-fix-and-structured-diagnostics.md`: companion memo on the auto-fix domain.
- `mock/crates/mockspace-rs/src/preprocessor.rs:47`: integration point.
- `mock/crates/mockspace-rs/src/scope_filter.rs:123`: existing `introduced_categories` call site that the new IntroducerMap backs.

## Tasks

Existing #119 and #186 retitled (descriptions updated). New tasks land as #543-#549.
