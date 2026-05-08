//! AST lint rules for mockspace workspaces.
//!
//! Each lint implements the `Lint` trait (per-crate) or `CrossCrateLint` trait
//! (cross-crate). Consumers build `LintContext` for each crate and call
//! `check_crate()` / `check_cross_crate()` to run the rule sets.
//!
//! # External lint packs
//!
//! A third-party crate can ship its own lint set and be consumed by any
//! mockspace project via the `[lint-crates]` section in `mockspace.toml`.
//! The pack must expose two public functions:
//!
//! ```rust,ignore
//! pub fn lints() -> Vec<Box<dyn mockspace_lint_rules::Lint>>;
//! pub fn cross_lints() -> Vec<Box<dyn mockspace_lint_rules::CrossCrateLint>>;
//! ```
//!
//! Either may return empty. The [`lint_pack!`] macro generates both from a
//! list of lint structs. Example:
//!
//! ```rust,ignore
//! mockspace_lint_rules::lint_pack! {
//!     lints: [MyRuleA, MyRuleB],
//!     cross_lints: [MyCrossRule],
//! }
//! ```
//!
//! The generated proxy crate (`target/mockspace-proxy/`) concatenates every
//! pack's lints with any in-tree `mock/lints/*.rs` files and runs the union.

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
mod forbidden_imports;
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

use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// A single source file surfaced to lints: its repo-relative path
/// and its full text.
#[derive(Debug, Clone)]
pub struct CrateSourceFile {
    /// Crate-relative path (e.g. `src/lib.rs`, `src/bits.rs`).
    pub rel_path: std::path::PathBuf,
    /// Full file contents.
    pub text: String,
}

/// Context provided to each lint for a single crate.
pub struct LintContext<'a> {
    /// Directory name of the crate (e.g. "<prefix>-signal").
    pub crate_name: &'a str,
    /// Short name (e.g. "signal").
    pub short_name: &'a str,
    /// The raw source text of `src/lib.rs` (back-compat; lints that
    /// want to scan every module file should use `all_sources`).
    pub source: &'a str,
    /// The tree-sitter AST of `src/lib.rs`.
    pub tree: &'a Tree,
    /// Every `.rs` file under the crate's `src/**`, in path order.
    /// The first entry is always `src/lib.rs`. Lints that used to
    /// inspect only `source` should iterate over this to catch drift
    /// in module files (`bits.rs`, `prim.rs`, etc.).
    pub all_sources: &'a [CrateSourceFile],
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
    /// Whether source-scanning lints should run against proc-macro crate
    /// source. Default false: skip source lints for proc-macro crates
    /// because their heap-using parsers do not ship with consumer binaries.
    /// Set true (via mockspace.toml `lint_proc_macro_source = true`) to
    /// force source lints to apply to proc-macro crates as well.
    ///
    /// Independent of expansion-based linting (future feature): what a
    /// macro emits must always satisfy consumer-crate rules because the
    /// emitted code compiles into consumer binaries.
    pub lint_proc_macro_source: bool,
    /// Crate name prefix (e.g. "loimu"). Used to build expected crate
    /// names dynamically instead of hardcoding project-specific names.
    pub crate_prefix: &'a str,
    /// Per-crate primitive-introductions map from mockspace.toml's
    /// `[primitive-introductions]` section. Key: crate directory
    /// name; value: list of primitive tokens the crate legitimately
    /// introduces. Lints that enforce "no bare primitives" should
    /// call [`LintContext::introduces`] to check whether the current
    /// crate legitimately uses a given primitive token.
    ///
    /// This explicit map is the belt-and-suspenders path. Long-term
    /// the introductions set should be *detected* from each crate's
    /// DESIGN.md.tmpl / source parse, not declared in a parallel
    /// TOML table. See the `Config.primitive_introductions` docs on
    /// the mockspace crate for the future direction — once that
    /// lands, the TOML map becomes additive rather than the sole
    /// source of truth.
    pub primitive_introductions: &'a BTreeMap<String, Vec<String>>,
}

impl<'a> LintContext<'a> {
    /// Whether source-scanning lints should skip this crate because it is
    /// a proc-macro crate AND the project has not opted into linting
    /// proc-macro source. This is the helper source-scanning lints should
    /// call to decide whether to short-circuit. Use this instead of
    /// [`Self::is_proc_macro_crate`] for the skip decision; that method
    /// answers the narrower question "is this a proc-macro crate?"
    /// without consulting the project's lint-behavior preference.
    #[must_use]
    pub fn should_skip_proc_macro_source_lint(&self) -> bool {
        !self.lint_proc_macro_source && self.is_proc_macro_crate()
    }

    /// Whether this crate is a proc-macro crate. Does NOT consider the
    /// `lint_proc_macro_source` preference; callers that want the
    /// "should I skip this source lint" decision should use
    /// [`Self::should_skip_proc_macro_source_lint`].
    #[must_use]
    pub fn is_proc_macro_crate(&self) -> bool {
        if self.proc_macro_crates.is_empty() {
            PROC_MACRO_CRATES.iter().any(|c| *c == self.crate_name)
        } else {
            self.proc_macro_crates.iter().any(|c| c == self.crate_name)
        }
    }

    /// Whether the current crate legitimately introduces the given
    /// primitive token per the `[primitive-introductions]` config.
    /// Lints that enforce "no bare primitives" should skip a specific
    /// token on a specific crate when this returns `true`.
    ///
    /// Example: `ctx.introduces("u8")` returns `true` when the crate
    /// is `arvo` and `arvo = ["u8", ...]` is configured in mockspace.toml.
    #[must_use]
    pub fn introduces(&self, primitive: &str) -> bool {
        self.primitive_introductions
            .get(self.crate_name)
            .map(|list| list.iter().any(|p| p == primitive))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Severity system: per-gate levels
// ---------------------------------------------------------------------------

/// What happens at a single validation gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    /// Parse a level from a string name.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "pass" | "off" => Some(Level::Pass),
            "info" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

/// Which validation gate is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    #[must_use]
    pub const fn new(on_commit: Level, on_build: Level, on_push: Level) -> Self {
        Self { on_commit, on_build, on_push }
    }

    /// Completely disabled — not reported at any gate.
    pub const OFF: Self = Self::new(Level::Pass, Level::Pass, Level::Pass);

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

    /// Whether all gates are `Level::Pass` (i.e. the lint is effectively off).
    #[must_use]
    pub fn is_off(&self) -> bool {
        self.on_commit == Level::Pass && self.on_build == Level::Pass && self.on_push == Level::Pass
    }

    /// Get the effective level for a given mode.
    #[must_use]
    pub fn effective(&self, mode: LintMode) -> Level {
        match mode {
            LintMode::Commit => self.on_commit,
            LintMode::Build => self.on_build,
            LintMode::Push => self.on_push,
        }
    }

    /// Whether this severity blocks at the given mode.
    #[must_use]
    pub fn is_blocking(&self, mode: LintMode) -> bool {
        self.effective(mode) == Level::Error
    }

    /// Human-readable label based on the gate profile.
    #[must_use]
    pub fn label(&self) -> &'static str {
        if *self == Self::OFF { "off" }
        else if *self == Self::HARD_ERROR { "error" }
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

/// Parse a severity preset from a string name.
///
/// Supports: "off", "error"/"hard-error", "build-gate", "push-gate",
/// "advisory"/"warn", "info".
#[must_use]
pub fn parse_severity(s: &str) -> Option<Severity> {
    match s.trim().to_lowercase().as_str() {
        "off" => Some(Severity::OFF),
        "error" | "hard-error" | "hard_error" => Some(Severity::HARD_ERROR),
        "build-gate" | "build_gate" => Some(Severity::BUILD_GATE),
        "push-gate" | "push_gate" => Some(Severity::PUSH_GATE),
        "advisory" | "warn" | "warning" => Some(Severity::ADVISORY),
        "info" | "info-only" | "info_only" => Some(Severity::INFO_ONLY),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Lint configuration
// ---------------------------------------------------------------------------

/// Configuration for lint severity overrides and parameters.
///
/// Parsed from the `[lints]` section of `mockspace.toml`.
#[derive(Debug, Clone)]
pub struct LintConfig {
    /// Base severity overrides per lint name.
    pub base: HashMap<String, Severity>,
    /// Per-finding-kind severity overrides: lint_name -> { finding_kind -> severity }.
    pub findings: HashMap<String, HashMap<String, Severity>>,
    /// Per-lint parameters: lint_name -> { key -> value }.
    pub params: HashMap<String, HashMap<String, String>>,
}

impl LintConfig {
    /// Create an empty config (all defaults).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            base: HashMap::new(),
            findings: HashMap::new(),
            params: HashMap::new(),
        }
    }

    /// Whether this config has any overrides at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.findings.is_empty() && self.params.is_empty()
    }

    /// Build a LintConfig from a simple base-only HashMap (backwards compat).
    pub fn from_base(base: HashMap<String, Severity>) -> Self {
        Self {
            base,
            findings: HashMap::new(),
            params: HashMap::new(),
        }
    }
}

/// A single lint violation.
#[derive(Debug, Clone)]
pub struct LintError {
    pub crate_name: String,
    pub line: usize,
    pub lint_name: &'static str,
    pub message: String,
    pub severity: Severity,
    /// Optional sub-category for per-finding severity overrides.
    pub finding_kind: Option<&'static str>,
}

impl LintError {
    /// Create a violation that blocks all gates (commit, build, push).
    #[must_use]
    pub fn error(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::HARD_ERROR, finding_kind: None }
    }

    /// Create a violation that warns on commit, blocks build and push.
    #[must_use]
    pub fn build_error(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::BUILD_GATE, finding_kind: None }
    }

    /// Create a violation that warns on commit and build, blocks push.
    #[must_use]
    pub fn push_error(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::PUSH_GATE, finding_kind: None }
    }

    /// Create a warning-level violation (reported but never blocks).
    #[must_use]
    pub fn warning(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::ADVISORY, finding_kind: None }
    }

    /// Create an info-level violation (informational, never blocks).
    #[must_use]
    pub fn info(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self { crate_name, line, lint_name, message, severity: Severity::INFO_ONLY, finding_kind: None }
    }

    /// Create a violation with custom per-gate severity.
    #[must_use]
    pub fn with_severity(crate_name: String, line: usize, lint_name: &'static str, message: String, severity: Severity) -> Self {
        Self { crate_name, line, lint_name, message, severity, finding_kind: None }
    }

    /// Create a violation with a specific finding kind for per-finding severity overrides.
    #[must_use]
    pub fn with_finding_kind(crate_name: String, line: usize, lint_name: &'static str, message: String, severity: Severity, finding_kind: &'static str) -> Self {
        Self { crate_name, line, lint_name, message, severity, finding_kind: Some(finding_kind) }
    }
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(kind) = self.finding_kind {
            write!(
                f,
                "  [{lint}/{kind}] {crate_name}:{line}: [{level}] {msg}",
                lint = self.lint_name,
                kind = kind,
                crate_name = self.crate_name,
                line = self.line,
                level = self.severity.label(),
                msg = self.message,
            )
        } else {
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

    /// The default severity for this lint's violations.
    ///
    /// Used when no config override is present. Lints should override
    /// this to match what they currently hardcode.
    fn default_severity(&self) -> Severity { Severity::HARD_ERROR }

    /// Sub-categories of findings this lint can produce.
    ///
    /// Used for per-finding-kind severity overrides in config.
    fn finding_kinds(&self) -> &[&str] { &[] }

    /// Configuration keys this lint accepts.
    fn config_keys(&self) -> &[&str] { &[] }

    /// Apply configuration parameters to this lint.
    fn configure(&mut self, _params: &HashMap<String, String>) {}
}

/// Trait for cross-crate lints that compare data across all crates at once.
pub trait CrossCrateLint {
    /// Human-readable name for error reporting.
    fn name(&self) -> &'static str;

    /// Check all crates simultaneously and return any violations found.
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError>;

    /// Whether this lint only inspects source code (not docs).
    fn source_only(&self) -> bool { true }

    /// The default severity for this lint's violations.
    ///
    /// Used when no config override is present. Lints should override
    /// this to match what they currently hardcode.
    fn default_severity(&self) -> Severity { Severity::HARD_ERROR }
}

// ---------------------------------------------------------------------------
// External lint-pack convention
// ---------------------------------------------------------------------------

/// Declare the two `lints()` and `cross_lints()` entry points that every
/// external lint pack must expose.
///
/// Each entry is an expression producing a value that implements `Lint`
/// (in the `lints:` list) or `CrossCrateLint` (in the `cross_lints:` list).
/// Unit-struct lints are spelled `MyLint`; lints with constructors are
/// spelled `MyLint::new(args)`.
///
/// Either list may be omitted; an omitted list produces an empty `Vec`.
///
/// # Example
///
/// ```rust,ignore
/// pub struct NoBareFoo;
/// impl mockspace_lint_rules::Lint for NoBareFoo { /* ... */ }
///
/// mockspace_lint_rules::lint_pack! {
///     lints: [NoBareFoo, FileSize::new()],
/// }
/// ```
#[macro_export]
macro_rules! lint_pack {
    (
        $(lints: [ $( $lint:expr ),* $(,)? ] $(,)?)?
        $(cross_lints: [ $( $cross:expr ),* $(,)? ] $(,)?)?
    ) => {
        pub fn lints() -> ::std::vec::Vec<::std::boxed::Box<dyn $crate::Lint>> {
            #[allow(unused_mut)]
            let mut v: ::std::vec::Vec<::std::boxed::Box<dyn $crate::Lint>> = ::std::vec::Vec::new();
            $( $( v.push(::std::boxed::Box::new($lint)); )* )?
            v
        }

        pub fn cross_lints() -> ::std::vec::Vec<::std::boxed::Box<dyn $crate::CrossCrateLint>> {
            #[allow(unused_mut)]
            let mut v: ::std::vec::Vec<::std::boxed::Box<dyn $crate::CrossCrateLint>> = ::std::vec::Vec::new();
            $( $( v.push(::std::boxed::Box::new($cross)); )* )?
            v
        }
    };
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
        Box::new(file_size::FileSize::new()),
        Box::new(no_float::NoFloat),
        Box::new(export_count::ExportCount),
        Box::new(no_todo::NoTodo),
        Box::new(no_adhoc_framework::NoAdhocFramework),
        Box::new(no_bare_string::NoBareString),
        Box::new(no_self_define::NoSelfDefine),
        Box::new(registrable_completeness::RegistrableCompleteness),
        Box::new(repr_c_abi_safety::ReprCAbiSafety),
        Box::new(no_bare_pub::NoBarePublic),
        Box::new(forbidden_imports::ForbiddenImports::new()),
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
///
/// When `overrides` is provided, lint severities can be overridden:
/// - If a lint name maps to a severity where all gates are `Pass`, the lint is skipped entirely.
/// - If a lint name maps to another severity, all errors from that lint use the configured severity.
/// - If a lint is not in the map, it uses its `default_severity()`.
pub fn check_crate(ctx: &LintContext, doc_only: bool, overrides: Option<&LintConfig>) -> Vec<LintError> {
    check_crate_with_extra(ctx, doc_only, overrides, &[])
}

/// Run all per-crate lints plus any custom lints on a single crate, returning violations.
pub fn check_crate_with_extra(
    ctx: &LintContext,
    doc_only: bool,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn Lint>],
) -> Vec<LintError> {
    let mut lints = all_lints();

    // Configure lints with parameters from config
    if let Some(cfg) = overrides {
        for lint in &mut lints {
            if let Some(params) = cfg.params.get(lint.name()) {
                lint.configure(params);
            }
        }
    }

    let mut errors = Vec::new();

    // Helper closure to process a single lint
    let process_lint = |lint: &dyn Lint, errors: &mut Vec<LintError>| {
        if doc_only && lint.source_only() {
            return;
        }

        // Check if there's a base severity override
        let base_override = if let Some(cfg) = overrides {
            if let Some(sev) = cfg.base.get(lint.name()) {
                if sev.is_off() {
                    return; // skip entirely
                }
                Some(*sev)
            } else {
                None
            }
        } else {
            None
        };

        let mut lint_errors = lint.check(ctx);

        // Apply per-finding or base severity overrides
        if let Some(cfg) = overrides {
            for err in &mut lint_errors {
                // Check finding-specific override first
                let effective = if let (Some(kind), Some(finding_map)) = (err.finding_kind, cfg.findings.get(lint.name())) {
                    if let Some(sev) = finding_map.get(kind) {
                        Some(*sev)
                    } else {
                        base_override
                    }
                } else {
                    base_override
                };

                if let Some(sev) = effective {
                    err.severity = sev;
                }
                // If no override, preserve the error's own severity
            }
        }

        errors.extend(lint_errors);
    };

    for lint in &lints {
        process_lint(lint.as_ref(), &mut errors);
    }
    for lint in extra_lints {
        process_lint(lint.as_ref(), &mut errors);
    }

    errors
}

/// Run all cross-crate lints, returning violations.
///
/// When `doc_only` is true, skip lints that only inspect source code.
///
/// When `overrides` is provided, lint severities can be overridden
/// (same semantics as `check_crate`).
pub fn check_cross_crate(crates: &[(&str, &LintContext)], doc_only: bool, overrides: Option<&LintConfig>) -> Vec<LintError> {
    check_cross_crate_with_extra(crates, doc_only, overrides, &[])
}

/// Run all cross-crate lints plus any custom lints, returning violations.
pub fn check_cross_crate_with_extra(
    crates: &[(&str, &LintContext)],
    doc_only: bool,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn CrossCrateLint>],
) -> Vec<LintError> {
    let lints = all_cross_crate_lints();
    let mut errors = Vec::new();

    let process_lint = |lint: &dyn CrossCrateLint, errors: &mut Vec<LintError>| {
        if doc_only && lint.source_only() {
            return;
        }

        // Check if there's a base severity override
        let base_override = if let Some(cfg) = overrides {
            if let Some(sev) = cfg.base.get(lint.name()) {
                if sev.is_off() {
                    return; // skip entirely
                }
                Some(*sev)
            } else {
                None
            }
        } else {
            None
        };

        let mut lint_errors = lint.check_all(crates);

        // Apply per-finding or base severity overrides
        if let Some(cfg) = overrides {
            for err in &mut lint_errors {
                let effective = if let (Some(kind), Some(finding_map)) = (err.finding_kind, cfg.findings.get(lint.name())) {
                    if let Some(sev) = finding_map.get(kind) {
                        Some(*sev)
                    } else {
                        base_override
                    }
                } else {
                    base_override
                };

                if let Some(sev) = effective {
                    err.severity = sev;
                }
            }
        }

        errors.extend(lint_errors);
    };

    for lint in &lints {
        process_lint(lint.as_ref(), &mut errors);
    }
    for lint in extra_lints {
        process_lint(lint.as_ref(), &mut errors);
    }

    errors
}

#[cfg(test)]
mod pack_tests {
    use super::*;

    struct SmokeLint;
    impl Lint for SmokeLint {
        fn name(&self) -> &'static str { "smoke-lint" }
        fn check(&self, _ctx: &LintContext) -> Vec<LintError> { Vec::new() }
    }

    struct SmokeCross;
    impl CrossCrateLint for SmokeCross {
        fn name(&self) -> &'static str { "smoke-cross" }
        fn check_all(&self, _crates: &[(&str, &LintContext)]) -> Vec<LintError> { Vec::new() }
    }

    mod full_pack {
        use super::{SmokeLint, SmokeCross};
        crate::lint_pack! {
            lints: [SmokeLint],
            cross_lints: [SmokeCross],
        }
    }

    mod lints_only {
        use super::SmokeLint;
        crate::lint_pack! {
            lints: [SmokeLint],
        }
    }

    mod empty_pack {
        crate::lint_pack! {}
    }

    #[test]
    fn full_pack_produces_both_vecs() {
        assert_eq!(full_pack::lints().len(), 1);
        assert_eq!(full_pack::cross_lints().len(), 1);
        assert_eq!(full_pack::lints()[0].name(), "smoke-lint");
        assert_eq!(full_pack::cross_lints()[0].name(), "smoke-cross");
    }

    #[test]
    fn lints_only_pack_empty_cross() {
        assert_eq!(lints_only::lints().len(), 1);
        assert_eq!(lints_only::cross_lints().len(), 0);
    }

    #[test]
    fn empty_pack_empty_both() {
        assert_eq!(empty_pack::lints().len(), 0);
        assert_eq!(empty_pack::cross_lints().len(), 0);
    }
}
