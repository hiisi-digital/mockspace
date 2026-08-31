---
date: 2026-05-22
phase: research
status: superseded-by-reconciliation
superseded_by: mock/research/202605221700_directive-vocabulary-reconciled.md
scope: mockspace-rs 6th directive (lint-provided per-site properties consumed by lints)
related:
  - mock/research/202605220000_canonical-directive-vocabulary.md
review:
  - 2026-05-22 architect (feature-dev:code-architect): all 8 questions answered, name locked to `prop`, dual-index PropMap, three explicit scope accessors, declared_props on Lint trait
---

> **Status update 2026-05-22**: This memo is preserved as a research artifact. The implementation took `lint:prop` as the 5th directive (not the 6th) because `lint:introduces` was never shipped. References below to `Introduces { /* existing */ }` and `IntroducerMap` describe surfaces that do not exist in source. The truth-of-impl is documented in `mock/research/202605221700_directive-vocabulary-reconciled.md`. Per #570 user decision: option (b). Read the body below for the `lint:prop` design rationale (name choice, dual-index PropMap, scope accessors, declared_props); the surrounding "6th-directive joining the canonical 5" framing is historical.


# A sixth directive: `lint:prop` (lint-provided per-site properties)

This memo proposes a sixth source-level directive joining the canonical five (`allow`, `introduces`, `scope-add`, `defer`, `file-disable`). The architect-reviewed shape is locked subject to user confirmation; implementation tasks follow once locked.

## Origin

The user surfaced a gap during the comment-parser integration work for #544 (six PRs of v2 work landed earlier this session). The first five canonical directives are all framework-managed: each maps to a specific behaviour the engine implements directly. A recurring pattern from real consumers does not fit any of those five.

A lint wants to declare that this site has property X, then a lint (possibly the same one, possibly a different one) wants to read that declaration to decide its own behaviour. The existing `lint:introduces` directive is one specialised case of the pattern: the bare-primitives lint declares that this item legitimately introduces a category, and the same lint family reads that declaration when checking neighbouring sites.

Generalising the pattern gives every lint a place to put domain-specific per-site state without growing the framework's directive vocabulary every time a lint needs a new flag.

## Name: `prop`

Rejected names with reasoning:

- **`env`**: wrong semantics. "Environment" suggests OS / runtime state. The directive is about authored-source state.
- **`meta`**: too generic. Every directive is "meta" in some sense. Says nothing about what the directive does.
- **`flag`**: too thin. Implies boolean presence only; the design wants key-value too.
- **`attr` / `attribute`**: collides with Rust attributes, which are already a separate aliasing surface.
- **`pragma`**: historically loaded, suggests C-preprocessor-style behaviour.
- **`claim`**: carries epistemic weight (author asserting) that breaks down at the call site. What the consumer actually does is read whether a property is declared, not weigh the assertion.
- **`tag`**: ecosystem collision (git tags, classification tags, HTML tags).
- **`fact`**: reads Prolog-flavoured in code comments.
- **`mark`**: too close to a cursor position.
- **`note`**: doc-flavoured, sounds like a documentation comment.
- **`label`**: clean and viable; loses narrowly to `prop` because "label" implies classification without implying a value.

**`prop`** (short for "property") is the locked name. Reasoning:

- Universal vocabulary in type systems and configuration systems.
- Carries both presence and key-value forms neutrally.
- No ecosystem collision.
- Reads clean in code:

```rust
// lint:prop(audited)
// lint:prop(arena_size = 4096)
// lint:prop(audit_id = "A-2026-04")
```

## Abstraction

The framing "lint-declared per-site property consumed by lints" is correct. `introduces` does NOT get generalised in place. Keeping the two as separate directives matters because:

- `introduces` has tightly scoped semantics: it carves out a specific category check for a specific introducer site. The carve-out is the payload, not the name.
- If `prop` absorbed `introduces`, every consumer reading an `introduces` directive would have to pattern-match on a known prop name (`// lint:prop(introduces = "string-foundation")`) instead of reading a first-class directive variant.
- Engine validation differs: `introduces` is validated against registered categories at startup; `prop` names are validated against registered lint schemas (a softer, future check). Two different lifecycles in the same directive variant is a design mistake.

`introduces` and `prop` ship side-by-side. The five-directive design becomes a six-directive design; the framework directive grammar stays closed.

## Surface

Three forms, all routing to the same internal `Directive::Prop { name, value }`:

```text
// lint:prop(<name>)                       presence (parses as PropValue::Bool(true))
// lint:prop(<name> = <value>)             key-value (typed)
```

Where `<value>` is one of: a quoted string, a bare integer, `true`, `false`.

Examples from realistic consumer use cases:

```rust
// lint:prop(audited)
fn unsafe_ffi_handle() {}

// lint:prop(arena_size = 4096)
struct StaticBuffer { /* ... */ }

// lint:prop(audit_id = "A-2026-04")
pub fn export_descriptor() {}

// lint:prop(thread_safe)
impl<T: Send + Sync> Pool<T> { /* ... */ }
```

The directive accepts the same trailing `reason: "..."` clause the other directives use, for the rare case a prop wants to carry a human note alongside its name and value.

### Presence and key-value are one abstraction

Presence (`// lint:prop(audited)`) is syntactic sugar for a key-value pair with an implicit `true` value. A single `Directive::Prop { name, value: PropValue }` variant with `PropValue::Bool(true)` as the presence-form arm is the right shape. No separate `lint:flag` directive.

### Relational operators do NOT belong in the directive

`// lint:prop(arena_size > 0)`. Rejected. The directive is the author's declaration of a property's value. The consuming lint applies comparisons in Rust code. Pushing comparison logic into the directive grammar creates a parser dependency on precedence, type coercion, and whitespace that pays nothing the consuming lint cannot already do in two lines.

If a follow-up genuinely needs the relational form, the grammar can absorb it without breaking the simpler form. Do not ship it now.

### List values do NOT belong in v1

`// lint:prop(allowed_imports = ["alloc", "core"])`. Rejected. Array syntax pushes the comment grammar past permissive-and-minimal. The consumer who needs multi-value writes multiple `prop` directives on consecutive lines:

```rust
// lint:prop(allowed_import = "alloc")
// lint:prop(allowed_import = "core")
```

Multiple prop directives on the same item accumulate in the `PropMap` naturally. `all_named("allowed_import")` returns both. Simpler grammar, identical semantics.

## Internal shape

### Directive enum extension

```rust
pub enum Directive {
    Allow { /* existing */ },
    Introduces { /* existing */ },
    ScopeAdd { /* existing */ },
    Defer { /* existing */ },
    FileDisable { /* existing */ },
    Prop {
        name: String,
        value: PropValue,
        reason: Option<String>,
    },
}

pub enum PropValue {
    Bool(bool),
    Integer(i64),
    String(String),
}
```

### PropMap with dual-index

The two common queries are "what props does this site have?" and "which sites have this prop?". A single-index map makes one O(1) and the other O(n). Dual-index makes both log-time at the cost of slightly more memory:

```rust
pub struct PropMap {
    by_name: BTreeMap<String, Vec<(Span, PropValue, Option<String>)>>,
    by_span: BTreeMap<Span, Vec<(String, PropValue, Option<String>)>>,
}
```

(The `Option<String>` is the optional reason clause.)

`PropMap` lives in `mockspace-core` and is host-side / `std`-permitted, peer of `SuppressionMap` / `IntroducerMap` / etc. The `BTreeMap` + `Vec` + `String` shape is appropriate at this layer; no need to push this into a `no_std` storage primitive.

### Three explicit scope accessors

The default attachment is "item + direct impl blocks", the same rule `introduces` already uses. **Implementation note**: when slice 3 of #544 (the per-kind maps) introduces the `IntroducerMap` shape, the attachment-walk should be extracted into a shared helper that both `IntroducerMap::including_impl_blocks` and `PropMap::including_impl_blocks` call into. Two parallel implementations of the same rule would drift; one shared helper keeps them in lockstep.

Lints may legitimately want a broader scope. Hide the ambiguity behind named accessors:

```rust
impl PropMap {
    /// Props declared exactly at this span.
    pub fn at_site(&self, span: &Span) -> impl Iterator<Item = (&str, &PropValue)>;

    /// Props at this span and on its direct impl blocks (same file, same module, same type).
    /// Default attachment rule, matches `introduces`.
    pub fn including_impl_blocks(&self, span: &Span) -> impl Iterator<Item = (&str, &PropValue)>;

    /// Props anywhere in the enclosing item chain (module, file, crate root).
    pub fn walk_ancestors(&self, span: &Span) -> impl Iterator<Item = (&Span, &str, &PropValue)>;

    /// All sites carrying a prop with this name, regardless of scope.
    pub fn all_named(&self, name: &str) -> &[(Span, PropValue, Option<String>)];
}
```

Each lint declares which resolution it uses. The `directive-style-consistency` lint (#548) can eventually flag prop usages where the lint's declared resolution does not match the site's prop placement.

### `Lint::declared_props`

Add an optional method to the `Lint` trait so the future consistency lint can check that every `lint:prop(name = ...)` in the project has at least one registered lint that declares it:

```rust
pub trait Lint: Send + Sync {
    // existing methods ...

    /// Names of props this lint reads. Default empty.
    fn declared_props(&self) -> &'static [&'static str] {
        &[]
    }
}
```

Ship the trait method now; the consistency lint that consumes it ships later as a follow-up. The trait method's existence does not affect any existing lint impl.

### Namespace handling: detect, do not require

Reserve `mockspace::` as a framework namespace for first-party prop names. Third-party lint packs use unqualified names by default. At engine startup, if two registered lints both declare the same `declared_props()` name and are from different lint packs, emit a `PropNameConflict` warning (not error) that names both lints. Requiring namespace prefixes for all third-party names (`stack-lints::audited`) is more friction than the problem warrants at v1.

If a confirmed conflict emerges, the affected lint packs can adopt namespacing on their own schedule. The detection warning surfaces the need.

**Coverage gap**: `PropNameConflict` only fires when two lints both declare the same name. The other failure mode (a source file uses `// lint:prop(name = ...)` for a name no lint declares) is caught downstream by the `directive-style-consistency` lint from #548 once that lint ships. The memo for #548 should cross-reference this gap explicitly when it lands.

## Parser surface impact

The 5-directive parser in `mock/crates/mockspace-rs/src/preprocessor/comment.rs` extends near-trivially:

1. Add `Directive::Prop { name, value, reason }` to the `Directive` enum (mockspace-core).
2. Add `PropValue` enum to mockspace-core.
3. Add a `parse_prop(args, tail)` arm to the keyword dispatch in `comment.rs`.
4. Inside `parse_prop`, the args parser tries `name = value` first (parses string / int / bool literal), falls back to bare `name` for the presence form (yields `PropValue::Bool(true)`).
5. No new comment delimiter shapes; no new tail handling. Existing `reason: "..."` clause applies as-is.
6. Add `PropMap` to mockspace-core/src/lint.rs (peer of `SuppressionMap`).
7. `RustPreprocessor::extract` routes `Directive::Prop` into the `PropMap` slot. Integration shape depends on #546's per-kind-map architecture; defer wiring until #546 settles.

The framework changes are purely additive: existing directives, `DirectiveRecord`, and existing maps stay unchanged. Only the `Directive` enum grows one variant and mockspace-core grows one new map type plus one new value enum.

## Implementation plan once user-confirmed

Tasks to create:

- **Slice 1**: add `Directive::Prop` + `PropValue` to mockspace-core, plus serde round-trip tests. Pattern matches the type-only slice 1 of #544 (PR #42).
- **Slice 2**: extend `comment::parse_directives` with the `parse_prop` arm. Pattern matches the parser slice 2 of #544 (PR #43).
- **Slice 3**: add `PropMap` to mockspace-core with the dual-index shape and three scope accessors. Add `Lint::declared_props` trait method (default empty).
- **Slice 4**: route `Directive::Prop` records into `PropMap` in `RustPreprocessor::extract`. Pattern matches the integration slice 3 of #544 (PR #44, just merged).
- **Slice 5**: namespace-conflict detection at engine startup (warning, not error).

## Cross-references

- `mock/research/202605220000_canonical-directive-vocabulary.md`: parent design for the 5-directive vocabulary. Extends to six.
- `mock/crates/mockspace-rs/src/preprocessor/comment.rs`: parser to extend.
- `mock/crates/mockspace-core/src/lint.rs`: `Directive` enum (line ~584), maps (line ~666-715), `Lint` trait (in mockspace-rs/src/lint.rs).
- Companion #546: per-kind-map architecture. Slice 4 of this work waits on #546 settling.
