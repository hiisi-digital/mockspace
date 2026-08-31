# RefTo<T> + NamedRefTo<T> trait design

**Date**: 2026-05-22
**Context**: Type-system harness audit (#591), course-correction during the cascade

## The lesson behind this memo

The first four stack frames of the audit (#597 Slug, #599 Namespace, #598 TaskId, #600 RefPath) introduced traits named for the **concrete identifier shape** (`trait Slug`, `trait Namespace`, `trait TaskId`, `trait RefPath`). The user pushed back on this naming during the BranchName PR review: identifier-shape names hardcode the assumption that future impls will follow the same shape rules. They also leave the abstraction question unanswered: what does a slug actually IS? It is not a slug-of-itself. It is a named reference to a thing.

This memo locks the corrected design before the rename ships. The corrected design moves the trait names up one level of abstraction (function over shape) and pushes the entity identity down into a type parameter.

## The traits

Two traits. One module: `crates/mockspace-core/src/identity.rs`.

```rust
/// Marks that a type acts as a reference to some `T`.
///
/// Carries no behaviour. Generic code that needs only the type-level
/// statement "this is a reference to T" constrains on this trait.
pub trait RefTo<T>: Eq + Hash + Clone + Sized {}

/// A reference to `T` that has a visible, parseable, formattable name.
pub trait NamedRefTo<T>: RefTo<T> + AsRef<str> + Display {
    type Error: Display + Debug;
    fn parse(s: &str) -> Result<Self, Self::Error>;
}
```

`RefTo<T>` is the anchor. It says: "I am a reference to a thing of kind T." That is the entire contract. No methods, no read or write or parse surface. Generic code that needs to verify a relationship between references, or pass references around opaquely, reaches for `RefTo<T>`.

`NamedRefTo<T>` is the read/write surface. It extends `RefTo<T>` with the supertrait bundle (`AsRef<str>`, `Display`) plus the parse contract. Generic code that needs to read the string form, render the name, or accept a parsed identifier reaches for `NamedRefTo<T>`.

Composites don't need a separate trait. Their composite structure is impl detail; the trait abstraction only asks "is this a (named) ref to T?". How the impl assembles its bytes (single-segment, hierarchical, base64-encoded, whatever) is opaque to consumers.

## T is the entity, not the identifier shape

The corrected pattern: the `T` parameter is the **actual thing the identifier refers to**, not the shape of the identifier.

For mockspace v2, the entity universe is:

| Entity | What it is | Defined where |
|---|---|---|
| `Round` | A mockspace round (the lifecycle unit) | round.rs has RoundMeta; the entity itself can be a unit marker for now |
| `Task` | A mockspace task | task.rs has TaskMeta; the entity itself can be a unit marker |
| `Branch` | A git branch in the consumer's repository | NEW entity marker |
| `GitRef` | A fully-qualified git ref | NEW entity marker |
| `Instant` | A point in time | NEW entity marker (timestamp's T) |

Namespace and namespace-segment do NOT appear in the entity table. A namespace segment is structural composition of a TaskId, not a thing the segment refers to. A namespace path is similarly structural, it organises tasks but does not itself name a distinct entity that mockspace tracks at the type level today. If mockspace later grows a real "Namespace" entity (a categorical grouping with its own data), the table extends; until then, namespace machinery stays purely structural with inherent methods on the concrete `Namespace` type and no `RefTo` / `NamedRefTo` impl.

Some entities are full structs already (RoundMeta, TaskMeta); some land as zero-sized markers (Branch, GitRef, Instant) because mockspace doesn't carry a full data type for them yet. Markers are honest: they encode "yes, this thing exists conceptually" without inventing premature structure.

## The two roles a reference plays for the same entity

The RefTo/NamedRefTo split is not a workaround for awkward shapes; it expresses two distinct **roles** a reference can play for the same entity. The trait names declare which role:

- **Stable identifier**. `RefTo<T>`. The contract for "this is THE reference to this T". Long-lived, machine-consumed, used as a key in storage, indexes, foreign-key-like positions. Identity is what it *is*, not what it *spells*. A canonical identifier can be structural (a composite of parts) or flat (a single token); either way, the identity is the type-level fact + the structural payload, not the rendered string.
- **Human-readable name**. `NamedRefTo<T>`. The contract for "this is a string-form name for a T that a human can read, type, parse back, and reason about". Short, displayable, parseable from a single token.

For Task, both roles exist naturally:
- `TaskId` impls `RefTo<Task>`, the stable identifier, structural composition of namespace + leaf. Mockspace stores tasks under refs keyed by TaskId; the identity contract is the (namespace, leaf) pair, not its joined string.
- `Slug` impls `NamedRefTo<Task>`, the human-readable name (the leaf portion when used at a task position). Reads, displays, parses as a single token.

This is true compositing, not internal-plumbing. The two refs coexist because Task as an entity legitimately wants both kinds of reference. The trait split captures that natural duality.

Some entities will have only one role represented:
- `Branch` has `BranchName` (NamedRefTo) as its human name. No separate stable-identifier ref needed today, the branch name IS the identifier, no structural decomposition.
- `Round` has `Slug` (NamedRefTo) as its human name. The slug IS the round identifier today; no separate composite ref.

Some entities might gain a stable-ID role later (a Round might grow a content-hashed canonical identity for archival; a Branch might gain a typed BranchHandle). When they do, those slot in as new `RefTo<T>` impls without changing the existing `NamedRefTo<T>` impls.

### Which bound to pick at a call site

A function that needs the stable identifier (storage key, foreign-key check, durable cross-reference) takes `<R: RefTo<Task>>`. Both `Slug` (when it's the leaf-naming) and `TaskId` (when it's the full identifier) satisfy this. The body works with the type-level fact but cannot read the string form.

A function that needs the human-readable name (display in a diagnostic, write to a config field, accept user input) takes `<R: NamedRefTo<Task>>`. Slug satisfies this. TaskId does not. its string form is a serialization of the composite structure, not the canonical name. Consumers who do want TaskId's rendered form reach for the inherent `task_id.as_uri_form()` on the concrete type.

A function that needs structural decomposition (walk namespace segments, separate leaf from path) takes concrete `&TaskId` or `&Namespace`. The trait abstraction stops where impl-specific structure begins.

## Identifiers and their entity impls

| Identifier | Role | Trait impls |
|---|---|---|
| `Slug` | human-readable name for Round + Task | `RefTo<Round>` + `NamedRefTo<Round>`, `RefTo<Task>` + `NamedRefTo<Task>` |
| `BranchName` | human-readable name for Branch | `RefTo<Branch>` + `NamedRefTo<Branch>` |
| `RefPath` | human-readable name for GitRef | `RefTo<GitRef>` + `NamedRefTo<GitRef>` |
| `TaskId` | stable identifier for Task | `RefTo<Task>` only |
| `Namespace` | structural piece of TaskId (no entity) | neither |
| `Iso8601Utc` | human-readable name for Instant | `RefTo<Instant>` + `NamedRefTo<Instant>` |

Reading the table: rows that impl both `RefTo<T>` and `NamedRefTo<T>` play the human-name role for `T`. Rows that impl `RefTo<T>` only play the stable-identifier role for `T`. Rows that impl neither are structural composition (not references in their own right).

So:

```rust
// In slug.rs
impl RefTo<Round> for Slug {}
impl NamedRefTo<Round> for Slug { /* same parse impl */ }

impl RefTo<Task> for Slug {}
impl NamedRefTo<Task> for Slug { /* same parse impl */ }

// In branch_name.rs (NEW file)
impl RefTo<Branch> for BranchName {}
impl NamedRefTo<Branch> for BranchName { /* parse impl */ }

// In task.rs. TaskId is composite, RefTo only
impl RefTo<Task> for TaskId {}
// NO impl NamedRefTo<Task> for TaskId. TaskId's string form is a
// serialization of its (namespace, slug) structure, not its canonical
// identity. Consumers that need the string form read it via inherent
// methods (`as_uri_form()`, `as_ref_path()`) on the concrete type.

// In namespace.rs. Namespace is structural, no entity
// (no RefTo or NamedRefTo impl. Methods stay inherent.)
```

Both `Slug` and `TaskId` are references-to-Task; a function taking `<T: RefTo<Task>>(id: &T, ...)` accepts either. A function taking `<T: NamedRefTo<Task>>(id: &T, ...)` accepts only Slug because TaskId isn't a simple name. The bound captures both the **intent** (a task reference) and the **kind of access** (structural vs flat-name).

## Disambiguation at the call site

Because a single concrete type can impl `NamedRefTo<T>` for multiple `T`s, calling the trait `parse` method requires explicit `T`:

```rust
let s = <Slug as NamedRefTo<Round>>::parse("arvo-graph-csr")?;
```

The validation is identical regardless of `T` (Slug's charset rules don't care what the slug names). The `T` is purely type-level tagging. Inherent methods stay on the concrete type for convenience:

```rust
let s = Slug::new("arvo-graph-csr")?;  // returns Result<Slug, SlugError>; no T disambiguation needed
```

The trait method exists for generic code; the inherent method exists for the common case. Both produce the same value.

## Function signatures using the abstraction

Three patterns, picked per what the function actually needs.

### Concrete (most internal mockspace code)

When the function needs impl-specific behaviour (rendering as a ref-path, accessing namespace segments, walking a TaskId's structural view, comparing the underlying validated string against a known shape), take the concrete type:

```rust
pub fn task_ref_path(ns: &Namespace, slug: &Slug) -> RefPath {
    /* uses ns.as_ref_path(), inherent on the concrete Namespace */
}

pub fn show_task(task_id: &TaskId) -> Result<TaskMeta, ShowError> {
    /* uses task_id.slug(), task_id.namespace_segments(), inherent */
}
```

### Generic `NamedRefTo<T>` (when the surface is a flat name)

When the function only needs the string form of a simple-name identifier (display, parse, AsRef<str>), constrain on the trait:

```rust
pub fn record_close<B: NamedRefTo<Branch>>(branch: &B) {
    let s: &str = branch.as_ref();
    /* ... */
}
```

This accepts any branch identifier shape that satisfies `NamedRefTo<Branch>`. Critically, it does NOT accept composite shapes (`TaskId`-like things), those don't impl `NamedRefTo` per the simple-name vs composite distinction above.

### Generic `RefTo<T>` (opaque task reference, simple or composite)

When the function takes "any reference to T" regardless of shape (simple-name or composite), constrain on the anchor:

```rust
pub fn relates_to<A: RefTo<Task>, B: RefTo<Task>>(a: &A, b: &B) -> Relation { /* ... */ }
```

This accepts BOTH `Slug` (when it's the leaf-naming a task) and `TaskId` (composite naming a task). The body can't use the string form (no `AsRef<str>` from `RefTo<T>` alone), it only knows the type-level fact "this is a reference to a Task". Useful for relationship validators, opaque registries, and pass-through plumbing.

In practice mockspace's IO functions land mostly in the first two patterns. The `RefTo<T>` anchor is there for relationship logic that treats simple and composite refs uniformly.

## What the audit's rename PR will look like

This memo's design replaces the four already-merged trait introductions (#597-#600) and lands BranchName + Timestamp under the new scheme. The rename PR:

1. Add `crates/mockspace-core/src/identity.rs` with `RefTo<T>` + `NamedRefTo<T>` traits.
2. Add entity types: a small `entity` module under mockspace-core with `Round`, `Task`, `Branch`, `GitRef`, `NamespaceSegment`, `Instant` markers (or pull in existing struct types where they exist; e.g. `Namespace` the struct already implies its own entity).
3. Delete `trait Slug`, `trait Namespace`, `trait TaskId`, `trait RefPath`. Concrete impls keep their natural names (`Slug`, `Namespace`, `TaskId`, `RefPath`, drop the `Default` prefix).
4. Implement `RefTo<Entity> + NamedRefTo<Entity>` for each concrete identifier against its entity type(s).
5. Cascade the rename across consumers: `DefaultSlug` → `Slug`, etc.
6. Generic bounds in IO functions become `<S: NamedRefTo<Round>>` etc. where the entity intent is clear; revert to concrete `&NamespacePath` where the body needs impl-specific behaviour.
7. Add BranchName + Iso8601Utc fresh under the new scheme.

The single big PR replaces #155 (closed) and lands the corrected pattern in one go.

## On `#[marker]` for `RefTo<T>`

`RefTo<T>` is item-less and grants no behaviour. only the type-level fact "this is a reference to T". By the soundness rules for marker traits, overlapping impls are sound: there are no methods that could disagree across impls, no associated types to conflict, no read-or-write capability that could grant inconsistent rights. A reference under `RefTo<T>` cannot mutate the T it points at; once a type's API admits mutation of T (a writer, an editor, a builder), it leaves the `RefTo<T>` contract for a different trait.

Rust's nightly `#[marker]` annotation (the `marker_trait_attr` unstable feature) captures this property: it permits overlapping blanket impls of marker traits exactly because the choice between them is observably indistinguishable. Applying it to `RefTo<T>` would let consumers write overlapping blanket impls like:

```rust
impl<R: SomeContract, T> RefTo<T> for R {}
impl<R: AnotherContract, T> RefTo<T> for R {}
```

without the trait coherence rules rejecting them.

**Decision for this audit**: apply `#[marker]` to `RefTo<T>` and switch mockspace-core to nightly. The rename PR adds:

- A `rust-toolchain.toml` at the mockspace mock-workspace root pinning a recent nightly.
- `#![feature(marker_trait_attr)]` at the top of `crates/mockspace-core/src/lib.rs`.
- `#[marker]` on the `RefTo<T>` declaration.

The cost is acceptable: the broader clause-dev workspace (arvo, hilavitkutin, vehje, notko) already runs on nightly for const generics and other unstable features; mockspace-core remaining stable-only would be the outlier, not the norm. The `marker_trait_attr` feature has been on nightly for ~8 years (tracked at rust-lang/rust#29864) without churn; treating it as durable-nightly is reasonable.

The benefit is structural correctness. `RefTo<T>` is item-less; the soundness of overlapping impls is a property of the trait shape, not an implementation choice. `#[marker]` is how Rust spells that property in the type system. Annotating it now sets up consumer extensibility for free: a downstream crate that wants `impl<T: ConsumerContract> RefTo<MyEntity> for T {}` alongside our canonical impls gets the design they need without having to wait for a separate nightly migration round.

`NamedRefTo<T>` is NOT a marker trait, it has items (an associated `Error` type and a `parse` method). Overlapping impls of `NamedRefTo<T>` would be unsound (the method body and the associated type could disagree). The annotation lives only on `RefTo<T>`.

## What this memo does NOT decide

- Entity types' final names (`Instant` vs `PointInTime`, `Branch` qualifications, etc.). Working names; lock at PR time.
- Whether `Round` and `Task` markers should reference the existing RoundMeta / TaskMeta structs as their canonical owners, or stay separate ZST markers. Likely the latter (markers are tag-only; the meta structs carry the actual data).
- Where the entity types live: `entity.rs` at mockspace-core root, vs distributed across the existing modules. Likely a new `entity.rs` for discoverability.

The rename PR will lock these one way or the other. This memo locks: the trait shape, the T-is-entity principle, the simple-name vs composite distinction (NamedRefTo for simple-name, RefTo only for composite), and the rejection of structural-piece entities (namespace segments are not entities; they're composition).

## Why this matters

The audit's whole point is "harness the type system". The trait names are part of the harness. `trait Slug` named the shape (kebab-case validated string) but bound that shape to its name, which forced future variants to either rename or fork. `trait NamedRefTo<T>` names the function (this is a named reference to T) and leaves the shape to the concrete impl. Future variants. alternative slug charsets, alternative branch-name validators, alternative ref-path layouts. slot in as new `impl NamedRefTo<...>` lines without renaming or forking.

The T parameter being the entity (not the shape) is what makes the abstraction useful: generic code can say "I need a reference to a Task" and the impl picks the carrying shape. Without the entity-as-T design, generic code would still hardcode which identifier shape it expects, and the trait would only buy slot-replacement (one shape swappable for another of the same name), not slot-extensibility (multiple shapes coexisting per kind).
