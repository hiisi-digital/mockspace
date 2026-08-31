//! Marker attributes for the mockspace Rust lint engine.
//!
//! The engine's preprocessor reads attribute tokens directly during its
//! source walk; the proc-macros here exist solely to make rustc accept
//! the attribute syntax at consumer call sites. No code is generated;
//! there is no runtime cost.
//!
//! # Attributes
//!
//! ## `#[mock::lints::allow(...)]`
//!
//! Suppresses one or more lints within the attached item's scope. Form:
//!
//! ```ignore
//! #[mock::lints::allow(no_bare_string, tracked = "#509")]
//! pub fn returns_legacy_str() -> String { /* ... */ }
//!
//! #[mock::lints::allow(no_bare_string, no_alloc, tracked = "#477", reason = "FFI boundary")]
//! pub fn alloc_bridge() -> Box<[u8]> { /* ... */ }
//!
//! // Crate-level (suppresses everywhere in the crate):
//! #![mock::lints::allow(no_todo, tracked = "#999")]
//! ```
//!
//! The `tracked = "#N"` parameter is mandatory per the
//! `lint-allow-requires-task-id` workspace rule; the engine emits a
//! meta-finding if a suppression scope is populated without one.
//!
//! # Why a separate crate
//!
//! The marker attribute uses the `mock::lints::allow` path. Rust resolves
//! that path through whatever crate provides those modules; shipping the
//! markers as a tiny no-op proc-macro crate is the simplest way for
//! consumers to opt in without pulling the full engine.

use proc_macro::TokenStream;

/// `#[mock::lints::allow(name1, name2, tracked = "#N", reason = "...")]`
///
/// Marker; expansion drops the attribute and returns the unmodified item.
/// The engine's source preprocessor consumes the attribute tokens before
/// the macro runs (or in parallel; either way the macro itself adds no
/// code).
#[proc_macro_attribute]
pub fn allow(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}
