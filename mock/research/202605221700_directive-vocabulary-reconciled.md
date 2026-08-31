---
date: 2026-05-22
phase: research
status: reconciled-with-impl
scope: mockspace-rs canonical directive vocabulary as actually shipped
supersedes:
  - mock/research/202605220000_canonical-directive-vocabulary.md (5-directive design including lint:introduces)
  - mock/research/202605220600_lint-provided-marker-directive.md (proposed 6th directive)
related:
  - mock/crates/mockspace-core/src/lint.rs (Directive enum, lines 589-656)
  - mock/crates/mockspace-rs/src/preprocessor/comment.rs (5-directive parser)
  - mock/crates/mockspace-rs/src/preprocessor/rust_attr.rs (Rust attribute alias parser)
review:
  - 2026-05-22 user decision: option (b) on #570, update memo to match impl
---

# Canonical directive vocabulary (reconciled)

Mockspace ships five source-level directives. Earlier design memos (`202605220000_canonical-directive-vocabulary.md` and `202605220600_lint-provided-marker-directive.md`) described a different set: the original 5 included `lint:introduces` with a backing `IntroducerMap`; the prop memo proposed promoting that to a 6th. The implementation took a different path: `lint:introduces` was never shipped, the `IntroducerMap` does not exist, and `lint:prop` took the 5th-directive slot. This memo records the directive vocabulary as actually implemented so future agents and consumers read the truth-of-impl rather than the design history.

The decision (user, 2026-05-22, in response to `#570`'s audit findings): keep the implementation as shipped; update the memos to match. The introducer concept is not promoted to a first-class directive variant. Consumers needing per-site category carve-outs use the per-site `lint:allow` mechanism plus optional `lint:scope-add` for transitive helpers.

## The five directives

The canonical set is five. Lint packs ship new categories and new lint names within these directives. Lint packs cannot ship new directive keywords; new directive shapes are framework-side schema changes requiring a version bump (see `LINT_CONTRACT_VERSION` in `mockspace-core/src/lint.rs`).

### 1. `lint:allow(<name>)`

Per-site suppression of a specific lint, with `reason:` (minimum word count enforced by `SuppressionMetaLint`) and `tracked: #N`. Both fields are `Option<String>` at the parse surface; the meta-lint validates them downstream so ill-formed directives surface as findings rather than panicking the parser.

```rust
// lint:allow(no-bare-numeric) reason: hardcoded slot count for the FNV-1a constant table per the IETF spec; substituting USize loses const-evaluability here. tracked: #427
const FNV_PRIME: u64 = 0x100000001b3;
```

### 2. `lint:scope-add(<lint-name>, <axis>=<value>)`

At a module or file boundary, extends a lint's scope along one axis for the contained items. The axis set is bounded to `ScopeConfig` fields: `paths`, `exempt_paths`, `crates`, `exempt_crates`, `languages`, `proc_macro_exempt`. Lint packs cannot invent new axes through this mechanism; that would be a framework schema change.

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

### 3. `lint:defer(<name>, until: #N)`

Acknowledges a violation is known and will be fixed when the linked task closes. Distinct from `lint:allow`: allows are intentional exceptions that accumulate as policy; defers expire when the linked task closes. `SuppressionMetaLint`'s `forbid_expired` config controls whether expired defers flip from permitted to forbidden.

```rust
// lint:defer(no-bare-string, until: #185) reason: clause test rehab pending API migration of String to Str across test fixtures
fn legacy_test_helper(name: String) { /* ... */ }
```

### 4. `lint:file-disable(<name>)`

File-level disable for the named lint. Placed at the top of a file. Requires the same `reason:` and `tracked:` fields as `lint:allow`. Distinct from `lint:scope-add` in that it is a disable, not a scope extension.

```rust
// lint:file-disable(writing-style) reason: this is a generated FFI binding file; the formatter does not preserve the project's writing-style invariants. tracked: #207
```

### 5. `lint:prop(<name>)` / `lint:prop(<name> = <value>)`

Lint-declared per-site property consumed by lints. The framework does not interpret the prop name or value; lints declare via `Lint::declared_props` which names they read and query the resolved `PropMap` for matches at dispatch time.

Three forms, all routing to the same internal `Directive::Prop { name, value, reason }`:

```rust
// lint:prop(audited)
fn unsafe_ffi_handle() {}

// lint:prop(arena_size = 4096)
struct StaticBuffer { /* ... */ }

// lint:prop(audit_id = "A-2026-04")
pub fn export_descriptor() {}
```

Presence form (`lint:prop(audited)`) parses to `PropValue::Bool(true)`. Key-value forms accept Bool / Integer / String literals. An optional trailing `reason: "..."` clause attaches to any variant for human notes.

The `mockspace::` prop name prefix is the reserved first-party namespace; collisions among `mockspace::`-prefixed names are silent (one pack's coordinated namespace). Unqualified prop collisions across two or more lints surface as `StartupWarning::PropNameConflict` at engine startup.

## Dual surface: comments are canonical, language-domain decorators are aliases

The directive vocabulary is comment-based across all languages. Each language's preprocessor knows its comment delimiter and parses the same five directive forms:

| Language   | Comment form                            |
|------------|-----------------------------------------|
| Rust       | `// lint:allow(no-bare-numeric)`        |
| Zig        | `// lint:allow(no-bare-numeric)`        |
| TypeScript | `// lint:allow(no-bare-numeric)`        |
| Markdown   | `<!-- lint:allow(no-bare-numeric) -->`  |
| TOML       | `# lint:allow(no-bare-numeric)`         |

Comments work everywhere; the parser cost is one branch per language paid once.

In addition, languages with idiomatic decorator syntax MAY support attribute-based aliases. The Rust attribute parser at `preprocessor/rust_attr.rs` aliases all five directives (`allow`, `scope-add`, `defer`, `file-disable`, `prop`). The canonical internal record is the same regardless of which surface the author used; the alias is a parsing-time transformation, not a separate concept.

For Rust:

```rust
#[mockspace::allow(no-bare-numeric, reason = "...", tracked = "#427")]
const FNV_PRIME: u64 = 0x100000001b3;

// equivalent to:

// lint:allow(no-bare-numeric) reason: ... tracked: #427
const FNV_PRIME: u64 = 0x100000001b3;
```

Both produce the same `Directive::Allow` entry. The Rust language-extension parser at `preprocessor/rust_attr.rs` walks the syn AST for `#[mockspace::*]` attributes and emits the same directive records the comment parser emits. The engine downstream does not know which surface was used; it operates on the unified record.

## `[primitive-introductions]` retirement (no directive replacement)

The `[primitive-introductions]` TOML table was retired in PR #60 (#549). It did not gain a directive replacement. The shipped per-site approach uses the existing `lint:allow` and `lint:scope-add` directives:

- For each item that legitimately introduces a category (e.g. `pub struct Str` introduces `string-foundation`), the introducer item plus any helper that needs the bare primitive carries a `lint:allow(<lint-name>) reason: "..." tracked: #N` on the specific site.
- For transitive helpers in a module that all need the same carve-out, a single `lint:scope-add(<lint-name>, exempt_paths=<glob>)` at the module head extends the scope rather than repeating per-site allows.

This gives more granular control than the per-crate TOML table did (the per-site form names the precise item) at the cost of slightly more comments at the source-site level. The per-crate table's coarse "this whole crate is the introducer for category X" was load-bearing only because the table could not address individual items; the per-site form does not need that level of indirection.

### Migration shape

Consumer repos with `[primitive-introductions]` in their `mockspace.toml`:

1. Drop the `[primitive-introductions]` table from `mock/mockspace.toml`.
2. Find each item the table previously carved out. Usually the obvious `pub struct` named after the category root (`string-foundation` → `pub struct Str`; `bit-foundation` → `pub struct Bits`).
3. Add a `// lint:allow(<lint-name>) reason: "introducer for <category>" tracked: #N` on the introducer site. Use the lint name that the v1 carve-out was actually addressing (commonly `no-bare-string`, `no-bare-numeric`, etc.).
4. For transitive helpers that legitimately need the same primitive (constructors, parsers, codecs), add either their own per-site `lint:allow` or a single `lint:scope-add(<lint-name>, exempt_paths=<file-glob>)` at the module head.

Verification: after migration, `cargo mock check` should produce the same finding count it did before the v1 table existed. New findings indicate either a missed allow site or a transitive helper that needs the scope-add.

The v2 schema validator hard-fails on a present `[primitive-introductions]` table with a clear "deprecated; use per-site lint:allow / lint:scope-add directives" error.

## Validation timing

Unknown lint names in any directive are hard errors at engine startup post-catalog-build, not at file-parse time. The preprocessor stays catalog-agnostic; it emits structured `Directive` records without knowing which lints are registered. The engine validates the records against the assembled catalog during its post-load step (`validate_directives` at `mockspace-rs/src/engine.rs:244`), before any dispatching begins. The gate surfaces as `ParseError::DirectiveValidation` on the project's `scope_project` call so CI sees every unknown name in one report.

## Carve-out scope rules

Each directive carves out a specific span:

| Directive       | Default attachment span                                    |
|-----------------|------------------------------------------------------------|
| `lint:allow`    | The item the directive is attached to                      |
| `lint:scope-add`| The containing module or file                              |
| `lint:defer`    | The item the directive is attached to                      |
| `lint:file-disable` | The whole file                                         |
| `lint:prop`     | The item + its direct impl blocks (same file/module/type)  |

For `lint:prop`, the "item + impl blocks" default matches the semantic of "this site has this property", since impl blocks contribute to the type's surface. Lints reading prop values can request a different scope through named accessors (`PropMap::at_site` / `including_impl_blocks` / `transitive`).

## Auto-fix integration

This vocabulary integrates with the auto-fix domain (see `202605220030_auto-fix-and-structured-diagnostics.md`). Auto-fixable directive-related issues:

- Migrating comment form to attribute form when project style is `attributes-when-available`
- Migrating attribute form back to comment when project style is `comments-only`
- Adding missing `tracked: #N` placeholder when a `lint:allow` lacks it (with `#NNN` placeholder requiring human resolution)

Each gets a `Fix` recipe attached to the corresponding finding so `cargo mock check --fix` can apply them automatically.

## Style-consistency lint

`directive-style-consistency` enforces project-level uniformity. Config (wire form is kebab-case via serde rename, matching `Style::{CommentsOnly, AttributesWhenAvailable, Mixed}` in `builtins/directive_style_consistency.rs`):

```toml
[lint.directive-style-consistency]
style = "mixed"  # | "comments-only" | "attributes-when-available"
```

- `comments-only`: every directive must be in comment form; attribute usage is a finding
- `attributes-when-available`: in languages with native decorators, every directive must use the native form; comment usage in those languages is a finding (other languages still use comments)
- `mixed`: both forms accepted, no consistency check

Default: `mixed`. Per-repo opt-in to a stricter style is one TOML line.

## Extensibility boundary

The framework defines the directive grammar. Lint packs extend by registering new lint names and lint-pack-internal prop names:

- A lint pack registers lint `no-bare-bit-primitive`; a consumer writes `lint:allow(no-bare-bit-primitive) reason: ...`
- A lint pack's lint reads prop `arena_size`; a consumer writes `lint:prop(arena_size = 4096)` on the relevant site.

A lint pack cannot ship a new directive keyword. That would require a framework schema change with a version bump.

## What changed from the original design memos

`202605220000_canonical-directive-vocabulary.md` (the 5-directive memo) described `lint:introduces` as the 2nd directive, carving out items as canonical category introducers. The implementation did not ship that variant. The `IntroducerMap` referenced by that memo and by the `202605220600_lint-provided-marker-directive.md` follow-up does not exist in source.

Reasons the implementation diverged from the design (deducible from the audit; the actual decision history was not recorded in code):

- The `[primitive-introductions]` TOML retirement (PR #60) did not block on the replacement directive shipping. It went out first, leaving consumers without the carve-out mechanism the memo expected.
- The 5th directive slot was taken by `lint:prop` (per `202605220600`'s sixth-directive proposal), with `prop` filling the slot that the original memo allocated to `introduces`.
- The per-site `lint:allow` mechanism was sufficient for actually-shipped consumer use cases. The introducer-concept-as-directive turned out to be unnecessary; per-site `lint:allow` with a clear `reason:` field carries the same semantic with one extra line per introducer site.

This memo (2026-05-22) reconciles the design history with the shipped impl. The earlier memos are kept as research artifacts; this memo is the truth-of-impl going forward. PR #70 was closed without merge because it premised on the original-memo directive set; the migration guide that re-opens after this memo lands uses the reconciled set.

## Cross-references

- `mock/research/202605220000_canonical-directive-vocabulary.md`: original 5-directive design (preserved as research artifact)
- `mock/research/202605220600_lint-provided-marker-directive.md`: original 6th-directive proposal (preserved as research artifact)
- `mock/research/202605220030_auto-fix-and-structured-diagnostics.md`: companion memo on the auto-fix domain (unchanged by this reconciliation)
- `mock/crates/mockspace-core/src/lint.rs:589-656`: `Directive` enum + `PropValue` definitions (truth-of-impl)
- `mock/crates/mockspace-rs/src/preprocessor/comment.rs`: 5-directive comment parser
- `mock/crates/mockspace-rs/src/preprocessor/rust_attr.rs`: Rust attribute alias parser
- `mock/crates/mockspace-rs/src/builtins/suppression_meta.rs`: `lint:allow` / `lint:defer` / `lint:file-disable` meta-validation
- `mock/crates/mockspace-rs/src/builtins/directive_style_consistency.rs`: `directive-style-consistency` lint
- `docs/MIGRATION-v1-to-v2-lints.md`: consumer migration guide grounded in this memo

## Tasks

- #119 (introducer directive + IntroducerMap): superseded by this memo. No directive variant lands. Consumers use per-site `lint:allow` + `lint:scope-add`.
- #546 (`IntroducerMap`, `ScopeAddMap`, extend `SuppressionMap` for defer entries): partial completion accepted; the IntroducerMap part is dropped per option (b) on #570. ScopeAddMap and SuppressionMap.defer landed and are correct.
- #570 (drift documentation): closes when the migration guide PR merges referencing this memo.
- #556 (downstream migration guide): unblocked by this memo; the migration guide grounds on the reconciled 5-directive set documented above.
