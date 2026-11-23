//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The references held inside registry data, resolved bottom-up before any document renders.

use super::resolve::resolve_once;
use super::*;

/// Resolve every reference held inside registry data, bottom-up.
///
/// Data stores templates and never resolved values, so a row's field may cite
/// another row's field. Resolving depth-first from the leaves means a chain of
/// any length settles in one traversal, where re-running a whole-document pass
/// a fixed number of times silently caps how deep a chain may go.
///
/// The graph is acyclic in every sane registry, but nothing enforces that: two
/// rows can reference each other, and such a graph has no leaves to start from.
/// That case is detected and named rather than assumed away, because the
/// alternative is a document that ships with unexplained placeholders and no
/// clue which rows caused it.
pub fn resolve_data(
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> (Registry, Vec<RegistryFinding>) {
    let by_key: BTreeMap<&str, &RegistryNamespace> =
        namespaces.iter().map(|n| (n.key.as_str(), n)).collect();
    let mut done: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut findings = Vec::new();

    // What every row's resolution reads and none changes, built once. The namespace index in
    // particular lists every row of the registry, so a copy of it per row cost rows times
    // registry. An ad-hoc spike on a real registry (outside the bench harness, no committed artifact) put it at seconds a run.
    let ns_keys: BTreeSet<String> = by_key.keys().map(|k| k.to_string()).collect();
    let namespaces: Vec<RegistryNamespace> = by_key.values().map(|n| (*n).clone()).collect();
    // The view a row resolves against: the namespace index, and the rows it cites, put in and
    // taken out for each row. Deliberately without the corpus-level `files_read`,
    // `unquoted_times` and `duplicates`, which nothing reads off a per-row view.
    let mut view = Registry {
        rows:           BTreeMap::new(),
        by_namespace:   reg.by_namespace.clone(),
        duplicates:     BTreeMap::new(),
        files_read:     Vec::new(),
        unquoted_times: Vec::new(),
        folds:          Vec::new(),
    };

    for id in reg.rows.keys() {
        let mut path = Vec::new();
        resolve_row(
            id,
            reg,
            &by_key,
            &ns_keys,
            &namespaces,
            &mut view,
            roots,
            repo_root,
            docs_dir,
            cfg,
            &mut done,
            &mut path,
            &mut findings,
        );
    }

    let mut out = Registry {
        rows:         BTreeMap::new(),
        by_namespace: reg.by_namespace.clone(),
        duplicates:   reg.duplicates.clone(),
        // The same corpus, resolved, so it was read from the same files.
        files_read:   reg.files_read.clone(),
        unquoted_times: reg.unquoted_times.clone(),
        folds: reg.folds.clone(),
    };
    for (id, row) in &reg.rows {
        let mut r = row.clone();
        if let Some(fields) = done.get(id) {
            r.fields = fields.clone();
        }
        out.rows.insert(id.clone(), r);
    }
    (out, findings)
}

#[allow(clippy::too_many_arguments)]
fn resolve_row(
    id: &str,
    reg: &Registry,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    ns_keys: &BTreeSet<String>,
    namespaces: &[RegistryNamespace],
    view: &mut Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
    done: &mut BTreeMap<String, BTreeMap<String, String>>,
    path: &mut Vec<String>,
    findings: &mut Vec<RegistryFinding>,
) {
    if done.contains_key(id) {
        return;
    }
    if path.iter().any(|p| p == id) {
        // Name the whole cycle, not just the row we happened to re-enter: the
        // fix is to break the loop somewhere, and that needs all of it visible.
        let start = path.iter().position(|p| p == id).unwrap_or(0);
        let mut cycle: Vec<String> = path[start ..].to_vec();
        cycle.push(id.to_string());
        findings.push(RegistryFinding {
            kind: "reference-cycle",
            message: format!(
                "these rows reference each other in a loop and cannot be resolved: {}. Break the loop by inlining one side.",
                cycle.join(" -> ")
            ),
            source: reg.get(id).map(|r| r.source.clone()),
            gate:   GateScope::Always,
        });
        return;
    }
    let Some(row) = reg.get(id) else { return };

    // Resolve everything this row depends on first, so by the time its own
    // fields are rewritten the values they cite are final.
    path.push(id.to_string());
    for value in row.fields.values() {
        for (dep, _) in find_registry_refs(value, ns_keys, reg) {
            resolve_row(
                &dep, reg, by_key, ns_keys, namespaces, view, roots, repo_root, docs_dir, cfg,
                done, path, findings,
            );
        }
    }
    path.pop();

    // Only the rows this one actually cites need to be settled, so the shared view holds
    // those and nothing else. The recursion above is finished with the view before this
    // row fills it, and nothing below recurses, so one view serves every row in turn.
    view.rows.clear();
    for value in row.fields.values() {
        for (dep, _) in find_registry_refs(value, ns_keys, reg) {
            if view.rows.contains_key(&dep) {
                continue;
            }
            if let Some(v) = reg.get(&dep) {
                let mut r = v.clone();
                if let Some(f) = done.get(&dep) {
                    r.fields = f.clone();
                }
                view.rows.insert(dep, r);
            }
        }
    }

    let fields = row
        .fields
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                resolve_once(v, namespaces, view, roots, repo_root, docs_dir, cfg),
            )
        })
        .collect();
    done.insert(id.to_string(), fields);
}
