//! Per-language preprocessors that produce [`SuppressionMap`] entries.
//!
//! Each preprocessor reads source bytes for one [`Language`] and emits
//! [`SuppressionScope`]s into a project-level [`SuppressionMap`]. The
//! engine merges per-document maps before resolving findings.
//!
//! # Today
//!
//! [`RustPreprocessor::extract`] calls [`comment::parse_directives`]
//! over the document's source and routes each directive variant into
//! the right field of a [`DirectiveExtracts`] bundle: `Allow` and
//! `Defer` become [`SuppressionScope`] entries (distinguished by
//! [`SuppressionKind`]); `ScopeAdd` becomes a [`ScopeAddEntry`];
//! `FileDisable` becomes a [`FileDisableEntry`]; `Prop` becomes a
//! [`PropMap`] entry. The parser implements the canonical directive
//! vocabulary per the design memo at
//! `mock/research/202605220000_canonical-directive-vocabulary.md`.

pub mod comment;
pub mod rust_attr;

use mockspace_core::lint::{
    Directive, DirectiveRecord, Document, FileDisableEntry, FileDisableSet, Language, PropMap,
    ScopeAddEntry, ScopeAddMap, SuppressionKind, SuppressionMap, SuppressionScope,
};
use std::collections::BTreeSet;
use std::path::Path;

/// Per-document directive extracts. Returned from
/// [`LanguagePreprocessor::extract`]; engines merge the per-document
/// bundles into a project-level bundle before resolving findings.
///
/// One bundled output: a single parse over the document populates
/// every per-kind map and the trait surface stays a single method.
/// Each new [`Directive`] variant grows a field on this struct rather
/// than a method on [`LanguagePreprocessor`].
#[derive(Debug, Default)]
pub struct DirectiveExtracts {
    /// Every parsed [`DirectiveRecord`] from this document, preserving
    /// `source_form` per record. Cross-cutting lints (notably
    /// `directive-style-consistency` per the design memo) read this
    /// to compare directives against a project-wide style policy.
    /// Each record is also routed into the appropriate per-kind field
    /// below; this field is the raw inventory.
    pub records: Vec<DirectiveRecord>,
    /// `lint:allow` and `lint:defer` records routed into
    /// [`SuppressionMap`]. Allows carry [`SuppressionKind::Allow`];
    /// defers carry [`SuppressionKind::Defer`] with the `until: <task>`
    /// argument stored in `tracked`.
    pub suppressions: SuppressionMap,
    /// `lint:prop` records routed into [`PropMap`].
    pub props: PropMap,
    /// `lint:scope-add` records routed into [`ScopeAddMap`].
    pub scope_adds: ScopeAddMap,
    /// `lint:file-disable` records routed into [`FileDisableSet`].
    pub file_disables: FileDisableSet,
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

    fn extract(&self, document: &dyn Document) -> Result<DirectiveExtracts, PreprocessorError>;
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
/// [`comment::parse_directives`], and routes each variant into its
/// per-kind field on [`DirectiveExtracts`]:
///
/// - `Allow` and `Defer` records → [`SuppressionMap`] (distinguished
///   by [`SuppressionKind`]; `Defer` carries the `until: <task>`
///   argument as `tracked`).
/// - `ScopeAdd` records → [`ScopeAddMap`].
/// - `FileDisable` records → [`FileDisableSet`].
/// - `Prop` records → [`PropMap`].
#[derive(Debug, Default)]
pub struct RustPreprocessor;

impl LanguagePreprocessor for RustPreprocessor {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extract(&self, document: &dyn Document) -> Result<DirectiveExtracts, PreprocessorError> {
        let path_str = document.path().to_string_lossy();
        let mut out = DirectiveExtracts::default();
        // The directive path inside the document is the file the
        // record lives in. File-disable entries key off this for
        // per-file lookup.
        let doc_path = document.path().to_path_buf();

        // Pass 1: comment-form directives. The canonical surface; works
        // even when the source does not parse as valid Rust.
        for record in comment::parse_directives(document.source(), &path_str) {
            route_record(record, &doc_path, &mut out);
        }

        // Pass 2: native attribute-form directives. Requires the source
        // to be a parseable `syn::File`. Parse failures here are
        // recoverable: comment-form directives have already been routed,
        // and lints for `Language::Rust` still see what the comment
        // parser found. The native parser also handles `#[mockspace::*]`
        // attributes that no comment-form pre-image would catch.
        //
        // NOTE: dedup across the two passes is deliberately not done
        // here. A consumer who writes both a comment-form and an
        // attribute-form directive at the same site produces two
        // SuppressionScopes; the `directive-style-consistency` lint
        // (#548) is the right home to flag the mixed-style case.
        // Erasing the signal here would defeat that lint.
        //
        // FOLLOWUP: `Document` does not expose the cached `syn::File`,
        // so this re-parses what `MockspaceDocument` already cached. A
        // Rust-specific subtrait or typed downcast on the concrete
        // `MockspaceDocument` is the right shape; extending `Document`
        // would leak Rust-specific state onto a language-agnostic
        // trait.
        if let Ok(ast) = syn::parse_file(document.source()) {
            for record in rust_attr::parse_directive_attributes(&ast, &path_str) {
                route_record(record, &doc_path, &mut out);
            }
        }

        Ok(out)
    }
}

/// Route a single [`DirectiveRecord`] into the matching field of
/// [`DirectiveExtracts`]. Shared by the comment-form and attribute-form
/// passes inside [`RustPreprocessor::extract`].
fn route_record(record: DirectiveRecord, doc_path: &Path, out: &mut DirectiveExtracts) {
    // Preserve the raw record (with its `source_form`) for
    // cross-cutting lints. The destructured per-kind copies below
    // lose `source_form`; this field is the inventory the
    // directive-style-consistency lint reads.
    out.records.push(record.clone());

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
                kind: SuppressionKind::Allow,
                tracked,
                reason,
            });
        }
        Directive::Defer {
            lint_name,
            until,
            reason,
        } => {
            // Defer suppresses the named lint within the same scope as
            // the directive. The `until: <task-id>` argument fills
            // `tracked`; the meta-lint reads `kind == Defer` to apply
            // expiration semantics distinct from `Allow`.
            let mut lints = BTreeSet::new();
            lints.insert(lint_name);
            out.suppressions.push(SuppressionScope {
                scope: record.span,
                lints,
                kind: SuppressionKind::Defer,
                tracked: Some(until),
                reason,
            });
        }
        Directive::ScopeAdd {
            lint_name,
            axis,
            value,
        } => {
            out.scope_adds.push(ScopeAddEntry {
                scope: record.span,
                lint_name,
                axis,
                value,
            });
        }
        Directive::FileDisable {
            lint_name,
            reason,
            tracked,
        } => {
            out.file_disables.push(FileDisableEntry {
                file: doc_path.to_path_buf(),
                lint_name,
                directive_span: record.span,
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
    fn extract_routes_scope_add_defer_file_disable() {
        // Per #546: every Directive variant routes into its per-kind
        // map. The bundled-output collapse means a new variant fails
        // compile in the preprocessor `match` until it has a route,
        // and the per-kind fields surface the resolved state to the
        // engine.
        use mockspace_core::lint::{ScopeAxis, SuppressionKind};

        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r#"// lint:scope-add(no-bare-numeric, exempt_paths="tests/**")
// lint:defer(no-bare-string, until: #185)
// lint:file-disable(writing-style) reason: "generated" tracked: #207
"#
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();

        // ScopeAdd → ScopeAddMap.
        let adds = extracts.scope_adds.entries();
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0].lint_name, "no-bare-numeric");
        assert_eq!(adds[0].axis, ScopeAxis::ExemptPaths);
        assert_eq!(adds[0].value, "tests/**");

        // Defer → SuppressionMap with SuppressionKind::Defer.
        let scopes = extracts.suppressions.scopes();
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].kind, SuppressionKind::Defer);
        assert!(scopes[0].lints.contains("no-bare-string"));
        assert_eq!(scopes[0].tracked.as_deref(), Some("#185"));

        // FileDisable → FileDisableSet.
        assert!(extracts
            .file_disables
            .disabled(std::path::Path::new("lib.rs"), "writing-style"));
        let file_entries = extracts.file_disables.entries();
        assert_eq!(file_entries.len(), 1);
        assert_eq!(file_entries[0].reason.as_deref(), Some("generated"));
        assert_eq!(file_entries[0].tracked.as_deref(), Some("#207"));

        // Props untouched.
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
            source: "// lint:prop(audited) reason: \"audit pass 2026-04\"\nfn x() {}\n".to_string(),
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

    // ---- Attribute-form integration (#545) ----

    #[test]
    fn extract_picks_up_attribute_form_allow() {
        // Pure attribute form: no comment-form directive in the source.
        // The native parser pass picks this up and routes it into the
        // suppression map alongside what comment-form directives would
        // produce.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r##"
#[mockspace::allow("no-bare-numeric", reason = "fixture", tracked = "#427")]
const X: u64 = 1;
"##
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        let scopes = extracts.suppressions.scopes();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].lints.contains("no-bare-numeric"));
        assert_eq!(scopes[0].reason.as_deref(), Some("fixture"));
        assert_eq!(scopes[0].tracked.as_deref(), Some("#427"));
    }

    #[test]
    fn extract_merges_comment_and_attribute_forms() {
        // Both surfaces produce records in one extract call. The engine
        // does not care which surface a directive came from.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r##"// lint:allow(no-bare-string) reason: "comment-form" tracked: #3

#[mockspace::allow("no-bare-numeric", reason = "attr-form", tracked = "#4")]
const X: u64 = 1;
"##
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        let scopes = extracts.suppressions.scopes();
        assert_eq!(scopes.len(), 2);
        let names: BTreeSet<&str> = scopes
            .iter()
            .flat_map(|s| s.lints.iter().map(|l| l.as_str()))
            .collect();
        assert!(names.contains("no-bare-numeric"));
        assert!(names.contains("no-bare-string"));
    }

    #[test]
    fn extract_routes_attribute_form_scope_add_and_file_disable() {
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r##"
#[mockspace::scope_add("no-bare-numeric", axis = "exempt_paths", value = "tests/**")]
mod ffi {}

#[mockspace::file_disable("writing-style", reason = "generated", tracked = "#207")]
fn generated_thing() {}
"##
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        assert_eq!(extracts.scope_adds.entries().len(), 1);
        assert_eq!(
            extracts.scope_adds.entries()[0].lint_name,
            "no-bare-numeric"
        );
        assert_eq!(extracts.file_disables.entries().len(), 1);
        assert!(extracts
            .file_disables
            .disabled(std::path::Path::new("lib.rs"), "writing-style",));
    }

    #[test]
    fn extract_prop_attribute_is_dropped_today() {
        // Documents the current gap noted by the rust_attr.rs FOLLOWUP
        // comment: `prop` is not yet recognised by the attribute parser
        // (the AttrArg parser only handles string literals, but prop
        // values can be Bool / Integer / String). Consumers must use
        // the comment form `// lint:prop(audited)` until the gap is
        // closed. This test locks the silent-drop so a future patch
        // that fixes the gap also updates this regression record.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: r##"
#[mockspace::prop("audited")]
fn x() {}
"##
            .to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        assert!(
            extracts.props.is_empty(),
            "attribute-form prop is not yet routed"
        );
    }

    #[test]
    fn extract_tolerates_unparseable_rust_falls_back_to_comments() {
        // Source is not valid Rust but the comment-form parser is
        // syntax-agnostic. The attribute parse fails silently; the
        // comment directive still lands.
        let doc = StubDocument {
            path: "lib.rs".into(),
            source: "// lint:allow(no-bare-numeric) reason: \"x\" tracked: #1\n@@@ this is not rust @@@\n".to_string(),
            hash: ContentHash::ZERO,
        };
        let extracts = RustPreprocessor.extract(&doc).unwrap();
        let scopes = extracts.suppressions.scopes();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].lints.contains("no-bare-numeric"));
    }
}
