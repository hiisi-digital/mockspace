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
//! - a path through `std::fs`, `std::process` or `std::env`, in a `use` or in an expression, and a
//!   `use` that brings them in unnamed, as `use std::*` or `use std as s` does;
//! - a crate that reads the disk or a repository: `walkdir`, `ignore`, `glob`, `jwalk`, `git2` and
//!   `fs_err`, and `read_dir`;
//! - a [`Reading`] of the lint's own: a lint is handed its view, and a view of another reading
//!   records into no trace the run keeps. The type is named through `mockspace_lint_rules`,
//!   `mockspace` or `reading`, or as a bare `Reading` in a file that declares no type of that name
//!   of its own;
//! - a call reading the file system through a path with no argument: `.exists()`, `.metadata()`,
//!   `.symlink_metadata()`, `.canonicalize()`, `.try_exists()`, `.read_link()`, `.is_file()`,
//!   `.is_dir()` and `.is_symlink()`, and the same written `Path::exists(p)`. The view's own
//!   `exists`, `is_file` and `is_dir` take the path as an argument, so only the `Path` form is
//!   caught. An [`Entry`] of a listing answers `is_file` and `is_dir` too, and without types the
//!   two receivers cannot be told apart, so an entry is asked as `Entry::is_dir(entry)`.
//!
//! All of it is found inside a macro's arguments as well, `vec![..]` and `format!(..)`, where the
//! parse keeps only tokens.
//!
//! Test code is a `#[cfg(test)]` module, a file opening with `#![cfg(test)]`, a file under a
//! `tests` directory, and a file named `tests.rs` or ending in `_tests.rs`, which is where a
//! `#[cfg(test)] mod tests;` keeps its body. Tests build their fixtures on disk and may.
//!
//! A tool is not a lint: it is run on demand, keeps no unit and may read anything, a [`Reading`]
//! of its own included. The guard tells a tool by where it lives. A file under a directory named
//! `tools` is a tool's and is not read, so a pack keeps its tools under `src/tools/`, and a
//! repository keeps them in `<mock>/tools/`, which the repo lint never reads.
//!
//! A lint that truly has to read outside the view says so with [`View::untraced`], and the unit
//! that asks it is never saved, so it reads afresh on every run. What a function reads after a
//! statement that is a call of `untraced` and nothing else, at the top of its body, is therefore
//! not looked at, since that call runs whenever the reads after it do. A call in a branch, a
//! closure or a nested block exempts nothing, and neither does one after the read. A helper
//! reading for a function calls `untraced` itself.
//!
//! Severity: Error (blocks commit, push, and build).

use std::fmt;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Tree};

use crate::reading::{Entry, Reading};
use crate::{Lint, LintError, RepoContext, RepoLint, Severity, View, is_cfg_test_mod};

const LINT_NAME: &str = "lints-read-through-the-view";

/// Paths a lint may not reach, by their leading segments.
const FORBIDDEN_PATHS: &[&str] = &["std::fs", "std::process", "std::env"];

/// Crates that read the disk or a repository.
const READER_CRATES: &[&str] = &["walkdir", "ignore", "glob", "jwalk", "git2", "fs_err"];

/// The type a lint may not build or name: it is handed its view.
const READING: &str = "Reading";

/// The paths `Reading` is reached under: this crate, the engine that re-exports it, and the module
/// that holds it.
const READING_HOMES: &[&str] = &["mockspace_lint_rules", "mockspace", "reading"];

/// `Path` methods that read the file system and take no argument.
const PATH_READS: &[&str] = &[
    "exists",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "try_exists",
    "read_link",
    "is_file",
    "is_dir",
    "is_symlink",
];

/// One place lint source reads around its view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bypass {
    /// The file, as the caller named it.
    pub path: PathBuf,
    /// One-based line.
    pub line: usize,
    /// What it reaches: a forbidden path such as `std::fs`, `read_dir`, a reading crate,
    /// `Reading`, or `.<method>()`.
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
    let mut scan = Scan {
        source,
        own_reading: false,
    };
    scan.own_reading = declares_type(root, &scan, READING);
    let mut found = Vec::new();
    visit(root, &scan, &mut |node, what| {
        found.push(Bypass {
            path: path.to_path_buf(),
            line: node.start_position().row + 1,
            what,
        });
    });
    found
}

/// Every place the lint source under `dir` reads around the view: each `.rs` file neither test
/// code nor a tool's, `target` directories left out.
#[must_use]
pub fn bypasses_under(view: &View<'_>, dir: &Path) -> Vec<Bypass> {
    let mut found = Vec::new();
    for file in view.walk(dir, skip_entry) {
        let shown = file.strip_prefix(view.root()).unwrap_or(&file);
        if not_lint_source(shown) || file.extension().is_none_or(|e| e != "rs") {
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

/// Directories holding no lint source: a build's, test code and tools.
const NOT_LINTS: &[&str] = &["target", "tests", "tools"];

fn skip_entry(_path: &Path, entry: &Entry) -> bool {
    Entry::is_dir(entry) && (NOT_LINTS.contains(&entry.name()) || entry.name().starts_with('.'))
}

/// A file of test code or a tool's, which the guard does not read.
fn not_lint_source(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "tests" || c.as_os_str() == "tools")
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

/// What a scan of one file knows beside the node in hand.
struct Scan<'s> {
    source:      &'s str,
    /// The file declares a type named `Reading` of its own, so a bare `Reading` is that one.
    own_reading: bool,
}

impl Scan<'_> {
    fn text(&self, node: Node) -> &str {
        &self.source[node.byte_range()]
    }
}

fn visit(node: Node, scan: &Scan, report: &mut impl FnMut(Node, String)) {
    if is_cfg_test_mod(node, scan.source) {
        return;
    }
    match node.kind() {
        "function_item" => {
            let body = node.child_by_field_name("body");
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if Some(child) == body {
                    visit_body(child, scan, report);
                } else {
                    visit(child, scan, report);
                }
            }
            return;
        },
        "use_declaration" => {
            if let Some(arg) = node.child_by_field_name("argument") {
                let mut paths = Vec::new();
                use_paths(arg, scan, "", &mut paths);
                if let Some(what) = paths.iter().find_map(|p| imported(p, scan)) {
                    report(node, what);
                }
            }
            return;
        },
        "scoped_identifier" | "scoped_type_identifier" => {
            if let Some(what) = forbidden(&squash(scan.text(node)), scan) {
                report(node, what);
                return;
            }
        },
        "call_expression" => {
            if let Some(what) = path_read(node, scan) {
                report(node, what);
            }
        },
        "token_tree" => token_reads(node, scan, report),
        "identifier" | "field_identifier" if scan.text(node) == "read_dir" => {
            report(node, "read_dir".to_string());
        },
        _ => {},
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, scan, report);
    }
}

/// A function's body, statement by statement, up to a statement that calls `untraced` and nothing
/// else. Such a call always runs once it is reached, and it keeps the unit from being saved, so
/// what the function reads after it is read again on every run and is not looked at. A call in a
/// branch, a closure or any expression that may not run exempts nothing, and a read before the
/// call is read as any other.
fn visit_body(body: Node, scan: &Scan, report: &mut impl FnMut(Node, String)) {
    let mut cursor = body.walk();
    for statement in body.children(&mut cursor) {
        if calls_untraced(statement, scan) {
            return;
        }
        visit(statement, scan, report);
    }
}

/// Whether `statement` is a call of `untraced` standing alone, `view.untraced(reason);`.
fn calls_untraced(statement: Node, scan: &Scan) -> bool {
    let call = match statement.kind() {
        "expression_statement" => statement.named_child(0),
        "call_expression" => Some(statement),
        _ => None,
    };
    call.is_some_and(|call| {
        call.kind() == "call_expression"
            && call.child_by_field_name("function").is_some_and(|f| {
                f.kind() == "field_expression"
                    && f.child_by_field_name("field").is_some_and(|name| scan.text(name) == "untraced")
            })
    })
}

/// Whether the file declares a type named `name` of its own.
fn declares_type(node: Node, scan: &Scan, name: &str) -> bool {
    let declares = matches!(node.kind(), "struct_item" | "enum_item" | "union_item" | "type_item")
        && node.child_by_field_name("name").is_some_and(|n| scan.text(n) == name);
    let mut cursor = node.walk();
    declares || node.children(&mut cursor).any(|child| declares_type(child, scan, name))
}

/// How a `use` names a path: as itself, every name under it, or under a name of its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Imported {
    Itself,
    Glob,
    Renamed,
}

/// Every full path a `use` tree names, `prefix` already joined.
fn use_paths(node: Node, scan: &Scan, prefix: &str, out: &mut Vec<(String, Imported)>) {
    let join = |rest: &str| {
        let joined = if prefix.is_empty() { rest.to_string() } else { format!("{prefix}::{rest}") };
        joined.trim_end_matches("::self").to_string()
    };
    match node.kind() {
        "scoped_use_list" => {
            let path = node
                .child_by_field_name("path")
                .map(|p| join(&squash(scan.text(p))))
                .unwrap_or_else(|| prefix.to_string());
            if let Some(list) = node.child_by_field_name("list") {
                use_paths(list, scan, &path, out);
            }
        },
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                use_paths(child, scan, prefix, out);
            }
        },
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                out.push((join(&squash(scan.text(path))), Imported::Renamed));
            }
        },
        "use_wildcard" => {
            let text = squash(scan.text(node));
            out.push((join(text.trim_end_matches("::*").trim_end_matches('*')), Imported::Glob));
        },
        _ => out.push((join(&squash(scan.text(node))), Imported::Itself)),
    }
}

/// What an imported path reaches that a lint may not. A glob or a rename of a path above a
/// forbidden one brings it in under a name the expression check cannot see, `fs::read` after
/// `use std::*`, so it is refused at the `use`.
fn imported((path, how): &(String, Imported), scan: &Scan) -> Option<String> {
    if let Some(what) = forbidden(path, scan) {
        return Some(what);
    }
    let path = path.trim_start_matches("::");
    let above = FORBIDDEN_PATHS.iter().any(|reach| reach.starts_with(&format!("{path}::")));
    match how {
        Imported::Glob if above => Some(format!("{path}::*")),
        Imported::Renamed if above => Some(path.to_string()),
        _ => None,
    }
}

/// What a path reaches that a lint may not, `None` where it is allowed.
fn forbidden(path: &str, scan: &Scan) -> Option<String> {
    let path = path.trim_start_matches("::");
    for reach in FORBIDDEN_PATHS {
        if path == *reach || path.starts_with(&format!("{reach}::")) {
            return Some((*reach).to_string());
        }
    }
    let segments: Vec<&str> = path.split("::").collect();
    if READER_CRATES.contains(&segments[0]) {
        return Some(segments[0].to_string());
    }
    if let Some(at) = segments.iter().position(|s| *s == READING) {
        let ours = match at.checked_sub(1).map(|before| segments[before]) {
            Some(before) => READING_HOMES.contains(&before),
            None => !scan.own_reading,
        };
        if ours {
            return Some(READING.to_string());
        }
    }
    if let [.., ty, method] = segments[..]
        && (ty == "Path" || ty == "PathBuf")
        && (PATH_READS.contains(&method) || method == "read_dir")
    {
        return Some(format!("{ty}::{method}"));
    }
    None
}

/// A call of a `Path` method that reads the file system: a method of one of those names called
/// with no argument.
fn path_read(call: Node, scan: &Scan) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let name = scan.text(function.child_by_field_name("field")?);
    let args = call.child_by_field_name("arguments")?;
    (PATH_READS.contains(&name) && args.named_child_count() == 0).then(|| format!(".{name}()"))
}

/// The same refusals over a macro's arguments, which the parse leaves as a flat run of tokens: a
/// path is segments joined by `::`, a call is `.`, a name and an empty `()`. A nested group is a
/// token tree of its own and is read when the visitor reaches it.
fn token_reads(tree: Node, scan: &Scan, report: &mut impl FnMut(Node, String)) {
    let mut cursor = tree.walk();
    let tokens: Vec<Node> = tree.children(&mut cursor).collect();
    let segment = |n: Node| matches!(n.kind(), "identifier" | "self" | "crate" | "super");
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if scan.text(token) == "." {
            if let (Some(&name), Some(&args)) = (tokens.get(i + 1), tokens.get(i + 2))
                && name.kind() == "identifier"
                && PATH_READS.contains(&scan.text(name))
                && args.kind() == "token_tree"
                && squash(scan.text(args)) == "()"
            {
                report(name, format!(".{}()", scan.text(name)));
            }
            i += 1;
            continue;
        }
        let mut j = if scan.text(token) == "::" { i + 1 } else { i };
        let mut parts = Vec::new();
        while j < tokens.len() && segment(tokens[j]) {
            parts.push(scan.text(tokens[j]));
            j += 1;
            if j + 1 < tokens.len() && scan.text(tokens[j]) == "::" && segment(tokens[j + 1]) {
                j += 1;
            } else {
                break;
            }
        }
        if parts.len() > 1
            && let Some(what) = forbidden(&parts.join("::"), scan)
        {
            report(token, what);
        }
        i = j.max(i + 1);
    }
}

fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[cfg(test)]
#[path = "lints_read_through_the_view_tests.rs"]
mod tests;
