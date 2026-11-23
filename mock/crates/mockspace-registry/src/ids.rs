//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A row's id as a path.
//!
//! A row's id is a path of one or more names under the letkajenkka standard: `allowed`, or
//! `question::engine::allowed`. The ids of one namespace are the leaves of a tree whose branches
//! are their proper prefixes, and the standard gives every node one type, so no id may be a proper
//! prefix of another in the same namespace (`id-is-a-branch`). That is what lets the walker
//! (`walk`) read `ticket::question::engine::allowed::status` without being told where the id ends.

use super::*;

/// The names of a row id, each a plain name under the standard and the id nothing else.
///
/// # Errors
///
/// The text as a finding says it, when it is not a path of one or more plain names: empty, refused
/// by the standard, widening, or carrying a name in a host's own spelling (a hyphen), which a
/// reference may root on and an id may not.
pub fn row_id_names(id: &str) -> Result<Vec<&str>, String> {
    let names = crate::names(id).map_err(|r| r.say(id))?;
    match names.iter().find(|n| !is_valid_slug(n)) {
        Some(bad) => Err(format!("`{id}` is no row id: `{bad}` is not a name under the letkajenkka standard")),
        None => Ok(names),
    }
}

/// Whether `id` is a row id: a path of one or more plain names (see [`row_id_names`]).
pub fn is_valid_row_id(id: &str) -> bool {
    row_id_names(id).is_ok()
}

/// The anchor a row with this id carries on its namespace page.
///
/// Each name with its underscores written as hyphens, which is the anchor a one-name id has always
/// had, and the names joined by a dot. A name holds no hyphen and no dot, so the map is one to one:
/// `a::b_c` anchors as `a.b-c` and `a_b::c` as `a-b.c`, where joining by a hyphen would anchor both
/// as `a-b-c`, and `a__b` (a name) as `a--b` and `a::b` as `a.b`.
pub fn row_anchor(id: &str) -> String {
    match row_id_names(id) {
        Ok(names) => names.iter().map(|n| n.replace('_', "-")).collect::<Vec<_>>().join("."),
        // An id that is no path still gets an anchor, the old one, and the schema reports the id.
        Err(_) => id.replace('_', "-"),
    }
}

/// `steps` without the registry root it opens on, where it opens on one and a step follows it:
/// `reg::ticket::a` reads as `ticket::a`. The one place the optional root is passed over, for names
/// and for a parsed path's steps alike.
pub fn after_registry_root<T>(steps: &[T], is_root: impl Fn(&T) -> bool) -> &[T] {
    match steps {
        [first, _, ..] if is_root(first) => &steps[1 ..],
        _ => steps,
    }
}

/// [`after_registry_root`] over the names of a reference.
pub fn names_after_root<'a, 'n>(names: &'a [&'n str]) -> &'a [&'n str] {
    after_registry_root(names, |n| *n == REGISTRY_ROOT)
}

/// Every pair of ids of one namespace where the first is a proper prefix of the second, shorter
/// first and in byte order: `question::engine` and `question::engine::allowed`, never
/// `question::engine` and `question::engine_x`. A pure function of the ids, for a tool that holds
/// rows its own way; [`id_branches`] is the same over a loaded registry.
pub fn id_prefix_pairs<'a>(ids: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    // Sorted, so every id an id is a prefix of follows it before any id that is not.
    for (i, short) in ids.iter().enumerate() {
        let stem = format!("{short}::");
        for long in ids[i + 1 ..].iter().take_while(|l| l.starts_with(short)) {
            if long.starts_with(&stem) {
                out.push((*short, *long));
            }
        }
    }
    out
}

/// The `id-is-a-branch` finding's text for `short`, a proper prefix of `long`, each with where it is
/// written (a file, or whatever place a tool holding rows its own way can name).
pub fn id_is_a_branch_message(short: &str, short_at: &str, long: &str, long_at: &str) -> String {
    format!(
        "{short} ({short_at}) is a proper prefix of {long} ({long_at}). A row's id is a leaf of its \
         namespace's tree, and this one would be a record and a branch at once, so a reference \
         through it could not say whether a name after it is a field or a step to the longer id. \
         Rename one of the two."
    )
}

/// Every row whose id is a proper prefix of another row's id in its namespace.
///
/// The shorter one would be a record and a branch at once, which the standard refuses (§5), and a
/// reference reaching through it could not say whether its next name is a field of the row or a
/// step to the longer id. An error, like `duplicate-id`, naming both rows and both files.
pub fn id_branches(reg: &Registry) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    for keys in reg.by_namespace.values() {
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        for (short, long) in id_prefix_pairs(&keys) {
            let (Some(a), Some(b)) = (reg.rows.get(short), reg.rows.get(long)) else { continue };
            out.push(RegistryFinding {
                kind:    "id-is-a-branch",
                message: id_is_a_branch_message(short, &a.source.display().to_string(), long, &b.source.display().to_string()),
                source:  Some(a.source.clone()),
                gate:    GateScope::Always,
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "ids_tests.rs"]
mod tests;
