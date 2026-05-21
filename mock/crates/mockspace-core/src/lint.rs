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
use std::collections::{BTreeMap, BTreeSet};
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
/// `Span` derives `Ord`, but the `Ord` is on the field tuple
/// `(file, start_line, start_column, end_line, end_column)`. The
/// `PathBuf` component sorts by byte-wise `OsStr` ordering, which is
/// platform-dependent and NOT lexicographic in any UTF-8 sense.
/// Consumers must not treat sorted span iteration as "alphabetical by
/// path"; the order is deterministic within a run and that is all the
/// `Ord` impl promises. Internal indices (e.g. `PropMap`) only need
/// the total order to be a deterministic `BTreeMap` key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Kind of suppression carried by a [`SuppressionScope`]. Distinguishes
/// long-lived `Allow` policy entries from time-bounded `Defer`
/// acknowledgements that expire when their `tracked` task closes.
///
/// At suppression-resolution time both kinds suppress the matching
/// finding. The meta-lint (`suppression-meta`) reads this field to
/// enforce per-kind validation: `Allow` accumulates as a policy
/// question; `Defer` carries an expiration semantics and surfaces a
/// finding once the linked task is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionKind {
    /// `// lint:allow(...)`: accumulating per-site policy.
    Allow,
    /// `// lint:defer(...)`: acknowledgement bounded by `tracked`.
    Defer,
}

impl Default for SuppressionKind {
    fn default() -> Self {
        Self::Allow
    }
}

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
    /// Whether the scope was created by `lint:allow` (long-lived policy)
    /// or `lint:defer` (expiring acknowledgement). Defaults to `Allow`
    /// so pre-existing callers that construct the struct field-by-field
    /// continue to compile.
    pub kind: SuppressionKind,
    /// Tracking task identifier. Mandatory per the
    /// `lint-allow-requires-task-id` workspace rule; engines emit a
    /// meta-finding if a scope is populated without one. For
    /// [`SuppressionKind::Defer`] this holds the `until: <task-id>`
    /// argument.
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
// PropMap (lint:prop directive resolver).
// =========================================================================

/// Project-level resolver for `lint:prop(...)` directives.
///
/// Per the design memo at
/// `mock/research/202605220600_lint-provided-marker-directive.md`.
/// The framework does not interpret prop names or values; lints declare
/// the names they read via [`crate::lint::Lint::declared_props`] (in
/// mockspace-rs) and query `PropMap` for matches.
///
/// # Indexing
///
/// `PropMap` keeps a dual index so both common queries are log-time:
///
/// - `by_name`: "which sites carry a prop with this name?"
/// - `by_span`: "what props are declared at exactly this span?"
///
/// `push` keeps both indices in sync. Callers cannot insert into one
/// without the other.
///
/// # Scope accessors
///
/// Three scope resolutions are exposed as distinct methods so consuming
/// lints declare which one they want. The default attachment rule is
/// "item + direct impl blocks", matching `introduces`.
///
/// - [`at_site`](Self::at_site): props with span equal to the query.
/// - [`including_impl_blocks`](Self::including_impl_blocks): item plus
///   its direct impl blocks (same file, same module, same type).
/// - [`walk_ancestors`](Self::walk_ancestors): props anywhere in the
///   enclosing item chain (module, file, crate root).
///
/// # AST-aware resolution is a slice 4 concern
///
/// At this slice, `including_impl_blocks` and `walk_ancestors` operate
/// only on the spans stored in the map; they do not consult an AST.
/// Concretely:
///
/// - `including_impl_blocks(query)` returns at-site matches today; the
///   "direct impl blocks" walk requires an AST and lands when
///   `RustPreprocessor::extract` wires up structural context in slice 4
///   of the lint:prop work.
/// - `walk_ancestors(query)` returns every prop in the same file whose
///   start_line is at or before the query's start_line. This is a
///   defensible best-effort that gives lints something to query today;
///   slice 4 sharpens it to respect actual scope nesting.
///
/// `at_site` and `all_named` are exact today and stay exact.
#[derive(Debug, Clone, Default)]
pub struct PropMap {
    by_name: BTreeMap<String, Vec<(Span, PropValue, Option<String>)>>,
    by_span: BTreeMap<Span, Vec<(String, PropValue, Option<String>)>>,
}

/// A borrowed view of one prop declaration. Returned by every
/// [`PropMap`] accessor so lint authors do not have to remember which
/// tuple position holds what across name-keyed / span-keyed / ancestor-
/// walking queries.
///
/// Named struct rather than a positional tuple because the three index
/// shapes used to disagree about whether name or span came first; the
/// named shape eliminates the footgun. Lifetime ties to the `PropMap`.
#[derive(Debug, Clone, Copy)]
pub struct PropEntry<'a> {
    pub span: &'a Span,
    pub name: &'a str,
    pub value: &'a PropValue,
    pub reason: Option<&'a str>,
}

impl PropMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a prop declaration at `span`. Both indices update.
    ///
    /// **No dedup**: pushing the same `(span, name)` pair twice records
    /// two entries. Both indices store equal multiplicity for each
    /// `(span, name)` pair; the two indices stay in sync because
    /// `push` is the only mutator and updates both atomically.
    /// Consumers that need uniqueness must enforce it before pushing.
    pub fn push(&mut self, span: Span, name: String, value: PropValue, reason: Option<String>) {
        self.by_name
            .entry(name.clone())
            .or_default()
            .push((span.clone(), value.clone(), reason.clone()));
        self.by_span
            .entry(span)
            .or_default()
            .push((name, value, reason));
    }

    /// All sites carrying a prop with this name.
    pub fn all_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = PropEntry<'a>> + 'a {
        self.by_name
            .get(name)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(move |(span, value, reason)| PropEntry {
                span,
                name,
                value,
                reason: reason.as_deref(),
            })
    }

    /// Props declared at exactly this span.
    pub fn at_site<'a>(&'a self, span: &Span) -> impl Iterator<Item = PropEntry<'a>> + 'a {
        self.by_span
            .get_key_value(span)
            .into_iter()
            .flat_map(|(span_ref, entries)| {
                entries.iter().map(move |(name, value, reason)| PropEntry {
                    span: span_ref,
                    name: name.as_str(),
                    value,
                    reason: reason.as_deref(),
                })
            })
    }

    /// Props attached to this span plus its direct impl blocks. At
    /// this slice this delegates to [`at_site`]; AST-aware resolution
    /// lands in a follow-up to slice 4 of the lint:prop work, where
    /// `RustPreprocessor::extract` gains structural context.
    pub fn including_impl_blocks<'a>(
        &'a self,
        span: &Span,
    ) -> impl Iterator<Item = PropEntry<'a>> + 'a {
        self.at_site(span)
    }

    /// Props in the enclosing item chain (strict ancestor scope).
    ///
    /// At this slice this is approximated by "props in the same file
    /// strictly above the query's start_line". A prop on the query's
    /// own line is NOT an ancestor of itself; consumers querying for
    /// at-site matches should use [`at_site`] instead. A follow-up to
    /// slice 4 sharpens this to respect AST scope nesting.
    pub fn walk_ancestors<'a>(
        &'a self,
        query: &'a Span,
    ) -> impl Iterator<Item = PropEntry<'a>> + 'a {
        self.by_span
            .iter()
            .filter(move |(span, _)| {
                span.file == query.file && span.start_line < query.start_line
            })
            .flat_map(|(span, entries)| {
                entries.iter().map(move |(name, value, reason)| PropEntry {
                    span,
                    name: name.as_str(),
                    value,
                    reason: reason.as_deref(),
                })
            })
    }

    /// Number of stored prop declarations across all names.
    pub fn len(&self) -> usize {
        self.by_span.values().map(|v| v.len()).sum()
    }

    /// `true` if no props have been pushed.
    pub fn is_empty(&self) -> bool {
        self.by_span.is_empty()
    }
}

// =========================================================================
// IntroducerMap (lint:introduces directive resolver).
// =========================================================================

/// Project-level resolver for `// lint:introduces(<category>)` directives.
///
/// Records sites that declare themselves as the canonical introducer of
/// a primitive category. Category-checked lints (`no-bare-numeric`,
/// `no-bare-string`, the rest of the bare-primitive family) consult
/// `IntroducerMap` to carve out their own findings on the introducer
/// site itself.
///
/// Replaces the pre-v2 `[primitive-introductions]` TOML table. Per the
/// canonical-directive-vocabulary memo at
/// `mock/research/202605220000_canonical-directive-vocabulary.md` and
/// the implementing task #546.
///
/// # Indexing
///
/// Dual `BTreeMap` index for the two common queries:
///
/// - `by_category`: "which sites introduce this category?"
/// - `by_span`:     "what categories does this site introduce?"
///
/// `push` keeps both indices in sync. Same dedup contract as
/// [`PropMap`]: repeated `(span, category)` pairs record both
/// occurrences and the meta-lint surfaces duplicates downstream.
#[derive(Debug, Clone, Default)]
pub struct IntroducerMap {
    by_category: BTreeMap<String, Vec<Span>>,
    by_span: BTreeMap<Span, Vec<String>>,
}

impl IntroducerMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a `(span, category)` introducer entry.
    pub fn push(&mut self, span: Span, category: String) {
        self.by_category
            .entry(category.clone())
            .or_default()
            .push(span.clone());
        self.by_span.entry(span).or_default().push(category);
    }

    /// All sites declaring themselves introducer of `category`.
    pub fn sites_for<'a>(&'a self, category: &str) -> impl Iterator<Item = &'a Span> + 'a {
        self.by_category
            .get(category)
            .into_iter()
            .flat_map(|v| v.iter())
    }

    /// Categories introduced at this span.
    pub fn categories_at<'a>(&'a self, span: &Span) -> impl Iterator<Item = &'a str> + 'a {
        self.by_span
            .get(span)
            .into_iter()
            .flat_map(|v| v.iter().map(String::as_str))
    }

    /// `true` if any site declares itself introducer of `category`
    /// covering `finding_span`. Coverage is span-contained: the
    /// introducer's span must contain the finding's span. Lints
    /// use this as the carve-out check.
    pub fn covers(&self, category: &str, finding_span: &Span) -> bool {
        self.sites_for(category)
            .any(|introducer_span| introducer_span.contains(finding_span))
    }

    /// Flat iterator over every `(span, category)` introducer pair.
    /// Used by engines aggregating per-document maps into a project-
    /// level resolver.
    pub fn entries<'a>(&'a self) -> impl Iterator<Item = (&'a Span, &'a str)> + 'a {
        self.by_span.iter().flat_map(|(span, cats)| {
            cats.iter().map(move |c| (span, c.as_str()))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.by_span.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_span.values().map(|v| v.len()).sum()
    }
}

// =========================================================================
// ScopeAddMap (lint:scope-add directive resolver).
// =========================================================================

/// One `// lint:scope-add(<lint_name>, <axis>=<value>)` directive
/// resolved to a structured record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAddEntry {
    /// The span over which the scope extension applies. For an
    /// item-attached directive, the item's span; for a module-level
    /// directive, the module's span; for a file-level directive,
    /// the file's span.
    pub scope: Span,
    /// Lint whose scope the directive extends.
    pub lint_name: String,
    /// Which axis of `ScopeConfig` is extended.
    pub axis: ScopeAxis,
    /// Value added to the named axis.
    pub value: String,
}

/// Project-level collection of `lint:scope-add` directives. Engines
/// merge per-document maps before scope-filter evaluation.
///
/// Stored as a flat `Vec` because the three plausible queries
/// (entries-for-lint, entries-covering-span, full enumeration) all
/// stream over the collection and the realistic count per project is
/// small. A future index lands additively if a bench shows it pays.
#[derive(Debug, Clone, Default)]
pub struct ScopeAddMap {
    entries: Vec<ScopeAddEntry>,
}

impl ScopeAddMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: ScopeAddEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[ScopeAddEntry] {
        &self.entries
    }

    /// Entries that extend `lint_name`'s scope and cover `finding_span`.
    pub fn entries_for<'a>(
        &'a self,
        lint_name: &'a str,
        finding_span: &'a Span,
    ) -> impl Iterator<Item = &'a ScopeAddEntry> + 'a {
        self.entries.iter().filter(move |e| {
            e.lint_name == lint_name && e.scope.contains(finding_span)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// =========================================================================
// FileDisableSet (lint:file-disable directive resolver).
// =========================================================================

/// One `// lint:file-disable(<lint_name>) reason: "..." tracked: #...`
/// directive resolved to a structured record. The `reason` and
/// `tracked` fields parallel [`SuppressionScope`] so the meta-lint
/// validates both surfaces with one rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDisableEntry {
    pub file: PathBuf,
    pub lint_name: String,
    pub tracked: Option<String>,
    pub reason: Option<String>,
}

/// Per-file lint-disable set. Engines consult before emitting a
/// finding: if `disabled(finding.span.file, finding.lint_name)` is
/// true, the finding is dropped before suppression resolution.
///
/// File-disable is structurally distinct from [`SuppressionMap`]:
/// suppression scopes are span-bounded and nest; file-disable is
/// whole-file and flat. Folding it into the suppression map would
/// require constructing a synthetic file-spanning [`Span`], which the
/// engine cannot do cheaply without reading the file to find
/// end-of-file. Keeping the two separate sidesteps the synthesis.
#[derive(Debug, Clone, Default)]
pub struct FileDisableSet {
    entries: Vec<FileDisableEntry>,
    by_file: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl FileDisableSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: FileDisableEntry) {
        self.by_file
            .entry(entry.file.clone())
            .or_default()
            .insert(entry.lint_name.clone());
        self.entries.push(entry);
    }

    /// `true` if the given lint is disabled for the given file.
    pub fn disabled(&self, file: &Path, lint_name: &str) -> bool {
        self.by_file
            .get(file)
            .is_some_and(|lints| lints.contains(lint_name))
    }

    /// All lint names disabled for `file`. Empty set when no
    /// directive named `file`.
    pub fn disabled_lints(&self, file: &Path) -> Option<&BTreeSet<String>> {
        self.by_file.get(file)
    }

    pub fn entries(&self) -> &[FileDisableEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
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
    fn propmap_push_keeps_both_indices_in_sync() {
        let mut map = PropMap::new();
        let span = Span::single_line("a.rs", 10, 1, 5);
        map.push(
            span.clone(),
            "audited".to_string(),
            PropValue::Bool(true),
            None,
        );
        assert_eq!(map.len(), 1);
        // by_name index: lookup by "audited" returns the span
        let by_name: Vec<PropEntry<'_>> = map.all_named("audited").collect();
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].span, &span);
        assert_eq!(by_name[0].name, "audited");
        // by_span index: lookup by the same span returns "audited"
        let at: Vec<PropEntry<'_>> = map.at_site(&span).collect();
        assert_eq!(at.len(), 1);
        assert_eq!(at[0].name, "audited");
        assert_eq!(at[0].span, &span);
    }

    #[test]
    fn propmap_at_site_distinguishes_spans() {
        let mut map = PropMap::new();
        let span_a = Span::single_line("a.rs", 1, 1, 1);
        let span_b = Span::single_line("a.rs", 2, 1, 1);
        map.push(
            span_a.clone(),
            "tag-a".to_string(),
            PropValue::Bool(true),
            None,
        );
        map.push(
            span_b.clone(),
            "tag-b".to_string(),
            PropValue::Bool(true),
            None,
        );
        let at_a: Vec<PropEntry<'_>> = map.at_site(&span_a).collect();
        let at_b: Vec<PropEntry<'_>> = map.at_site(&span_b).collect();
        assert_eq!(at_a[0].name, "tag-a");
        assert_eq!(at_b[0].name, "tag-b");
    }

    #[test]
    fn propmap_all_named_returns_every_site() {
        let mut map = PropMap::new();
        map.push(
            Span::single_line("a.rs", 1, 1, 1),
            "audited".to_string(),
            PropValue::Bool(true),
            None,
        );
        map.push(
            Span::single_line("a.rs", 5, 1, 1),
            "audited".to_string(),
            PropValue::Bool(true),
            None,
        );
        assert_eq!(map.all_named("audited").count(), 2);
        assert_eq!(map.all_named("does-not-exist").count(), 0);
    }

    #[test]
    fn propmap_pushing_same_span_name_twice_preserves_both() {
        // Reviewer #49 finding 3 / push contract: no dedup. Pushing the
        // same (span, name) pair twice records two entries in both
        // indices. Locks the contract before a future agent "helpfully"
        // dedupes it.
        let mut map = PropMap::new();
        let span = Span::single_line("a.rs", 1, 1, 1);
        map.push(
            span.clone(),
            "audited".to_string(),
            PropValue::Bool(true),
            Some("first".to_string()),
        );
        map.push(
            span.clone(),
            "audited".to_string(),
            PropValue::Bool(true),
            Some("second".to_string()),
        );
        assert_eq!(map.len(), 2);
        assert_eq!(map.all_named("audited").count(), 2);
        assert_eq!(map.at_site(&span).count(), 2);
    }

    #[test]
    fn propmap_walk_ancestors_excludes_query_own_line() {
        // Reviewer #49 finding 2: ancestor semantics are strict. A
        // prop on the query's own line is at_site, not an ancestor.
        let mut map = PropMap::new();
        let query_line = Span::single_line("a.rs", 10, 1, 1);
        map.push(
            query_line.clone(),
            "at-query".to_string(),
            PropValue::Bool(true),
            None,
        );
        let found: Vec<&str> = map.walk_ancestors(&query_line).map(|e| e.name).collect();
        assert!(!found.contains(&"at-query"), "found at-query in walk: {found:?}");
    }

    #[test]
    fn propmap_walk_ancestors_filters_by_file_and_line() {
        let mut map = PropMap::new();
        // Prop on line 2 (an ancestor of line 10)
        map.push(
            Span::single_line("a.rs", 2, 1, 1),
            "ancestor".to_string(),
            PropValue::Bool(true),
            None,
        );
        // Prop on line 15 (below the query line)
        map.push(
            Span::single_line("a.rs", 15, 1, 1),
            "below".to_string(),
            PropValue::Bool(true),
            None,
        );
        // Prop in a different file
        map.push(
            Span::single_line("b.rs", 1, 1, 1),
            "other-file".to_string(),
            PropValue::Bool(true),
            None,
        );

        let query = Span::single_line("a.rs", 10, 1, 1);
        let found: Vec<&str> = map.walk_ancestors(&query).map(|e| e.name).collect();
        assert!(found.contains(&"ancestor"));
        assert!(!found.contains(&"below"));
        assert!(!found.contains(&"other-file"));
    }

    #[test]
    fn propmap_carries_optional_reason() {
        let mut map = PropMap::new();
        let span = Span::single_line("a.rs", 1, 1, 1);
        map.push(
            span.clone(),
            "audited".to_string(),
            PropValue::Bool(true),
            Some("audit pass 2026-04".to_string()),
        );
        let at: Vec<PropEntry<'_>> = map.at_site(&span).collect();
        assert_eq!(at[0].reason, Some("audit pass 2026-04"));
    }

    #[test]
    fn propmap_empty_by_default() {
        let map = PropMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.all_named("anything").count(), 0);
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
            kind: SuppressionKind::Allow,
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

    // ---- IntroducerMap ----

    #[test]
    fn introducer_map_dual_index_returns_both_views() {
        let mut map = IntroducerMap::new();
        let span_a = Span::range("a.rs", 1, 0, 5, 0);
        let span_b = Span::range("b.rs", 1, 0, 5, 0);
        map.push(span_a.clone(), "string-foundation".to_string());
        map.push(span_b.clone(), "string-foundation".to_string());
        map.push(span_a.clone(), "numeric-foundation".to_string());

        let sites: Vec<&Span> = map.sites_for("string-foundation").collect();
        assert_eq!(sites.len(), 2);
        let cats_at_a: Vec<&str> = map.categories_at(&span_a).collect();
        assert_eq!(cats_at_a.len(), 2);
        assert!(cats_at_a.contains(&"string-foundation"));
        assert!(cats_at_a.contains(&"numeric-foundation"));
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn introducer_map_covers_uses_span_containment() {
        let mut map = IntroducerMap::new();
        let outer = Span::range("a.rs", 1, 0, 100, 0);
        map.push(outer.clone(), "string-foundation".to_string());
        let inner_finding = Span::single_line("a.rs", 50, 5, 3);
        assert!(map.covers("string-foundation", &inner_finding));
        let other_file = Span::single_line("b.rs", 50, 5, 3);
        assert!(!map.covers("string-foundation", &other_file));
        assert!(!map.covers("never-introduced", &inner_finding));
    }

    // ---- ScopeAddMap ----

    #[test]
    fn scope_add_map_entries_for_filters_by_lint_and_span() {
        let mut map = ScopeAddMap::new();
        let scope_a = Span::range("a.rs", 1, 0, 50, 0);
        let scope_b = Span::range("b.rs", 1, 0, 50, 0);
        map.push(ScopeAddEntry {
            scope: scope_a.clone(),
            lint_name: "no-bare-numeric".to_string(),
            axis: ScopeAxis::ExemptCategories,
            value: "ffi".to_string(),
        });
        map.push(ScopeAddEntry {
            scope: scope_b.clone(),
            lint_name: "no-bare-numeric".to_string(),
            axis: ScopeAxis::ExemptCategories,
            value: "tests".to_string(),
        });

        let in_a = Span::single_line("a.rs", 25, 0, 1);
        let entries_a: Vec<&ScopeAddEntry> =
            map.entries_for("no-bare-numeric", &in_a).collect();
        assert_eq!(entries_a.len(), 1);
        assert_eq!(entries_a[0].value, "ffi");

        let entries_other_lint: Vec<&ScopeAddEntry> =
            map.entries_for("no-bare-string", &in_a).collect();
        assert!(entries_other_lint.is_empty());
    }

    // ---- FileDisableSet ----

    #[test]
    fn file_disable_set_disabled_lookup() {
        let mut set = FileDisableSet::new();
        set.push(FileDisableEntry {
            file: "a.rs".into(),
            lint_name: "writing-style".to_string(),
            tracked: Some("#207".to_string()),
            reason: Some("generated".to_string()),
        });
        assert!(set.disabled(Path::new("a.rs"), "writing-style"));
        assert!(!set.disabled(Path::new("a.rs"), "no-bare-numeric"));
        assert!(!set.disabled(Path::new("b.rs"), "writing-style"));
    }

    #[test]
    fn file_disable_set_multiple_lints_per_file() {
        let mut set = FileDisableSet::new();
        set.push(FileDisableEntry {
            file: "a.rs".into(),
            lint_name: "lint-a".to_string(),
            tracked: None,
            reason: None,
        });
        set.push(FileDisableEntry {
            file: "a.rs".into(),
            lint_name: "lint-b".to_string(),
            tracked: None,
            reason: None,
        });
        let disabled = set.disabled_lints(Path::new("a.rs")).unwrap();
        assert!(disabled.contains("lint-a"));
        assert!(disabled.contains("lint-b"));
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
