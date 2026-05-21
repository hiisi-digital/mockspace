//! Lint engine vocabulary + minimal swap point (spec §5).
//!
//! This module is the entire substrate-side surface for lints. Engines
//! ship everything else: dispatch, lint authorship traits, preprocessing,
//! tree walks, attribute parsing. The substrate provides only the types
//! engines consume and return, plus the trait that swaps engines.
//!
//! # What the substrate ships
//!
//! - **Input vocabulary** engines consume: [`Document`], [`Project`],
//!   [`Language`], [`RunSurface`], [`ContentHash`].
//! - **Output vocabulary** engines return: [`Finding`], [`Span`],
//!   [`Severity`], [`Impact`], [`Category`], [`Gate`], [`GateSeverity`].
//! - **Suppression model**: [`SuppressionMap`], [`SuppressionScope`].
//!   Shared across engines so `#[mock::lints::allow(...)]` and
//!   `// lint:allow(...)` resolve identically regardless of which engine
//!   produced the finding.
//! - **Engine trait**: [`LintEngine`] with one runtime method returning
//!   `Vec<Finding>` with suppressions already applied.
//! - **Config carrier trait**: [`LintCfgStore`].
//! - **Errors**: [`LintError`].
//! - **Pattern matcher**: [`matches_pattern`] used by the no-suffix /
//!   no-prefix configurable lint families in
//!   `mockspace-hilavitkutin-stack-lints`.
//!
//! # What the substrate does NOT ship
//!
//! Lint authoring traits (`Lint`, `PerDocumentLint`), dispatch protocols,
//! descriptor formats, preprocessor traits, node-interest tables, single-
//! walk machinery, concrete engines: all engine-internal. The Rust engine
//! lives in `mockspace-rs`; viola's engine will live in
//! `viola-mockspace-engine` when viola integrates. Both implement
//! [`LintEngine`] and produce `Vec<Finding>`.
//!
//! # Severity / impact / category vocabulary
//!
//! Mirrors viola's TOML v2 schema directly so `lints.toml` is wire-
//! compatible with viola's eventual `viola.toml`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Wire-format / trait-surface contract version.
///
/// Bumped on any non-additive change to [`LintEngine`], [`Finding`],
/// [`Severity`], [`Span`], [`Document`], [`Project`], or
/// [`SuppressionMap`] shapes. Custom and external lint cdylibs cache by
/// `(source_content_sha, rustc_version, LINT_CONTRACT_VERSION)`; bumping
/// invalidates every cached cdylib.
pub const LINT_CONTRACT_VERSION: u32 = 3;

// =========================================================================
// Language and content hashing.
// =========================================================================

/// Source language tag. Mirrors viola's grammar-plugin coverage. `Other`
/// is the fallback for files whose extension does not map to a known
/// language; per-document lints that key on language skip these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Tsx,
    Jsx,
    Python,
    Go,
    Zig,
    C,
    Cpp,
    Json,
    Toml,
    Yaml,
    Markdown,
    Shell,
    Other,
}

/// 32-byte content digest. Engines document which algorithm fills it via
/// [`LintEngine::HASH_ALGORITHM`]. Lints treat hashes as opaque identifiers,
/// not stable cryptographic claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub const ZERO: Self = Self([0u8; 32]);
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Hash algorithm filling [`ContentHash`]. `#[non_exhaustive]` so future
/// engines can extend without breaking pattern matches.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    Blake3,
    Fnv1a,
}

// =========================================================================
// Run surface.
// =========================================================================

/// The surface a lint run originates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSurface {
    Local,
    Ci,
    Editor,
}

// =========================================================================
// Severity / Impact / Category.
// =========================================================================

/// Severity vocabulary matching viola's TOML v2 schema (six variants).
///
/// Ordered from most-suppressive to most-loud: `Skip` < `Off` < `Hint` <
/// `Info` < `Warn` < `Error`. Only `Error` blocks gates.
///
/// - `Skip`: suppress the entire engine run on this file.
/// - `Off`: silence the lint; engine skips invocation.
/// - `Hint`: dim suggestion (host-side display concession).
/// - `Info` / `Warn`: report; do not block.
/// - `Error`: block at the gate threshold.
///
/// Wire-level mapping to viola's current `DiagnosticSeverity` (Info / Warn /
/// Error): the three shared variants map directly; Skip / Off / Hint are
/// host-side until viola grows ABI v1.1 to add them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Skip,
    Off,
    Hint,
    Info,
    Warn,
    Error,
}

impl Severity {
    pub const fn blocks(self) -> bool {
        matches!(self, Self::Error)
    }
    pub const fn visible(self) -> bool {
        matches!(self, Self::Error | Self::Warn | Self::Info | Self::Hint)
    }
    pub const fn silent(self) -> bool {
        matches!(self, Self::Off | Self::Skip)
    }
}

/// Impact axis from viola's TOML v2. Optional on findings; engines that
/// do not classify by impact leave this `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Impact {
    Critical,
    Major,
    Minor,
    Trivial,
}

/// Category axis from viola's TOML v2. Optional on findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Correctness,
    Maintainability,
    Consistency,
    Performance,
    Style,
}

// =========================================================================
// Gate and GateSeverity.
// =========================================================================

/// The three gates at which a lint may fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gate {
    Commit,
    Build,
    Push,
}

/// Per-gate severity configuration for a single lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSeverity {
    pub commit: Severity,
    pub build: Severity,
    pub push: Severity,
}

impl GateSeverity {
    pub const fn uniform(severity: Severity) -> Self {
        Self {
            commit: severity,
            build: severity,
            push: severity,
        }
    }
    pub const fn at(self, gate: Gate) -> Severity {
        match gate {
            Gate::Commit => self.commit,
            Gate::Build => self.build,
            Gate::Push => self.push,
        }
    }
    pub const fn any_blocks(self) -> bool {
        self.commit.blocks() || self.build.blocks() || self.push.blocks()
    }
}

impl Default for GateSeverity {
    fn default() -> Self {
        Self::uniform(Severity::Off)
    }
}

// =========================================================================
// Spans and findings.
// =========================================================================

/// A source-position range. Half-open `[start, end)` over 1-indexed
/// `(line, column)` pairs. Matches viola's `SourceRange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: PathBuf,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl Span {
    pub fn single_line(file: impl Into<PathBuf>, line: u32, column: u32, length: u32) -> Self {
        Self {
            file: file.into(),
            start_line: line,
            start_column: column,
            end_line: line,
            end_column: column + length,
        }
    }
    pub fn range(
        file: impl Into<PathBuf>,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Self {
        Self {
            file: file.into(),
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
    pub fn is_single_line(&self) -> bool {
        self.start_line == self.end_line
    }
    /// Whether `inner` is fully contained within `self`. Same-file plus
    /// half-open range containment. Used by [`SuppressionMap`].
    pub fn contains(&self, inner: &Self) -> bool {
        if self.file != inner.file {
            return false;
        }
        let starts_after =
            (inner.start_line, inner.start_column) >= (self.start_line, self.start_column);
        let ends_before =
            (inner.end_line, inner.end_column) <= (self.end_line, self.end_column);
        starts_after && ends_before
    }
}

/// A labelled related span attached to a [`Finding`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedSpan {
    pub span: Span,
    pub label: Cow<'static, str>,
}

/// Opaque structured metadata attached to a [`Finding`]. `schema` names
/// the shape (e.g. `"viola/diag-meta/v1"`); `bytes` is an opaque payload
/// the consumer parses against the schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataBlob {
    pub schema: Cow<'static, str>,
    pub bytes: Vec<u8>,
}

/// A structured suggestion attached to a finding. Carries human-readable
/// `description` plus an optional [`Fix`] recipe. When `fix` is present
/// the suggestion is mechanically applicable; when absent it is advice
/// the human reviews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub description: Cow<'static, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

/// A mechanically applicable edit recipe.
///
/// # Reference frame for byte offsets
///
/// All byte offsets (`start`, `end`, `position`) are **UTF-8 byte indices
/// into the original source bytes** of the finding's containing document,
/// as read from disk. Specifically:
///
/// - Offsets are byte indices, not char indices and not grapheme indices.
/// - Offsets are into the source pre-strip, even when the lint that emitted
///   the finding ran against a stripped view (with comments / strings
///   removed). A lint scanning a stripped view that wants to emit a `Fix`
///   must translate stripped-view offsets back to original-source offsets
///   before constructing the recipe. Until that translation is wired, such
///   lints should emit `Suggestion { description, fix: None }` — advice
///   only.
/// - The `Span` on the parent `Finding` is for human-readable display
///   (file, line, column, length). `Fix` byte ranges are the authoritative
///   coordinate system for mechanical application; they are not derived
///   from `Span` and must be set independently.
///
/// # Composition
///
/// `Multi` permits arbitrary nesting of any variant, including other
/// `Multi` and `File` nodes. The runner walks the tree, collects all leaf
/// edits, verifies no byte-range overlaps among `Replace`/`Insert`/`Delete`
/// touching the same file, and applies them atomically. A `File` node
/// inside a `Multi` is allowed (e.g. "replace bytes here AND create this
/// sidecar file"). Conflicts between `File::Delete` of a path and any
/// in-buffer edit to the same path are detected at apply time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fix {
    /// Replace bytes `[start, end)` with `replacement`. Byte offsets are
    /// UTF-8 indices into the original (pre-strip) source.
    Replace {
        start: usize,
        end: usize,
        replacement: Cow<'static, str>,
    },

    /// Insert `text` at byte `position`. Adjacent inserts at the same
    /// position from different findings are a conflict caught at apply.
    /// Byte `position` is a UTF-8 index into the original (pre-strip)
    /// source.
    Insert {
        position: usize,
        text: Cow<'static, str>,
    },

    /// Delete bytes `[start, end)`. Byte offsets are UTF-8 indices into
    /// the original (pre-strip) source.
    Delete { start: usize, end: usize },

    /// Multiple sub-fixes applied atomically. Inner fixes may be any
    /// variant including nested `Multi`. The runner walks the tree,
    /// collects all leaf edits, and verifies no byte-range overlaps
    /// among byte-edit variants on the same file.
    Multi { fixes: Vec<Fix> },

    /// File-level operation (create, delete, rename). Distinct from
    /// the byte-range variants so the runner can dispatch to a
    /// filesystem path rather than an in-memory buffer. May appear at
    /// any nesting depth, including as a sibling of byte-edits inside
    /// a `Multi`.
    File { op: FileOp },
}

/// File-level operations a [`Fix::File`] can represent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FileOp {
    /// Create `path` with `content`. Errors if path already exists.
    Create {
        path: Cow<'static, str>,
        content: Cow<'static, str>,
    },

    /// Delete `path`. Errors if path does not exist.
    Delete { path: Cow<'static, str> },

    /// Rename `from` to `to`. Errors if `to` exists or `from` does
    /// not.
    Rename {
        from: Cow<'static, str>,
        to: Cow<'static, str>,
    },
}

/// A single lint finding. Strict superset of viola's `Diagnostic`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub lint_name: Cow<'static, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<Cow<'static, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<Cow<'static, str>>,
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<Impact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    pub message: Cow<'static, str>,
    pub span: Span,
    /// Short hint pointing at what the author should consider. One
    /// line; not a full explanation. Example: "consider Maybe<T>,
    /// Just<T>, or Outcome<T, E>".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Cow<'static, str>>,
    /// Broader help text explaining why the lint exists. May span
    /// multiple lines. Example: "arvo is the workspace's exclusive
    /// numeric substrate; bare primitives are forbidden in pub API
    /// per .claude/rules/no-bare-primitives.md".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<Cow<'static, str>>,
    /// Optional structured suggestion. Carries a description and a
    /// possibly-mechanical [`Fix`] recipe. Replaces the older
    /// `fix_suggestion` shape; the new form expresses the same simple
    /// cases via `Suggestion { description, fix: Some(Fix::Replace {
    /// ... }) }` plus the richer multi-edit and file-level shapes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Suggestion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_spans: Vec<RelatedSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MetadataBlob>,
}

// =========================================================================
// LintError.
// =========================================================================

/// Error returned by a lint when it cannot run to a conclusion.
#[derive(Debug)]
pub enum LintError {
    AnalysisFailure {
        lint: Cow<'static, str>,
        reason: Cow<'static, str>,
    },
    BadConfig {
        lint: Cow<'static, str>,
        reason: Cow<'static, str>,
    },
    Io {
        lint: Cow<'static, str>,
        source: std::io::Error,
    },
    Internal {
        lint: Cow<'static, str>,
        message: Cow<'static, str>,
    },
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AnalysisFailure { lint, reason } => {
                write!(f, "analysis failure in lint `{lint}`: {reason}")
            }
            Self::BadConfig { lint, reason } => {
                write!(f, "missing or invalid config for lint `{lint}`: {reason}")
            }
            Self::Io { lint, source } => {
                write!(f, "io error in lint `{lint}`: {source}")
            }
            Self::Internal { lint, message } => {
                write!(f, "internal error in lint `{lint}`: {message}")
            }
        }
    }
}

impl std::error::Error for LintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

// =========================================================================
// Document and Project.
// =========================================================================

/// A single source document in a project.
///
/// Engines impl this on whatever concrete document they produce. The
/// `Debug` supertrait keeps `Vec<Box<dyn Document>>` debug-printable
/// through the trait object.
pub trait Document: fmt::Debug {
    fn path(&self) -> &Path;
    fn language(&self) -> Language;
    fn source(&self) -> &str;
    fn content_hash(&self) -> &ContentHash;
}

/// The engine-produced project model.
///
/// Each engine impls [`Project`] on its concrete project type. Lints see
/// a uniform shape; richer accessors (item indices, type trees, cross-
/// document references) land additively as new trait methods if and when
/// the substrate needs them universally.
///
/// The `documents()` method has a default returning an empty slice.
/// Engines that materialise a `Box<dyn Document>` slice override it; the
/// rest expose their concrete document set through their own typed
/// accessor (e.g. `MockspaceProject::documents() -> impl Iterator`) and
/// leave the trait default in place. The substrate does not call this
/// method internally; it exists as an opt-in fallback for cross-engine
/// consumers that genuinely need a trait-object iter.
pub trait Project {
    fn root(&self) -> &Path;
    fn surface(&self) -> RunSurface;
    fn documents(&self) -> &[Box<dyn Document>] {
        &[]
    }
}

// =========================================================================
// LintCfgStore and LintContext.
// =========================================================================

/// A typed-config carrier passed to lints via [`LintContext`].
pub trait LintCfgStore: Send + Sync {
    /// Return the TOML sub-table configured for `lint_name`.
    fn get(&self, lint_name: &str) -> Option<&toml::Table>;

    /// Resolve per-gate severity. Default deserialises the lint's
    /// sub-table into [`GateSeverity`] if present; returns `None` to
    /// signal "fall back to the lint's default".
    fn resolve_severity(&self, lint_name: &str) -> Option<GateSeverity> {
        self.get(lint_name)
            .and_then(|t| t.clone().try_into::<GateSeverity>().ok())
    }
}

/// Per-lint dispatch context. Engines build this internally before
/// invoking a lint; consumers do not construct it directly.
pub struct LintContext<'ctx> {
    pub gate: Gate,
    pub severities: GateSeverity,
    pub surface: RunSurface,
    pub project_root: &'ctx Path,
    pub config: &'ctx dyn LintCfgStore,
}

impl<'ctx> LintContext<'ctx> {
    pub fn active_severity(&self) -> Severity {
        self.severities.at(self.gate)
    }
}

// =========================================================================
// Canonical directive vocabulary.
// =========================================================================

/// One of the five canonical source-level directives per the design
/// memo at `mock/research/202605220000_canonical-directive-vocabulary.md`.
///
/// Preprocessors parse these from comments (canonical surface) or
/// language-native decorator aliases. The internal `DirectiveRecord`
/// shape is identical regardless of which surface produced it; the
/// engine downstream operates on this unified form.
///
/// Lint packs ship new lint names and new categories, but cannot ship
/// new directive variants. Adding a sixth directive is a framework
/// schema change requiring a version bump (see
/// [`LINT_CONTRACT_VERSION`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Directive {
    /// Per-site suppression of a specific lint. Equivalent to the
    /// existing comment-based `lint:allow` mechanism with no semantic
    /// change; only the parsing surface unifies under
    /// `DirectiveRecord`. `reason` and `tracked` are validated
    /// downstream by `SuppressionMetaLint` per the
    /// `lint-allow-requires-task-id` workspace rule; both `Option`
    /// fields are present at the parse surface because directives may
    /// be ill-formed at source-comment time and the meta-lint reports
    /// the missing fields as findings rather than panicking the
    /// parser.
    Allow {
        lint_name: String,
        reason: Option<String>,
        tracked: Option<String>,
    },

    /// Marks the current item as the canonical introducer of the named
    /// primitive category. Carves out the marked item and its direct
    /// impl blocks (same file, same module, same type) from category-
    /// checked lints. Does not extend to transitive helpers or whole
    /// modules. Replaces the pre-v2 `[primitive-introductions]` TOML
    /// table.
    Introduces { category: String },

    /// At a module or file boundary, extends a lint's scope along one
    /// axis for the contained items. Axis set is bounded to
    /// `ScopeConfig` fields ([`ScopeAxis`]); lint packs cannot invent
    /// new axes through this directive.
    ScopeAdd {
        lint_name: String,
        axis: ScopeAxis,
        value: String,
    },

    /// Acknowledges a known violation that will be fixed when the linked
    /// task closes. Semantically distinct from `Allow`: defers expire
    /// when the linked task closes, while allows accumulate as a policy
    /// question. The `SuppressionMetaLint`'s `forbid_expired` config
    /// distinguishes the two.
    Defer {
        lint_name: String,
        until: String,
        reason: Option<String>,
    },

    /// File-level disable for the named lint. Placed at the top of a
    /// file. Requires the same `reason` + `tracked` as `Allow` (also
    /// `Option<String>` at the parse surface; meta-lint validates
    /// downstream). Distinct from `ScopeAdd` in that it is a disable,
    /// not a scope extension.
    FileDisable {
        lint_name: String,
        reason: Option<String>,
        tracked: Option<String>,
    },

    /// Lint-provided per-site property consumed by lints. Per the
    /// design memo at
    /// `mock/research/202605220600_lint-provided-marker-directive.md`.
    /// The framework does not interpret the prop name or value; lints
    /// declare via `Lint::declared_props` which names they read and
    /// query the resolved `PropMap` for matches.
    ///
    /// Presence form (`lint:prop(audited)`) parses to
    /// `PropValue::Bool(true)`. Key-value forms accept Bool / Integer
    /// / String literals. The optional `reason` clause attaches to
    /// any prop variant for human notes.
    Prop {
        name: String,
        value: PropValue,
        reason: Option<String>,
    },
}

/// The value carried by a [`Directive::Prop`]. Three concrete leaf
/// types covering the common cases: presence and boolean flags, sized
/// counts, and free-form string identifiers. No `List` variant in v1;
/// multi-value props write multiple directives that accumulate in the
/// `PropMap` naturally.
///
/// The serde shape is `#[untagged]`: the wire form is the raw value
/// (`true`, `42`, `"foo"`) without a discriminator tag. The three
/// primitive types are distinguishable by TOML / JSON type alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropValue {
    Bool(bool),
    Integer(i64),
    String(String),
}

/// The bounded set of `ScopeConfig` axes a `Directive::ScopeAdd` may
/// extend. Mirrors the seven fields of the `ScopeConfig` struct
/// (defined in `mockspace-rs/src/config_types.rs`) exactly so the
/// resolver can dispatch by axis without string-matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeAxis {
    Paths,
    ExemptPaths,
    Crates,
    ExemptCrates,
    Languages,
    ExemptCategories,
    ProcMacroExempt,
}

/// A parsed directive with its source location.
///
/// Preprocessors emit a `Vec<DirectiveRecord>` per document; the engine
/// folds those into per-kind maps (suppressions into `SuppressionMap`,
/// introductions into `IntroducerMap`, scope extensions into
/// `ScopeAddMap`, defers into expanded `SuppressionMap` entries,
/// file-disables into `FileDisableSet`) before dispatching lints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectiveRecord {
    pub directive: Directive,
    pub span: Span,
}

// =========================================================================
// Suppression model.
// =========================================================================

/// A single suppression scope. Covers a span; suppresses a set of lint
/// names within that span; carries a mandatory tracking task id and
/// optional human reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressionScope {
    /// The span this suppression covers. For an attribute on a fn, the
    /// fn's span. For a module-level allow, the module's span. For a
    /// crate-level allow, the crate root's span.
    pub scope: Span,
    /// Lint names suppressed within this scope.
    pub lints: BTreeSet<String>,
    /// Tracking task identifier. Mandatory per the
    /// `lint-allow-requires-task-id` workspace rule; engines emit a
    /// meta-finding if a scope is populated without one.
    pub tracked: Option<String>,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

/// Project-level suppression resolver. Engines populate per-document and
/// merge into one project-level map before filtering findings.
///
/// Resolution matches Rust's `#[allow(...)]` semantics: scopes nest, and
/// the innermost enclosing scope that suppresses the finding's lint name
/// wins. Findings emitted outside any suppression scope reach the engine's
/// output unfiltered.
#[derive(Debug, Clone, Default)]
pub struct SuppressionMap {
    scopes: Vec<SuppressionScope>,
}

impl SuppressionMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, scope: SuppressionScope) {
        self.scopes.push(scope);
    }
    pub fn scopes(&self) -> &[SuppressionScope] {
        &self.scopes
    }
    /// Resolve whether `(lint_name, finding)` is suppressed. Returns the
    /// innermost enclosing scope that covers the finding; `None` if no
    /// scope suppresses.
    pub fn resolves(&self, lint_name: &str, finding: &Span) -> Option<&SuppressionScope> {
        let mut best: Option<&SuppressionScope> = None;
        for scope in &self.scopes {
            let lint_name_string = lint_name.to_string();
            if !scope.lints.contains(&lint_name_string) {
                continue;
            }
            if !scope.scope.contains(finding) {
                continue;
            }
            best = match best {
                Some(b) if b.scope.contains(&scope.scope) => Some(scope),
                Some(b) => Some(b),
                None => Some(scope),
            };
        }
        best
    }
}

// =========================================================================
// Pattern matcher.
// =========================================================================

/// Match `name` against forbidden / exempt lists. Used by the `no-suffix`
/// and `no-prefix` configurable lint families in
/// `mockspace-hilavitkutin-stack-lints`. Exact-string matching at v1;
/// glob support lands additively if a consumer needs it.
///
/// Returns `true` when `name` is in `forbidden` and not in `exempt`.
pub fn matches_pattern(name: &str, forbidden: &[String], exempt: &[String]) -> bool {
    if exempt.iter().any(|e| e == name) {
        return false;
    }
    forbidden
        .iter()
        .any(|f| name == f || name.ends_with(f) || name.starts_with(f))
}

// =========================================================================
// LintEngine.
// =========================================================================

/// The lint engine swap point.
///
/// One method runs every configured lint and returns findings with
/// suppressions already applied. Whether lints execute in parallel, share
/// a single tree walk, dispatch via node-interest tables, run as cdylibs,
/// or anything else is the engine's concern. The substrate does not know.
///
/// The engine swap is a single type-alias line at the top of the
/// invocation chain (`pub type ActiveEngine = ...;`). Mockspace's Rust
/// engine lives in `mockspace-rs`; viola's engine will live in
/// `viola-mockspace-engine` when viola integrates.
pub trait LintEngine: 'static + Send + Sync {
    type Project: Project;
    type ParseError: std::error::Error + Send + Sync + 'static;
    type LoadError: std::error::Error + Send + Sync + 'static;
    type DispatchError: std::error::Error + Send + Sync + 'static;

    /// Hash algorithm filling [`ContentHash`] on documents this engine
    /// produces.
    const HASH_ALGORITHM: HashAlgorithm;

    /// Construct a new engine instance.
    fn new() -> Result<Self, Self::LoadError>
    where
        Self: Sized;

    /// Walk the project root, run the engine's runner (disk walk, NAM
    /// deserialisation, etc.), produce the engine's [`Project`].
    fn scope_project(
        &self,
        root: &Path,
        surface: RunSurface,
    ) -> Result<Self::Project, Self::ParseError>;

    /// Run every configured lint against the project. Returns findings
    /// with suppressions already applied. The substrate does not see
    /// suppressed findings; meta-lints reading the [`SuppressionMap`] do
    /// so engine-internally during dispatch.
    fn run(
        &self,
        project: &Self::Project,
        gate: Gate,
        cfg: &dyn LintCfgStore,
    ) -> Result<Vec<Finding>, Self::DispatchError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ---- Severity / Impact / Category ----

    #[test]
    fn severity_orders_skip_lt_error() {
        assert!(Severity::Skip < Severity::Off);
        assert!(Severity::Off < Severity::Hint);
        assert!(Severity::Hint < Severity::Info);
        assert!(Severity::Info < Severity::Warn);
        assert!(Severity::Warn < Severity::Error);
    }

    #[test]
    fn severity_blocks_visible_silent() {
        assert!(Severity::Error.blocks());
        assert!(!Severity::Warn.blocks());

        assert!(Severity::Error.visible());
        assert!(Severity::Warn.visible());
        assert!(Severity::Info.visible());
        assert!(Severity::Hint.visible());
        assert!(!Severity::Off.visible());
        assert!(!Severity::Skip.visible());

        assert!(Severity::Off.silent());
        assert!(Severity::Skip.silent());
        assert!(!Severity::Hint.silent());
    }

    #[test]
    fn severity_round_trips_through_toml() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            s: Severity,
        }
        for variant in [
            Severity::Skip,
            Severity::Off,
            Severity::Hint,
            Severity::Info,
            Severity::Warn,
            Severity::Error,
        ] {
            let s = toml::to_string(&Wrap { s: variant }).unwrap();
            let r: Wrap = toml::from_str(&s).unwrap();
            assert_eq!(r.s, variant);
        }
    }

    #[test]
    fn impact_round_trips() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            i: Impact,
        }
        let s = toml::to_string(&Wrap { i: Impact::Critical }).unwrap();
        assert!(s.contains("critical"));
        let r: Wrap = toml::from_str(&s).unwrap();
        assert_eq!(r.i, Impact::Critical);
    }

    #[test]
    fn category_round_trips() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            c: Category,
        }
        let s = toml::to_string(&Wrap { c: Category::Correctness }).unwrap();
        assert!(s.contains("correctness"));
        let r: Wrap = toml::from_str(&s).unwrap();
        assert_eq!(r.c, Category::Correctness);
    }

    // ---- Language / RunSurface / ContentHash ----

    #[test]
    fn language_round_trips_through_toml() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            l: Language,
        }
        let s = toml::to_string(&Wrap { l: Language::TypeScript }).unwrap();
        assert!(s.contains("type_script"));
        let r: Wrap = toml::from_str(&s).unwrap();
        assert_eq!(r.l, Language::TypeScript);
    }

    #[test]
    fn run_surface_round_trips_through_toml() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            s: RunSurface,
        }
        let s = toml::to_string(&Wrap { s: RunSurface::Ci }).unwrap();
        assert!(s.contains("ci"));
        let r: Wrap = toml::from_str(&s).unwrap();
        assert_eq!(r.s, RunSurface::Ci);
    }

    #[test]
    fn content_hash_zero_is_all_zeros() {
        assert_eq!(ContentHash::ZERO.0, [0u8; 32]);
    }

    // ---- Span ----

    #[test]
    fn span_constructors() {
        let s = Span::single_line("a.rs", 10, 5, 3);
        assert!(s.is_single_line());
        assert_eq!(s.end_column, 8);
        let r = Span::range("a.rs", 10, 5, 12, 3);
        assert!(!r.is_single_line());
    }

    #[test]
    fn span_contains_same_file() {
        let outer = Span::range("a.rs", 10, 0, 20, 0);
        let inner = Span::range("a.rs", 15, 4, 15, 12);
        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
    }

    #[test]
    fn span_contains_rejects_different_file() {
        let a = Span::range("a.rs", 10, 0, 20, 0);
        let b = Span::range("b.rs", 12, 0, 18, 0);
        assert!(!a.contains(&b));
    }

    // ---- GateSeverity ----

    #[test]
    fn gate_severity_at_selects_correct_gate() {
        let g = GateSeverity {
            commit: Severity::Error,
            build: Severity::Warn,
            push: Severity::Off,
        };
        assert_eq!(g.at(Gate::Commit), Severity::Error);
        assert_eq!(g.at(Gate::Push), Severity::Off);
        assert!(g.any_blocks());
    }

    // ---- Finding ----

    #[test]
    fn finding_round_trips_with_full_payload() {
        let f = Finding {
            lint_name: Cow::Borrowed("no-bare-numeric"),
            rule_id: Some(Cow::Borrowed("rule-42")),
            plugin_id: Some(Cow::Borrowed("viola-rust")),
            severity: Severity::Error,
            impact: Some(Impact::Major),
            category: Some(Category::Correctness),
            message: Cow::Borrowed("found `u32`"),
            span: Span::single_line("a.rs", 10, 5, 3),
            hint: Some(Cow::Borrowed("consider Uint32 or USize")),
            help: Some(Cow::Borrowed(
                "arvo is the workspace's exclusive numeric substrate",
            )),
            suggestion: Some(Suggestion {
                description: Cow::Borrowed("replace bare u32 with UFixed<32, 0, Hot>"),
                fix: Some(Fix::Replace {
                    start: 42,
                    end: 45,
                    replacement: Cow::Borrowed("UFixed<32, 0, Hot>"),
                }),
            }),
            related_spans: vec![RelatedSpan {
                span: Span::single_line("a.rs", 8, 1, 6),
                label: Cow::Borrowed("def"),
            }],
            metadata: Some(MetadataBlob {
                schema: Cow::Borrowed("viola/diag-meta/v1"),
                bytes: vec![0xde, 0xad],
            }),
        };
        let s = toml::to_string(&f).unwrap();
        let r: Finding = toml::from_str(&s).unwrap();
        assert_eq!(r, f);
    }

    #[test]
    fn finding_minimal_form_omits_optional_fields() {
        let f = Finding {
            lint_name: Cow::Borrowed("forbids-tab"),
            rule_id: None,
            plugin_id: None,
            severity: Severity::Warn,
            impact: None,
            category: None,
            message: Cow::Borrowed("tab"),
            span: Span::single_line("a.rs", 1, 1, 1),
            hint: None,
            help: None,
            suggestion: None,
            related_spans: Vec::new(),
            metadata: None,
        };
        let s = toml::to_string(&f).unwrap();
        assert!(!s.contains("rule_id"));
        assert!(!s.contains("plugin_id"));
        assert!(!s.contains("impact"));
        assert!(!s.contains("category"));
        assert!(!s.contains("hint"));
        assert!(!s.contains("help"));
        assert!(!s.contains("suggestion"));
        assert!(!s.contains("metadata"));
    }

    // ---- LINT_CONTRACT_VERSION ----

    #[test]
    fn lint_contract_version_is_three() {
        assert_eq!(LINT_CONTRACT_VERSION, 3);
    }

    // ---- Directive ----

    #[test]
    fn directive_allow_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::Allow {
                lint_name: "no-bare-numeric".to_string(),
                reason: Some("hardcoded constant per spec".to_string()),
                tracked: Some("#427".to_string()),
            },
            span: Span::single_line("a.rs", 10, 5, 12),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn directive_introduces_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::Introduces {
                category: "string-foundation".to_string(),
            },
            span: Span::single_line("hilavitkutin-str/src/lib.rs", 42, 1, 10),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn directive_scope_add_round_trips_each_axis() {
        for axis in [
            ScopeAxis::Paths,
            ScopeAxis::ExemptPaths,
            ScopeAxis::Crates,
            ScopeAxis::ExemptCrates,
            ScopeAxis::Languages,
            ScopeAxis::ExemptCategories,
            ScopeAxis::ProcMacroExempt,
        ] {
            let r = DirectiveRecord {
                directive: Directive::ScopeAdd {
                    lint_name: "no-bare-numeric".to_string(),
                    axis,
                    value: "ffi-boundary".to_string(),
                },
                span: Span::single_line("m.rs", 1, 1, 1),
            };
            let s = toml::to_string(&r).unwrap();
            let back: DirectiveRecord = toml::from_str(&s).unwrap();
            assert_eq!(back, r, "round-trip failed for {axis:?}");
        }
    }

    #[test]
    fn directive_defer_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::Defer {
                lint_name: "no-bare-string".to_string(),
                until: "#185".to_string(),
                reason: Some("clause test rehab pending".to_string()),
            },
            span: Span::single_line("test.rs", 5, 1, 30),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn directive_file_disable_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::FileDisable {
                lint_name: "writing-style".to_string(),
                reason: Some("generated FFI binding file".to_string()),
                tracked: Some("#207".to_string()),
            },
            span: Span::single_line("generated.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn directive_kind_tag_uses_kebab_case() {
        let r = DirectiveRecord {
            directive: Directive::FileDisable {
                lint_name: "x".to_string(),
                reason: None,
                tracked: None,
            },
            span: Span::single_line("a.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        assert!(s.contains("kind = \"file-disable\""), "got: {s}");
    }

    #[test]
    fn directive_prop_presence_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::Prop {
                name: "audited".to_string(),
                value: PropValue::Bool(true),
                reason: None,
            },
            span: Span::single_line("a.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn directive_prop_integer_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::Prop {
                name: "arena_size".to_string(),
                value: PropValue::Integer(4096),
                reason: None,
            },
            span: Span::single_line("a.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn directive_prop_string_round_trips() {
        let r = DirectiveRecord {
            directive: Directive::Prop {
                name: "audit_id".to_string(),
                value: PropValue::String("A-2026-04".to_string()),
                reason: Some("audit pass 2026-04".to_string()),
            },
            span: Span::single_line("a.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        let back: DirectiveRecord = toml::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn prop_value_serialises_as_untagged_primitive() {
        // PropValue is #[serde(untagged)] — the wire form is the raw
        // value with no discriminator. TOML primitive type carries the
        // PropValue variant.
        let r = DirectiveRecord {
            directive: Directive::Prop {
                name: "x".to_string(),
                value: PropValue::Integer(7),
                reason: None,
            },
            span: Span::single_line("a.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        // The directive carries `kind = "prop"` from Directive's
        // serde tag. The PropValue itself serialises as the raw
        // untagged integer (no inner kind tag).
        assert!(s.contains("kind = \"prop\""), "got: {s}");
        assert!(s.contains("value = 7"), "got: {s}");
    }

    #[test]
    fn scope_axis_serialises_snake_case() {
        let r = DirectiveRecord {
            directive: Directive::ScopeAdd {
                lint_name: "x".to_string(),
                axis: ScopeAxis::ExemptCategories,
                value: "y".to_string(),
            },
            span: Span::single_line("a.rs", 1, 1, 1),
        };
        let s = toml::to_string(&r).unwrap();
        assert!(s.contains("axis = \"exempt_categories\""), "got: {s}");
    }

    // ---- LintCfgStore ----

    #[derive(Default)]
    struct EmptyCfg;
    impl LintCfgStore for EmptyCfg {
        fn get(&self, _lint_name: &str) -> Option<&toml::Table> {
            None
        }
    }

    struct StaticCfg {
        entries: HashMap<String, toml::Table>,
    }
    impl LintCfgStore for StaticCfg {
        fn get(&self, lint_name: &str) -> Option<&toml::Table> {
            self.entries.get(lint_name)
        }
    }

    #[test]
    fn lint_cfg_store_resolve_severity_parses_typed_gateseverity() {
        let mut entries = HashMap::new();
        let mut tbl = toml::Table::new();
        tbl.insert("commit".into(), toml::Value::String("error".into()));
        tbl.insert("build".into(), toml::Value::String("warn".into()));
        tbl.insert("push".into(), toml::Value::String("error".into()));
        entries.insert("my-lint".to_owned(), tbl);
        let cfg = StaticCfg { entries };
        let g = cfg.resolve_severity("my-lint").unwrap();
        assert_eq!(g.commit, Severity::Error);
        assert_eq!(g.build, Severity::Warn);
        assert_eq!(g.push, Severity::Error);
    }

    #[test]
    fn lint_cfg_store_empty_falls_back_to_none() {
        let cfg = EmptyCfg;
        assert!(cfg.resolve_severity("absent").is_none());
    }

    // ---- SuppressionMap ----

    fn scope(file: &str, lines: (u32, u32), names: &[&str], tracked: Option<&str>) -> SuppressionScope {
        SuppressionScope {
            scope: Span::range(file, lines.0, 0, lines.1, 0),
            lints: names.iter().map(|s| s.to_string()).collect(),
            tracked: tracked.map(|s| s.to_string()),
            reason: None,
        }
    }

    #[test]
    fn suppression_map_no_match_returns_none() {
        let mut map = SuppressionMap::new();
        map.push(scope("a.rs", (10, 20), &["no-bare-string"], Some("#1")));
        let finding = Span::single_line("a.rs", 50, 0, 1);
        assert!(map.resolves("no-bare-string", &finding).is_none());
    }

    #[test]
    fn suppression_map_exact_match_resolves() {
        let mut map = SuppressionMap::new();
        map.push(scope("a.rs", (10, 20), &["no-bare-string"], Some("#1")));
        let finding = Span::single_line("a.rs", 15, 0, 1);
        let s = map.resolves("no-bare-string", &finding).unwrap();
        assert_eq!(s.tracked.as_deref(), Some("#1"));
    }

    #[test]
    fn suppression_map_innermost_wins() {
        let mut map = SuppressionMap::new();
        map.push(scope("a.rs", (1, 100), &["no-foo"], Some("#outer")));
        map.push(scope("a.rs", (10, 20), &["no-foo"], Some("#inner")));
        let finding = Span::single_line("a.rs", 15, 0, 1);
        let s = map.resolves("no-foo", &finding).unwrap();
        assert_eq!(s.tracked.as_deref(), Some("#inner"));
    }

    #[test]
    fn suppression_map_wrong_lint_name_does_not_resolve() {
        let mut map = SuppressionMap::new();
        map.push(scope("a.rs", (10, 20), &["no-foo"], Some("#1")));
        let finding = Span::single_line("a.rs", 15, 0, 1);
        assert!(map.resolves("no-bar", &finding).is_none());
    }

    #[test]
    fn suppression_map_other_file_does_not_resolve() {
        let mut map = SuppressionMap::new();
        map.push(scope("a.rs", (10, 20), &["no-foo"], Some("#1")));
        let finding = Span::single_line("b.rs", 15, 0, 1);
        assert!(map.resolves("no-foo", &finding).is_none());
    }

    // ---- matches_pattern ----

    #[test]
    fn matches_pattern_exact() {
        assert!(matches_pattern("Manager", &["Manager".into()], &[]));
        assert!(!matches_pattern("Other", &["Manager".into()], &[]));
    }

    #[test]
    fn matches_pattern_suffix_or_prefix() {
        assert!(matches_pattern("UserManager", &["Manager".into()], &[]));
        assert!(matches_pattern("ManagerImpl", &["Manager".into()], &[]));
    }

    #[test]
    fn matches_pattern_exempt_overrides_forbidden() {
        assert!(!matches_pattern(
            "AllowedManager",
            &["Manager".into()],
            &["AllowedManager".into()],
        ));
    }

    // ---- LintEngine compile-shape proof ----

    /// A tiny test engine confirms the trait shape compiles and the
    /// associated-type / `HASH_ALGORITHM` plumbing works. The real engine
    /// lives in `mockspace-rs`.
    #[derive(Debug)]
    struct TinyProject {
        root: PathBuf,
        surface: RunSurface,
        docs: Vec<Box<dyn Document>>,
    }
    impl Project for TinyProject {
        fn root(&self) -> &Path {
            &self.root
        }
        fn surface(&self) -> RunSurface {
            self.surface
        }
        fn documents(&self) -> &[Box<dyn Document>] {
            &self.docs
        }
    }

    #[derive(Debug)]
    struct TinyEngineError(&'static str);
    impl fmt::Display for TinyEngineError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }
    impl std::error::Error for TinyEngineError {}

    struct TinyEngine;
    impl LintEngine for TinyEngine {
        type Project = TinyProject;
        type ParseError = TinyEngineError;
        type LoadError = TinyEngineError;
        type DispatchError = TinyEngineError;

        const HASH_ALGORITHM: HashAlgorithm = HashAlgorithm::Blake3;

        fn new() -> Result<Self, Self::LoadError> {
            Ok(Self)
        }
        fn scope_project(
            &self,
            root: &Path,
            surface: RunSurface,
        ) -> Result<Self::Project, Self::ParseError> {
            Ok(TinyProject {
                root: root.to_path_buf(),
                surface,
                docs: Vec::new(),
            })
        }
        fn run(
            &self,
            _project: &Self::Project,
            _gate: Gate,
            _cfg: &dyn LintCfgStore,
        ) -> Result<Vec<Finding>, Self::DispatchError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn lint_engine_trait_implementable() {
        let engine = TinyEngine::new().unwrap();
        let project = engine
            .scope_project(Path::new("/tmp"), RunSurface::Local)
            .unwrap();
        let cfg = EmptyCfg;
        let findings = engine.run(&project, Gate::Push, &cfg).unwrap();
        assert!(findings.is_empty());
        assert_eq!(<TinyEngine as LintEngine>::HASH_ALGORITHM, HashAlgorithm::Blake3);
    }
}
