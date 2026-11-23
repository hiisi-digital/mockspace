//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A namespace as a markdown table, and a reference row as a citation.

use super::*;

pub fn render_table(ns: &RegistryNamespace, reg: &Registry, cfg: &dyn RegistryEnv) -> String {
    let Some(ids) = reg.by_namespace.get(&ns.key) else {
        return String::new();
    };

    if let Some(group) = &ns.group_by {
        // Groups are ordered by the group value. First-seen order would be
        // whichever group holds the alphabetically first key path, since the
        // rows are listed by key path, and renaming one slug would flip it.
        // Rows within a group keep their key path order.
        //
        // Rows with no group value are kept in a list of their own, never under a group name,
        // so they cannot sort among the real groups or merge with a group really called
        // anything.
        let mut buckets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ungrouped: Vec<String> = Vec::new();
        for id in ids {
            match reg.get(id).and_then(|r| r.fields.get(group)) {
                Some(key) => buckets.entry(key.clone()).or_default().push(id.clone()),
                None => ungrouped.push(id.clone()),
            }
        }
        let mut out = String::new();
        for (key, members) in &buckets {
            out.push_str(&format!("\n### {key}\n\n"));
            out.push_str(&render_rows(ns, reg, members, cfg));
        }
        if !ungrouped.is_empty() {
            out.push_str("\n### no group\n\n");
            out.push_str(&render_rows(ns, reg, &ungrouped, cfg));
        }
        return out;
    }

    render_rows(ns, reg, ids, cfg)
}

/// A field's value with citations into internal roots removed.
///
/// Filtering is per item, not per cell. A provenance field routinely carries
/// several citations, and a row whose sources are one internal corpus and one
/// public document should keep the public one. Dropping the whole cell would
/// lose a citation the reader can actually follow, to hide one they cannot.
///
/// Only items that parse as citations are considered. A field holding ordinary
/// prose is returned unchanged, so this cannot quietly eat text that happens to
/// contain a `::`.
fn visible_value(raw: &str, cfg: &dyn RegistryEnv) -> String {
    if cfg.internal_roots().is_empty() || raw.is_empty() {
        return raw.to_string();
    }
    let kept: Vec<&str> = crate::refpath::list(raw)
        .filter(|item| {
            match FileRef::parse(item) {
                Some(r) => !cfg.internal_roots().contains(&r.root),
                None => true,
            }
        })
        .collect();
    kept.join(", ")
}

/// One table over the given rows.
fn render_rows(
    ns: &RegistryNamespace,
    reg: &Registry,
    ids: &[String],
    cfg: &dyn RegistryEnv,
) -> String {
    // `id` first, then declared fields in declaration order. A field that no
    // row actually carries is dropped: an always-empty column is noise.
    //
    // The test is what the column would RENDER, not what the rows carry, so a
    // column emptied by internal-root filtering drops by the same rule rather
    // than needing one of its own.
    let mut columns: Vec<String> = vec!["id".to_string()];
    for f in &ns.fields {
        if f.visibility == FieldVisibility::Internal {
            continue;
        }
        if ids
            .iter()
            .filter_map(|id| reg.get(id))
            .filter_map(|r| r.fields.get(&f.name))
            .any(|v| !visible_value(v, cfg).is_empty())
        {
            columns.push(f.name.clone());
        }
    }

    // Which of this namespace's fields hold row references, by field name.
    // Read from the declared types once rather than per cell.
    let row_refs: BTreeMap<&str, &str> = ns
        .fields
        .iter()
        .filter_map(|f| {
            let (target, _) = super::row_reference_target(&f.r#type)?;
            cfg.namespaces()
                .iter()
                .any(|n| n.key == target)
                .then_some((f.name.as_str(), target))
        })
        .collect();
    // The fields holding file citations, which may hold row links as well.
    let citing: BTreeSet<&str> = ns
        .fields
        .iter()
        .filter(|f| is_reference_type(&f.r#type))
        .map(|f| f.name.as_str())
        .collect();
    let keys: BTreeSet<&str> = cfg
        .namespaces()
        .iter()
        .map(|n| n.key.as_str())
        .collect();

    let mut out = String::new();
    out.push_str("| ");
    out.push_str(&columns.join(" | "));
    out.push_str(" |\n|");
    for _ in &columns {
        out.push_str("---|");
    }
    out.push('\n');

    for id in ids {
        let Some(row) = reg.get(id) else { continue };
        let cells: Vec<String> = columns
            .iter()
            .map(|c| {
                // `id` is a first-class field of the row rather than just an
                // entry in its map, so read it from there. The loader happens
                // to also put it in the map, but relying on that would make
                // the table depend on a loader detail.
                if c == "id" {
                    // The anchor rides the row's own first cell, so a reference
                    // lands on the row rather than on a restatement of it.
                    // Markdown has no per-row anchor of its own, and the
                    // alternative of a separate anchor block below the table
                    // duplicates every row to say nothing new.
                    return format!("<a id=\"{}\"></a>{}", row.anchor(), row.id);
                }
                // A row reference is stored as a bare slug and rendered as a
                // reference, which the pass that follows turns into the row's slug.
                // Emitting the reference rather than the slug keeps one
                // implementation of what a row reference renders as, and a
                // reference to a missing row stays visibly unresolved.
                if let Some(target) = row_refs.get(c.as_str()) {
                    let cell = crate::refpath::list(row.fields.get(c).map(String::as_str).unwrap_or(""))
                        .map(|slug| format!("{{{{ {target}::{slug} }}}}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return cell;
                }
                let raw = visible_value(row.fields.get(c).map(String::as_str).unwrap_or(""), cfg);
                // A `ref` cell may hold row links beside its file citations.
                let raw = if citing.contains(c.as_str()) {
                    super::render_links(&raw, &keys)
                } else {
                    raw
                };
                // A pipe inside a cell would end the column early, and a
                // newline would end the row. Both are realistic in prose
                // fields, which is exactly why the source is TOML.
                raw.replace('|', "\\|").replace('\n', " ")
            })
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }
    out
}


/// The short form: who and when, enough to recognise the work.
///
/// What a reader needs at a glance, and what fits in a table cell beside four
/// hundred others. The full citation is one reference away, `::citation`, which is
/// the whole reason the short form is safe.
pub fn short_citation(row: &RegistryRow) -> String {
    let f = |k: &str| row.fields.get(k).filter(|v| !v.is_empty()).cloned();
    match (f("authors"), f("year")) {
        (Some(a), Some(y)) => format!("{a} {y}"),
        (Some(a), None) => a,
        (None, Some(y)) => format!("{}, {y}", f("title").unwrap_or_else(|| row.id.clone())),
        (None, None) => f("title").unwrap_or_else(|| row.id.clone()),
    }
}

/// Render a reference row as a citation.
///
/// Assembled from the fields the row declares, skipping what it does not, so a
/// specification with an institutional author and no page reads as cleanly as
/// a journal paper. The format is deliberately plain: the value is that one
/// work cited in twenty places renders identically in all twenty, not that it
/// matches any particular house style.
pub fn format_citation(row: &RegistryRow) -> String {
    let f = |k: &str| row.fields.get(k).filter(|v| !v.is_empty()).cloned();
    let mut out = String::new();
    if let Some(a) = f("authors") {
        out.push_str(&a);
        out.push_str(", ");
    }
    out.push_str(&f("title").unwrap_or_else(|| row.id.clone()));
    if let Some(v) = f("venue") {
        let year = f("year").map(|y| format!(" {y}")).unwrap_or_default();
        out.push_str(&format!(" ({v}{year})"));
    } else if let Some(y) = f("year") {
        out.push_str(&format!(" ({y})"));
    }
    out
}
