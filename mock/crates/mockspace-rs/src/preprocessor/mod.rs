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

/// Per-document directive extracts. Returned from
/// [`LanguagePreprocessor::extract`]; engines merge the per-document
/// bundles into a project-level bundle before resolving findings.
///
/// One bundled output replaces the previous parallel-methods shape
/// (`extract` + `extract_props`) so future directive-routing additions
/// (#546: IntroducerMap, ScopeAddMap, FileDisableSet, deferred entries)
/// extend this struct rather than the trait. Single parse per document
/// drives every routed map.
#[derive(Debug, Default)]
pub struct DirectiveExtracts {
    /// `lint:allow` records routed into [`SuppressionMap`].
    pub suppressions: SuppressionMap,
    /// `lint:prop` records routed into [`PropMap`].
    pub props: PropMap,
    // Future fields (#546): introducers, scope_adds, file_disables,
    // deferred. Each grows a field here; trait stays one method.
}

/// Per-language preprocessor. Engines invoke one per document; the
/// returned [`DirectiveExtracts`] bundle is merged into project-level
/// resolvers by the engine.
///
/// One method, one parse, bundled output. Concrete preprocessors call
/// the parser once and route each directive variant into the right
/// field of [`DirectiveExtracts`]. As more directive kinds gain
/// resolved-state containers (#546), they become new fields on
/// `DirectiveExtracts`; the trait signature stays stable.
pub trait LanguagePreprocessor {
    fn language(&self) -> Language;

    fn extract(
        &self,
        document: &dyn Document,
    ) -> Result<DirectiveExtracts, PreprocessorError>;
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
    ) -> Result<DirectiveExtracts, PreprocessorError> {
        let path_str = document.path().to_string_lossy();
        let records = comment::parse_directives(document.source(), &path_str);
        let mut out = DirectiveExtracts::default();
        for record in records {
            match record.directive {
                Directive::Allow {
                    lint_name,
                    reason,
                    tracked,
                } => {
                    let mut lints = BTreeSet::new();
                    lints.insert(lint_name);
                    out.suppressions.push(SuppressionScope {
                        scope: record.span,
                        lints,
                        tracked,
                        reason,
                    });
                }
                Directive::Prop {
                    name,
                    value,
                    reason,
                } => {
                    out.props.push(record.span, name, value, reason);
                }
                // Introduces, ScopeAdd, Defer, FileDisable parsed but
                // unrouted at this slice; #546 adds their per-kind
                // fields to DirectiveExtracts and routes them here.
                //
                // Explicit arms, not a wildcard: the whole point of
                // the bundled-output collapse is for a new Directive
                // variant to fail compile until it has a route. A
                // wildcard would silently swallow new variants and
                // defeat the contract.
                Directive::Introduces { .. }
                | Directive::ScopeAdd { .. }
                | Directive::Defer { .. }
                | Directive::FileDisable { .. } => {}
            }
        }
        Ok(out)
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

    use mockspace_core::lint::{PropEntry, PropValue};

    #[test]
    fn extract_writes_allow_into_suppression_map() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "// lint:allow(no-bare-numeric) reason: \"const constant\" tracked: #427\nconst X: u64 = 1;\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        let scopes = extracts.suppressions.scopes();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].lints.contains("no-bare-numeric"));
        assert_eq!(scopes[0].reason.as_deref(), Some("const constant"));
        assert_eq!(scopes[0].tracked.as_deref(), Some("#427"));
    }

    #[test]
    fn extract_silently_drops_unrouted_directives() {
        // Introduces, ScopeAdd, Defer, FileDisable are parsed but not
        // routed at this slice; #546 wires them up. They must not
        // generate suppression or prop entries.
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
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        assert!(extracts.suppressions.scopes().is_empty());
        assert!(extracts.props.is_empty());
    }

    #[test]
    fn extract_routes_allow_and_prop_in_one_pass() {
        // Single parse drives both suppressions and props. Verifies
        // the bundled-output collapse.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:allow(no-bare-numeric) reason: "constants" tracked: #1
// lint:prop(audited)
// lint:prop(arena_size = 4096)
// lint:allow(no-bare-string) reason: "test fixture" tracked: #3
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        // Two allow scopes
        assert_eq!(extracts.suppressions.scopes().len(), 2);
        let lint_names: BTreeSet<&str> = extracts
            .suppressions
            .scopes()
            .iter()
            .flat_map(|s| s.lints.iter().map(|l| l.as_str()))
            .collect();
        assert!(lint_names.contains("no-bare-numeric"));
        assert!(lint_names.contains("no-bare-string"));
        // Two prop entries
        assert_eq!(extracts.props.len(), 2);
        let audited: Vec<PropEntry<'_>> = extracts.props.all_named("audited").collect();
        assert_eq!(audited.len(), 1);
        assert_eq!(audited[0].value, &PropValue::Bool(true));
        let arena: Vec<PropEntry<'_>> = extracts.props.all_named("arena_size").collect();
        assert_eq!(arena[0].value, &PropValue::Integer(4096));
    }

    #[test]
    fn extract_writes_prop_keyvalue_forms() {
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
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        assert_eq!(extracts.props.len(), 3);
        let arena: Vec<PropEntry<'_>> = extracts.props.all_named("arena_size").collect();
        assert_eq!(arena[0].value, &PropValue::Integer(4096));
        let id: Vec<PropEntry<'_>> = extracts.props.all_named("audit_id").collect();
        assert_eq!(id[0].value, &PropValue::String("A-2026-04".to_string()));
        let enabled: Vec<PropEntry<'_>> = extracts.props.all_named("enabled").collect();
        assert_eq!(enabled[0].value, &PropValue::Bool(false));
    }

    #[test]
    fn extract_prop_carries_optional_reason() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "// lint:prop(audited) reason: \"audit pass 2026-04\"\nfn x() {}\n"
                .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        let audited: Vec<PropEntry<'_>> = extracts.props.all_named("audited").collect();
        assert_eq!(audited[0].reason, Some("audit pass 2026-04"));
    }

    #[test]
    fn extract_accumulates_multiple_prop_directives() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:prop(allowed_import = "alloc")
// lint:prop(allowed_import = "core")
fn imports() {}
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        let imports: Vec<PropEntry<'_>> = extracts.props.all_named("allowed_import").collect();
        assert_eq!(imports.len(), 2);
        let strs: Vec<&PropValue> = imports.iter().map(|e| e.value).collect();
        assert!(strs.contains(&&PropValue::String("alloc".to_string())));
        assert!(strs.contains(&&PropValue::String("core".to_string())));
    }

    #[test]
    fn extract_on_source_without_directives_yields_empty_extracts() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "fn main() {}\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        assert!(extracts.suppressions.scopes().is_empty());
        assert!(extracts.props.is_empty());
    }
}
