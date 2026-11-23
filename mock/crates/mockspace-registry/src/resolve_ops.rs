//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The three operations a reference may be, `pathof`, `sourcesof` and `refsto`, and a query that
//! selects rows, which each of them takes as well as a plain reference does.

use super::resolve::{rel_to_repo, resolve_expr};
use super::resolved::{Resolved, table_cells_of};
use super::*;

/// Where the named thing is declared: the file to open to change it, or for a query selecting
/// several rows, each file once in the order their rows came.
pub(crate) fn resolve_pathof(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    repo_root: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<Resolved> {
    let parts = crate::names(expr).unwrap_or_default();

    if parts.first() == Some(&CRATE_ROOT) && parts.len() == 2 {
        return cfg.crate_dir(parts[1]).map(|dir| Resolved::Path(rel_to_repo(&dir, repo_root)));
    }

    if let Some(rows) = rows_named(expr, by_key, reg, cfg) {
        let rows = rows?;
        if let (Ok(_), [one]) = (crate::names(expr), rows.as_slice()) {
            if reg.locate(&parts).is_some() {
                return Some(Resolved::Path(rel_to_repo(&one.source, repo_root)));
            }
        }
        let mut files: Vec<String> = Vec::new();
        for row in rows {
            let f = rel_to_repo(&row.source, repo_root);
            if !files.contains(&f) {
                files.push(f);
            }
        }
        return Some(Resolved::List(files));
    }

    // A citation names a file, and that file is where the thing is.
    let r = FileRef::parse(expr)?;
    if cfg.internal_roots().contains(&r.root) {
        return Some(Resolved::Path(String::new()));
    }
    let rel = cfg.roots().get(&r.root)?;
    match r.locate(&repo_root.join(rel)) {
        Location::Found(target) => Some(Resolved::Path(rel_to_repo(&target, repo_root))),
        _ => None,
    }
}

/// What the named rows rest on: their provenance, resolved as citations, each once.
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
) -> Option<Vec<String>> {
    let rows = rows_named(expr, by_key, reg, cfg)??;
    // A row that rests on nothing it states has no sources to give, which is no empty answer.
    if !rows.iter().any(|r| r.fields.contains_key("provenance")) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        let Some(raw) = row.fields.get("provenance") else { continue };
        for c in crate::refpath::list(raw) {
            let Some(r) = resolve_expr(c, by_key, reg, roots, repo_root, docs_dir, cfg) else { continue };
            if !r.is_empty() && !out.contains(&r) {
                out.push(r);
            }
        }
    }
    Some(out)
}

/// What references the named rows, resolved as links to the referring rows, each once.
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
    // A row that does not exist is not a row with no referrers. Returning an
    // empty list for it would answer a question about nothing, and the empty
    // answer is the one a reader is most likely to act on.
    let rows = rows_named(expr, by_key, reg, cfg)??;
    let edges = super::reverse_edges(reg, cfg.namespaces());
    let mut seen = BTreeSet::new();
    Some(
        rows.iter()
            .flat_map(|row| edges.get(&row.qualified()).cloned().unwrap_or_default())
            .filter(|q| seen.insert(q.clone()))
            .filter_map(|q| resolve_expr(&q, by_key, reg, roots, repo_root, docs_dir, cfg))
            .collect(),
    )
}

/// A query that selects in the registry, as a value: a widening path, or a plain one naming a
/// branch of the id tree. `None` when `expr` is neither, so the caller reads it another way; an
/// inner `None` when it is one and is refused or reaches nothing a plain path must.
///
/// Rows of one namespace come as that namespace's table holding them, which every table method
/// narrows further; anything else is a list, a row as its `namespace::id` and a field as its value.
pub(crate) fn resolve_selection(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    cfg: &dyn RegistryEnv,
) -> Option<Option<Resolved>> {
    let query = selection_query(expr, by_key, reg)?;
    let Ok(hits) = reg.select(cfg.namespaces(), &query) else { return Some(None) };
    let mut namespace: Option<&str> = query_namespace(&query, by_key);
    let mut only_rows = true;
    for h in &hits {
        match h {
            Selected::Row(r) if namespace.is_none_or(|n| n == r.namespace) => namespace = Some(&r.namespace),
            Selected::Row(_) => namespace = Some(""),
            Selected::Field(..) => only_rows = false,
        }
    }
    if let (true, Some(ns)) = (only_rows, namespace.and_then(|n| by_key.get(n))) {
        let ids: Vec<String> = hits
            .iter()
            .filter_map(|h| {
                match h {
                    Selected::Row(r) => Some(r.qualified()),
                    Selected::Field(..) => None,
                }
            })
            .collect();
        let (columns, rows) = table_cells_of(ns, reg, &ids);
        return Some(Some(Resolved::Table {
            namespace: ns.key.clone(),
            columns,
            rows,
        }));
    }
    Some(Some(Resolved::List(
        hits.into_iter()
            .map(|h| {
                match h {
                    Selected::Row(r) => r.qualified(),
                    Selected::Field(r, f) if f == "id" => r.id.clone(),
                    Selected::Field(r, f) => r.fields.get(&f).cloned().unwrap_or_default(),
                }
            })
            .collect(),
    )))
}

/// The text to select with, where `expr` is a query over the registry: a widening path as written,
/// or a plain path naming a branch with its namespace in the standard's spelling.
fn selection_query(expr: &str, by_key: &BTreeMap<&str, &RegistryNamespace>, reg: &Registry) -> Option<String> {
    if let Ok(names) = crate::names(expr) {
        let names = names_after_root(&names);
        let (namespace, _) = names.split_first()?;
        if !by_key.contains_key(namespace) || reg.locate(names).is_some() || !reg.is_branch(names) {
            return None;
        }
        let folded = mockspace_refnames::fold(namespace)?.to_string();
        let mut text = vec![folded.as_str()];
        text.extend(&names[1 ..]);
        return Some(text.join("::"));
    }
    let path = letkajenkka::Query::parse(expr).ok()?.path()?;
    path.widens().then(|| expr.trim().to_string())
}

/// The namespace every expansion of `query` opens on, by a name it folds to.
fn query_namespace<'a>(query: &str, by_key: &BTreeMap<&str, &'a RegistryNamespace>) -> Option<&'a str> {
    let path = letkajenkka::Query::parse(query).ok()?.path()?;
    let mut found: Option<&'a str> = None;
    for variant in path.expand()? {
        let steps: Vec<letkajenkka::Step<'_>> = variant.steps().collect();
        let steps = after_registry_root(&steps, |s| s.selector == letkajenkka::Selector::Ident(REGISTRY_ROOT));
        let letkajenkka::Selector::Ident(name) = steps.first()?.selector else { return None };
        let ns = by_key.values().find(|ns| mockspace_refnames::fold(&ns.key).is_some_and(|f| f == name))?;
        match found {
            Some(k) if k != ns.key => return None,
            _ => found = Some(&ns.key),
        }
    }
    found
}

/// The rows a reference names for an operation: the one row a plain path names whole, or the rows
/// a selecting query selects. `None` when `expr` names neither; an inner `None` when it is refused,
/// names a row and a field, or reaches a field.
fn rows_named<'r>(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &'r Registry,
    cfg: &dyn RegistryEnv,
) -> Option<Option<Vec<&'r RegistryRow>>> {
    if let Ok(names) = crate::names(expr) {
        if !names_after_root(&names).first().is_some_and(|n| by_key.contains_key(n)) {
            return None;
        }
        if let Some(found) = reg.locate(&names) {
            return Some(found.fields.is_empty().then(|| vec![found.row]));
        }
    }
    let query = selection_query(expr, by_key, reg)?;
    let Ok(hits) = reg.select(cfg.namespaces(), &query) else { return Some(None) };
    let mut rows = Vec::new();
    for h in hits {
        match h {
            Selected::Row(r) => rows.push(r),
            Selected::Field(..) => return Some(None),
        }
    }
    Some(Some(rows))
}

/// The rows a value's references read: each row a plain reference names, and each row a selecting
/// query selects, so a row citing `{{ ticket::question::** }}` is resolved after every row it lists.
pub(crate) fn row_deps(
    value: &str,
    ns_keys: &BTreeSet<String>,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
) -> Vec<String> {
    let mut deps: Vec<String> = find_registry_refs(value, ns_keys, reg).into_iter().map(|(q, _)| q).collect();
    for (_, expr) in placeholder_exprs(value) {
        let base = crate::chain::split_methods(&expr).map_or(expr.clone(), |(b, _)| b);
        let inner = ["pathof", "refsto", "sourcesof"]
            .iter()
            .find_map(|op| crate::operation(&base, op))
            .unwrap_or(&base);
        let Some(query) = selection_query(inner, by_key, reg) else { continue };
        if let Ok(rows) = reg.select_rows(namespaces, &query) {
            deps.extend(rows.into_iter().map(RegistryRow::qualified));
        }
    }
    deps
}
