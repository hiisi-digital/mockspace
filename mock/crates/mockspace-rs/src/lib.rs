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

pub mod agent_builtin;
pub mod bootstrap;
pub mod builtins;
pub mod catalog;
pub mod config_loader;
pub mod config_types;
pub mod crate_graph;
pub mod design_rounds;
pub mod document;
pub mod engine;
pub mod errors;
pub mod explain;
pub mod finding_sink;
pub mod fix;
pub mod invoke;
pub mod lint;
pub mod preprocessor;
pub mod preset_source;
pub mod project;
pub mod scope;
pub mod scope_filter;
pub mod staging;
pub mod strip;

pub use catalog::{catalog_entries, find_entry, CatalogEntry};
pub use config_loader::{
    find_and_read_lints_toml, InstantiatedLint, LintsConfig, LintsTomlFile, OverrideCascade,
};
pub use config_types::{ItemKind, Language, TypePosition, Visibility};
pub use document::{MockspaceDocument, StripOpts};
pub use engine::MockspaceEngine;
pub use fix::{
    apply_plan, plan_fixes, render_unified_diff, ConflictReport, FileChange, FixError, FixOpts,
    FixPlan,
};
pub use explain::{explain_lint, explain_with_entry, ExplainError, ExplainReport, FinalEntry, LayerContribution};
pub use preset_source::{FirstPartyPresetSource, FIRST_PARTY_HOST};
pub use errors::{
    ConfigError, ConfigErrorKind, DirectiveValidationError, DispatchError, LintError, LoadError,
    ParseError, StartupWarning,
};
pub use finding_sink::{FindingSink, RunReport, VecFindingSink};
pub use lint::{Lint, LintMode};
pub use preprocessor::{LanguagePreprocessor, RustPreprocessor};
pub use project::{
    CrateGraph, CrateInfo, DesignRound, DesignRoundsView, MockspaceProject, RoundState,
    WorkspaceMetadata,
};
pub use staging::{StagedSet, StagingFilter, StagingFilterError};

// Re-export the lint-engine vocabulary from mockspace-core so
// consumer crates (e.g. the `mock` binary) can import via this
// crate rather than reaching into mockspace-core directly. Keeps
// the dependency surface "consumer talks to mockspace-rs only";
// substrate types stay one indirection away.
pub use mockspace_core::lint::{
    Finding, Gate, GateSeverity, LintCfgStore, LintEngine, RunSurface, Severity, Span,
};

// Re-export the Phase 5 IO surface (transition executors + the
// types they operate on) so the `mock` binary composes them
// without reaching into mockspace-core. Same indirection rule as
// the lint-engine vocabulary above.
pub use mockspace_core::io::{
    AdvanceError, AdvanceReport, AdvanceVerb, ArchiveError, ArchiveReport, FlockTransitionLock,
    LockError, RepoError, RepoHandle, SealError, SealReport,
};
pub use mockspace_core::phase::{ManifestSide, Phase};
pub use mockspace_core::slug::{Slug, SlugError};
pub use mockspace_core::transition::ReplanMode;
// Re-export gix's ObjectId so callers can parse user-supplied
// hex OIDs (e.g. `--source-tip <hex>` on `mock phase apply`)
// without taking a direct gix dep on top of mockspace-rs.
pub use gix::ObjectId;

/// The active engine on the host. Swap point: change this alias to switch
/// engines workspace-wide (e.g. to a future viola-driven engine).
pub type ActiveEngine = MockspaceEngine;
