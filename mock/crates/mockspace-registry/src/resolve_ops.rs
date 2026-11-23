//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The three operations a reference may be: `pathof`, `sourcesof` and `refsto`.

use super::resolve::{rel_to_repo, resolve_expr};
use super::*;

/// Where the named thing is declared: the file to open to change it.
pub(crate) fn resolve_pathof(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    repo_root: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<String> {
    let parts = crate::names(expr).unwrap_or_default();

    if parts.first() == Some(&CRATE_ROOT) && parts.len() == 2 {
        return cfg.crate_dir(parts[1]).map(|dir| rel_to_repo(&dir, repo_root));
    }

    if names_a_namespace(&parts, by_key) {
        return row_named(&parts, reg).map(|row| rel_to_repo(&row.source, repo_root));
    }

    // A citation names a file, and that file is where the thing is.
    let r = FileRef::parse(expr)?;
    if cfg.internal_roots().contains(&r.root) {
        return Some(String::new());
    }
    let rel = cfg.roots().get(&r.root)?;
    match r.locate(&repo_root.join(rel)) {
        Location::Found(target) => Some(rel_to_repo(&target, repo_root)),
        _ => None,
    }
}

/// What the named thing rests on: its provenance, resolved as citations.
///
/// Each entry goes through ordinary citation resolution, so an internal root
/// drops here too rather than leaking through a second door.
pub(crate) fn resolve_sourcesof(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<String> {
    let names = crate::names(expr).ok()?;
    if !names_a_namespace(&names, by_key) {
        return None;
    }
    let row = row_named(&names, reg)?;
    let raw = row.fields.get("provenance")?;
    let rendered: Vec<String> = crate::refpath::list(raw)
        .filter_map(|c| resolve_expr(c, by_key, reg, roots, repo_root, docs_dir, cfg))
        .filter(|r| !r.is_empty())
        .collect();
    Some(rendered.join(", "))
}

/// What references the named row, resolved as links to the referring rows.
///
/// The inverse of a typed row-reference field, derived rather than stored. A
/// row states what it points at; nothing states what points at it, so there is
/// no second list to fall out of step with the first.
///
/// This is the direction most questions are asked in. A field typed `slot`
/// lets a row say which slot it answers; `refsto(slot::display)` is how a
/// reader finds out whether anything answers that slot at all, and an empty
/// result is the finding rather than an absence of one.
pub(crate) fn resolve_refsto(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<Vec<String>> {
    let names = crate::names(expr).ok()?;
    if !names_a_namespace(&names, by_key) {
        return None;
    }
    // A row that does not exist is not a row with no referrers. Returning an
    // empty list for it would answer a question about nothing, and the empty
    // answer is the one a reader is most likely to act on.
    let qualified = row_named(&names, reg)?.qualified();
    Some(
        super::referrers(reg, cfg.namespaces(), &qualified)
            .into_iter()
            .filter_map(|q| resolve_expr(&q, by_key, reg, roots, repo_root, docs_dir, cfg))
            .collect(),
    )
}

/// Whether the names open on a declared namespace, after the optional registry root.
fn names_a_namespace(names: &[&str], by_key: &BTreeMap<&str, &RegistryNamespace>) -> bool {
    let at = usize::from(names.first() == Some(&REGISTRY_ROOT));
    names.get(at).is_some_and(|n| by_key.contains_key(n))
}

/// The row the names name whole, with no field step after its id: what `pathof`, `sourcesof` and
/// `refsto` take.
fn row_named<'r>(names: &[&str], reg: &'r Registry) -> Option<&'r RegistryRow> {
    reg.locate(names).filter(|found| found.fields.is_empty()).map(|found| found.row)
}

