//! Entity types, the `T` in `RefTo<T>` and `NamedRefTo<T>`.
//!
//! Each entity here is a conceptual thing mockspace tracks references
//! to. Some entities are zero-sized markers because mockspace does
//! not (yet) carry a full data type for the entity itself; the marker
//! is honest about "this concept exists" without inventing premature
//! structure. The actual data lives on the identifier types
//! ([`crate::task::TaskMeta`], [`crate::round::RoundMeta`]) or on
//! external state (git for `Branch` / `GitRef`, system time for
//! `Instant`).
//!
//! Adding a new entity:
//!
//! 1. Decide whether mockspace tracks references to this kind of
//!    thing as a distinct concept. (Not whether it has a data type.)
//! 2. Add a zero-sized marker (or pull in the existing struct if
//!    mockspace already models the data).
//! 3. Define identifier impls in the appropriate identifier module
//!    (`slug.rs`, `branch_name.rs`, etc.) that impl `RefTo<NewEntity>`
//!    and (where the simple-name role applies) `NamedRefTo<NewEntity>`.
//!
//! The full design rationale is at
//! `mock/research/202605222200_refto-trait-design.md`.

/// A mockspace round: the lifecycle unit that carries a manifest
/// through the six-phase state machine (TOPIC → PLAN(doc) → APPLY(doc)
/// → PLAN(src) → APPLY(src) → DONE).
///
/// Marker only; the round's data lives in
/// [`crate::round::RoundMeta`] and its ref-tree content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Round;

/// A mockspace task: a work item with identity, lifecycle, and
/// content per spec §16.
///
/// Marker only; the task's data lives in
/// [`crate::task::TaskMeta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Task;

/// A git branch in the consumer's repository.
///
/// Marker only; mockspace does not carry a `Branch` data type. The
/// branch's data is whatever git records under `refs/heads/<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Branch;

/// A fully-qualified git ref (e.g. `refs/mock/round/<slug>`,
/// `refs/heads/round/<slug>`).
///
/// Marker only; mockspace does not carry a `GitRef` data type. The
/// ref's data is whatever git records under that path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GitRef;

/// A point in time (used by ISO-8601 timestamps in task closure
/// metadata, anchor capture times, and similar provenance positions).
///
/// Marker only; the actual instant data lives in the formatted
/// string of the identifier or in the system clock at construction.
/// The canonical identifier impl is [`crate::iso8601::Iso8601Utc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instant;
