//! Rust lint engine for mockspace v2.
//!
//! Architecture per the schema design memo at
//! `mock/research/202605211200_lint-schema-design.md`. Catalog-based dispatch
//! over 11 reusable primitives + 6 bespoke primitives + external lint-pack
//! entries contributed via the `inventory` distributed slice.
//!
//! # Module map
//!
//! - [`lint`]: the [`Lint`] trait + [`LintMode`] enum (per §1, §2).
//! - [`catalog`]: [`CatalogEntry`] shape + registry mechanism (per §3, §12).
//! - [`finding_sink`]: [`FindingSink`] trait + concrete sinks (per §15).
//! - [`document`]: [`MockspaceDocument`] with `syn` + tree-sitter caches (per §7).
//! - [`project`]: [`MockspaceProject`] with documents, crate graph, design rounds,
//!   suppressions (per §8).
//! - [`strip`]: source-stripping utilities (`StripOpts`, `strip`).
//! - [`config_types`]: shared per-primitive configuration enums (`Visibility`,
//!   `TypePosition`, `ItemKind`, etc.).
//! - [`config_loader`]: TOML loader + override cascade (per §11).
//! - [`staging`]: [`StagingFilter`] git-driven staged-file detection (per §10).
//! - [`errors`]: engine error types ([`ConfigError`], [`LintError`],
//!   [`DispatchError`] etc.) (per §9).
//! - [`preprocessor`]: [`LanguagePreprocessor`] for suppression extraction.
//! - [`engine`]: [`MockspaceEngine`] and `LintEngine` trait impl.
//! - [`builtins`]: the 11 reusable + 6 bespoke primitive impls.
//!
//! # User framing
//!
//! Mockspace-rs is a host-side tool, distinct from the no-alloc / no-std
//! language stack (notko / arvo / hilavitkutin / vehje). It uses std, Vec,
//! HashMap, and rayon freely. The discipline this engine *enforces* on
//! consumer crates does not apply to the engine itself.

pub mod builtins;
pub mod catalog;
pub mod config_loader;
pub mod config_types;
pub mod document;
pub mod engine;
pub mod errors;
pub mod finding_sink;
pub mod lint;
pub mod preprocessor;
pub mod project;
pub mod staging;
pub mod strip;

pub use catalog::{CatalogEntry, catalog_entries, find_entry};
pub use config_loader::{InstantiatedLint, LintsConfig, OverrideCascade};
pub use config_types::{ItemKind, Language, TypePosition, Visibility};
pub use document::{MockspaceDocument, StripOpts};
pub use engine::MockspaceEngine;
pub use errors::{ConfigError, ConfigErrorKind, DispatchError, LintError, LoadError, ParseError};
pub use finding_sink::{FindingSink, RunReport, VecFindingSink};
pub use lint::{Lint, LintMode};
pub use preprocessor::{LanguagePreprocessor, RustPreprocessor};
pub use project::{
    CrateGraph, CrateInfo, DesignRound, DesignRoundsView, MockspaceProject, RoundState,
    WorkspaceMetadata,
};
pub use staging::{StagedSet, StagingFilter, StagingFilterError};

/// The active engine on the host. Swap point: change this alias to switch
/// engines workspace-wide (e.g. to a future viola-driven engine).
pub type ActiveEngine = MockspaceEngine;
