//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which files one integration-test binary is built from, so a change confined
//! to one of them need not run the member's other binaries.
//!
//! **The unit of selection under a member is a test binary.** A member's
//! `tests/<name>.rs` and `tests/<name>/main.rs` are each their own binary,
//! linked against the member's library and its dependencies, and nothing else
//! of the member is compiled into them. What a binary is built from is its root
//! file, the files its root reaches by `mod` declarations, and (for the
//! directory form) everything under `tests/<name>/`. Those are its **unit**.
//! Every other file of the member is **shared**: the library, its build script,
//! its manifest, the library's own unit tests, the examples and binaries, and any
//! file under `tests/` that no unit claims. A shared file moves every binary of
//! the member, so a binary is skipped only when neither its unit nor the shared
//! set moved.
//!
//! **The argument for soundness is that a file is claimed only on evidence and
//! everything else widens.**
//!
//! - A `mod name;` is resolved by the compiler's own rule (the crate root and a
//!   `mod.rs` look beside themselves, any other file looks in a directory of its
//!   own stem), against the files git lists. An unresolvable one claims nothing.
//! - A string literal that names a file or a directory inside the member (an
//!   `include_str!`, a `#[path]`, a `Path::join("fixtures")`, a `trybuild` glob
//!   cut at its first wildcard) claims that file, or everything under that
//!   directory, for the binary that wrote it. The literal is matched against the
//!   file's own directory and the member's, the two places a path in a test
//!   resolves from. A literal that matches by accident only widens.
//! - A binary is **wide** when it does something this cannot see through: it reads
//!   the environment or `file!()` (so it may build a path to anything in the
//!   member), a `mod` sits inside an inline module or a macro, a file pulled in by
//!   a literal declares modules of its own, or the file does not parse. A wide
//!   binary is rebuilt on the digest of every claimed file of the member, which is
//!   the widest its own inputs can be: the shared files are in every digest anyway.
//! - A member whose manifest cannot be read, names a test target outside
//!   `tests/`, or points a library, binary, example or bench at a file under
//!   `tests/` claims nothing, and its binaries all move together.
//!
//! **What it cannot see**: a path computed at run time out of pieces none of
//! which is a literal naming it, read by a binary that mentions neither the
//! environment nor `file!()`, and naming a file that some other binary claims. A
//! file nobody claims is shared, so the same computed read of an unclaimed file
//! is still seen. A literal that is only `.` names the whole member and is not
//! followed, since following it would make every binary that splits a string on a
//! dot read everything. A placeholder is read only at the start of a literal, as the
//! macros that expand them read it, so `tests/$X` is a path under the member and
//! not wide; `$CARGO_MANIFEST_DIR/.` is the member itself and is. The values of
//! environment variables are not part of any fingerprint, here or at the member
//! level, so a binary that reads one is wide but is not rerun when only the
//! variable changed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::fingerprint::{normalise, reach};

/// What reading one Rust source file tells about what it is built from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The string literals that climb out of where they resolve, the reads of
    /// content outside the member that the member-level fingerprint has always
    /// followed.
    pub climbing: Vec<String>,
    /// Every string literal that could name a path: no escape, not empty, short,
    /// on one line. The climbing ones are among them.
    pub strings:  Vec<String>,
    /// The `mod name;` declarations at the top level of the file.
    pub mods:     Vec<ModDecl>,
    /// It reads the environment or `file!()`, so it may build a path to anything.
    pub ambient:  bool,
    /// What it declares or includes cannot be followed: it did not parse, a `mod`
    /// sits inside an inline module or a macro.
    pub opaque:   bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModDecl {
    pub name:      String,
    /// It carries a `#[path]`, so the literal in the attribute says where it is.
    pub path_attr: bool,
}

/// Longest literal kept: a path is not a paragraph.
const LONGEST_LITERAL: usize = 300;

/// Reads a Rust source with tree-sitter, so a path in a comment is no input and
/// a raw string is read as written.
pub fn facts(source: &str) -> Facts {
    let mut out = Facts::default();
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        out.opaque = true;
        return out;
    }
    let Some(tree) = parser.parse(source, None) else {
        out.opaque = true;
        return out;
    };
    // A tree with an error in it may have lost a `mod` or a literal.
    out.opaque = tree.root_node().has_error();
    let bytes = source.as_bytes();
    let text = |n: tree_sitter::Node| n.utf8_text(bytes).unwrap_or_default().to_string();
    let mut stack = vec![(tree.root_node(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        match node.kind() {
            "string_literal" | "raw_string_literal" => {
                if let Some(v) = literal_value(&text(node)).map(str::to_string)
                    && !v.is_empty()
                    && v.len() <= LONGEST_LITERAL
                    && !v.contains('\n')
                {
                    // A placeholder the macros expand from the environment names
                    // something this cannot place, unless it is the member's own
                    // directory and a path under it. That path may climb out of
                    // the member, as `rust-embed` and `include_dir` are pointed at
                    // a folder beside the crate, and is then a read like any other.
                    let holder = placeholder(&v);
                    if holder.is_some_and(|(n, rest)| {
                        n != "CARGO_MANIFEST_DIR"
                            || Path::new(rest)
                                .components()
                                .all(|c| matches!(c, std::path::Component::CurDir))
                    }) {
                        out.ambient = true;
                    }
                    if let Some(("CARGO_MANIFEST_DIR", rest)) = holder
                        && climbs_out(rest)
                    {
                        out.climbing.push(rest.to_string());
                    }
                    if climbs_out(&v) {
                        out.climbing.push(v.clone());
                    }
                    out.strings.push(v);
                }
                continue;
            },
            "mod_item" => {
                if let Some(body) = node.child_by_field_name("body") {
                    stack.push((body, depth + 1));
                } else if depth > 0 {
                    out.opaque = true;
                } else if let Some(name) = node.child_by_field_name("name") {
                    out.mods.push(ModDecl {
                        name:      text(name),
                        path_attr: has_path_attribute(node, bytes),
                    });
                }
                continue;
            },
            "macro_invocation" => {
                let name = node
                    .child_by_field_name("macro")
                    .map(text)
                    .unwrap_or_default();
                if matches!(
                    name.rsplit("::").next().unwrap_or_default(),
                    "env" | "option_env" | "file"
                ) {
                    out.ambient = true;
                }
            },
            "token_tree" if node.parent().is_some_and(|p| p.kind() != "token_tree") => {
                if declares_a_module(&text(node)) {
                    out.opaque = true;
                }
            },
            "identifier" if text(node) == "env" => {
                // As a path segment, or as a bare token inside a macro's
                // arguments, which tree-sitter does not parse into paths:
                // `assert!(!env!("X").is_empty())` is tokens, not a call.
                if node.parent().is_some_and(|p| {
                    matches!(
                        p.kind(),
                        "scoped_identifier"
                            | "scoped_use_list"
                            | "use_list"
                            | "use_declaration"
                            | "use_as_clause"
                            | "use_wildcard"
                            | "token_tree"
                    )
                }) {
                    out.ambient = true;
                }
            },
            "identifier" if node.parent().is_some_and(|p| p.kind() == "token_tree") => {
                let word = text(node);
                let bang = node.next_sibling().is_some_and(|s| s.kind() == "!");
                if word == "option_env" || (word == "file" && bang) {
                    out.ambient = true;
                }
            },
            _ => {},
        }
        let mut cursor = node.walk();
        let kids: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(kids.into_iter().rev().map(|c| (c, depth)));
    }
    out.climbing.sort();
    out.climbing.dedup();
    out.strings.sort();
    out.strings.dedup();
    out
}

/// The text between a literal's quotes, or nothing for one with an escape in
/// it, which a path is never written with.
fn literal_value(text: &str) -> Option<&str> {
    let body = text.trim_start_matches('r').trim_start_matches('#');
    let body = body.trim_end_matches('#');
    let inner = body.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('\\')).then_some(inner)
}

/// A path that starts by climbing and names something after: the member-level
/// rule for a read of content outside the member.
pub fn climbs_out(v: &str) -> bool {
    use std::path::Component;
    let p = Path::new(v.trim_start_matches('/'));
    let mut comps = p.components();
    matches!(comps.next(), Some(Component::ParentDir))
        && p.components().any(|c| matches!(c, Component::Normal(_)))
}

/// Whether an outer attribute on this item says `path =`, however it is wrapped.
fn has_path_attribute(item: tree_sitter::Node, src: &[u8]) -> bool {
    let mut prev = item.prev_sibling();
    while let Some(s) = prev {
        match s.kind() {
            "attribute_item" => {
                if names_path(s.utf8_text(src).unwrap_or_default()) {
                    return true;
                }
            },
            "line_comment" | "block_comment" => {},
            _ => break,
        }
        prev = s.prev_sibling();
    }
    false
}

/// `path` as a word followed by `=`, which `#[path = "x"]` and
/// `#[cfg_attr(unix, path = "x")]` both contain.
fn names_path(attr: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    attr.match_indices("path").any(|(at, _)| {
        let before = attr[.. at].chars().next_back();
        let after = attr[at + 4 ..].trim_start();
        before.is_none_or(|c| !word(c)) && after.starts_with('=')
    })
}

/// Whether the text of a macro's tokens contains `mod name`, which such a
/// declaration inside a macro would, and which this does not resolve.
fn declares_a_module(tokens: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    tokens.match_indices("mod").any(|(at, _)| {
        let before = tokens[.. at].chars().next_back();
        let rest = &tokens[at + 3 ..];
        before.is_none_or(|c| !word(c))
            && rest.starts_with(char::is_whitespace)
            && rest
                .trim_start()
                .starts_with(|c: char| c.is_alphabetic() || c == '_')
    })
}

/// The files git lists, as a set that answers "is this a file" and "what is
/// under this directory" without touching the disk.
#[derive(Debug, Clone, Default)]
pub struct Index {
    files: BTreeSet<PathBuf>,
}

impl Index {
    pub fn new(files: &[PathBuf]) -> Index {
        Index {
            files: files.iter().cloned().collect(),
        }
    }

    pub fn has_file(&self, p: &Path) -> bool {
        self.files.contains(p)
    }

    /// The file itself, or every file under the directory, sorted.
    pub fn under(&self, p: &Path) -> Vec<PathBuf> {
        if self.files.contains(p) {
            return vec![p.to_path_buf()];
        }
        self.files
            .range(p.to_path_buf() ..)
            .take_while(|f| f.starts_with(p))
            .cloned()
            .collect()
    }
}

/// A test target and where its root sits, relative to the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub name:     String,
    pub file:     PathBuf,
    /// `tests/<name>/main.rs`, where everything beside the root is the target's.
    pub dir_form: bool,
}

/// The integration-test targets of a package, from its manifest and the files
/// under `tests/`. `None` when they cannot be told, and then nothing is
/// narrowed.
pub fn roots(manifest: &str, member_dir: &Path, index: &Index) -> Option<Vec<Root>> {
    let doc = manifest.parse::<toml_edit::DocumentMut>().ok()?;
    let tests = member_dir.join("tests");
    let inside = |p: &str| -> Option<PathBuf> {
        normalise(&member_dir.join(p)).filter(|n| n.starts_with(&tests))
    };
    // A library, binary, example or bench whose root sits under `tests/` makes
    // that file the target's rather than a test's.
    let table_path =
        |t: &toml_edit::Table| t.get("path").and_then(|p| p.as_str()).map(str::to_string);
    for key in ["lib", "bin", "example", "bench"] {
        let Some(item) = doc.get(key) else {
            continue;
        };
        let paths: Vec<String> = if let Some(arr) = item.as_array_of_tables() {
            arr.iter().filter_map(table_path).collect()
        } else {
            table_path(item.as_table()?).into_iter().collect()
        };
        for p in paths {
            if normalise(&member_dir.join(&p)).is_some_and(|n| n.starts_with(&tests)) {
                return None;
            }
        }
    }

    let auto = doc
        .get("package")
        .and_then(|p| p.get("autotests"))
        .and_then(|a| a.as_bool())
        .unwrap_or(true);
    let mut found: BTreeMap<String, Root> = BTreeMap::new();
    if auto {
        for f in index.under(&tests) {
            let Ok(rel) = f.strip_prefix(&tests) else {
                continue;
            };
            let parts: Vec<_> = rel.components().collect();
            let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned());
            match parts.as_slice() {
                [_] if f.extension().is_some_and(|e| e == "rs") => {
                    if let Some(name) = stem(&f) {
                        found.insert(name.clone(), Root {
                            name,
                            file: f.clone(),
                            dir_form: false,
                        });
                    }
                },
                [dir, _] if f.file_name().is_some_and(|n| n == "main.rs") => {
                    let name = dir.as_os_str().to_string_lossy().into_owned();
                    found.insert(name.clone(), Root {
                        name,
                        file: f.clone(),
                        dir_form: true,
                    });
                },
                _ => {},
            }
        }
    }
    if let Some(explicit) = doc.get("test") {
        let tables: Vec<&toml_edit::Table> = explicit.as_array_of_tables()?.iter().collect();
        for t in tables {
            let name = t.get("name")?.as_str()?.to_string();
            let file = match t.get("path").and_then(|p| p.as_str()) {
                Some(p) => inside(p)?,
                None => {
                    let flat = tests.join(format!("{name}.rs"));
                    if index.has_file(&flat) { flat } else { tests.join(&name).join("main.rs") }
                },
            };
            let dir_form = file.file_name().is_some_and(|n| n == "main.rs")
                && file.parent().and_then(Path::parent) == Some(tests.as_path());
            if !index.has_file(&file) {
                return None;
            }
            found.insert(name.clone(), Root {
                name,
                file,
                dir_form,
            });
        }
    }
    Some(found.into_values().collect())
}

/// One binary's inputs under `tests/`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unit {
    /// Every file it is built from or names, all under `tests/`.
    pub files:   BTreeSet<PathBuf>,
    /// The paths outside the member that its sources climb to.
    pub reaches: BTreeSet<PathBuf>,
    /// It may read anything in the member, so it is rebuilt on all claimed files.
    pub wide:    bool,
}

/// How a member's files divide into binaries and what they share.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// Each test target by name.
    pub units:          BTreeMap<String, Unit>,
    /// Every file some unit claims.
    pub claimed:        BTreeSet<PathBuf>,
    /// The claimed files that something shared names, which are shared too.
    pub shared_pulls:   BTreeSet<PathBuf>,
    /// The paths outside the member that shared sources climb to.
    pub shared_reaches: BTreeSet<PathBuf>,
}

/// What the layout is computed from.
pub struct Ctx<'a> {
    pub repo_root:  &'a Path,
    pub member_dir: &'a Path,
    pub index:      &'a Index,
    /// Every Rust source of the member, read.
    pub facts:      &'a BTreeMap<PathBuf, Facts>,
}

/// The layout of a member whose test targets are `roots`.
pub fn layout(ctx: &Ctx, roots: &[Root]) -> Layout {
    let tests = ctx.member_dir.join("tests");
    let mut l = Layout::default();
    for r in roots {
        l.units.insert(r.name.clone(), claim(ctx, &tests, r));
    }
    l.claimed = l
        .units
        .values()
        .flat_map(|u| u.files.iter().cloned())
        .collect();

    // What the shared sources name: their reads outside the member, and any
    // claimed file they pull in, which then belongs to every binary.
    let mut queue: Vec<PathBuf> = Vec::new();
    for (path, f) in ctx.facts {
        if l.claimed.contains(path) {
            continue;
        }
        let file_dir = path.parent().unwrap_or(Path::new(""));
        for lit in &f.climbing {
            if let Some(r) = reach(lit, file_dir, ctx.member_dir, ctx.repo_root) {
                l.shared_reaches.insert(r);
            }
        }
        for lit in &f.strings {
            for p in resolve(lit, file_dir, ctx.member_dir, ctx.index) {
                if l.claimed.contains(&p) && l.shared_pulls.insert(p.clone()) {
                    queue.push(p);
                }
            }
        }
    }

    // A pulled file is compiled into the library, so what it declares and names
    // is shared as well, however deep: its modules, the files its literals name,
    // and its reads outside the member. Its modules are looked for where a
    // crate root or a `mod.rs` would find them and where any other file would,
    // since nothing says which it is when a `#[path]` pulled it in.
    let mut everything = false;
    while let Some(path) = queue.pop() {
        let Some(f) = ctx.facts.get(&path) else {
            continue;
        };
        // A pulled file whose modules cannot be followed may declare any of them.
        everything |= f.opaque;
        let file_dir = path.parent().unwrap_or(Path::new(""));
        for lit in &f.climbing {
            if let Some(r) = reach(lit, file_dir, ctx.member_dir, ctx.repo_root) {
                l.shared_reaches.insert(r);
            }
        }
        let own_dir = file_dir.join(path.file_stem().unwrap_or_default());
        let mut named: Vec<PathBuf> = Vec::new();
        for m in f.mods.iter().filter(|m| !m.path_attr) {
            for dir in [file_dir, own_dir.as_path()] {
                named.push(dir.join(format!("{}.rs", m.name)));
                named.push(dir.join(&m.name).join("mod.rs"));
            }
        }
        for lit in &f.strings {
            named.extend(resolve(lit, file_dir, ctx.member_dir, ctx.index));
        }
        for p in named {
            if l.claimed.contains(&p) && l.shared_pulls.insert(p.clone()) {
                queue.push(p);
            }
        }
    }
    if everything {
        l.shared_pulls.extend(l.claimed.iter().cloned());
    }
    l
}

/// How a file's own `mod name;` declarations are looked for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A crate root or a `mod.rs`: its modules sit beside it.
    ModRs,
    /// Any other module file: its modules sit in a directory of its stem.
    File,
    /// Pulled in by a literal: no rule says where its modules would sit.
    Pulled,
}

fn claim(ctx: &Ctx, tests: &Path, root: &Root) -> Unit {
    let mut u = Unit::default();
    let mut queue: Vec<(PathBuf, Kind)> = vec![(root.file.clone(), Kind::ModRs)];
    u.files.insert(root.file.clone());
    if root.dir_form {
        let dir = root.file.parent().unwrap_or(Path::new(""));
        for f in ctx.index.under(dir) {
            u.files.insert(f.clone());
            let kind = if f.file_name().is_some_and(|n| n == "mod.rs") {
                Kind::ModRs
            } else {
                Kind::File
            };
            queue.push((f, kind));
        }
    }
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    while let Some((path, kind)) = queue.pop() {
        if !seen.insert(path.clone()) || path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Some(f) = ctx.facts.get(&path) else {
            u.wide = true;
            continue;
        };
        u.wide |= f.ambient || f.opaque;
        let file_dir = path.parent().unwrap_or(Path::new(""));
        for lit in &f.climbing {
            if let Some(r) = reach(lit, file_dir, ctx.member_dir, ctx.repo_root) {
                u.reaches.insert(r);
            }
        }
        if kind == Kind::Pulled && !f.mods.is_empty() {
            u.wide = true;
        } else {
            for m in f.mods.iter().filter(|m| !m.path_attr) {
                let dir = match kind {
                    Kind::ModRs => file_dir.to_path_buf(),
                    _ => file_dir.join(path.file_stem().unwrap_or_default()),
                };
                for (cand, k) in [
                    (dir.join(format!("{}.rs", m.name)), Kind::File),
                    (dir.join(&m.name).join("mod.rs"), Kind::ModRs),
                ] {
                    if ctx.index.has_file(&cand) {
                        take(tests, cand, k, &mut u, &mut queue);
                    }
                }
            }
        }
        for lit in &f.strings {
            for p in resolve(lit, file_dir, ctx.member_dir, ctx.index) {
                take(tests, p, Kind::Pulled, &mut u, &mut queue);
            }
        }
    }
    u
}

/// Claims `p` for the unit and queues it to be read, if it sits under `tests/`
/// and the unit does not hold it already. A file outside `tests/` is shared and
/// moves every binary on its own.
fn take(tests: &Path, p: PathBuf, kind: Kind, u: &mut Unit, queue: &mut Vec<(PathBuf, Kind)>) {
    if p.starts_with(tests) && u.files.insert(p.clone()) {
        queue.push((p, kind));
    }
}

/// A literal rooted at a placeholder, `$NAME/rest` or `${NAME}/rest`, as
/// `include_dir!` and `rust-embed` take their paths: the name and the rest.
/// Only a literal that begins with it and goes on as a path is one.
pub fn placeholder(lit: &str) -> Option<(&str, &str)> {
    let after = lit.strip_prefix('$')?;
    let (name, rest) = if let Some(braced) = after.strip_prefix('{') {
        braced.split_once('}')?
    } else {
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        after.split_at(end)
    };
    let named = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let path = rest.strip_prefix('/');
    (named && (rest.is_empty() || path.is_some())).then_some((name, path.unwrap_or(rest)))
}

/// The files and directories a literal names inside the member, resolved from
/// the directory of the file it is written in and from the member's own, the two
/// places a path in a test resolves from. A wildcard cuts the path at the
/// component holding it. Empty for a literal that names nothing, and for `.`,
/// which names everything and would make every binary read the member.
pub fn resolve(lit: &str, file_dir: &Path, member_dir: &Path, index: &Index) -> Vec<PathBuf> {
    // A leading `$CARGO_MANIFEST_DIR` is the member's own directory, spelled as
    // the macros that expand it spell it; the rest is relative to the member.
    let (trimmed, bases): (&str, &[&Path]) = match placeholder(lit) {
        Some(("CARGO_MANIFEST_DIR", rest)) => (rest, &[member_dir]),
        Some(_) => return Vec::new(),
        None => (lit.trim_start_matches('/'), &[file_dir, member_dir]),
    };
    let mut kept: Vec<&str> = Vec::new();
    for part in trimmed.split('/') {
        if part.contains(['*', '?', '[', '{']) {
            break;
        }
        kept.push(part);
    }
    let rel = kept.join("/");
    if rel.is_empty() || rel == "." {
        return Vec::new();
    }
    let rel = Path::new(&rel);
    let mut out: Vec<PathBuf> = Vec::new();
    for base in bases {
        let Some(p) = normalise(&base.join(rel)) else {
            continue;
        };
        if p.starts_with(member_dir) && p != member_dir {
            out.extend(index.under(&p));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
#[path = "units_tests.rs"]
mod tests;
