//! Function-named identity traits per the workspace
//! harness-the-type-system rule.
//!
//! Two-trait abstraction. [`RefTo<T>`] is the base: anchors the fact
//! that a type acts as a reference to some `T`, but carries no
//! behaviour. Generic code that only needs the type-level statement
//! "this is a reference to T" constrains on [`RefTo<T>`].
//!
//! [`NamedRefTo<T>`] extends [`RefTo<T>`] with the parse + visible-name
//! surface (`AsRef<str>` + `Display` + `parse`). Generic code that
//! needs to read, write, or display the reference reaches for this
//! richer contract.
//!
//! The `T` parameter is the actual thing the reference refers to.
//! For mockspace, T is an entity from [`crate::entity`]: `Round`,
//! `Task`, `Branch`, `GitRef`, `Instant`. The split between the two
//! traits captures the two roles a reference can play for the same
//! entity:
//!
//! - **Stable identifier** ([`RefTo<T>`]), long-lived, machine-keyed,
//!   structural. Identity is what it IS, not what it spells.
//! - **Human-readable name** ([`NamedRefTo<T>`]), short, displayable,
//!   parseable from a single token. The string form IS the canonical
//!   name.
//!
//! For Task, both roles exist naturally:
//! [`crate::task::TaskId`] impls `RefTo<Task>` (composite identifier),
//! and [`crate::slug::Slug`] impls `NamedRefTo<Task>` (leaf-naming).
//!
//! The full design rationale is at
//! `mock/research/202605222200_refto-trait-design.md`.

use core::fmt;
use core::hash::Hash;

/// Marks that a type acts as a reference to some `T`.
///
/// Carries no behaviour. Generic code that needs only the type-level
/// statement "this is a reference to T" (relationship validators,
/// opaque registries, pass-through plumbing) constrains on this
/// trait.
///
/// The `#[marker]` annotation declares that overlapping impls are
/// sound for this trait. Soundness follows from the trait shape:
/// no methods, no associated types, no read-or-write capability that
/// could grant inconsistent rights across impls. A reference under
/// `RefTo<T>` cannot mutate the T it points at; once a type's API
/// admits mutation of T, it leaves the `RefTo<T>` contract for a
/// different trait. Consumers that want overlapping blanket impls
/// (e.g. `impl<R: SomeContract, T> RefTo<T> for R {}` alongside our
/// canonical impls) get them for free.
#[marker]
pub trait RefTo<T>: Eq + Hash + Clone + Sized {}

/// A reference to `T` that has a visible, parseable, formattable
/// name.
///
/// Extends [`RefTo<T>`] with the supertrait bundle that lets
/// consumers treat any named-reference value uniformly: `AsRef<str>`
/// for plumbing, `Display` for diagnostics, plus the parse contract.
/// Not a marker trait, it has items (an associated `Error` type
/// and a `parse` method); overlapping impls would be unsound at the
/// method-body level.
pub trait NamedRefTo<T>: RefTo<T> + AsRef<str> + fmt::Display {
    /// Why parsing failed.
    type Error: fmt::Display + fmt::Debug;

    /// Parse a named reference from its string form. Validates the
    /// impl's shape invariants.
    fn parse(s: &str) -> Result<Self, Self::Error>;
}
