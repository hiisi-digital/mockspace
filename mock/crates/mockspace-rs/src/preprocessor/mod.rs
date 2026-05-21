//! Per-language preprocessors that produce [`SuppressionMap`] entries.
//!
//! Each preprocessor reads source bytes for one [`Language`] and emits
//! [`SuppressionScope`]s into a project-level [`SuppressionMap`]. The
//! engine merges per-document maps before resolving findings.
//!
//! # Today
//!
//! [`RustPreprocessor::extract`] calls
//! [`comment::parse_directives`] over the document's source, converts
//! every [`Directive::Allow`] record into a [`SuppressionScope`] entry,
//! and writes the result into the caller's [`SuppressionMap`]. The
//! other four directive kinds (Introduces, ScopeAdd, Defer, FileDisable)
//! are parsed but currently dropped; their per-kind maps land in #546.
//! [`comment::parse_directives`] itself parses the canonical five-
//! directive vocabulary per the design memo at
//! `mock/research/202605220000_canonical-directive-vocabulary.md`.

pub mod comment;

use mockspace_core::lint::{
    Directive, Document, Language, SuppressionMap, SuppressionScope,
};
use std::collections::BTreeSet;

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

/// Rust preprocessor.
///
/// Reads source comments, parses canonical directives via
/// [`comment::parse_directives`], and forwards `Allow` directives as
/// [`SuppressionScope`] entries into the caller's [`SuppressionMap`].
///
/// The other four directive kinds (`Introduces`, `ScopeAdd`, `Defer`,
/// `FileDisable`) are parsed but currently dropped; the maps they feed
/// into land in #546. `Defer` will join `Allow` in the suppression map
/// at that point; `Introduces` / `ScopeAdd` route into separate
/// `IntroducerMap` / `ScopeAddMap` shapes; `FileDisable` becomes a
/// per-document set checked at finding-emit time.
#[derive(Debug, Default)]
pub struct RustPreprocessor;

impl LanguagePreprocessor for RustPreprocessor {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extract(
        &self,
        document: &dyn Document,
        out: &mut SuppressionMap,
    ) -> Result<(), PreprocessorError> {
        let path_str = document.path().to_string_lossy();
        let records = comment::parse_directives(document.source(), &path_str);
        for record in records {
            if let Directive::Allow {
                lint_name,
                reason,
                tracked,
            } = record.directive
            {
                let mut lints = BTreeSet::new();
                lints.insert(lint_name);
                out.push(SuppressionScope {
                    scope: record.span,
                    lints,
                    tracked,
                    reason,
                });
            }
            // Other directive kinds (Introduces, ScopeAdd, Defer,
            // FileDisable) are parsed but ignored here; #546 wires
            // their per-kind maps.
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::ContentHash;

    /// Minimal Document impl for unit tests. The real
    /// [`crate::document::MockspaceDocument`] carries syn/tree-sitter
    /// caches we don't need here; a fixed-source stub keeps the
    /// integration test focused on directive extraction.
    #[derive(Debug)]
    struct StubDocument {
        path: std::path::PathBuf,
        source: String,
        hash: ContentHash,
    }

    impl Document for StubDocument {
        fn path(&self) -> &std::path::Path {
            &self.path
        }
        fn language(&self) -> Language {
            Language::Rust
        }
        fn source(&self) -> &str {
            &self.source
        }
        fn content_hash(&self) -> &ContentHash {
            &self.hash
        }
    }

    #[test]
    fn extract_writes_allow_into_suppression_map() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "// lint:allow(no-bare-numeric) reason: \"const constant\" tracked: #427\nconst X: u64 = 1;\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let mut map = SuppressionMap::new();
        RustPreprocessor.extract(&doc, &mut map).unwrap();
        assert_eq!(map.scopes().len(), 1);
        let scope = &map.scopes()[0];
        assert!(scope.lints.contains("no-bare-numeric"));
        assert_eq!(scope.reason.as_deref(), Some("const constant"));
        assert_eq!(scope.tracked.as_deref(), Some("#427"));
    }

    #[test]
    fn extract_silently_drops_non_allow_directives() {
        // Until #546 lands, the other four directive kinds parse
        // without being written into the SuppressionMap. They are not
        // an error; they just don't generate suppression entries.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:introduces(string-foundation)
// lint:scope-add(no-bare-numeric, exempt_categories=ffi)
// lint:defer(no-bare-string, until: #185)
// lint:file-disable(writing-style) reason: "generated" tracked: #207
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let mut map = SuppressionMap::new();
        RustPreprocessor.extract(&doc, &mut map).unwrap();
        assert!(map.scopes().is_empty(), "got {} scopes", map.scopes().len());
    }

    #[test]
    fn extract_mixes_allow_with_other_kinds() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:introduces(string-foundation)
// lint:allow(no-bare-numeric) reason: "constants" tracked: #1
// lint:file-disable(writing-style) reason: "generated" tracked: #2
// lint:allow(no-bare-string) reason: "test fixture" tracked: #3
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let mut map = SuppressionMap::new();
        RustPreprocessor.extract(&doc, &mut map).unwrap();
        assert_eq!(map.scopes().len(), 2);
        let names: BTreeSet<&str> = map
            .scopes()
            .iter()
            .flat_map(|s| s.lints.iter().map(|l| l.as_str()))
            .collect();
        assert!(names.contains("no-bare-numeric"));
        assert!(names.contains("no-bare-string"));
    }

    #[test]
    fn extract_on_source_without_directives_yields_empty_map() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "fn main() {}\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let mut map = SuppressionMap::new();
        RustPreprocessor.extract(&doc, &mut map).unwrap();
        assert!(map.scopes().is_empty());
    }
}
