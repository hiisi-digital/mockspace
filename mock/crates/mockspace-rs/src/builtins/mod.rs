//! Built-in primitives: 11 reusable + 6 bespoke.
//!
//! Per schema design memo §4. Each module declares:
//! - The primitive's `Config` type (TOML-deserialised).
//! - The primitive's `Lint` impl.
//! - The primitive's catalog kind string.
//! - Tests covering positive / negative / scope-variation cases.
//!
//! Concrete catalog entries (with default configs and `inventory::submit!`
//! invocations) live in this module under `builtins/registry.rs` once the
//! primitives are wired (Phase 2D-7).
//!
//! Phase 2D-4 will fill each primitive in turn. The modules are declared
//! up front so module paths stabilise.

// Reusable primitives (11). All 11 wired.
pub mod ast_node_position;
pub mod ast_type_position;
pub mod content_regex;
pub mod cross_doc_symbol;
pub mod file_metric;
pub mod identifier_pattern;
pub mod suppression_meta;
pub mod term_replacement;
pub mod token_scan;
pub mod undocumented_item;
pub mod workflow_state;

// Bespoke primitives (6).
pub mod deprecation_comparison;
pub mod no_adhoc_framework;
pub mod no_bare_vec;
pub mod no_manual_id;
pub mod no_manual_impl;
pub mod registrable_completeness;

// Catalog entry registrations (inventory::submit!).
pub mod registry;

// Bespoke primitives (6).
// pub mod deprecation_comparison;
// pub mod no_adhoc_framework;
// pub mod no_bare_vec;
// pub mod no_manual_id;
// pub mod no_manual_impl;
// pub mod registrable_completeness;

// Catalog entry registry (Phase 2D-7).
// pub mod registry;
