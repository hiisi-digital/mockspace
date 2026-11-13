//! Building a [`LintContext`] outside the engine, so a lint can be tested.
//!
//! A [`CrateLint`](crate::CrateLint) takes a `LintContext`, which is a view of
//! a [`Reading`] plus the values the engine computed for the crate. A test has
//! no workspace to read, so [`LintFixture`] holds a reading whose every file is
//! given in memory, at a root that does not exist on disk, and hands out a
//! context over it:
//!
//! ```
//! use mockspace_lint_rules::testkit::LintFixture;
//!
//! let fixture = LintFixture::new("use std::fmt;\n");
//! let ctx = fixture.ctx();
//! assert_eq!(ctx.source(), "use std::fmt;\n");
//! ```
//!
//! The source is parsed by the reading, so a lint that reads `ctx.tree()` sees
//! the syntax tree of the text it was given rather than an empty one.
//!
//! # Why this is not behind a feature
//!
//! It compiles into every consumer's generated cdylib, including when that
//! cdylib is built to run the gate rather than to run tests, and nothing in the
//! dispatch path ever touches it. A `testkit` feature defaulting off would keep
//! it out of the gate build, and it cannot: the engine generates one crate with
//! one manifest and uses it for both, so `cargo mock test` and a pre-commit run
//! are the same artifact built twice.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::context::{CrateInput, RepoInput, WorkspaceInput};
use crate::reading::{Reading, View};
use crate::{CrateSourceFile, LintContext, LintError, RepoContext, RepoLint};

/// Where a fixture's files sit. Nothing is ever read from here on disk.
const ROOT: &str = "/nonexistent-lint-fixture";

/// What a context over the fixture borrows, built on first use.
///
/// Fields drop in order, so the sources and the view go before the reading
/// they borrow, and the reading is boxed so moving the fixture leaves it where
/// they point.
struct Built {
    sources: Vec<CrateSourceFile<'static>>,
    view:    View<'static>,
    reading: Owned,
}

/// The fixture's reading, on the heap and held by a pointer rather than a `Box`, so that moving it
/// asserts nothing about the references into it.
struct Owned(std::ptr::NonNull<Reading>);

impl Owned {
    fn get(&self) -> &Reading {
        // SAFETY: the pointer came from `Box::into_raw` and is freed only by `drop`.
        unsafe { self.0.as_ref() }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: made by `Box::into_raw` in `built`, and dropped once.
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

/// Owns everything a [`LintContext`] borrows, for use in a test.
///
/// Built with [`new`](Self::new) and narrowed with the `with_*` methods, each
/// of which takes and returns `self` so they chain. [`ctx`](Self::ctx) then
/// borrows the whole thing.
pub struct LintFixture {
    built: OnceLock<Built>,
    krate: CrateInput,
    ws:    WorkspaceInput,
    root:  String,
    files: Vec<(PathBuf, String)>,
    docs:  BTreeMap<String, String>,
}

impl LintFixture {
    /// A fixture over one crate root, with the source as `src/lib.rs`.
    ///
    /// Defaults: crate name `test-crate`, prefix `test`, no dependencies, no
    /// documents, workspace root `/nonexistent`, and `all_sources` holding the
    /// one file. A lint iterating `all_sources` and one reading `source`
    /// therefore see the same text, which is what the engine does for a crate
    /// whose only file is its root.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            built: OnceLock::new(),
            krate: CrateInput {
                name:       "test-crate".to_string(),
                short_name: "test-crate".to_string(),
                deps:       Vec::new(),
                dir:        Path::new(ROOT).join("test-crate"),
            },
            ws: WorkspaceInput {
                workspace_root: PathBuf::from("/nonexistent"),
                crate_prefix: "test".to_string(),
                ..WorkspaceInput::default()
            },
            root: source.to_string(),
            files: vec![(PathBuf::from("src/lib.rs"), source.to_string())],
            docs: BTreeMap::new(),
        }
    }

    fn change(mut self, f: impl FnOnce(&mut Self)) -> Self {
        self.built = OnceLock::new();
        f(&mut self);
        self
    }

    /// Name the crate, and its short name with the prefix stripped.
    #[must_use]
    pub fn with_crate_name(self, name: &str, short: &str) -> Self {
        self.change(|f| {
            f.krate.name = name.to_string();
            f.krate.short_name = short.to_string();
            f.krate.dir = Path::new(ROOT).join(name);
        })
    }

    /// Replace the files `all_sources` holds, each a crate-relative path and its text.
    ///
    /// The root stays whatever [`new`](Self::new) was given, so a lint that
    /// reads only `source` and one that walks `all_sources` disagree where the
    /// test sets them up to.
    #[must_use]
    pub fn with_sources(self, sources: &[(&str, &str)]) -> Self {
        self.change(|f| {
            f.files = sources
                .iter()
                .map(|(p, t)| (PathBuf::from(p), (*t).to_string()))
                .collect();
        })
    }

    /// Add one module file beside the root.
    #[must_use]
    pub fn with_module(self, rel_path: &str, text: &str) -> Self {
        self.change(|f| f.files.push((PathBuf::from(rel_path), text.to_string())))
    }

    /// Name the crates this one depends on, by directory name.
    #[must_use]
    pub fn with_deps(self, deps: &[&str]) -> Self {
        self.change(|f| f.krate.deps = deps.iter().map(|d| (*d).to_string()).collect())
    }

    /// Name every crate in the workspace, by directory name.
    #[must_use]
    pub fn with_all_crates(self, crates: &[&str]) -> Self {
        self.change(|f| f.ws.all_crates = crates.iter().map(|c| (*c).to_string()).collect())
    }

    /// Put a document in the crate's directory, `README.md.tmpl` or `DEEPDIVE_x.md.tmpl`.
    #[must_use]
    pub fn with_doc(self, name: &str, text: &str) -> Self {
        self.change(|f| {
            f.docs.insert(name.to_string(), text.to_string());
        })
    }

    /// Set the crate's `DESIGN.md.tmpl`.
    #[must_use]
    pub fn with_design_doc(self, doc: &str) -> Self {
        self.with_doc("DESIGN.md.tmpl", doc)
    }

    /// Set the crate's `SHAME.md.tmpl`.
    #[must_use]
    pub fn with_shame_doc(self, doc: &str) -> Self {
        self.with_doc("SHAME.md.tmpl", doc)
    }

    /// Set the crate prefix, as `crate_prefix` in `mockspace.toml`.
    #[must_use]
    pub fn with_crate_prefix(self, prefix: &str) -> Self {
        self.change(|f| f.ws.crate_prefix = prefix.to_string())
    }

    /// Mark crates as proc-macro crates, by directory name.
    #[must_use]
    pub fn with_proc_macro_crates(self, crates: &[&str]) -> Self {
        self.change(|f| f.ws.proc_macro_crates = crates.iter().map(|c| (*c).to_string()).collect())
    }

    /// Whether source lints run against proc-macro crate source.
    #[must_use]
    pub fn with_lint_proc_macro_source(self, lint: bool) -> Self {
        self.change(|f| f.ws.lint_proc_macro_source = lint)
    }

    /// Set the workspace root the lint resolves paths against.
    #[must_use]
    pub fn with_workspace_root(self, root: &Path) -> Self {
        self.change(|f| f.ws.workspace_root = root.to_path_buf())
    }

    /// Declare the primitives a crate legitimately introduces.
    #[must_use]
    pub fn with_primitive_introductions(self, introductions: BTreeMap<String, Vec<String>>) -> Self {
        self.change(|f| f.ws.primitive_introductions = introductions)
    }

    /// The reading the context is a view of, for a test that counts what it did.
    #[must_use]
    pub fn reading(&self) -> &Reading {
        self.built().reading.get()
    }

    /// The context, borrowing everything this fixture owns.
    #[must_use]
    pub fn ctx(&self) -> LintContext<'_> {
        let built = self.built();
        LintContext::new(&built.view, &self.krate, &self.ws, Path::new("src/lib.rs")).narrowed_to(&built.sources)
    }

    /// `reading` with this fixture's files laid over it. A context reads the root from `src/lib.rs`
    /// whatever `all_sources` holds, so `root` lays it there over any file of that name; a run
    /// walks the crate, so without `root` the crate is exactly its files, and the root only where
    /// there are none.
    fn overlay(&self, mut reading: Reading, root: bool) -> Reading {
        let dir = &self.krate.dir;
        for (rel, text) in &self.files {
            reading = reading.with_overlay(dir.join(rel), text.clone());
        }
        if root || self.files.is_empty() {
            reading = reading.with_overlay(dir.join("src/lib.rs"), self.root.clone());
        }
        for (name, text) in &self.docs {
            reading = reading.with_overlay(dir.join(name), text.clone());
        }
        reading
    }

    fn built(&self) -> &Built {
        self.built.get_or_init(|| {
            let dir = &self.krate.dir;
            let reading = self.overlay(Reading::new(ROOT), true);
            let reading = Owned(std::ptr::NonNull::from(Box::leak(Box::new(reading))));
            // SAFETY: every reference made `'static` here points into heap memory this fixture
            // owns and does not touch until `built` is reset: the reading behind `Owned`, and the buffers of
            // `files` and of the crate's directory. `change` resets `built` before mutating
            // anything, the fixture never hands out `&mut`, and `built` is declared first so it is
            // dropped before what it points into.
            let r: &'static Reading = unsafe { reading.0.as_ref() };
            let dir: &'static Path = unsafe { &*std::ptr::from_ref::<Path>(dir.as_path()) };
            let sources = self
                .files
                .iter()
                .map(|(rel, text)| {
                    let rel: &'static Path = unsafe { &*std::ptr::from_ref::<Path>(rel.as_path()) };
                    let text: &'static str = unsafe { &*std::ptr::from_ref::<str>(text.as_str()) };
                    CrateSourceFile::new(rel, text, crate::reading::TreeCell::of(r, &dir.join(rel)))
                })
                .collect();
            Built {
                sources,
                view: r.view(),
                reading,
            }
        })
    }
}

/// What `lint` finds over the repository `input` describes, judged through a view of a reading
/// rooted at its `repo_root`, as the runner judges it but with no store.
#[must_use]
pub fn check_repo(lint: &dyn RepoLint, input: RepoInput<'_>) -> Vec<LintError> {
    let reading = Reading::new(input.repo_root);
    let view = reading.view();
    let wanted = lint.invocation_wanted();
    let input = RepoInput {
        invocation: if wanted { input.invocation } else { None },
        ..input
    };
    lint.check_repo(&RepoContext::new(&view, input, wanted))
}

/// Every lint over the crates `fixtures` describe, through the runner, as one workspace of one
/// reading with no store: crate lints crate by crate in the order given, then workspace lints.
///
/// The crates' workspace values are the first fixture's, with every fixture's crate counted in
/// `all_crates`. A fixture's `all_sources` is what the reading finds under its `src/`, root
/// included, since the runner walks the crate as the engine does.
#[must_use]
pub fn run(fixtures: &[LintFixture], overrides: Option<&crate::LintConfig>, pack: Option<&crate::LintPack>) -> Vec<LintError> {
    let mut reading = Reading::new(ROOT);
    let mut crates = Vec::with_capacity(fixtures.len());
    let mut ws = fixtures.first().map(|f| f.ws.clone()).unwrap_or_default();
    for f in fixtures {
        reading = f.overlay(reading, false);
        ws.all_crates.insert(f.krate.name.clone());
        crates.push(f.krate.clone());
    }
    let input = crate::RunInput {
        reading: &reading,
        cache: None,
        overrides,
        doc_only: false,
        workspace: &ws,
        crates: &crates,
        scope: None,
        repo: None,
        only: None,
    };
    crate::run(&input, pack)
}

/// Every repo lint, built-in and from `pack`, over `input`, through the runner with no crates and
/// no store, the reading rooted at `input.repo_root`.
#[must_use]
pub fn run_repo(input: RepoInput<'_>, overrides: Option<&crate::LintConfig>, pack: Option<&crate::LintPack>) -> Vec<LintError> {
    let reading = Reading::new(input.repo_root);
    let ws = WorkspaceInput::default();
    let input = crate::RunInput {
        reading: &reading,
        cache: None,
        overrides,
        doc_only: false,
        workspace: &ws,
        crates: &[],
        scope: None,
        repo: Some(input),
        only: None,
    };
    crate::run(&input, pack)
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
        let root = fixture.ctx().tree().root_node();
        assert_eq!(root.kind(), "source_file");
        assert_eq!(root.child_count(), 2, "two items were given");
    }

    #[test]
    fn an_empty_source_parses_to_an_empty_root() {
        let fixture = LintFixture::new("");
        assert_eq!(fixture.ctx().tree().root_node().child_count(), 0);
    }

    /// A syntax error is a tree with an error in it, not a panic and
    /// not a `None`. A lint handed unparseable source has to be able
    /// to run, because the gate fires on trees the compiler has not
    /// accepted yet.
    #[test]
    fn unparseable_source_still_yields_a_tree() {
        let fixture = LintFixture::new("fn ( { ] unclosed");
        assert!(fixture.ctx().tree().root_node().has_error());
    }

    #[test]
    fn the_root_is_the_only_source_by_default() {
        let fixture = LintFixture::new("fn a() {}\n");
        let ctx = fixture.ctx();
        assert_eq!(ctx.all_sources().len(), 1);
        assert_eq!(ctx.all_sources()[0].rel_path, PathBuf::from("src/lib.rs"));
        assert_eq!(ctx.all_sources()[0].text, ctx.source());
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

        assert!(!ctx.source().contains("use std::"), "the root is untouched");
        assert_eq!(ctx.all_sources().len(), 2);
        assert!(
            ctx.all_sources().iter().any(|f| f.text.contains("use std::")),
            "the module carries it"
        );
    }

    #[test]
    fn with_sources_replaces_rather_than_appends() {
        let fixture = LintFixture::new("fn a() {}\n").with_sources(&[("src/only.rs", "fn b() {}\n")]);
        let ctx = fixture.ctx();
        assert_eq!(ctx.all_sources().len(), 1);
        assert_eq!(ctx.all_sources()[0].rel_path, PathBuf::from("src/only.rs"));
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
        assert!(bare.ctx().design_doc().is_none());
        assert!(bare.ctx().shame_doc().is_none());
        assert!(bare.ctx().doc_templates().is_empty());

        let filled = LintFixture::new("")
            .with_design_doc("# Design\n")
            .with_shame_doc("# Shame\n")
            .with_doc("README.md.tmpl", "# Readme\n");
        assert_eq!(filled.ctx().design_doc(), Some("# Design\n"));
        assert_eq!(filled.ctx().shame_doc(), Some("# Shame\n"));
        assert_eq!(filled.ctx().doc_templates(), ["# Readme\n", "# Design\n"], "readme, then design, and never shame");
    }

    #[test]
    fn deps_and_workspace_crates_are_separate_sets() {
        let fixture = LintFixture::new("")
            .with_deps(&["acme-core"])
            .with_all_crates(&["acme-core", "acme-other", "test-crate"]);
        let ctx = fixture.ctx();

        assert_eq!(ctx.deps(), ["acme-core"]);
        assert_eq!(ctx.all_crates().len(), 3);
        assert!(
            !ctx.all_crates().contains("acme-nonexistent"),
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
        assert_eq!(fixture.ctx().workspace_root(), Path::new("/tmp/somewhere"));
    }
}
