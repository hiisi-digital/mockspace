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
pub mod rust_attr;

use mockspace_core::lint::{
    Directive, Document, Language, PropMap, SuppressionMap, SuppressionScope,
};
use std::collections::BTreeSet;

/// Per-language preprocessor. Engines invoke one per document; the
/// resulting maps merge into project-level resolvers.
///
/// At this slice two map kinds are supported via dedicated methods:
///
/// - [`extract`](Self::extract): routes `Directive::Allow` into a
///   [`SuppressionMap`].
/// - [`extract_props`](Self::extract_props): routes `Directive::Prop`
///   into a [`PropMap`].
///
/// `extract_props` has a default empty impl so existing preprocessors
/// keep compiling. The two passes currently re-parse the same source;
/// #546 (per-kind maps for the remaining four directive kinds) will
/// rationalise both passes into a single parse driving a bundled
/// output type.
pub trait LanguagePreprocessor {
    fn language(&self) -> Language;

    fn extract(
        &self,
        document: &dyn Document,
        out: &mut SuppressionMap,
    ) -> Result<(), PreprocessorError>;

    /// Route `Directive::Prop` records into a [`PropMap`].
    ///
    /// Default impl does nothing; preprocessors that support the
    /// `lint:prop` directive override. The Rust preprocessor's impl
    /// parses comment-form directives and pushes each `Prop` record
    /// into the caller's map.
    fn extract_props(
        &self,
        document: &dyn Document,
        out: &mut PropMap,
    ) -> Result<(), PreprocessorError> {
        let _ = (document, out);
        Ok(())
    }
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
/// The three remaining directive kinds (`Introduces`, `ScopeAdd`,
/// `FileDisable`) plus `Defer` are parsed but currently dropped by
/// [`Self::extract`]; the maps they feed into land in #546. `Defer`
/// will join `Allow` in the suppression map at that point;
/// `Introduces` / `ScopeAdd` route into separate `IntroducerMap` /
/// `ScopeAddMap` shapes; `FileDisable` becomes a per-document set
/// checked at finding-emit time.
///
/// `lint:prop` directives are routed via [`Self::extract_props`] into
/// a [`PropMap`] (slice 4 of `lint:prop`; the only routed kind that
/// landed before #546).
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
            // FileDisable, Prop) are not the suppression-map concern;
            // Prop routes via extract_props (slice 4 of lint:prop),
            // and the remaining four wire up in #546.
        }
        Ok(())
    }

    fn extract_props(
        &self,
        document: &dyn Document,
        out: &mut PropMap,
    ) -> Result<(), PreprocessorError> {
        let path_str = document.path().to_string_lossy();
        let records = comment::parse_directives(document.source(), &path_str);
        for record in records {
            if let Directive::Prop {
                name,
                value,
                reason,
            } = record.directive
            {
                out.push(record.span, name, value, reason);
            }
            // Other directive kinds parsed; they belong to other maps.
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

    // -------- extract_props tests --------

    use mockspace_core::lint::{PropMap, PropValue};

    #[test]
    fn extract_props_writes_presence_form() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "// lint:prop(audited)\nfn unsafe_op() {}\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let mut props = PropMap::new();
        RustPreprocessor.extract_props(&doc, &mut props).unwrap();
        let entries = props.all_named("audited");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, PropValue::Bool(true));
    }

    #[test]
    fn extract_props_writes_keyvalue_forms() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:prop(arena_size = 4096)
// lint:prop(audit_id = "A-2026-04")
// lint:prop(enabled = false)
struct Cfg;
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let mut props = PropMap::new();
        RustPreprocessor.extract_props(&doc, &mut props).unwrap();
        assert_eq!(props.len(), 3);
        assert_eq!(
            props.all_named("arena_size")[0].1,
            PropValue::Integer(4096)
        );
        assert_eq!(
            props.all_named("audit_id")[0].1,
            PropValue::String("A-2026-04".to_string())
        );
        assert_eq!(props.all_named("enabled")[0].1, PropValue::Bool(false));
    }

    #[test]
    fn extract_props_silently_drops_non_prop_directives() {
        // The five non-prop directive kinds parse without being written
        // into the PropMap. Symmetric with extract dropping non-Allow
        // kinds.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r##"// lint:allow(no-bare-numeric) reason: "spec" tracked: "#1"
// lint:introduces(string-foundation)
// lint:scope-add(no-bare-numeric, exempt_categories=ffi)
// lint:defer(no-bare-string, until: #185)
// lint:file-disable(writing-style) reason: "gen" tracked: "#2"
"##
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let mut props = PropMap::new();
        RustPreprocessor.extract_props(&doc, &mut props).unwrap();
        assert!(props.is_empty(), "got {} entries", props.len());
    }

    #[test]
    fn extract_props_carries_optional_reason() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "// lint:prop(audited) reason: \"audit pass 2026-04\"\nfn x() {}\n"
                .to_string(),
            hash: ContentHash::ZERO,
        };
        let mut props = PropMap::new();
        RustPreprocessor.extract_props(&doc, &mut props).unwrap();
        let entries = props.all_named("audited");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].2.as_deref(), Some("audit pass 2026-04"));
    }

    #[test]
    fn extract_props_accumulates_multiple_directives_on_same_item() {
        // Per the memo: multi-value props write multiple directives;
        // PropMap accumulates them under all_named.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:prop(allowed_import = "alloc")
// lint:prop(allowed_import = "core")
fn imports() {}
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let mut props = PropMap::new();
        RustPreprocessor.extract_props(&doc, &mut props).unwrap();
        let entries = props.all_named("allowed_import");
        assert_eq!(entries.len(), 2);
        let values: Vec<&PropValue> = entries.iter().map(|(_, v, _)| v).collect();
        assert!(values.contains(&&PropValue::String("alloc".to_string())));
        assert!(values.contains(&&PropValue::String("core".to_string())));
    }

    #[test]
    fn extract_props_on_source_without_directives_yields_empty_map() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "fn main() {}\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let mut props = PropMap::new();
        RustPreprocessor.extract_props(&doc, &mut props).unwrap();
        assert!(props.is_empty());
    }
}
