//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a crate declares, read the way `single-source` reads it, for a tool that asks about types
//! across crates and is not a lint.
//!
//! `single-source` gates on one finding: the same name over the same canonical declaration in two
//! crates. A question that wants the declarations themselves, to tell how close two of them are or to
//! list the ones sharing a name over different declarations, reads them from here, so that the files a
//! crate counts, the tests it leaves out and the canonical form of a declaration are the lint's and
//! not a second reading of them.

use super::defs::{Item, collect_sources};
use std::collections::HashMap;

use super::shape::{Parts, canonical_parts};
use crate::{CrateSourceFile, make_parser};

pub use super::shape::{Member as DeclaredMember, Parts as DeclaredParts};

/// One struct, enum, trait or union a crate defines outside test code.
#[derive(Clone, Debug)]
pub struct DeclaredType {
    pub crate_name: String,
    /// The file within the crate, `None` for the crate root.
    pub path:       Option<String>,
    pub line:       usize,
    pub name:       String,
    /// `struct`, `enum`, `trait` or `union`.
    pub kind:       &'static str,
    /// The item's own visibility as written, empty when it has none.
    pub vis:        String,
    /// The whole declaration in canonical form, the name included: what `single-source` groups by.
    pub shape:      String,
    /// The generics and members in canonical form, the name left out.
    pub parts:      Parts,
}

impl DeclaredType {
    /// Whether the type is `pub` with no restriction.
    #[must_use]
    pub fn fully_pub(&self) -> bool {
        self.vis == "pub"
    }
}

/// Every type the crate defines outside test code, from its source files (crate-relative paths such
/// as `src/lib.rs`).
#[must_use]
pub fn declared_types(crate_name: &str, sources: &[CrateSourceFile]) -> Vec<DeclaredType> {
    let mut parser = make_parser();
    let trees: HashMap<String, (tree_sitter::Tree, &str)> = sources
        .iter()
        .filter_map(|s| {
            let tree = parser.parse(&s.text, None)?;
            let key = s.rel_path.to_string_lossy().replace('\\', "/");
            Some((key, (tree, s.text.as_str())))
        })
        .collect();
    collect_sources(crate_name, sources)
        .into_iter()
        .filter_map(|def| {
            let Item::Type(ty) = def.item else { return None };
            let key = def.path.clone().unwrap_or_else(|| "src/lib.rs".to_string());
            let (tree, text) = trees.get(&key)?;
            let node = declaration_at(tree.root_node(), def.line - 1, &def.name, text)?;
            Some(DeclaredType {
                crate_name: def.crate_name,
                path: def.path,
                line: def.line,
                name: def.name,
                kind: ty.kind,
                vis: def.vis,
                shape: ty.shape,
                parts: canonical_parts(node, text),
            })
        })
        .collect()
}

/// The type item that starts on `row` and carries `name`, found by walking down the tree.
fn declaration_at<'t>(
    node: tree_sitter::Node<'t>,
    row: usize,
    name: &str,
    source: &str,
) -> Option<tree_sitter::Node<'t>> {
    if node.start_position().row > row || node.end_position().row < row {
        return None;
    }
    let is_type = matches!(node.kind(), "struct_item" | "enum_item" | "trait_item" | "union_item");
    if is_type
        && node.start_position().row == row
        && node
            .child_by_field_name("name")
            .is_some_and(|n| &source[n.byte_range()] == name)
    {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| declaration_at(child, row, name, source))
}

