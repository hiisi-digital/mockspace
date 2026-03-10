//! AST lint rules for mockspace workspaces.
//!
//! Each lint implements the `Lint` trait (per-crate) or `CrossCrateLint` trait
//! (cross-crate). Consumers build `LintContext` for each crate and call
//! `check_crate()` / `check_cross_crate()` to run the rule sets.

mod actionable_errors;
mod no_adhoc_framework;
mod no_bare_string;
pub mod changelist_helpers;
mod changelist_doc_gate;
mod changelist_immutability;
mod changelist_lock;
mod changelist_required;
mod deprecation_comparison;
mod design_doc_source_mismatch;
pub mod type_scanner;
mod export_count;
mod file_size;
mod no_adhoc_error_enum;
mod no_bare_macro_types;
mod no_bare_pub;
mod no_bare_result;
mod no_bare_vec;
mod no_box;
mod no_empty_crate;
mod no_entry_suffix;
mod no_float;
mod no_manual_id;
mod no_manual_impl;
mod no_pool_access;
mod no_primitive_key;
mod no_raw_error_outside_primitives;
mod no_self_define;
mod no_todo;
mod no_duplicate_fn;
mod no_vec_in_resource;
mod registrable_completeness;
mod repr_c_abi_safety;
mod single_source;
mod undocumented_type;

use std::collections::BTreeSet;
use std::path::Path;

use tree_sitter::Tree;

// ---------------------------------------------------------------------------
// Proc-macro crate list (single source of truth)
// ---------------------------------------------------------------------------

/// Fallback proc-macro crate list. Empty — the caller should always pass
/// the project-specific list via `LintContext::proc_macro_crates`.
pub const PROC_MACRO_CRATES: &[&str] = &[];

// ---------------------------------------------------------------------------
// Lint trait and context
// ---------------------------------------------------------------------------

/// Context provided to each lint for a single crate.
pub struct LintContext<'a> {
    /// Directory name of the crate (e.g. "<prefix>-signal").
    pub crate_name: &'a str,
    /// Short name (e.g. "signal").
    pub short_name: &'a str,
    /// The raw source text of lib.rs.
    pub source: &'a str,
    /// The tree-sitter AST.
    pub tree: &'a Tree,
    /// Names of all crates this crate depends on (directory names).
    pub deps: &'a [String],
    /// Set of all crate directory names in the workspace.
    pub all_crates: &'a BTreeSet<String>,
    /// Content of DESIGN.md.tmpl for this crate, if it exists.
    pub design_doc: Option<&'a str>,
    /// Concatenated content of ALL doc templates for this crate
    /// (README.md.tmpl + DESIGN.md.tmpl + DEEPDIVE_*.md.tmpl).
    pub all_doc_content: &'a str,
    /// Content of SHAME.md.tmpl for this crate, if it exists.
    pub shame_doc: Option<&'a str>,
    /// Root directory of the mock workspace.
    pub workspace_root: &'a Path,
    /// Crates exempt from collection/box/float lints (proc-macro crates).
    /// Falls back to PROC_MACRO_CRATES if empty.
    pub proc_macro_crates: &'a [String],
    /// Crate name prefix (e.g. "loimu"). Used to build expected crate
    /// names dynamically instead of hardcoding project-specific names.
    pub crate_prefix: &'a str,
}

impl<'a> LintContext<'a> {
    /// Whether this crate is a proc-macro crate (exempt from some lints).
    pub fn is_proc_macro_crate(&self) -> bool {
        if self.proc_macro_crates.is_empty() {
            PROC_MACRO_CRATES.iter().any(|c| *c == self.crate_name)
        } else {
            self.proc_macro_crates.iter().any(|c| c == self.crate_name)
        }
    }
}

// ---------------------------------------------------------------------------
// Severity system: per-gate levels
// ---------------------------------------------------------------------------

/// What happens at a single validation gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Not reported.
    Pass,
    /// Informational note, never blocks.
    Info,
    /// Warning, never blocks but printed prominently.
    Warn,
    /// Error, blocks the gate.
    Error,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Pass => "pass",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// Which validation gate is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintMode {
    /// Pre-commit hook. Most permissive.
    Commit,
    /// Standalone xtask run or CI. Middle strictness.
    Build,
    /// Pre-push hook. Most strict.
    Push,
}

/// Per-gate severity configuration for a lint violation.
///
/// Each lint violation declares independently what happens at each gate.
/// Use the named presets for common patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Severity {
    pub on_commit: Level,
    pub on_build: Level,
    pub on_push: Level,
}

impl Severity {
    pub const fn new(on_commit: Level, on_build: Level, on_push: Level) -> Self {
        Self { on_commit, on_build, on_push }
    }

    /// Blocks commit, build, and push. For critical invariants.
    pub const HARD_ERROR: Self = Self::new(Level::Error, Level::Error, Level::Error);

    /// Warns on commit, blocks build and push. For rules that need local
    /// iteration room but must be fixed before building.
    pub const BUILD_GATE: Self = Self::new(Level::Warn, Level::Error, Level::Error);

    /// Warns on commit and build, blocks push only. For work-in-progress
    /// that must be clean before sharing.
    pub const PUSH_GATE: Self = Self::new(Level::Warn, Level::Warn, Level::Error);

    /// Warns everywhere, never blocks.
    pub const ADVISORY: Self = Self::new(Level::Warn, Level::Warn, Level::Warn);

    /// Informational only.
    pub const INFO_ONLY: Self = Self::new(Level::Info, Level::Info, Level::Info);

    /// Get the effective level for a given mode.
    pub fn effective(&self, mode: LintMode) -> Level {
        match mode {
            LintMode::Commit => self.on_commit,
            LintMode::Build => self.on_build,
            LintMode::Push => self.on_push,
        }
    }

    /// Whether this severity blocks at the given mode.
    pub fn is_blocking(&self, mode: LintMode) -> bool {
        self.effective(mode) == Level::Error
    }

    /// Human-readable label based on the gate profile.
    pub fn label(&self) -> &'static str {
        if *self == Self::HARD_ERROR { "error" }
        else if *self == Self::BUILD_GATE { "build-gate" }
        else if *self == Self::PUSH_GATE { "push-gate" }
        else if *self == Self::ADVISORY { "warn" }
        else if *self == Self::INFO_ONLY { "info" }
        else {
            // custom severity; label by strictest gate
            if self.on_push == Level::Error { "push-gate" }
            else if self.on_build == Level::Error { "build-gate" }
            else if self.on_commit == Level::Error { "error" }
            else { "warn" }
        }
    }
}

/// A single lint violation.
pub struct LintError {
    pub crate_name: String,
    pub line: usize,
    pub lint_name: &'static str,
    pub message: String,
    pub severity: Severity,
}

impl LintError {
    /// Create a violation that blocks all gates (commit, build, push).
    pub fn error(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::HARD_ERROR }
    }

    /// Create a violation that warns on commit, blocks build and push.
    pub fn build_error(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::BUILD_GATE }
    }

    /// Create a violation that warns on commit and build, blocks push.
    pub fn push_error(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::PUSH_GATE }
    }

    /// Create a warning-level violation (reported but never blocks).
    pub fn warning(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::ADVISORY }
    }

    /// Create an info-level violation (informational, never blocks).
    pub fn info(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::INFO_ONLY }
    }

    /// Create a violation with custom per-gate severity.
    pub fn with_severity(crate_name: String, line: usize, lint_name: &'static str, message: String, severity: Severity) -> Self {
        Self { crate_name, line, lint_name, message, severity }
    }
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "  [{lint}] {crate_name}:{line}: [{level}] {msg}",
            lint = self.lint_name,
            crate_name = self.crate_name,
            line = self.line,
            level = self.severity.label(),
            msg = self.message,
        )
    }
}

/// Trait for pluggable lints. Each lint inspects a single crate's AST.
pub trait Lint {
    /// Human-readable name for error reporting.
    fn name(&self) -> &'static str;

    /// Check a crate and return any violations found.
    fn check(&self, ctx: &LintContext) -> Vec<LintError>;

    /// Whether this lint only inspects source code (not docs).
    ///
    /// Source-only lints are skipped in `--doc-only` mode (when only
    /// doc templates are staged, no `.rs` files). Default: `true`.
    fn source_only(&self) -> bool { true }
}

/// Trait for cross-crate lints that compare data across all crates at once.
pub trait CrossCrateLint {
    /// Human-readable name for error reporting.
    fn name(&self) -> &'static str;

    /// Check all crates simultaneously and return any violations found.
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError>;

    /// Whether this lint only inspects source code (not docs).
    fn source_only(&self) -> bool { true }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns all registered lint rules.
pub fn all_lints() -> Vec<Box<dyn Lint>> {
    vec![
        Box::new(no_bare_result::NoBareResult),
        Box::new(no_bare_macro_types::NoBareMacroTypes),
        Box::new(no_entry_suffix::NoEntrySuffix),
        Box::new(no_manual_impl::NoManualImpl),
        Box::new(no_adhoc_error_enum::NoAdhocErrorEnum),
        Box::new(no_manual_id::NoManualId),
        Box::new(no_primitive_key::NoPrimitiveKey),
        Box::new(no_raw_error_outside_primitives::NoRawErrorOutsidePrimitives),
        Box::new(no_pool_access::NoPoolAccess),
        // no-vec-in-macros (NoVecInResource) is superseded by no-bare-vec
        Box::new(no_bare_vec::NoBareVec),
        Box::new(no_box::NoBox),
        Box::new(no_empty_crate::NoEmptyCrate),
        Box::new(design_doc_source_mismatch::DesignDocSourceMismatch),
        Box::new(actionable_errors::ActionableErrors),
        Box::new(file_size::FileSize),
        Box::new(no_float::NoFloat),
        Box::new(export_count::ExportCount),
        Box::new(no_todo::NoTodo),
        Box::new(no_adhoc_framework::NoAdhocFramework),
        Box::new(no_bare_string::NoBareString),
        Box::new(no_self_define::NoSelfDefine),
        Box::new(registrable_completeness::RegistrableCompleteness),
        Box::new(repr_c_abi_safety::ReprCAbiSafety),
        // NOTE: no_bare_pub is available but not enabled by default.
        // Enable after annotating all crates with #[public_api] / #[internal_api].
    ]
}

/// Returns future lint rules not yet enforced (need crate-wide annotation first).
pub fn pending_lints() -> Vec<Box<dyn Lint>> {
    vec![
        Box::new(no_bare_pub::NoBarePublic),
    ]
}

/// Create a tree-sitter parser configured for Rust.
pub fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set rust language");
    parser
}

/// Returns all registered cross-crate lint rules.
pub fn all_cross_crate_lints() -> Vec<Box<dyn CrossCrateLint>> {
    vec![
        Box::new(no_duplicate_fn::NoDuplicateFn),
        Box::new(single_source::SingleSource),
        Box::new(undocumented_type::UndocumentedType),
        Box::new(changelist_doc_gate::ChangelistDocGate),
        Box::new(changelist_lock::ChangelistLock),
        Box::new(changelist_required::ChangelistRequired),
        Box::new(changelist_immutability::ChangelistImmutability),
        Box::new(deprecation_comparison::DeprecationComparison),
    ]
}

/// Run all per-crate lints on a single crate, returning violations.
///
/// When `doc_only` is true, skip lints that only inspect source code.
/// This allows doc-only commits during DOC-EXEC phase without being
/// blocked by pre-existing source issues.
pub fn check_crate(ctx: &LintContext, doc_only: bool) -> Vec<LintError> {
    let mut lints = all_lints();
    lints.extend(pending_lints());
    let mut errors = Vec::new();
    for lint in &lints {
        if doc_only && lint.source_only() {
            continue;
        }
        errors.extend(lint.check(ctx));
    }
    errors
}

/// Run all cross-crate lints, returning violations.
///
/// When `doc_only` is true, skip lints that only inspect source code.
pub fn check_cross_crate(crates: &[(&str, &LintContext)], doc_only: bool) -> Vec<LintError> {
    let lints = all_cross_crate_lints();
    let mut errors = Vec::new();
    for lint in &lints {
        if doc_only && lint.source_only() {
            continue;
        }
        errors.extend(lint.check_all(crates));
    }
    errors
}
