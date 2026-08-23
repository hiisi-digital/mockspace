//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// The field types that are not a namespace name.
///
/// `ref` and `ref[]` hold a citation into a file: `root::path::anchor`. They
/// are named for what the string means rather than for where it points, which
/// is why a field holding a row reference cannot reuse them. A citation and a
/// row reference resolve against different things and fail in different ways,
/// so one word for both would make every finding ambiguous about which was
/// intended.
pub const SCALAR_TYPES: &[&str] = &["string", "string[]", "integer", "boolean", "ref", "ref[]"];

/// The namespace a field type names, and whether the field holds many.
///
/// A field type is either one of the scalars above or the key of a declared
/// namespace, so `type = "slot"` holds one slot and `type = "slot[]"` holds
/// several. There is no third syntax and no prefix, for the same reason a
/// reference addresses a namespace by its own name: slot zero is either a
/// scalar type or a declared namespace, and `namespace_type_collisions`
/// refuses a project that makes the two collide.
///
/// Returns `None` for a scalar. Returns the base name for anything else,
/// including a name no namespace declares, because deciding whether the name
/// is known is the caller's job and the two failures want different messages.
pub fn row_reference_target(ty: &str) -> Option<(&str, bool)> {
    if SCALAR_TYPES.contains(&ty) {
        return None;
    }
    match ty.strip_suffix("[]") {
        Some(base) => Some((base, true)),
        None => Some((ty, false)),
    }
}

/// The row-reference fields of each namespace: field name, target namespace.
pub fn row_reference_fields(
    namespaces: &[RegistryNamespace],
) -> BTreeMap<String, Vec<(String, String)>> {
    let known: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
    namespaces
        .iter()
        .map(|ns| {
            let fields = ns
                .fields
                .iter()
                .filter_map(|f| {
                    let (target, _) = row_reference_target(&f.r#type)?;
                    known
                        .contains(target)
                        .then(|| (f.name.clone(), target.to_string()))
                })
                .collect();
            (ns.key.clone(), fields)
        })
        .collect()
}

/// A field type that names neither a scalar nor a declared namespace.
///
/// Reported rather than defaulted. The schema generator's type match ends in a
/// catch-all that produces a string, so before this check a misspelled
/// `strng`, or a `slot` written before the namespace it names was declared,
/// became an unvalidated string field and every value in it went unchecked
/// while the declaration read as though it constrained something.
pub fn unknown_field_types(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let known: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
    let mut out = Vec::new();
    for ns in namespaces {
        for f in &ns.fields {
            let Some((target, _)) = row_reference_target(&f.r#type) else {
                continue;
            };
            if known.contains(target) {
                continue;
            }
            out.push(RegistryFinding {
                kind:    "unknown-field-type",
                message: format!(
                    "namespace `{}` declares field `{}` with type `{}`, which is neither a builtin type ({}) nor a declared namespace. A type nothing recognises used to become a plain string, so the field read as constrained and was not.",
                    ns.key,
                    f.name,
                    f.r#type,
                    SCALAR_TYPES.join(", ")
                ),
                source:  None,
            });
        }
    }
    out
}

/// A namespace whose key is also a builtin type name.
///
/// `key = "string"` would make `type = "string"` ambiguous between the scalar
/// and a reference into that namespace, and the ambiguity would resolve
/// silently in the scalar's favour because the scalar is checked first. Refused
/// at the declaration rather than left to surprise whoever writes the field.
pub fn namespace_type_collisions(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    namespaces
        .iter()
        .filter(|ns| SCALAR_TYPES.contains(&ns.key.as_str()))
        .map(|ns| RegistryFinding {
            kind:    "namespace-shadows-type",
            message: format!(
                "namespace `{}` has the same name as a builtin field type. A field declaring that type would mean the scalar, silently, and no field could ever reference this namespace.",
                ns.key
            ),
            source:  None,
        })
        .collect()
}

/// A row-reference field whose target renders a value instead of a link.
///
/// A namespace declaring `value_field` renders a bare reference as that field's
/// text, which is right for a constant stated once and named everywhere. It is
/// wrong under a row reference twice over. The reference exists to link, and a
/// link is exactly what such a target cannot produce. And the text arrives in a
/// table cell after the cell's own escaping has run, so a value carrying a pipe
/// ends the column early and the row silently grows a column.
///
/// Refused at the declaration rather than escaped at the cell, because the cell
/// does not have the value: it emits a reference and something later resolves
/// it, which is what keeps one implementation of what a row link is.
pub fn value_field_targets(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let by_key: BTreeMap<&str, &RegistryNamespace> =
        namespaces.iter().map(|n| (n.key.as_str(), n)).collect();
    let mut out = Vec::new();
    for ns in namespaces {
        for f in &ns.fields {
            let Some((target, _)) = row_reference_target(&f.r#type) else {
                continue;
            };
            let Some(t) = by_key.get(target) else { continue };
            let Some(vf) = &t.value_field else { continue };
            out.push(RegistryFinding {
                kind:    "row-reference-to-a-value-namespace",
                message: format!(
                    "namespace `{}` declares field `{}` as a reference into `{target}`, which declares `value_field = \"{vf}\"`. A reference into it renders that field's text rather than a link, and the text reaches a table cell after escaping has run, so a value containing `|` breaks the row. Reference a namespace that has rows to link to, or drop `value_field` from `{target}`.",
                    ns.key, f.name
                ),
                source:  None,
            });
        }
    }
    out
}

/// Check every row reference against the rows that exist.
///
/// A row reference is a bare slug. The field's type already names the
/// namespace, so repeating it in the value would be a second place for the two
/// to disagree, and a qualified value is refused rather than accepted as a
/// second spelling: one thing written two ways needs normalising everywhere it
/// is read, and the normalisations drift.
pub fn validate_row_references(
    reg: &Registry,
    namespaces: &[RegistryNamespace],
) -> Vec<RegistryFinding> {
    let bearing = row_reference_fields(namespaces);
    let mut out = Vec::new();

    for row in reg.rows.values() {
        let empty = Vec::new();
        let fields = bearing.get(&row.namespace).unwrap_or(&empty);
        for (name, target) in fields {
            let Some(raw) = row.fields.get(name) else {
                continue;
            };
            // Empty elements are not skipped. `", display"` and `"a, , b"`
            // are a stray separator in hand-written TOML, and skipping them
            // meant a trailing comma was reported while a leading one was not,
            // which is an arbitrary difference for an author to run into.
            for item in raw.split(", ").map(str::trim) {
                if !is_valid_slug(item) {
                    let hint = match item.split_once("::") {
                        Some((ns, slug)) if ns == target => {
                            format!(" Write `{slug}`: the field's type already says `{target}`.")
                        },
                        Some((ns, _)) => format!(
                            " It names namespace `{ns}`, but this field is typed `{target}`; a field references one namespace and the type is where that is stated."
                        ),
                        None => String::new(),
                    };
                    out.push(RegistryFinding {
                        kind:    "malformed-row-reference",
                        message: format!(
                            "{}: field `{name}` holds `{item}`, which is not a slug (snake_case, starting with a letter).{hint}",
                            row.qualified()
                        ),
                        source:  Some(row.source.clone()),
                    });
                    continue;
                }
                let qualified = format!("{target}::{item}");
                if reg.rows.contains_key(&qualified) {
                    continue;
                }
                let near = nearest(reg, target, item);
                out.push(RegistryFinding {
                    kind:    "unknown-row-reference",
                    message: format!(
                        "{}: field `{name}` references `{qualified}`, which no row declares.{near}",
                        row.qualified()
                    ),
                    source:  Some(row.source.clone()),
                });
            }
        }
    }
    out
}

/// The closest existing slug in the target namespace, when one is close enough
/// to be worth naming. A typo and a genuinely absent row are different
/// problems, and the message should not make them read alike.
fn nearest(reg: &Registry, namespace: &str, slug: &str) -> String {
    let empty = Vec::new();
    let ids = reg.by_namespace.get(namespace).unwrap_or(&empty);
    if ids.is_empty() {
        return format!(" Namespace `{namespace}` has no rows at all.");
    }
    let best = ids
        .iter()
        .filter_map(|q| q.split_once("::").map(|(_, s)| s))
        .map(|s| (edit_distance(slug, s), s))
        .min_by_key(|(d, _)| *d);
    match best {
        // A third of the length, so a short slug is not matched to an
        // unrelated short slug: `at` and `on` differ by two and share nothing.
        Some((d, s)) if d * 3 <= slug.len().max(1) => format!(" Did you mean `{s}`?"),
        _ => String::new(),
    }
}

/// Levenshtein distance, two rows at a time.
///
/// Written out rather than pulled in: the crate has no dependency for this,
/// and a suggestion is the only consumer, so being approximate would be fine
/// and being exact costs a dozen lines.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Call `emit(target, source)` once for every reverse edge in the registry.
///
/// One definition of what an edge is, because there were two: `referrers`
/// scanned for one target and `build_view` collected them all, each splitting
/// the field's own way. They agreed, and the only assertion tying them covered
/// one identifier on one fixture, so an edit to either spelling would have left
/// the test green while a document and a lint diverged.
///
/// **An edge whose target is not a row is not emitted.** A field naming a slug
/// nothing declares is a dangling reference, which the loader already reports
/// as `unknown-row-reference`; treating it as an edge would answer a question
/// about a row that does not exist, and it is what made a lint disagree with
/// `refsto` about the same data.
fn for_each_edge(
    reg: &Registry,
    namespaces: &[RegistryNamespace],
    mut emit: impl FnMut(String, String),
) {
    let bearing = row_reference_fields(namespaces);
    for row in reg.rows.values() {
        let empty = Vec::new();
        for (name, target) in bearing.get(&row.namespace).unwrap_or(&empty) {
            let Some(raw) = row.fields.get(name) else {
                continue;
            };
            for slug in raw.split(", ").map(str::trim).filter(|s| !s.is_empty()) {
                let q = format!("{target}::{slug}");
                if reg.rows.contains_key(&q) {
                    emit(q, row.qualified());
                }
            }
        }
    }
}

/// Every row that references `qualified` through a typed row-reference field.
///
/// The direction the data is not stored in. A row states what it references;
/// what references it is derived, so the two cannot disagree and neither has
/// to be maintained. This is what makes a demand-side question answerable:
/// given a slot, which rows answer it.
///
/// Returned as `namespace::slug`, sorted, each appearing once however many of
/// its fields point at the target.
pub fn referrers(reg: &Registry, namespaces: &[RegistryNamespace], qualified: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for_each_edge(reg, namespaces, |target, source| {
        if target == qualified {
            out.insert(source);
        }
    });
    out.into_iter().collect()
}

#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod tests;

/// The flattened, reverse-edged view a lint or a tool checks the registry with.
///
/// Built here rather than in the lint crate because computing the reverse edges
/// needs the declared field types, which are configuration. Handing over the
/// answer keeps that knowledge in one place, and a lint asking "what references
/// this" cannot get a different answer from `refsto` in a document.
pub fn build_view(
    reg: &Registry,
    namespaces: &[RegistryNamespace],
) -> mockspace_lint_rules::RegistryView {
    let rows: BTreeMap<String, mockspace_lint_rules::RowFields> = reg
        .rows
        .iter()
        .map(|(q, row)| (q.clone(), row.fields.clone()))
        .collect();
    // One pass over every edge rather than `referrers` per row, which would be
    // quadratic on a corpus of a few thousand rows. Same definition of an edge
    // either way, because both go through `for_each_edge`.
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for_each_edge(reg, namespaces, |target, source| {
        edges.entry(target).or_default().push(source);
    });
    for v in edges.values_mut() {
        v.sort();
        v.dedup();
    }
    mockspace_lint_rules::RegistryView::new(rows, edges)
}
