//--------------------------------------------------------------------------------------------------
// Copyright () 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Collecting what one crate defines, from every file that belongs to it.
//!
//! The files come from `LintContext::all_sources`, and which of them count is
//! decided by following `mod` declarations from the crate roots, the way the
//! compiler does: `mod x;` resolves to `x.rs` or `x/mod.rs` beside the declaring
//! module, `#[path = ".."]` overrides that, and an inline `mod x { .. }` is
//! walked in place and nests the directory for what it declares. A file reached
//! only through `#[cfg(test)]` is test code and defines nothing here. A file no
//! declaration reaches is counted as ordinary source, because a definition in it
//! is a definition for as long as something includes it.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use tree_sitter::Node;

use super::value::{canonical_type, canonical_value};
use crate::{CrateSourceFile, LintContext, attribute_is_cfg_test, is_cfg_test_mod, make_parser};

/// One definition found in a crate.
pub(super) struct Def {
    pub crate_name: String,
    /// The file within the crate, `None` for the crate root.
    pub path:       Option<String>,
    pub name:       String,
    pub line:       usize,
    /// The item's own visibility qualifier as written: `pub`, `pub(crate)`, or
    /// empty for a private item.
    pub vis:        String,
    pub item:       Item,
}

impl Def {
    /// Whether the item is `pub` with no restriction. Only the item's own
    /// qualifier is read, not whether the module holding it is reachable.
    pub fn fully_pub(&self) -> bool {
        self.vis == "pub"
    }
}

pub(super) enum Item {
    /// A struct, enum, trait or union, by the keyword that introduced it.
    Type(&'static str),
    /// A constant or static whose type and value could both be put in a
    /// canonical form.
    Value(ValueDef),
}

pub(super) struct ValueDef {
    /// `const`, `static` or `static mut`.
    pub kind:  &'static str,
    pub ty:    String,
    pub value: String,
}

const CROSS_CRATE_TYPES: &[(&str, &str)] = &[
    ("struct_item", "struct"),
    ("enum_item", "enum"),
    ("trait_item", "trait"),
    ("union_item", "union"),
];

struct File<'a> {
    key:       String,
    text:      &'a str,
    tree:      tree_sitter::Tree,
    /// `#![cfg(test)]` at the top of the file.
    test_only: bool,
}

/// Every definition the crate makes outside test code.
pub(super) fn collect_crate(crate_name: &str, ctx: &LintContext) -> Vec<Def> {
    let fallback;
    let sources: &[CrateSourceFile] = if ctx.all_sources.is_empty() {
        fallback = [CrateSourceFile {
            rel_path: PathBuf::from("src/lib.rs"),
            text:     ctx.source.to_string(),
        }];
        &fallback
    } else {
        ctx.all_sources
    };

    let mut parser = make_parser();
    let files: Vec<File> = sources
        .iter()
        .filter_map(|s| {
            let tree = parser.parse(&s.text, None)?;
            let test_only = has_inner_cfg_test(tree.root_node(), &s.text);
            Some(File {
                key: normalise(&s.rel_path),
                text: &s.text,
                tree,
                test_only,
            })
        })
        .collect();
    let index: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.key.as_str(), i))
        .collect();

    let mut walker = Walker {
        crate_name,
        files: &files,
        index: &index,
        seen: HashSet::new(),
        reached: HashSet::new(),
        out: Vec::new(),
    };
    for (i, file) in files.iter().enumerate() {
        if let Some(mod_rs) = root_kind(&file.key) {
            walker.visit_file(i, false, mod_rs);
        }
    }
    for i in 0 .. files.len() {
        if !walker.reached.contains(&i) {
            walker.visit_file(i, false, false);
        }
    }
    walker.out
}

struct Walker<'a> {
    crate_name: &'a str,
    files:      &'a [File<'a>],
    index:      &'a HashMap<&'a str, usize>,
    /// `(file, under cfg(test))`: each is walked once, which also ends a cycle.
    seen:       HashSet<(usize, bool)>,
    reached:    HashSet<usize>,
    out:        Vec<Def>,
}

/// Where the modules a file declares are looked for.
#[derive(Clone)]
struct Scope {
    /// The directory `#[path]` is relative to.
    file_dir: PathBuf,
    /// The directory `mod x;` is resolved in.
    mod_dir:  PathBuf,
}

impl<'a> Walker<'a> {
    fn visit_file(&mut self, idx: usize, test: bool, mod_rs: bool) {
        let files = self.files;
        let file = &files[idx];
        let test = test || file.test_only;
        self.reached.insert(idx);
        if !self.seen.insert((idx, test)) {
            return;
        }
        let path = Path::new(&file.key);
        let parent = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let owns_dir = mod_rs || matches!(stem, "lib" | "main" | "mod");
        let scope = Scope {
            mod_dir:  if owns_dir { parent.clone() } else { parent.join(stem) },
            file_dir: parent,
        };
        self.walk(file.tree.root_node(), file.text, idx, &scope, test);
    }

    fn walk(&mut self, parent: Node, source: &str, file: usize, scope: &Scope, test: bool) {
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            match child.kind() {
                "struct_item" | "enum_item" | "trait_item" | "union_item" => {
                    if !test && !under_cfg_test(child, source) {
                        self.push_type(child, source, file);
                    }
                },
                "const_item" | "static_item" => {
                    if !test && !under_cfg_test(child, source) {
                        self.push_value(child, source, file);
                    }
                },
                "mod_item" => self.module(child, source, file, scope, test),
                _ => {},
            }
        }
    }

    fn module(&mut self, node: Node, source: &str, file: usize, scope: &Scope, test: bool) {
        let test = test || is_cfg_test_mod(node, source) || under_cfg_test(node, source);
        let Some(name) = node
            .child_by_field_name("name")
            .map(|n| &source[n.byte_range()])
        else {
            return;
        };
        let path_attr = path_attribute(node, source);
        if let Some(body) = node.child_by_field_name("body") {
            let inner = match path_attr {
                Some(p) => {
                    let dir = scope.file_dir.join(p);
                    Scope {
                        file_dir: dir.clone(),
                        mod_dir:  dir,
                    }
                },
                None => {
                    Scope {
                        file_dir: scope.mod_dir.join(name),
                        mod_dir:  scope.mod_dir.join(name),
                    }
                },
            };
            self.walk(body, source, file, &inner, test);
            return;
        }
        let candidates: Vec<PathBuf> = match path_attr {
            Some(p) => vec![scope.file_dir.join(p)],
            None => {
                vec![
                    scope.mod_dir.join(format!("{name}.rs")),
                    scope.mod_dir.join(name).join("mod.rs"),
                ]
            },
        };
        for candidate in candidates {
            if let Some(&target) = self.index.get(normalise(&candidate).as_str()) {
                // A file loaded through `#[path]` owns its directory, like a `mod.rs`.
                self.visit_file(target, test, path_attr.is_some());
                return;
            }
        }
    }

    fn push_type(&mut self, node: Node, source: &str, file: usize) {
        let Some(name) = name_of(node, source) else {
            return;
        };
        let label = CROSS_CRATE_TYPES
            .iter()
            .find(|(kind, _)| *kind == node.kind())
            .map(|(_, label)| *label)
            .unwrap_or("type");
        self.push(node, source, name, file, Item::Type(label));
    }

    fn push_value(&mut self, node: Node, source: &str, file: usize) {
        let Some(name) = name_of(node, source) else {
            return;
        };
        let (Some(ty), Some(value)) = (
            node.child_by_field_name("type"),
            node.child_by_field_name("value"),
        ) else {
            return;
        };
        let Some(value) = canonical_value(value, source) else {
            return;
        };
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        let mutable = children.iter().any(|c| c.kind() == "mutable_specifier");
        let kind = match (node.kind(), mutable) {
            ("const_item", _) => "const",
            (_, true) => "static mut",
            _ => "static",
        };
        let ty = canonical_type(ty, source);
        self.push(
            node,
            source,
            name,
            file,
            Item::Value(ValueDef {
                kind,
                ty,
                value,
            }),
        );
    }

    fn push(&mut self, node: Node, source: &str, name: String, file: usize, item: Item) {
        let key = &self.files[file].key;
        self.out.push(Def {
            crate_name: self.crate_name.to_string(),
            path: (key != "src/lib.rs").then(|| key.clone()),
            name,
            line: node.start_position().row + 1,
            vis: visibility(node, source),
            item,
        });
    }
}

/// The item's own name. `const _: () = ..` has none and defines nothing.
fn name_of(node: Node, source: &str) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    let text = &source[name.byte_range()];
    (matches!(name.kind(), "identifier" | "type_identifier") && text != "_")
        .then(|| text.to_string())
}

/// The attribute texts directly above an item, over doc comments.
fn leading_attributes<'s>(node: Node, source: &'s str) -> Vec<&'s str> {
    let mut out = Vec::new();
    let mut prev = node.prev_named_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            "line_comment" | "block_comment" => {},
            "attribute_item" => out.push(&source[sibling.byte_range()]),
            _ => break,
        }
        prev = sibling.prev_named_sibling();
    }
    out
}

fn under_cfg_test(node: Node, source: &str) -> bool {
    leading_attributes(node, source)
        .into_iter()
        .any(attribute_is_cfg_test)
}

/// The string of a `#[path = ".."]` on a module.
fn path_attribute<'s>(node: Node, source: &'s str) -> Option<&'s str> {
    leading_attributes(node, source)
        .into_iter()
        .find_map(|attr| {
            let inner = attr.trim().strip_prefix("#[")?.strip_suffix(']')?.trim();
            let rest = inner.strip_prefix("path")?.trim_start().strip_prefix('=')?;
            rest.trim().strip_prefix('"')?.strip_suffix('"')
        })
}

fn has_inner_cfg_test(root: Node, source: &str) -> bool {
    let mut cursor = root.walk();
    root.children(&mut cursor).any(|c| {
        c.kind() == "inner_attribute_item" && attribute_is_cfg_test(&source[c.byte_range()])
    })
}

/// A path with `.` and `..` resolved lexically, as a `/` string, which is the
/// key `all_sources` entries and `mod` resolutions meet at.
fn normalise(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {},
            Component::ParentDir => {
                parts.pop();
            },
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    parts.join("/")
}

/// The item's visibility qualifier as written, empty when it has none.
fn visibility(node: Node, source: &str) -> String {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|c| c.kind() == "visibility_modifier")
        .map(|c| {
            source[c.byte_range()]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// Whether a file is the root of a target, and if so whether it is a mod-rs
/// file (its modules resolve beside it): `src/lib.rs`, `src/main.rs`,
/// `src/bin/<name>.rs` and `src/bin/<name>/main.rs`. A custom `[lib] path` or
/// `[[bin]] path` is not read; see the module doc.
fn root_kind(key: &str) -> Option<bool> {
    match key.split('/').collect::<Vec<_>>()[..] {
        ["src", "lib.rs"] | ["src", "main.rs"] => Some(false),
        ["src", "bin", file] if file.ends_with(".rs") => Some(true),
        ["src", "bin", _, "main.rs"] => Some(false),
        _ => None,
    }
}
