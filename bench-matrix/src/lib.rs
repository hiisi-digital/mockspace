//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! # mockspace-bench-matrix
//!
//! The opinionated semantic-matrix layer on top of the mockspace bench harness.
//! It lets a project author a *composited* benchmark (one bench, many isolated
//! cells, each timed individually, swept across parameter axes) and get the
//! measurement discipline provided instead of re-derived: the anti-hoist chain,
//! the shared seed table, the setup-vs-iteration (S vs I) split, the
//! reps-invariant fidelity digest, the fold-one-keep-alive rule, and the
//! cold/aliased-predictor regime all live in one compiled, tested [`scaffold`]
//! function, not in a template string every consumer copies and gets wrong.
//!
//! ## The three layers
//!
//! - The harness (`mockspace-bench-harness`) is the transport: cdylib-per-variant
//!   isolation, the subprocess driver, the multi-axis spec to variant-crate
//!   codegen. It knows nothing about honest measurement.
//! - This crate is the opinionated discipline on top: the [`scaffold`] wrappers,
//!   the [`decl`] data model, [`generate::generate_all`], and the single canonical
//!   template. This is where the disciplines a review panel spent four experts
//!   validating are encoded once.
//! - The consumer owns its domain: the program generator, the value semantics, and
//!   the cell functions themselves.
//!
//! ## The mechanism (why cells can be real typed functions)
//!
//! A cell must compile as its own cdylib (fat LTO, one codegen unit) for
//! measurement isolation, which is why the pre-extraction approach spliced cells
//! as strings into generated crates. But the string `body` was ALREADY a call back
//! into the consumer crate that fat LTO already inlined across that boundary. So a
//! cell can be a real, type-checked `pub fn` in the consumer crate, and each
//! generated variant crate a one-line call to it by PATH; per-variant LTO inlines
//! it into the isolated cdylib identically. The sibling crate needs the cell's
//! path, never its body, so the generator is a plain function over data, with no
//! proc-macro and no closure-token capture. The one structural rule: [`scaffold`]
//! takes the cell as a generic `FnMut` parameter (monomorphized, inlined, no
//! indirection), never a `fn` pointer.
//!
//! ## Status
//!
//! First cut: the engine (scaffold, decl, generate_all, canonical template) is
//! complete and usable by hand-writing the `pub fn` cells plus a `matrix_decls()`
//! returning [`decl::MatrixDecl`]. The ergonomic `bench_matrix!` macro that emits
//! those from a declarative block, and the `FfiBenchCall` ABI extension that
//! surfaces the `setup_ticks` / `first_ticks` / `digest` columns, are the top
//! items in `TODO.md`. See `README.md` for the full design and a worked example.

pub mod decl;
/// The harness transport (sibling-crate + `bench.toml` codegen). Behind the
/// `generate` feature (default-on): a consumer that only authors cells does not
/// need it, and opting out drops the whole bench-harness dependency tree.
#[cfg(feature = "generate")]
pub mod generate;
#[macro_use]
pub mod macros;
pub mod scaffold;
/// The `boundary::Runtime` helper for boundary benches: a dlopen'd sibling runtime
/// cdylib a cell crosses into as a real cross-object call. Behind the `boundary`
/// feature (opt-in): only boundary benches need it, and it adds a `libloading` dep
/// to the per-variant-rebuilt cell crate.
#[cfg(feature = "boundary")]
pub mod boundary;

pub use decl::{CellDecl, MatrixDecl, Regime, SweepAxis};
#[cfg(feature = "generate")]
pub use generate::{generate_all, LIB_TEMPLATE};
pub use scaffold::{cold_cycle, stream, warm, Measured, SEEDS};
#[cfg(feature = "boundary")]
pub use boundary::Runtime;

// Re-exported for the `bench_matrix!` macro's `cell_<tag>` name concatenation, so
// a consumer using the macro needs no direct `paste` dependency. Not part of the
// public API surface.
#[doc(hidden)]
pub use paste;
