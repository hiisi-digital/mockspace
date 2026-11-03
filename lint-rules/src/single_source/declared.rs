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
//! not a second reading of them. The shape and the parts of a type are read at the one node, in the
//! one walk.

use super::defs::{Item, collect_sources};
use crate::CrateSourceFile;
pub use super::shape::{Member as DeclaredMember, Parts as DeclaredParts};

/// One struct, enum, trait or union a crate defines outside test code.
#[derive(Clone, Debug)]
pub struct DeclaredType {
    pub crate_name: String,
    /// The file within the crate, `None` for `src/lib.rs`.
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
    pub parts:      DeclaredParts,
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
    collect_sources(crate_name, sources)
        .into_iter()
        .filter_map(|def| {
            let Item::Type(ty) = def.item else { return None };
            Some(DeclaredType {
                crate_name: def.crate_name,
                path: def.path,
                line: def.line,
                name: def.name,
                kind: ty.kind,
                vis: def.vis,
                shape: ty.shape,
                parts: ty.parts,
            })
        })
        .collect()
}
