//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A lint reads through its view, and nothing else.
//!
//! A unit is replayed when every question in its trace still has the answer it had, and the trace
//! holds only what the lint asked its [`View`]. A lint that opens a file with `std::fs`, starts a
//! command with `std::process` or walks a directory with a walker of its own reads something no
//! trace holds, so a change to it replays the old findings. Nothing reports that; the finding is
//! just stale.
//!
//! This module refuses that shape in lint source, in two places: [`assert_lints_read_through_the_view`],
//! which a pack calls from its own tests over its source directory, and the repo lint
//! `lints-read-through-the-view`, which reads every `.rs` file under `<mock>/lints/`.
//!
//! What it refuses, outside test code:
//!
//! - a path through `std::fs` or `std::process`, in a `use` or in an expression;
//! - a directory walker: the crates `walkdir`, `ignore`, `glob` and `jwalk`, and `read_dir`;
//! - a call reading the file system through a path with no argument: `.exists()`, `.metadata()`,
//!   `.symlink_metadata()`, `.canonicalize()`, `.try_exists()` and `.read_link()`. The view's own
//!   `exists` takes the path as an argument, so only the `Path` form is caught. `.is_file()` and
//!   `.is_dir()` are not refused: an [`Entry`] of a listing answers both with no argument, and
//!   without types the two cannot be told apart.
//!
//! Test code is a `#[cfg(test)]` module, a file opening with `#![cfg(test)]`, a file under a
//! `tests` directory, and a file named `tests.rs` or ending in `_tests.rs`, which is where a
//! `#[cfg(test)] mod tests;` keeps its body. Tests build their fixtures on disk and may.
//!
//! A lint that truly has to read outside the view says so with [`View::untraced`], which this
//! does not look at, since that is the opt out the trace records.
//!
//! Severity: Error (blocks commit, push, and build).

use std::fmt;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Tree};

use crate::reading::{Entry, Reading};
use crate::{Lint, LintError, RepoContext, RepoLint, Severity, View, is_cfg_test_mod};

const LINT_NAME: &str = "lints-read-through-the-view";

/// Paths a lint may not reach, by their leading segments.
const FORBIDDEN_PATHS: &[&str] = &["std::fs", "std::process"];

/// Crates that walk a directory.
const WALKER_CRATES: &[&str] = &["walkdir", "ignore", "glob", "jwalk"];

/// `Path` methods that read the file system and take no argument.
const PATH_READS: &[&str] = &[
    "exists",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "try_exists",
    "read_link",
];

/// One place lint source reads around its view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bypass {
    /// The file, as the caller named it.
    pub path: PathBuf,
    /// One-based line.
    pub line: usize,
    /// What it reaches: `std::fs`, `std::process`, `read_dir`, a walker crate, or `.<method>()`.
    pub what: String,
}

impl fmt::Display for Bypass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: reaches `{}`", self.path.display(), self.line, self.what)
    }
}

/// Every place `source` reads around the view, outside test code, in source order. `tree` is the
/// parse of `source`; `path` is only carried into each [`Bypass`].
#[must_use]
pub fn bypasses(path: &Path, source: &str, tree: &Tree) -> Vec<Bypass> {
    let root = tree.root_node();
    if opens_with_cfg_test(root, source) {
        return Vec::new();
    }
    let mut found = Vec::new();
    visit(root, source, &mut |node, what| {
        found.push(Bypass {
            path: path.to_path_buf(),
            line: node.start_position().row + 1,
            what,
        });
    });
    found
}

/// Every place the lint source under `dir` reads around the view: each `.rs` file not test code,
/// `target` directories left out.
#[must_use]
pub fn bypasses_under(view: &View<'_>, dir: &Path) -> Vec<Bypass> {
    let mut found = Vec::new();
    for file in view.walk(dir, skip_entry) {
        let shown = file.strip_prefix(view.root()).unwrap_or(&file);
        if is_test_file(shown) || file.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let (Some(text), Some(tree)) = (view.text(&file), view.rust(&file)) else {
            continue;
        };
        found.extend(bypasses(shown, text, tree));
    }
    found
}

/// Panic naming every place the lint source under `dir` reads around the view. A pack calls this
/// from its own tests, over its own `src`:
///
/// ```ignore
/// #[test]
/// fn every_lint_reads_through_its_view() {
///     mockspace_lint_rules::assert_lints_read_through_the_view(
///         std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
///     );
/// }
/// ```
///
/// # Panics
///
/// Where any file under `dir` reaches around the view, and where `dir` is not a directory, since
/// a guard over nothing passes for the wrong reason.
pub fn assert_lints_read_through_the_view(dir: impl AsRef<Path>) {
    let dir = dir.as_ref();
    let reading = Reading::new(dir);
    let view = reading.view();
    assert!(view.is_dir(dir), "`{}` is not a directory, so no lint source was read", dir.display());
    let found = bypasses_under(&view, dir);
    let lines: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert!(
        found.is_empty(),
        "lint source reads around its view, so a saved unit would replay over a change it never \
         saw; read through `ctx.view()` instead:\n{}",
        lines.join("\n")
    );
}

/// Refuses lint source under `<mock>/lints/` reaching around its view.
pub struct LintsReadThroughTheView;

impl Lint for LintsReadThroughTheView {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for LintsReadThroughTheView {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let dir = ctx.mock_dir.join("lints");
        let view = ctx.view();
        if !view.is_dir(&dir) {
            return Vec::new();
        }
        bypasses_under(view, &dir)
            .into_iter()
            .map(|b| {
                let mut e = LintError::error(
                    "workspace".to_string(),
                    b.line,
                    LINT_NAME,
                    format!(
                        "`{}` reaches `{}`, which no trace records, so a saved unit of this lint \
                         replays over a change it never saw. Read through `ctx.view()`: `text`, \
                         `bytes`, `list`, `walk`, `glob`, `exists`, `git` and `run`. Where it has \
                         to read outside the view, call `ctx.view().untraced(reason)` and the unit \
                         is judged every run.",
                        b.path.display(),
                        b.what
                    ),
                );
                e.path = Some(b.path.display().to_string());
                e
            })
            .collect()
    }
}

fn skip_entry(_path: &Path, entry: &Entry) -> bool {
    entry.is_dir() && (entry.name() == "target" || entry.name() == "tests" || entry.name().starts_with('.'))
}

fn is_test_file(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "tests")
        || path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s == "tests" || s.ends_with("_tests"))
}

fn opens_with_cfg_test(root: Node, source: &str) -> bool {
    let mut cursor = root.walk();
    root.children(&mut cursor).any(|child| {
        child.kind() == "inner_attribute_item" && crate::attribute_is_cfg_test(&source[child.byte_range()])
    })
}

fn visit(node: Node, source: &str, report: &mut impl FnMut(Node, String)) {
    if is_cfg_test_mod(node, source) {
        return;
    }
    match node.kind() {
        "use_declaration" => {
            if let Some(arg) = node.child_by_field_name("argument") {
                let mut paths = Vec::new();
                use_paths(arg, source, "", &mut paths);
                if let Some(what) = paths.iter().find_map(|p| forbidden(p)) {
                    report(node, what);
                }
            }
            return;
        },
        "scoped_identifier" | "scoped_type_identifier" => {
            let text = squash(&source[node.byte_range()]);
            if let Some(what) = forbidden(&text) {
                report(node, what);
                return;
            }
        },
        "call_expression" => {
            if let Some(what) = path_read(node, source) {
                report(node, what);
            }
        },
        "identifier" | "field_identifier" if &source[node.byte_range()] == "read_dir" => {
            report(node, "read_dir".to_string());
        },
        _ => {},
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, source, report);
    }
}

/// Every full path a `use` tree names, `prefix` already joined.
fn use_paths(node: Node, source: &str, prefix: &str, out: &mut Vec<String>) {
    let join = |rest: &str| {
        if prefix.is_empty() { rest.to_string() } else { format!("{prefix}::{rest}") }
    };
    match node.kind() {
        "scoped_use_list" => {
            let path = node
                .child_by_field_name("path")
                .map(|p| join(&squash(&source[p.byte_range()])))
                .unwrap_or_else(|| prefix.to_string());
            if let Some(list) = node.child_by_field_name("list") {
                use_paths(list, source, &path, out);
            }
        },
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                use_paths(child, source, prefix, out);
            }
        },
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                use_paths(path, source, prefix, out);
            }
        },
        "use_wildcard" => {
            let text = squash(&source[node.byte_range()]);
            out.push(join(text.trim_end_matches("::*").trim_end_matches('*')));
        },
        _ => out.push(join(&squash(&source[node.byte_range()]))),
    }
}

/// What a path reaches that a lint may not, `None` where it is allowed.
fn forbidden(path: &str) -> Option<String> {
    let path = path.trim_start_matches("::");
    for reach in FORBIDDEN_PATHS {
        if path == *reach || path.starts_with(&format!("{reach}::")) {
            return Some((*reach).to_string());
        }
    }
    let first = path.split("::").next().unwrap_or(path);
    if WALKER_CRATES.contains(&first) {
        return Some(first.to_string());
    }
    None
}

/// A call of a `Path` method that reads the file system: a method of one of those names called
/// with no argument.
fn path_read(call: Node, source: &str) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let field = function.child_by_field_name("field")?;
    let name = &source[field.byte_range()];
    let args = call.child_by_field_name("arguments")?;
    (PATH_READS.contains(&name) && args.named_child_count() == 0).then(|| format!(".{name}()"))
}

fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[cfg(test)]
#[path = "lints_read_through_the_view_tests.rs"]
mod tests;
