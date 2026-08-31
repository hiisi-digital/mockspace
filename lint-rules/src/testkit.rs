//! Building a [`LintContext`] outside the engine, so a lint can be tested.
//!
//! A [`CrateLint`](crate::CrateLint) takes a `LintContext`, and that
//! context holds a `&tree_sitter::Tree`. The engine builds one per crate
//! while it walks the workspace. A test has no workspace to walk, and
//! the generated cdylib a repo's own `mock/lints/*.rs` compile into
//! depends on this crate and on the declared packs, so a consumer had
//! no route to a `Tree` at all. Source-side lints were untestable
//! everywhere as a consequence, which is why the repos that carry them
//! have tests on their `RepoLint`s and none on their `CrateLint`s.
//!
//! [`LintFixture`] owns the parsed tree and every collection the
//! context borrows, and hands out a context that borrows from it:
//!
//! ```
//! use mockspace_lint_rules::testkit::LintFixture;
//!
//! let fixture = LintFixture::new("use std::fmt;\n");
//! let ctx = fixture.ctx();
//! assert_eq!(ctx.source, "use std::fmt;\n");
//! ```
//!
//! The source is parsed, so a lint that reads `ctx.tree` sees the AST
//! of the text it was given rather than an empty one.
//!
//! # Why this is not behind a feature
//!
//! It compiles into every consumer's generated cdylib, including when that
//! cdylib is built to run the gate rather than to run tests, and nothing in the
//! dispatch path ever touches it. A `testkit` feature defaulting off would keep
//! it out of the gate build, and it cannot: the engine generates **one** crate
//! with one manifest and uses it for both, so `cargo mock test` and a pre-commit
//! run are the same artifact built twice. Defaulting the feature off would hide
//! the fixture from exactly the tests it exists for, and defaulting it on would
//! ship it anyway.
//!
//! Separating them means the generator writing a different feature list per
//! entry point, which is an engine change out of proportion to what is being
//! avoided: the cdylib is a local build artifact, rebuilt per repository, and
//! never distributed anywhere.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{CrateSourceFile, LintContext};

/// Owns everything a [`LintContext`] borrows, for use in a test.
///
/// Built with [`new`](Self::new) and narrowed with the `with_*`
/// methods, each of which takes and returns `self` so they chain.
/// [`ctx`](Self::ctx) then borrows the whole thing.
pub struct LintFixture {
    crate_name:              String,
    short_name:              String,
    source:                  String,
    tree:                    tree_sitter::Tree,
    all_sources:             Vec<CrateSourceFile>,
    deps:                    Vec<String>,
    all_crates:              BTreeSet<String>,
    design_doc:              Option<String>,
    all_doc_content:         String,
    shame_doc:               Option<String>,
    workspace_root:          PathBuf,
    proc_macro_crates:       Vec<String>,
    lint_proc_macro_source:  bool,
    crate_prefix:            String,
    primitive_introductions: BTreeMap<String, Vec<String>>,
}

impl LintFixture {
    /// A fixture over one crate root, with the source parsed.
    ///
    /// Defaults: crate name `test-crate`, prefix `test`, no
    /// dependencies, no documents, workspace root `/nonexistent`, and
    /// `all_sources` holding the one file as `src/lib.rs`. A lint
    /// iterating `all_sources` and one reading `source` therefore see
    /// the same text, which is what the engine does for a crate whose
    /// only file is its root.
    ///
    /// # Panics
    ///
    /// If the Rust grammar fails to load, which cannot happen with the
    /// grammar this crate is built against.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("the bundled Rust grammar loads");
        let tree = parser
            .parse(source, None)
            .expect("tree-sitter returns a tree for any input");

        Self {
            crate_name: "test-crate".to_string(),
            short_name: "test-crate".to_string(),
            source: source.to_string(),
            tree,
            all_sources: vec![CrateSourceFile {
                rel_path: PathBuf::from("src/lib.rs"),
                text:     source.to_string(),
            }],
            deps: Vec::new(),
            all_crates: BTreeSet::new(),
            design_doc: None,
            all_doc_content: String::new(),
            shame_doc: None,
            workspace_root: PathBuf::from("/nonexistent"),
            proc_macro_crates: Vec::new(),
            lint_proc_macro_source: false,
            crate_prefix: "test".to_string(),
            primitive_introductions: BTreeMap::new(),
        }
    }

    /// Name the crate, and its short name with the prefix stripped.
    #[must_use]
    pub fn with_crate_name(mut self, name: &str, short: &str) -> Self {
        self.crate_name = name.to_string();
        self.short_name = short.to_string();
        self
    }

    /// Replace the module files under the crate.
    ///
    /// The root stays whatever [`new`](Self::new) was given, so a lint
    /// that reads only `source` and one that walks `all_sources`
    /// disagree exactly where the engine would have them disagree.
    /// That difference is the point: it is what a test for the
    /// root-only blindness looks like.
    #[must_use]
    pub fn with_sources(mut self, sources: Vec<CrateSourceFile>) -> Self {
        self.all_sources = sources;
        self
    }

    /// Add one module file beside the root.
    #[must_use]
    pub fn with_module(mut self, rel_path: &str, text: &str) -> Self {
        self.all_sources.push(CrateSourceFile {
            rel_path: PathBuf::from(rel_path),
            text:     text.to_string(),
        });
        self
    }

    /// Name the crates this one depends on, by directory name.
    #[must_use]
    pub fn with_deps(mut self, deps: &[&str]) -> Self {
        self.deps = deps.iter().map(|d| (*d).to_string()).collect();
        self
    }

    /// Name every crate in the workspace, by directory name.
    #[must_use]
    pub fn with_all_crates(mut self, crates: &[&str]) -> Self {
        self.all_crates = crates.iter().map(|c| (*c).to_string()).collect();
        self
    }

    /// Set the crate's `DESIGN.md.tmpl` content.
    #[must_use]
    pub fn with_design_doc(mut self, doc: &str) -> Self {
        self.design_doc = Some(doc.to_string());
        self
    }

    /// Set the concatenation of every doc template for the crate.
    #[must_use]
    pub fn with_doc_content(mut self, doc: &str) -> Self {
        self.all_doc_content = doc.to_string();
        self
    }

    /// Set the crate's `SHAME.md.tmpl` content.
    #[must_use]
    pub fn with_shame_doc(mut self, doc: &str) -> Self {
        self.shame_doc = Some(doc.to_string());
        self
    }

    /// Set the crate prefix, as `crate_prefix` in `mockspace.toml`.
    #[must_use]
    pub fn with_crate_prefix(mut self, prefix: &str) -> Self {
        self.crate_prefix = prefix.to_string();
        self
    }

    /// Mark crates as proc-macro crates, by directory name.
    #[must_use]
    pub fn with_proc_macro_crates(mut self, crates: &[&str]) -> Self {
        self.proc_macro_crates = crates.iter().map(|c| (*c).to_string()).collect();
        self
    }

    /// Whether source lints run against proc-macro crate source.
    #[must_use]
    pub fn with_lint_proc_macro_source(mut self, lint: bool) -> Self {
        self.lint_proc_macro_source = lint;
        self
    }

    /// Set the workspace root the lint resolves paths against.
    #[must_use]
    pub fn with_workspace_root(mut self, root: &Path) -> Self {
        self.workspace_root = root.to_path_buf();
        self
    }

    /// Declare the primitives a crate legitimately introduces.
    #[must_use]
    pub fn with_primitive_introductions(
        mut self,
        introductions: BTreeMap<String, Vec<String>>,
    ) -> Self {
        self.primitive_introductions = introductions;
        self
    }

    /// The context, borrowing everything this fixture owns.
    #[must_use]
    pub fn ctx(&self) -> LintContext<'_> {
        LintContext {
            crate_name:              &self.crate_name,
            short_name:              &self.short_name,
            source:                  &self.source,
            tree:                    &self.tree,
            all_sources:             &self.all_sources,
            deps:                    &self.deps,
            all_crates:              &self.all_crates,
            design_doc:              self.design_doc.as_deref(),
            all_doc_content:         &self.all_doc_content,
            shame_doc:               self.shame_doc.as_deref(),
            workspace_root:          &self.workspace_root,
            proc_macro_crates:       &self.proc_macro_crates,
            lint_proc_macro_source:  self.lint_proc_macro_source,
            crate_prefix:            &self.crate_prefix,
            primitive_introductions: &self.primitive_introductions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree is a parse of the source it was given.
    ///
    /// Worth pinning because the parse is the part a caller cannot see
    /// is missing. `file_size` builds its context by parsing the empty
    /// string and gets away with it, since it counts lines and never
    /// reads the tree; a lint that does read the tree, built the same
    /// way, would pass for having been handed an empty root rather
    /// than for being right. The negative control is below it: an
    /// empty source really does produce an empty root, so a non-empty
    /// root here is a fact about the parse and not about the grammar
    /// always saying yes.
    #[test]
    fn the_tree_is_a_parse_of_the_source() {
        let fixture = LintFixture::new("fn a() {}\nfn b() {}\n");
        let root = fixture.ctx().tree.root_node();
        assert_eq!(root.kind(), "source_file");
        assert_eq!(root.child_count(), 2, "two items were given");
    }

    #[test]
    fn an_empty_source_parses_to_an_empty_root() {
        let fixture = LintFixture::new("");
        assert_eq!(fixture.ctx().tree.root_node().child_count(), 0);
    }

    /// A syntax error is a tree with an error in it, not a panic and
    /// not a `None`. A lint handed unparseable source has to be able
    /// to run, because the gate fires on trees the compiler has not
    /// accepted yet.
    #[test]
    fn unparseable_source_still_yields_a_tree() {
        let fixture = LintFixture::new("fn ( { ] unclosed");
        assert!(fixture.ctx().tree.root_node().has_error());
    }

    #[test]
    fn the_root_is_the_only_source_by_default() {
        let fixture = LintFixture::new("fn a() {}\n");
        let ctx = fixture.ctx();
        assert_eq!(ctx.all_sources.len(), 1);
        assert_eq!(ctx.all_sources[0].rel_path, PathBuf::from("src/lib.rs"));
        assert_eq!(ctx.all_sources[0].text, ctx.source);
    }

    /// A module added beside the root leaves the root's text alone, so
    /// a lint reading `source` and a lint walking `all_sources` see
    /// different things. That difference is what a test for root-only
    /// blindness asserts against, so the fixture has to preserve it
    /// rather than fold the module into the root.
    #[test]
    fn a_module_is_visible_only_through_all_sources() {
        let fixture =
            LintFixture::new("pub mod inner;\n").with_module("src/inner.rs", "use std::fmt;\n");
        let ctx = fixture.ctx();

        assert!(!ctx.source.contains("use std::"), "the root is untouched");
        assert_eq!(ctx.all_sources.len(), 2);
        assert!(
            ctx.all_sources.iter().any(|f| f.text.contains("use std::")),
            "the module carries it"
        );
    }

    #[test]
    fn with_sources_replaces_rather_than_appends() {
        let fixture = LintFixture::new("fn a() {}\n").with_sources(vec![CrateSourceFile {
            rel_path: PathBuf::from("src/only.rs"),
            text:     "fn b() {}\n".to_string(),
        }]);
        let ctx = fixture.ctx();
        assert_eq!(ctx.all_sources.len(), 1);
        assert_eq!(ctx.all_sources[0].rel_path, PathBuf::from("src/only.rs"));
    }

    /// With no `proc_macro_crates` set the context falls back to a
    /// built-in list, so a fixture that names none is not the same as
    /// one that names an empty set. Both arms are asserted because a
    /// lint branching on this is branching on the fallback too.
    #[test]
    fn proc_macro_membership_follows_the_configured_set() {
        let named = LintFixture::new("").with_crate_name("acme-macros", "macros");
        assert!(
            !named.ctx().is_proc_macro_crate(),
            "not in the fallback list"
        );

        let declared = named.with_proc_macro_crates(&["acme-macros"]);
        assert!(declared.ctx().is_proc_macro_crate());
    }

    #[test]
    fn the_skip_decision_consults_the_preference_and_membership_both() {
        let base = LintFixture::new("").with_crate_name("acme-macros", "macros");

        let skip = base.with_proc_macro_crates(&["acme-macros"]);
        assert!(skip.ctx().should_skip_proc_macro_source_lint());

        let opted_in = skip.with_lint_proc_macro_source(true);
        assert!(
            !opted_in.ctx().should_skip_proc_macro_source_lint(),
            "opting in overrides membership"
        );

        let not_a_macro_crate = LintFixture::new("")
            .with_crate_name("acme-core", "core")
            .with_proc_macro_crates(&["acme-macros"]);
        assert!(!not_a_macro_crate.ctx().should_skip_proc_macro_source_lint());
    }

    #[test]
    fn introductions_are_keyed_on_the_crate_name() {
        let mut map = BTreeMap::new();
        map.insert("acme-numerics".to_string(), vec!["u8".to_string()]);

        let named = LintFixture::new("")
            .with_crate_name("acme-numerics", "numerics")
            .with_primitive_introductions(map.clone());
        assert!(named.ctx().introduces("u8"));
        assert!(!named.ctx().introduces("u16"), "only what is listed");

        let other = LintFixture::new("")
            .with_crate_name("acme-other", "other")
            .with_primitive_introductions(map);
        assert!(
            !other.ctx().introduces("u8"),
            "another crate's entry does not carry"
        );
    }

    #[test]
    fn the_documents_are_absent_until_set() {
        let bare = LintFixture::new("");
        assert!(bare.ctx().design_doc.is_none());
        assert!(bare.ctx().shame_doc.is_none());
        assert_eq!(bare.ctx().all_doc_content, "");

        let filled = LintFixture::new("")
            .with_design_doc("# Design\n")
            .with_shame_doc("# Shame\n")
            .with_doc_content("# Design\n# Shame\n");
        assert_eq!(filled.ctx().design_doc, Some("# Design\n"));
        assert_eq!(filled.ctx().shame_doc, Some("# Shame\n"));
        assert!(filled.ctx().all_doc_content.contains("Shame"));
    }

    #[test]
    fn deps_and_workspace_crates_are_separate_sets() {
        let fixture = LintFixture::new("")
            .with_deps(&["acme-core"])
            .with_all_crates(&["acme-core", "acme-other", "test-crate"]);
        let ctx = fixture.ctx();

        assert_eq!(ctx.deps, ["acme-core"]);
        assert_eq!(ctx.all_crates.len(), 3);
        assert!(
            !ctx.all_crates.contains("acme-nonexistent"),
            "membership is what was named"
        );
    }

    #[test]
    fn the_context_reflects_a_later_setter() {
        let fixture = LintFixture::new("").with_crate_prefix("acme");
        assert_eq!(fixture.ctx().crate_prefix, "acme");

        let renamed = fixture.with_crate_prefix("other");
        assert_eq!(
            renamed.ctx().crate_prefix,
            "other",
            "the last setter wins rather than the first"
        );
    }

    #[test]
    fn the_workspace_root_is_what_was_set() {
        let fixture = LintFixture::new("").with_workspace_root(Path::new("/tmp/somewhere"));
        assert_eq!(fixture.ctx().workspace_root, Path::new("/tmp/somewhere"));
    }
}
