//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a crate lint and a repo lint are handed: a view of the run's reading, and the values the
//! engine computed, each read through a method that writes down that it was read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tree_sitter::Tree;

use crate::reading::{TreeCell, View, empty_tree};
use crate::{Invocation, PROC_MACRO_CRATES, RegistryView};

/// One source file of a crate, borrowed from the run's reading.
#[derive(Clone, Copy)]
pub struct CrateSourceFile<'r> {
    /// Crate-relative path, `src/lib.rs` or `src/bits.rs`.
    pub rel_path: &'r Path,
    /// The file's text, held once for the run.
    pub text:     &'r str,
    tree:         TreeCell<'r>,
}

impl std::fmt::Debug for CrateSourceFile<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrateSourceFile")
            .field("rel_path", &self.rel_path)
            .field("len", &self.text.len())
            .finish()
    }
}

impl<'r> CrateSourceFile<'r> {
    pub(crate) fn new(rel_path: &'r Path, text: &'r str, tree: TreeCell<'r>) -> Self {
        Self {
            rel_path,
            text,
            tree,
        }
    }

    /// The file's syntax tree, parsed once for the run whoever asks first.
    #[must_use]
    pub fn tree(&self) -> &'r Tree {
        let text = self.text;
        self.tree.get(|| Some(text)).unwrap_or_else(|| empty_tree())
    }
}

/// A crate as the engine hands it over.
#[derive(Debug, Clone)]
pub struct CrateInput {
    /// Directory name of the crate, `<prefix>-signal`.
    pub name:       String,
    /// The name with the prefix taken off, `signal`.
    pub short_name: String,
    /// The crates this one depends on, by directory name.
    pub deps:       Vec<String>,
    /// The crate's directory, absolute.
    pub dir:        PathBuf,
}

/// What every crate's context shares.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceInput {
    /// Every crate directory name in the workspace.
    pub all_crates:              BTreeSet<String>,
    /// Root directory of the mock workspace.
    pub workspace_root:          PathBuf,
    /// Crates exempt from collection, box and float lints (proc-macro crates). Falls back to
    /// [`PROC_MACRO_CRATES`] where empty.
    pub proc_macro_crates:       Vec<String>,
    /// Whether source lints run against proc-macro crate source, `lint_proc_macro_source` in
    /// `mockspace.toml`.
    pub lint_proc_macro_source:  bool,
    /// The crate name prefix, `acme`.
    pub crate_prefix:            String,
    /// The `[primitive-introductions]` map: per crate, the primitive tokens it introduces.
    pub primitive_introductions: BTreeMap<String, Vec<String>>,
}

/// What a [`CrateLint`](crate::CrateLint) is handed for one crate, or one file of it.
///
/// `Copy`, and cheap: it is a view and a handful of references. The four public fields are part of
/// what a saved verdict is keyed on and are read freely. Everything else is read through a method,
/// which writes into the view's trace what was read, so a change to it reruns the lint and a change
/// to anything else does not.
#[derive(Clone, Copy)]
pub struct LintContext<'a> {
    /// Directory name of the crate, `<prefix>-signal`.
    pub crate_name:             &'a str,
    /// Short name, `signal`.
    pub short_name:             &'a str,
    /// Crate name prefix, `acme`.
    pub crate_prefix:           &'a str,
    /// Whether source lints run against proc-macro crate source.
    pub lint_proc_macro_source: bool,
    view:                       &'a View<'a>,
    krate:                      &'a CrateInput,
    ws:                         &'a WorkspaceInput,
    file:                       &'a Path,
    narrowed:                   Option<&'a [CrateSourceFile<'a>]>,
}

impl<'a> LintContext<'a> {
    pub(crate) fn new(view: &'a View<'a>, krate: &'a CrateInput, ws: &'a WorkspaceInput, file: &'a Path) -> Self {
        Self {
            crate_name: &krate.name,
            short_name: &krate.short_name,
            crate_prefix: &ws.crate_prefix,
            lint_proc_macro_source: ws.lint_proc_macro_source,
            view,
            krate,
            ws,
            file,
            narrowed: None,
        }
    }

    /// This context with `all_sources` narrowed to `files`, for a lint a path filter narrows.
    pub(crate) fn narrowed_to(self, files: &'a [CrateSourceFile<'a>]) -> Self {
        Self {
            narrowed: Some(files),
            ..self
        }
    }

    /// This unit's view of the reading, for anything the context has no method for.
    #[must_use]
    pub fn view(&self) -> &'a View<'a> {
        self.view
    }

    /// The crate's directory, absolute.
    #[must_use]
    pub fn crate_dir(&self) -> &'a Path {
        &self.krate.dir
    }

    /// Which file [`Self::source`] and [`Self::tree`] are: the one a per-file lint is judging, or
    /// `src/lib.rs` for a crate-scoped one. Crate-relative.
    #[must_use]
    pub fn source_path(&self) -> &'a Path {
        self.file
    }

    /// The text of [`Self::source_path`], empty where it cannot be read.
    #[must_use]
    pub fn source(&self) -> &'a str {
        self.view.text(self.krate.dir.join(self.file)).unwrap_or("")
    }

    /// The syntax tree of [`Self::source_path`], parsed once for the run.
    #[must_use]
    pub fn tree(&self) -> &'a Tree {
        self.view
            .rust(self.krate.dir.join(self.file))
            .unwrap_or_else(|| empty_tree())
    }

    /// Every `.rs` file under the crate's `src/`, `src/lib.rs` first and the rest in path order.
    /// Reading this reads every file, and a file appearing or going reruns the lint.
    #[must_use]
    pub fn all_sources(&self) -> &'a [CrateSourceFile<'a>] {
        let reading = self.view.reading();
        let found = self.view.crate_files(&self.krate.dir);
        if let Some(files) = self.narrowed {
            for f in files {
                self.view.text(self.krate.dir.join(f.rel_path));
            }
            return files;
        }
        for rel in &found.files {
            self.view.text(self.krate.dir.join(rel));
        }
        reading.crate_sources(&self.krate.dir)
    }

    /// The crate's `DESIGN.md.tmpl`, if it has one.
    #[must_use]
    pub fn design_doc(&self) -> Option<&'a str> {
        self.view.text(self.krate.dir.join("DESIGN.md.tmpl"))
    }

    /// Every doc template of the crate: `README.md.tmpl`, `DESIGN.md.tmpl`, then each
    /// `DEEPDIVE_*.md.tmpl` in name order.
    #[must_use]
    pub fn doc_templates(&self) -> Vec<&'a str> {
        let dir = &self.krate.dir;
        let mut out: Vec<&'a str> = ["README.md.tmpl", "DESIGN.md.tmpl"]
            .iter()
            .filter_map(|n| self.view.text(dir.join(n)))
            .collect();
        if let Some(listing) = self.view.list(dir) {
            for e in listing.entries() {
                let n = e.name();
                if e.is_file() && n.starts_with("DEEPDIVE_") && n.ends_with(".md.tmpl") {
                    out.extend(self.view.text(dir.join(n)));
                }
            }
        }
        out
    }

    /// The crate's `SHAME.md.tmpl`, but only where the directory holds an entry spelled exactly
    /// that. See [`crate::read_shame_template`].
    #[must_use]
    pub fn shame_doc(&self) -> Option<&'a str> {
        crate::read_shame_template(self.view, &self.krate.dir)
    }

    /// Names of the crates this crate depends on, by directory name.
    #[must_use]
    pub fn deps(&self) -> &'a [String] {
        self.view.record_value(&format!("deps:{}", self.krate.name));
        &self.krate.deps
    }

    /// Every crate directory name in the workspace.
    #[must_use]
    pub fn all_crates(&self) -> &'a BTreeSet<String> {
        self.view.record_value("all_crates");
        &self.ws.all_crates
    }

    /// The mock workspace's root directory.
    #[must_use]
    pub fn workspace_root(&self) -> &'a Path {
        self.view.record_value("workspace_root");
        &self.ws.workspace_root
    }

    /// The configured proc-macro crates, empty where the project names none.
    #[must_use]
    pub fn proc_macro_crates(&self) -> &'a [String] {
        self.view.record_value("proc_macro_crates");
        &self.ws.proc_macro_crates
    }

    /// The whole `[primitive-introductions]` map. A lint asking about this crate alone asks
    /// [`Self::introduces`], which is part of the key and writes nothing down.
    #[must_use]
    pub fn primitive_introductions(&self) -> &'a BTreeMap<String, Vec<String>> {
        self.view.record_value("primitive_introductions");
        &self.ws.primitive_introductions
    }

    /// Whether source-scanning lints should skip this crate: it is a proc-macro crate and the
    /// project has not opted into linting proc-macro source.
    #[must_use]
    pub fn should_skip_proc_macro_source_lint(&self) -> bool {
        !self.lint_proc_macro_source && self.is_proc_macro_crate()
    }

    /// Whether this crate is a proc-macro crate, without the project's preference.
    #[must_use]
    pub fn is_proc_macro_crate(&self) -> bool {
        is_proc_macro(self.ws, self.crate_name)
    }

    /// Whether this crate legitimately introduces the primitive token, per
    /// `[primitive-introductions]`.
    #[must_use]
    pub fn introduces(&self, primitive: &str) -> bool {
        self.ws
            .primitive_introductions
            .get(self.crate_name)
            .is_some_and(|list| list.iter().any(|p| p == primitive))
    }

    /// What a saved verdict over this crate is keyed on besides the lint and the file: the four
    /// public fields and the two answers about this crate that are read without writing anything.
    pub(crate) fn scalars(krate: &CrateInput, ws: &WorkspaceInput) -> String {
        let mut introduced: Vec<&str> = ws
            .primitive_introductions
            .get(&krate.name)
            .map(|l| l.iter().map(String::as_str).collect())
            .unwrap_or_default();
        introduced.sort_unstable();
        format!(
            "crate\0{}\0{}\0{}\0{}\0{}\0{}",
            krate.name,
            krate.short_name,
            ws.crate_prefix,
            is_proc_macro(ws, &krate.name),
            ws.lint_proc_macro_source,
            introduced.join(","),
        )
    }
}

fn is_proc_macro(ws: &WorkspaceInput, name: &str) -> bool {
    if ws.proc_macro_crates.is_empty() {
        PROC_MACRO_CRATES.contains(&name)
    } else {
        ws.proc_macro_crates.iter().any(|c| c == name)
    }
}

/// What the engine hands a repo lint, by reference.
#[derive(Clone, Copy)]
pub struct RepoInput<'a> {
    /// Root directory of the mock workspace, where `design_rounds/` lives.
    pub mock_dir:    &'a Path,
    /// Root of the repository containing the mock workspace.
    pub repo_root:   &'a Path,
    /// Every crate directory name in the workspace.
    pub all_crates:  &'a BTreeSet<String>,
    /// Every directory holding source packages, absolute, in config order.
    pub src_dirs:    &'a [PathBuf],
    /// The invocation that triggered this run, when there was one.
    pub invocation:  Option<Invocation<'a>>,
    /// The globs a project declares as its canon, from `canon_paths`.
    pub canon_paths: &'a [String],
    /// The slugs of every panel presently open.
    pub open_panels: &'a [String],
    /// The files of the mock directory that a data ref holds.
    pub ref_files:   &'a crate::src_layout::RefFiles,
    /// The project's registry, flattened, with the reverse edges computed.
    pub registry:    &'a RegistryView,
}

/// What a [`RepoLint`](crate::RepoLint) is handed: the paths, a view, and the engine's values.
///
/// `mock_dir` and `repo_root` are part of what a saved verdict is keyed on. Everything else is
/// read through a method that writes down that it was read.
#[derive(Clone, Copy)]
pub struct RepoContext<'a> {
    /// Root directory of the mock workspace, where `design_rounds/` lives.
    pub mock_dir:  &'a Path,
    /// Root of the repository containing the mock workspace.
    pub repo_root: &'a Path,
    view:          &'a View<'a>,
    input:         RepoInput<'a>,
    wanted:        bool,
}

impl<'a> RepoContext<'a> {
    pub(crate) fn new(view: &'a View<'a>, input: RepoInput<'a>, wanted: bool) -> Self {
        Self {
            mock_dir: input.mock_dir,
            repo_root: input.repo_root,
            view,
            input,
            wanted,
        }
    }

    /// This unit's view of the reading.
    #[must_use]
    pub fn view(&self) -> &'a View<'a> {
        self.view
    }

    /// Every crate directory name in the workspace. Empty is a legitimate state.
    #[must_use]
    pub fn all_crates(&self) -> &'a BTreeSet<String> {
        self.view.record_value("repo_all_crates");
        self.input.all_crates
    }

    /// Every directory holding source packages, absolute, in config order.
    #[must_use]
    pub fn src_dirs(&self) -> &'a [PathBuf] {
        self.view.record_value("src_dirs");
        self.input.src_dirs
    }

    /// The invocation that triggered this run, when there was one and the lint asked for it via
    /// [`Lint::invocation_wanted`](crate::Lint::invocation_wanted).
    #[must_use]
    pub fn invocation(&self) -> Option<Invocation<'a>> {
        if !self.wanted {
            self.view.record_value("invocation:unwanted");
            return None;
        }
        self.view.record_value("invocation");
        self.input.invocation
    }

    /// The globs a project declares as its canon. Empty where it has not said.
    #[must_use]
    pub fn canon_paths(&self) -> &'a [String] {
        self.view.record_value("canon_paths");
        self.input.canon_paths
    }

    /// The slugs of every panel presently open.
    #[must_use]
    pub fn open_panels(&self) -> &'a [String] {
        self.view.record_value("open_panels");
        self.input.open_panels
    }

    /// The files of the mock directory that a data ref holds.
    #[must_use]
    pub fn ref_files(&self) -> &'a crate::src_layout::RefFiles {
        self.view.record_value("ref_files");
        self.input.ref_files
    }

    /// The project's registry, flattened. Empty where the project declares none.
    #[must_use]
    pub fn registry(&self) -> &'a RegistryView {
        self.view.record_value("registry");
        self.input.registry
    }
}
