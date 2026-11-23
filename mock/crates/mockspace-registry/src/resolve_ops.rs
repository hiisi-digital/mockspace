//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A path into the registry as a value, and the three operations a reference may be, `pathof`,
//! `sourcesof` and `refsto`, each over what the one walker reaches.

use super::resolve::{rel_to_repo, resolve_expr};
use super::resolved::{Resolved, row_fields, table_cells_of};
use super::*;

/// What a path into the registry resolves to. `None` where `expr` is no such path, so the caller
/// reads it another way; an inner `None` where it is one and is refused, or names a field the row
/// does not carry.
///
/// A plain read of one row or one field shows as that row or that value, as it always has. Rows of
/// one namespace otherwise come as that namespace's table holding just them, which every table
/// method narrows; anything else is a list, a row as its `namespace::id` and a field as its value,
/// a field a row does not carry left out.
pub(crate) fn resolve_registry(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<Option<Resolved>> {
    let path = registry_path(expr, by_key.keys().copied())?;
    let Ok(found) = reg.select_in(by_key, &path) else { return Some(None) };
    let field = |r: &RegistryRow, f: &str| render_field(r, f, by_key, reg, roots, repo_root, docs_dir, cfg);
    if found.single {
        return Some(match &found.items[0] {
            Selected::Row(r) => render_row(r, by_key),
            Selected::Field(r, f) => field(r, f),
        });
    }
    let only_rows = found.items.iter().all(|s| matches!(s, Selected::Row(_)));
    let namespace = found
        .items
        .first()
        .map(|s| {
            match s {
                Selected::Row(r) | Selected::Field(r, _) => r.namespace.as_str(),
            }
        })
        .or_else(|| path_namespace(&path, by_key));
    let one_namespace = found.items.iter().all(|s| {
        match s {
            Selected::Row(r) | Selected::Field(r, _) => Some(r.namespace.as_str()) == namespace,
        }
    });
    if let (true, true, Some(ns)) = (only_rows, one_namespace, namespace.and_then(|n| by_key.get(n))) {
        let ids: Vec<String> = found.rows().iter().map(|r| r.qualified()).collect();
        let (columns, rows) = table_cells_of(ns, reg, &ids);
        return Some(Some(Resolved::Table {
            namespace: ns.key.clone(),
            columns,
            rows,
        }));
    }
    Some(Some(Resolved::List(
        found
            .items
            .iter()
            .filter_map(|s| {
                match s {
                    Selected::Row(r) => Some(r.qualified()),
                    Selected::Field(r, f) => field(r, f).map(|v| v.as_scalar()),
                }
            })
            .collect(),
    )))
}

/// The namespace every expansion of `path` opens on, by the one spelling rule.
fn path_namespace<'a>(path: &str, by_key: &BTreeMap<&'a str, &RegistryNamespace>) -> Option<&'a str> {
    let parsed = letkajenkka::Query::parse(path).ok()?.path()?;
    let mut found: Option<&'a str> = None;
    for variant in parsed.expand()? {
        let steps: Vec<letkajenkka::Step<'_>> = variant.steps().collect();
        let steps = after_registry_root(&steps, |s| s.selector == letkajenkka::Selector::Ident(REGISTRY_ROOT));
        let letkajenkka::Selector::Ident(name) = steps.first()?.selector else { return None };
        let key = namespace_named(name, by_key.keys().copied())?;
        match found {
            Some(k) if k != key => return None,
            _ => found = Some(key),
        }
    }
    found
}

/// One row, as a plain read of it shows: a work of `reference` as its short citation, a row of a
/// namespace declaring `value_field` as that value, any other as itself.
fn render_row(row: &RegistryRow, by_key: &BTreeMap<&str, &RegistryNamespace>) -> Option<Resolved> {
    if row.namespace == "reference" {
        // A short form by default, the full one on request (`::citation`). They differ in width: a
        // sources column carrying four hundred full citations is unreadable, while the same
        // citation in prose wants its title. There is no page for either to link to, so the
        // citation is its text alone.
        return Some(Resolved::Citation {
            text: short_citation(row),
            link: None,
        });
    }
    let ns = by_key.get(row.namespace.as_str())?;
    // A namespace declaring a value field renders that instead of an id, so a constant states its
    // number wherever it is named.
    if let Some(v) = ns.value_field.as_ref().and_then(|f| row.fields.get(f)) {
        return Some(Resolved::Field(v.clone()));
    }
    Some(Resolved::Row {
        namespace: row.namespace.clone(),
        id:        row.id.clone(),
        fields:    row_fields(ns, row),
    })
}

/// One field of a row, as a plain read of it shows. `None` for a field the row does not carry.
///
/// A field typed as a namespace holds ids, and each is resolved through the same path as the
/// field's table cell, so a field read and the cell agree, and an id that names no row keeps its
/// reference form and is reported rather than passed through. Internal fields never reach here:
/// the walker does not name them, so a read of one is unresolved and reported like a missing one.
#[allow(clippy::too_many_arguments)]
fn render_field(
    row: &RegistryRow,
    field: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<Resolved> {
    match field {
        "id" => return Some(Resolved::Field(row.id.clone())),
        "citation" if row.namespace == "reference" => {
            return Some(Resolved::Citation {
                text: format_citation(row),
                link: None,
            });
        },
        _ => {},
    }
    let raw = row.fields.get(field)?;
    let target = by_key
        .get(row.namespace.as_str())
        .and_then(|ns| ns.fields.iter().find(|f| f.name == field))
        .and_then(|f| row_reference_target(&f.r#type))
        .map(|(t, _)| t)
        .filter(|t| by_key.contains_key(t));
    let Some(target) = target else { return Some(Resolved::Field(raw.clone())) };
    Some(Resolved::List(
        crate::refpath::list(raw)
            .map(|id| {
                let expr = format!("{target}::{id}");
                // An id that resolves to nothing keeps its reference form rather than vanishing.
                // The table cell for the same datum emits the reference and is caught by the
                // unresolved-token scan; dropping it here made one path loud and the other silent
                // about the identical bad value, and a field of two ids rendered as one.
                resolve_expr(&expr, by_key, reg, roots, repo_root, docs_dir, cfg).unwrap_or_else(|| format!("{{{{ {expr} }}}}"))
            })
            .collect(),
    ))
}

/// The rows a reference names for an operation: the rows a path into the registry reaches. `None`
/// when `expr` is no such path; an inner `None` when it is refused or reaches a field.
fn rows_named<'r>(expr: &str, by_key: &BTreeMap<&str, &RegistryNamespace>, reg: &'r Registry) -> Option<Option<Vec<&'r RegistryRow>>> {
    let path = registry_path(expr, by_key.keys().copied())?;
    let Ok(found) = reg.select_in(by_key, &path) else { return Some(None) };
    if found.items.iter().any(|s| matches!(s, Selected::Field(..))) {
        return Some(None);
    }
    Some(Some(found.rows()))
}

/// Where the named thing is declared: the file to open to change it, one path for a plain read of
/// one row, and for a path reaching several rows each file once in the order their rows came.
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
    if let Some(path) = registry_path(expr, by_key.keys().copied()) {
        let found = reg.select_in(by_key, &path).ok()?;
        if found.items.iter().any(|s| matches!(s, Selected::Field(..))) {
            return None;
        }
        let rows = found.rows();
        if let (true, [one]) = (found.single, rows.as_slice()) {
            return Some(Resolved::Path(rel_to_repo(&one.source, repo_root)));
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
    let rows = rows_named(expr, by_key, reg)??;
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
    let rows = rows_named(expr, by_key, reg)??;
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

/// The rows a value's references read, each operation and method taken off first, so a row citing
/// `{{ ticket::question::** }}` or `{{ pathof(ticket::a).dir() }}` is resolved after every row it
/// reads.
pub(crate) fn row_deps(value: &str, namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<String> {
    let mut deps = Vec::new();
    for (_, expr) in placeholder_exprs(value) {
        let base = crate::chain::split_methods(&expr).map_or(expr.clone(), |(b, _)| b);
        let (_, inner) = peel(&base);
        deps.extend(find_registry_refs(&format!("{{{{ {inner} }}}}"), namespaces, reg));
    }
    deps
}
