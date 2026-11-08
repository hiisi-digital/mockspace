//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! When a procedural macro of the tree may be held to its sources.
//!
//! A macro runs while the build runs, and a dep-info lists the files of the crate that holds it and
//! nothing the macro read. The store loads a library only where every input is what it was built
//! from, so a macro that could read a file, a variable or a process the dep-info does not list
//! leaves nothing kept. A macro that can reach nothing but its token input is held by its sources.
//!
//! Two things must both hold. The macro's normal-dependency closure is within [`DEPENDENCIES`]
//! (a crate of the tree in it is held to the next rule as well), and every source of the macro and
//! of each crate of the tree in that closure names no path of `std`, `core` or `alloc` outside
//! [`PURE_MODULES`], no foreign block and no assembly. A glob import of the roots, an import of a
//! refused module under any alias, and a source that does not parse are all refusals.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

/// The only crates a macro may depend on, besides the compiler's own `proc_macro` and the sysroot.
const DEPENDENCIES: [&str; 4] = ["proc-macro2", "quote", "syn", "unicode-ident"];

/// The roots of the standard library a path may start from.
const ROOTS: [&str; 3] = ["std", "core", "alloc"];

/// The modules and macros of the standard library that reach nothing outside the program: no file,
/// no variable, no process, no network, no clock, no thread, no path.
const PURE_MODULES: &[&str] = &[
    "alloc", "any", "array", "borrow", "boxed", "cell", "char", "clone", "cmp", "collections", "convert",
    "default", "error", "fmt", "hash", "iter", "marker", "mem", "num", "ops", "option", "prelude", "rc",
    "result", "slice", "str", "string", "vec", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize", "format", "matches", "write", "writeln", "assert",
    "assert_eq", "assert_ne", "debug_assert", "debug_assert_eq", "debug_assert_ne", "unreachable",
    "unimplemented", "todo", "panic", "concat", "stringify",
];

/// Macros whose body is the code they generate and not code the macro runs.
const QUOTING: [&str; 4] = ["quote", "quote_spanned", "parse_quote", "parse_quote_spanned"];

/// Macros that are assembly.
const ASSEMBLY: [&str; 3] = ["asm", "global_asm", "naked_asm"];

/// For each procedural macro package of the tree in a `cargo metadata` document whose normal
/// dependencies stay within [`DEPENDENCIES`], the manifest directories of the crates of the tree
/// in its closure. A macro with a dependency outside the list has no entry.
pub(crate) fn macro_closures(json: &serde_json::Value) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let mut out = BTreeMap::new();
    let (Some(packages), Some(nodes)) = (
        json.get("packages").and_then(|p| p.as_array()),
        json.pointer("/resolve/nodes").and_then(|n| n.as_array()),
    ) else {
        return out;
    };
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|s| s.as_str()).map(str::to_string);
    let package = |id: &str| packages.iter().find(|p| p.get("id").and_then(|i| i.as_str()) == Some(id));
    let dir_of = |p: &serde_json::Value| text(p, "manifest_path").and_then(|m| Path::new(&m).parent().map(Path::to_path_buf));
    for root in packages {
        let in_tree = root.get("source").is_none_or(|s| s.is_null());
        let is_macro = root
            .get("targets")
            .and_then(|t| t.as_array())
            .is_some_and(|ts| ts.iter().any(|t| {
                t.get("kind").and_then(|k| k.as_array()).is_some_and(|k| k.iter().any(|k| k == "proc-macro"))
            }));
        let (Some(root_id), Some(root_dir), true) = (text(root, "id"), dir_of(root), in_tree && is_macro) else {
            continue;
        };
        let mut seen = BTreeSet::from([root_id.clone()]);
        let mut todo = vec![root_id];
        let mut closure = Vec::new();
        let mut within = true;
        while let Some(id) = todo.pop() {
            let Some(node) = nodes.iter().find(|n| n.get("id").and_then(|i| i.as_str()) == Some(id.as_str())) else {
                within = false;
                break;
            };
            for dep in node.get("deps").and_then(|d| d.as_array()).into_iter().flatten() {
                let normal = dep
                    .get("dep_kinds")
                    .and_then(|k| k.as_array())
                    .is_some_and(|ks| ks.iter().any(|k| k.get("kind").is_none_or(|k| k.is_null())));
                let Some(dep_id) = text(dep, "pkg").filter(|_| normal) else { continue };
                if !seen.insert(dep_id.clone()) {
                    continue;
                }
                let Some(pkg) = package(&dep_id) else {
                    within = false;
                    continue;
                };
                match text(pkg, "source") {
                    None => {
                        closure.extend(dir_of(pkg));
                    },
                    Some(source) => {
                        let name = text(pkg, "name").unwrap_or_default();
                        within &= source.starts_with("registry+") && DEPENDENCIES.contains(&name.as_str());
                    },
                }
                todo.push(dep_id);
            }
        }
        if within {
            closure.sort();
            out.insert(root_dir, closure);
        }
    }
    out
}

/// Whether a macro whose dep-info is `info`, with the closure of the tree's crates `closure`, can
/// be held to its sources. Relative sources are read against `gen_dir`, as the build ran there.
pub(crate) fn held(info: &str, gen_dir: &Path, closure: &[PathBuf]) -> bool {
    let sources = super::depinfo::sources_of(info);
    if sources.is_empty() {
        return false;
    }
    let own = sources.iter().map(|s| if s.is_absolute() { s.clone() } else { gen_dir.join(s) });
    own.chain(closure.iter().flat_map(|dir| rust_files(dir)))
        .all(|file| std::fs::read_to_string(&file).is_ok_and(|text| source_is_pure(&text)))
}

/// Every `.rs` file under `dir`, except a `target` directory. A directory that cannot be read
/// yields the path itself, which then cannot be read as a source either.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![dir.to_path_buf()];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                out.extend(rust_files(&path));
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// Whether the standard library path whose segments are `segments` is one of the pure ones.
fn path_is_pure(segments: &[&str]) -> bool {
    match segments {
        [root, module, ..] if ROOTS.contains(root) => PURE_MODULES.contains(module),
        [root] => !ROOTS.contains(root),
        _ => true,
    }
}

/// The segments of a path as written, without whitespace and a leading `::`.
fn segments_of(text: &str) -> Vec<String> {
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.split("::").filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// Whether every path a `use` brings in, each under whatever name, is pure.
fn use_is_pure(node: Node, src: &[u8], prefix: &[String]) -> bool {
    let text = |n: Node| n.utf8_text(src).unwrap_or("").to_string();
    let with = |tail: Vec<String>| [prefix, tail.as_slice()].concat();
    match node.kind() {
        "use_as_clause" => node.child_by_field_name("path").is_some_and(|p| use_is_pure(p, src, prefix)),
        "use_wildcard" => {
            let body = text(node);
            let path = body.trim_end().trim_end_matches('*').trim_end_matches(':');
            let all = with(segments_of(path));
            // A glob of a root alone brings in the whole library.
            !(all.len() == 1 && ROOTS.contains(&all[0].as_str())) && path_is_pure(&refs(&all))
        },
        "scoped_use_list" => {
            let head = node.child_by_field_name("path").map(|p| segments_of(&text(p))).unwrap_or_default();
            let base = with(head);
            node.child_by_field_name("list").is_some_and(|list| {
                let mut walk = list.walk();
                list.named_children(&mut walk).all(|c| use_is_pure(c, src, &base))
            })
        },
        "use_list" => {
            let mut walk = node.walk();
            node.named_children(&mut walk).all(|c| use_is_pure(c, src, prefix))
        },
        _ => {
            let mut all = with(segments_of(&text(node)));
            if all.last().is_some_and(|l| l == "self") {
                all.pop();
            }
            path_is_pure(&refs(&all))
        },
    }
}

/// Whether the leaves of a token tree name a refused path of the standard library, as they are
/// the code of a macro that is not a quoting one.
fn tokens_are_pure(node: Node, src: &[u8]) -> bool {
    let mut leaves = Vec::new();
    collect_leaves(node, src, &mut leaves);
    for (i, leaf) in leaves.iter().enumerate() {
        if ROOTS.contains(leaf) {
            let module = (leaves.get(i + 1) == Some(&"::")).then(|| leaves.get(i + 2)).flatten();
            if !module.is_some_and(|m| PURE_MODULES.contains(m)) {
                return false;
            }
        }
    }
    true
}

fn collect_leaves<'a>(node: Node, src: &'a [u8], out: &mut Vec<&'a str>) {
    if node.child_count() == 0 {
        out.push(std::str::from_utf8(&src[node.byte_range()]).unwrap_or(""));
        return;
    }
    let mut walk = node.walk();
    for child in node.children(&mut walk) {
        collect_leaves(child, src, out);
    }
}

fn node_is_pure(node: Node, src: &[u8]) -> bool {
    let text = |n: Node| n.utf8_text(src).unwrap_or("").to_string();
    match node.kind() {
        "use_declaration" => {
            return node.child_by_field_name("argument").is_some_and(|a| use_is_pure(a, src, &[]));
        },
        "foreign_mod_item" => return false,
        "extern_crate_declaration" => {
            let squeezed: String = text(node).chars().filter(|c| !c.is_whitespace()).collect();
            return squeezed == "externcrateproc_macro;";
        },
        "macro_invocation" => {
            let name = node.child_by_field_name("macro").map(&text).unwrap_or_default();
            let last = segments_of(&name).pop().unwrap_or_default();
            if ASSEMBLY.contains(&last.as_str()) {
                return false;
            }
            if QUOTING.contains(&last.as_str()) {
                // The name itself may be a refused path; the body is generated code.
                return node.child_by_field_name("macro").is_none_or(|m| node_is_pure(m, src));
            }
        },
        "token_tree" => {
            if !tokens_are_pure(node, src) {
                return false;
            }
        },
        k if k.starts_with("scoped_")
            && k != "scoped_use_list"
            && !node.parent().is_some_and(|p| p.kind().starts_with("scoped_") && p.kind() != "scoped_use_list") =>
        {
            let all = segments_of(&text(node));
            if !path_is_pure(&refs(&all)) {
                return false;
            }
        },
        _ => {},
    }
    let mut walk = node.walk();
    node.children(&mut walk).all(|c| node_is_pure(c, src))
}

/// Whether the source text of a crate names nothing of the standard library but the pure modules,
/// no foreign block and no assembly. A source that does not parse is not pure.
pub(crate) fn source_is_pure(text: &str) -> bool {
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_err() {
        return false;
    }
    let Some(tree) = parser.parse(text, None) else {
        return false;
    };
    let root = tree.root_node();
    !root.has_error() && node_is_pure(root, text.as_bytes())
}

#[cfg(test)]
#[path = "macro_purity_tests.rs"]
mod tests;
