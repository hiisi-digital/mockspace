//! Engine error types.
//!
//! Two channels stay separate per schema design memo §9:
//!
//! - [`ConfigError`]: TOML configuration problems (wrong field types, bad
//!   regex, contradicts-catalog, etc.). Engine collects a vector of these
//!   from `LintsConfig::load`; CI fails the run with the vector reported
//!   against the lints.toml source locations.
//! - [`LintError`]: runtime errors from a single lint impl (parse failure,
//!   internal panic, workflow I/O failure). Engine catches per-lint and
//!   converts to a synthetic finding tagged with the lint name; the run
//!   continues with remaining lints.
//! - [`StartupWarning`]: non-fatal engine-config observations surfaced at
//!   construction time. Distinct from `ConfigError` because the engine
//!   continues to load; consumers query `MockspaceEngine::startup_warnings()`
//!   to surface them.
//!
//! Findings (about source code) and ConfigErrors (about configuration) are
//! distinct diagnostic types. Mixing them pollutes both.

use std::fmt;
use std::io;
use std::path::PathBuf;

use mockspace_core::lint::Span;

// =========================================================================
// ConfigError: TOML configuration faults.
// =========================================================================

/// Error from a lint's TOML configuration. Collected during
/// `LintsConfig::load`; reported as a vector against `lints.toml` source
/// locations before the engine dispatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// Name of the lint whose config failed.
    pub lint_name: String,

    /// Dotted path into the TOML where the fault was located
    /// (e.g. `lints.no-bare-vec.scope.crates`).
    pub field_path: String,

    /// Discriminator for the failure kind.
    pub kind: ConfigErrorKind,

    /// Free-form human message.
    pub message: String,

    /// Source span into lints.toml when known. None for catalog-default
    /// failures or for failures detected before lints.toml was parsed.
    pub source_location: Option<Span>,
}

/// Discriminator for [`ConfigError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigErrorKind {
    /// TOML field name not recognised at this position.
    UnknownField,
    /// TOML field type does not match the expected shape.
    TypeMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    /// Value is structurally valid but semantically rejected.
    InvalidValue,
    /// Value contradicts catalog metadata
    /// (e.g. `only_staged = true` on a non-staging-aware lint).
    ContradictsCatalog,
    /// `CatalogEntry::kind` does not match any registered primitive.
    UnknownKind,
    /// Per-finding-kind severity override names a kind absent from
    /// `CatalogEntry::finding_kinds`.
    UnknownFindingKind,
    /// Regex did not compile.
    UnparseableRegex { error: String },
    /// Glob did not compile.
    UnparseableGlob { error: String },
    /// Two entries collide on lint name.
    Duplicate,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "config error in `{}` at `{}`: ",
            self.lint_name, self.field_path
        )?;
        match &self.kind {
            ConfigErrorKind::UnknownField => write!(f, "unknown field")?,
            ConfigErrorKind::TypeMismatch { expected, actual } => {
                write!(f, "type mismatch (expected {expected}, found {actual})")?
            }
            ConfigErrorKind::InvalidValue => write!(f, "invalid value")?,
            ConfigErrorKind::ContradictsCatalog => write!(f, "contradicts catalog metadata")?,
            ConfigErrorKind::UnknownKind => write!(f, "unknown lint kind")?,
            ConfigErrorKind::UnknownFindingKind => write!(f, "unknown finding kind")?,
            ConfigErrorKind::UnparseableRegex { error } => {
                write!(f, "regex did not compile: {error}")?
            }
            ConfigErrorKind::UnparseableGlob { error } => {
                write!(f, "glob did not compile: {error}")?
            }
            ConfigErrorKind::Duplicate => write!(f, "duplicate lint name")?,
        }
        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

// =========================================================================
// LintError: lint runtime faults.
// =========================================================================

/// Error from a lint impl when it cannot run to a conclusion.
#[derive(Debug)]
pub enum LintError {
    /// Source parse failure on a document the lint required.
    ParseFailure {
        path: PathBuf,
        parser: &'static str,
        source: String,
    },
    /// Internal invariant violation in the lint impl.
    Internal(String),
    /// I/O failure reading workflow state.
    WorkflowIo(io::Error),
    /// Catalog config mismatch surfaced at dispatch time. Should have been
    /// caught at instantiate; this variant exists so the engine can report
    /// late-discovered drift without crashing.
    LateConfigError(ConfigError),
    /// Wraps a [`mockspace_core::LintError`] for compatibility with the
    /// engine's lint-error vocabulary.
    Core(mockspace_core::lint::LintError),
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseFailure {
                path,
                parser,
                source,
            } => {
                write!(f, "{parser} parse failure on {}: {source}", path.display())
            }
            Self::Internal(msg) => write!(f, "internal lint error: {msg}"),
            Self::WorkflowIo(e) => write!(f, "workflow I/O: {e}"),
            Self::LateConfigError(c) => write!(f, "late config error: {c}"),
            Self::Core(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for LintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorkflowIo(e) => Some(e),
            Self::LateConfigError(c) => Some(c),
            Self::Core(c) => Some(c),
            _ => None,
        }
    }
}

impl From<mockspace_core::lint::LintError> for LintError {
    fn from(value: mockspace_core::lint::LintError) -> Self {
        Self::Core(value)
    }
}

// =========================================================================
// ParseError: project scope-phase faults.
// =========================================================================

/// One issue found by the post-extraction validation gate (#547).
///
/// Distinct from [`ConfigError`] (TOML config) and `Finding` (source
/// code findings): directive-validation errors target the directive
/// records resolved at scope time. The gate collects every issue
/// into a vector and reports them all in one [`ParseError::DirectiveValidation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveValidationError {
    /// `lint:allow` / `lint:defer` / `lint:file-disable` / `lint:scope-add`
    /// named a lint name not in the registered catalog.
    UnknownLintName {
        directive: &'static str,
        name: String,
        span: Span,
    },
}

impl fmt::Display for DirectiveValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLintName {
                directive,
                name,
                span,
            } => write!(
                f,
                "`{directive}({name})` at {}:{} names a lint not in the catalog",
                span.file.display(),
                span.start_line
            ),
        }
    }
}

/// Error from `MockspaceEngine::scope_project`.
#[derive(Debug)]
pub enum ParseError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Syntax {
        path: PathBuf,
        errors: Vec<(Span, String)>,
    },
    UnsupportedLanguage {
        path: PathBuf,
        extension: String,
    },
    /// Preprocessor failed while resolving directives at scope time.
    /// Covers syntax-failure inside a comment-form directive header
    /// and other internal preprocessor faults.
    Preprocessor {
        message: String,
    },
    /// Post-extraction validation gate (#547) found one or more
    /// directive records naming lints or categories not in the
    /// registered catalog. Hard-fail at scope time: the project is
    /// not handed to dispatch when this fires.
    DirectiveValidation {
        errors: Vec<DirectiveValidationError>,
    },
    NotYetImplemented(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "io error reading {}: {source}", path.display())
            }
            Self::Syntax { path, errors } => write!(
                f,
                "syntax errors in {} ({} error(s))",
                path.display(),
                errors.len()
            ),
            Self::UnsupportedLanguage { path, extension } => write!(
                f,
                "no language adapter for extension `{extension}` ({})",
                path.display()
            ),
            Self::Preprocessor { message } => {
                write!(f, "preprocessor error during scope: {message}")
            }
            Self::DirectiveValidation { errors } => {
                writeln!(f, "{} directive validation error(s):", errors.len())?;
                for e in errors {
                    writeln!(f, "  - {e}")?;
                }
                Ok(())
            }
            Self::NotYetImplemented(msg) => write!(f, "not yet implemented: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

// =========================================================================
// LoadError: catalog / TOML load-phase faults.
// =========================================================================

/// Error from loading the lint catalog and configuration.
#[derive(Debug)]
pub enum LoadError {
    /// One or more `ConfigError`s during TOML load.
    Config(Vec<ConfigError>),
    /// Catalog duplicate-name detection.
    DuplicateCatalogName { name: String },
    /// I/O error reading config files.
    Io { context: String, source: io::Error },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(errs) => {
                writeln!(f, "{} configuration error(s):", errs.len())?;
                for e in errs {
                    writeln!(f, "  - {e}")?;
                }
                Ok(())
            }
            Self::DuplicateCatalogName { name } => {
                write!(f, "duplicate catalog entry name: `{name}`")
            }
            Self::Io { context, source } => write!(f, "io during {context}: {source}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

// =========================================================================
// DispatchError: per-run dispatch faults.
// =========================================================================

/// Error from a single lint run.
#[derive(Debug)]
pub enum DispatchError {
    LintErrored {
        lint_name: String,
        source: LintError,
    },
    RuntimeRefused {
        reason: String,
    },
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LintErrored { lint_name, source } => {
                write!(f, "lint `{lint_name}` errored: {source}")
            }
            Self::RuntimeRefused { reason } => {
                write!(f, "engine runtime refused dispatch: {reason}")
            }
        }
    }
}

impl std::error::Error for DispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LintErrored { source, .. } => Some(source),
            _ => None,
        }
    }
}

// =========================================================================
// StartupWarning: non-fatal engine-config observations.
// =========================================================================

/// Non-fatal warning surfaced at engine startup.
///
/// Distinct from [`ConfigError`] (which blocks load) and runtime
/// `Finding`s (which target source code). Startup warnings target the
/// engine's catalog-and-config state: the engine continues to load
/// after producing them. Consumers retrieve them via
/// `MockspaceEngine::startup_warnings()`.
///
/// Per the `lint:prop` design memo at
/// `mock/research/202605220600_lint-provided-marker-directive.md` §
/// "Namespace handling: detect, do not require".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupWarning {
    /// Two or more registered lints declare the same `lint:prop` name.
    ///
    /// Detection is namespace-aware: prop names prefixed `mockspace::`
    /// are reserved as the first-party namespace and collisions among
    /// them are silent (assumed coordinated within one pack). An
    /// unqualified prop name declared by two or more lints warns here,
    /// naming every lint that declared it.
    PropNameConflict {
        prop_name: String,
        lints: Vec<String>,
    },
}

impl fmt::Display for StartupWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PropNameConflict { prop_name, lints } => {
                write!(
                    f,
                    "prop `{prop_name}` declared by multiple lints: {}",
                    lints.join(", ")
                )
            }
        }
    }
}
