//! Per-language preprocessors that produce [`SuppressionMap`] entries.
//!
//! Each preprocessor reads source bytes for one [`Language`] and emits
//! [`SuppressionScope`]s into a project-level [`SuppressionMap`]. The
//! engine merges per-document maps before resolving findings.
//!
//! # Today
//!
//! [`RustPreprocessor`] is a stub: returns an empty map. A future round
//! fills it with a real walk over `#[mock::lints::allow(...)]` attributes
//! via `syn`.

use mockspace_core::lint::{Document, Language, SuppressionMap};

/// Per-language preprocessor. Engines invoke one per document; the
/// resulting maps merge into a project-level [`SuppressionMap`].
pub trait LanguagePreprocessor {
    fn language(&self) -> Language;

    fn extract(
        &self,
        document: &dyn Document,
        out: &mut SuppressionMap,
    ) -> Result<(), PreprocessorError>;
}

/// Error produced by a preprocessor when source is too malformed to walk.
#[derive(Debug)]
pub enum PreprocessorError {
    SyntaxFailure { path: String, reason: String },
    Internal { reason: String },
}

impl std::fmt::Display for PreprocessorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SyntaxFailure { path, reason } => {
                write!(f, "preprocessor syntax failure in {path}: {reason}")
            }
            Self::Internal { reason } => write!(f, "preprocessor internal: {reason}"),
        }
    }
}

impl std::error::Error for PreprocessorError {}

/// Rust preprocessor (stub).
#[derive(Debug, Default)]
pub struct RustPreprocessor;

impl LanguagePreprocessor for RustPreprocessor {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extract(
        &self,
        _document: &dyn Document,
        _out: &mut SuppressionMap,
    ) -> Result<(), PreprocessorError> {
        Ok(())
    }
}
