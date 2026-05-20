//! [`MockspaceDocument`] concrete type per schema design memo §7.
//!
//! Carries the source bytes plus lazy AST caches (`syn::File` for Rust,
//! `tree_sitter::Tree` for cross-language work) and a per-`StripOpts`
//! source-stripped cache. Lints walk the concrete type directly; the
//! mockspace-core [`mockspace_core::lint::Document`] trait impl carries
//! the engine-internal foundation surfaces.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mockspace_core::lint::{ContentHash, Document, Language};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::collections::HashMap;

pub use crate::strip::StripOpts;

/// Concrete document with cached AST views.
///
/// The `syn::File` cache populates on first call to [`Self::ast`]; failure
/// to parse yields `None` (the lint that needed it can emit a synthetic
/// `LintError::ParseFailure`). Similarly for `tree_sitter`. The
/// `source_stripped` cache holds `Arc<str>` views keyed by [`StripOpts`];
/// callers receive a cheap `Arc::clone`, not a fresh allocation.
pub struct MockspaceDocument {
    path: PathBuf,
    crate_name: String,
    language: Language,
    source: String,
    content_hash: ContentHash,

    syn_ast_cache: OnceCell<Option<syn::File>>,
    tree_sitter_cache: OnceCell<Option<tree_sitter::Tree>>,
    source_stripped_cache: RwLock<HashMap<StripOpts, Arc<str>>>,
}

impl std::fmt::Debug for MockspaceDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockspaceDocument")
            .field("path", &self.path)
            .field("crate_name", &self.crate_name)
            .field("language", &self.language)
            .field("source_len", &self.source.len())
            .field("content_hash", &self.content_hash)
            .finish()
    }
}

impl MockspaceDocument {
    /// Construct with placeholder zero content hash.
    pub fn new(
        path: impl Into<PathBuf>,
        crate_name: impl Into<String>,
        language: Language,
        source: impl Into<String>,
    ) -> Self {
        Self::with_hash(path, crate_name, language, source, ContentHash::ZERO)
    }

    pub fn with_hash(
        path: impl Into<PathBuf>,
        crate_name: impl Into<String>,
        language: Language,
        source: impl Into<String>,
        content_hash: ContentHash,
    ) -> Self {
        Self {
            path: path.into(),
            crate_name: crate_name.into(),
            language,
            source: source.into(),
            content_hash,
            syn_ast_cache: OnceCell::new(),
            tree_sitter_cache: OnceCell::new(),
            source_stripped_cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn crate_name(&self) -> &str {
        &self.crate_name
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }

    /// Lazily parse the source with `syn`. Returns `None` on parse failure.
    ///
    /// Only meaningful for [`Language::Rust`] documents. Non-Rust callers
    /// see `None`.
    pub fn ast(&self) -> Option<&syn::File> {
        if self.language != Language::Rust {
            return None;
        }
        self.syn_ast_cache
            .get_or_init(|| syn::parse_file(&self.source).ok())
            .as_ref()
    }

    /// Lazily parse with tree-sitter. Today wired for [`Language::Rust`]
    /// only; future languages plug in here as their grammars land.
    pub fn tree_sitter(&self) -> Option<&tree_sitter::Tree> {
        self.tree_sitter_cache
            .get_or_init(|| parse_with_tree_sitter(&self.source, self.language))
            .as_ref()
    }

    /// Return a stripped view per `opts`. Cached per `(document, opts)`
    /// pair. Callers receive `Arc<str>`; the cache retains the Arc, so
    /// repeated calls return the same allocation.
    pub fn source_stripped(&self, opts: StripOpts) -> Arc<str> {
        if let Some(arc) = self.source_stripped_cache.read().get(&opts) {
            return Arc::clone(arc);
        }
        let stripped: Arc<str> = Arc::from(crate::strip::strip(&self.source, opts));
        let mut write = self.source_stripped_cache.write();
        let arc = write.entry(opts).or_insert_with(|| Arc::clone(&stripped));
        Arc::clone(arc)
    }
}

impl Document for MockspaceDocument {
    fn path(&self) -> &Path {
        &self.path
    }
    fn language(&self) -> Language {
        self.language
    }
    fn source(&self) -> &str {
        &self.source
    }
    fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
}

fn parse_with_tree_sitter(source: &str, language: Language) -> Option<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    let grammar = match language {
        Language::Rust => tree_sitter_rust::language(),
        _ => return None,
    };
    parser.set_language(&grammar).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_parses_valid_rust() {
        let d = MockspaceDocument::new("a.rs", "my-crate", Language::Rust, "fn x() {}");
        let ast = d.ast().unwrap();
        assert_eq!(ast.items.len(), 1);
    }

    #[test]
    fn ast_returns_none_for_unparseable() {
        let d = MockspaceDocument::new("a.rs", "my-crate", Language::Rust, "fn x(");
        assert!(d.ast().is_none());
    }

    #[test]
    fn source_stripped_caches() {
        let d = MockspaceDocument::new("a.rs", "my-crate", Language::Rust, "let s = \"x\";");
        let opts = StripOpts {
            strings: true,
            ..Default::default()
        };
        let a = d.source_stripped(opts);
        let b = d.source_stripped(opts);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.as_ref(), "let s = \" \";");
    }

    #[test]
    fn tree_sitter_parses_rust() {
        let d = MockspaceDocument::new("a.rs", "my-crate", Language::Rust, "fn x() {}");
        let tree = d.tree_sitter().unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), "source_file");
    }
}
