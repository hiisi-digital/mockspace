//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The fields a namespace folds into its rows' ids.
//!
//! A namespace declaring `path = ["kind", "subject"]` gives each row the id
//! `<kind>::<subject>::<leaf>`, where each value is the id of a row of the namespace the field
//! references, itself a path where that namespace's ids are, and the leaf is one name. The id is
//! the one place the values are written; the loader reads them back into the row's fields, so a
//! reader of `kind` finds it where it always was.
//!
//! The fields fold in their declared order. A field declared optional may be absent, and then it
//! is no segment at all. Reading an id is a search over the ids each referenced namespace holds,
//! so it is decided by the data rather than by counting names, and an id is read every way it can
//! be: none is `unfoldable-id`, and so is more than one, since a reader must not be the one to
//! choose.
//!
//! [`fold_row_id`] and [`row_id_names`] are pure, over the declaration and the ids alone, so a
//! tool holding rows its own way reads ids by the same rule the loader does.

use super::*;

/// Why a row's id does not fold into its namespace's path fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdFault {
    /// The namespace's `path` names a field that is not one row reference into a declared
    /// namespace. Reported once for the declaration, by [`unfit_path_fields`], and not per row.
    UnfitPath(String),
    /// The id is no path of plain names, as [`row_id_names`] says it.
    NotAnId(String),
    /// No reading of the id gives each required field a row of its namespace and leaves one leaf.
    NoReading,
    /// More than one reading does: each as the field values it would give.
    Ambiguous(Vec<Vec<(String, String)>>),
    /// The row writes a field its id folds, which only the id may hold.
    Written(String),
}

impl IdFault {
    /// The kind of the finding this fault is.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnfitPath(_) => "unfit-path-field",
            Self::Written(_) => "folded-field-written",
            Self::NotAnId(_) | Self::NoReading | Self::Ambiguous(_) => "unfoldable-id",
        }
    }

    /// The fault as a finding about the row `qualified` of `ns` says it.
    pub fn message(&self, ns: &RegistryNamespace, qualified: &str) -> String {
        let shape = path_shape(ns);
        match self {
            Self::UnfitPath(why) => why.clone(),
            Self::NotAnId(why) => format!("{qualified}: {why}"),
            Self::NoReading => {
                format!(
                    "{qualified}: the id does not read as {shape}. Each value is the id of a row of the namespace \
                     its field references, and a segment naming no row of it is no value."
                )
            },
            Self::Ambiguous(readings) => {
                let each: Vec<String> = readings
                    .iter()
                    .map(|r| {
                        let kv: Vec<String> = r.iter().map(|(f, v)| format!("{f} = {v}")).collect();
                        format!("({})", kv.join(", "))
                    })
                    .collect();
                format!(
                    "{qualified}: the id reads as {shape} in {} ways: {}. Rename a row so only one remains.",
                    readings.len(),
                    each.join(", ")
                )
            },
            Self::Written(field) => {
                format!(
                    "{qualified}: the row writes `{field}`, which `{}` folds into the id ({shape}). The id is the \
                     field's one owner: remove the field from the row.",
                    ns.key
                )
            },
        }
    }
}

/// A fault the loader met, on the row it is about.
#[derive(Debug, Clone)]
pub struct FoldFault {
    /// The namespace of the row.
    pub namespace: String,
    /// `namespace::id` of the row.
    pub row:    String,
    pub source: PathBuf,
    pub fault:  IdFault,
}

/// One field of a namespace's `path`: its name, the namespace it references, and whether a row
/// must carry it.
struct PathField<'a> {
    name:     &'a str,
    target:   &'a str,
    required: bool,
}

/// The path fields of `ns`, or why its `path` names one that is not one row reference.
fn path_fields<'a>(ns: &'a RegistryNamespace, declared: &BTreeSet<&str>) -> Result<Vec<PathField<'a>>, String> {
    let mut out: Vec<PathField<'a>> = Vec::new();
    for name in &ns.path {
        let at = format!("namespace `{}` folds `{name}` into its ids (`path`)", ns.key);
        if out.iter().any(|f| f.name == name) {
            return Err(format!("{at} twice"));
        }
        let Some(field) = ns.fields.iter().find(|f| &f.name == name) else {
            return Err(format!("{at}, and declares no field of that name"));
        };
        match row_reference_target(&field.r#type) {
            Some((target, false)) if declared.contains(target) && target != ns.key => {
                out.push(PathField {
                    name,
                    target,
                    required: field.required,
                });
            },
            Some((target, false)) if target == ns.key => {
                return Err(format!("{at}, which references `{target}` itself, so no id could be read before it"));
            },
            _ => {
                return Err(format!(
                    "{at}, which is typed `{}`. A folded field holds one row of another declared namespace, so its type is that namespace's key, with no `[]`.",
                    field.r#type
                ));
            },
        }
    }
    Ok(out)
}

/// How an id of `ns` reads, for a message: each field and its namespace, then the leaf.
fn path_shape(ns: &RegistryNamespace) -> String {
    let mut parts: Vec<String> = ns
        .path
        .iter()
        .map(|name| {
            let f = ns.fields.iter().find(|f| &f.name == name);
            let target = f.and_then(|f| row_reference_target(&f.r#type)).map_or("?", |(t, _)| t);
            let optional = if f.is_some_and(|f| f.required) { "" } else { ", optional" };
            format!("`{name}` (a row of `{target}`{optional})")
        })
        .collect();
    parts.push("one leaf name".to_string());
    parts.join(", then ")
}

/// The values a row's id gives the fields its namespace folds, in `path` order, an absent optional
/// field left out.
///
/// `referenced` holds, for each namespace, the ids of its rows (bare, without the namespace). A
/// namespace with no `path` folds nothing and any id is returned no values.
///
/// # Errors
///
/// [`IdFault::UnfitPath`] where the declaration is unfit, [`IdFault::NotAnId`] where the id is no
/// path, [`IdFault::NoReading`] or [`IdFault::Ambiguous`] where it reads no way or several. Never
/// [`IdFault::Written`], which is about the row's fields and not its id.
pub fn fold_row_id(
    ns: &RegistryNamespace,
    referenced: &BTreeMap<String, BTreeSet<String>>,
    id: &str,
) -> Result<Vec<(String, String)>, IdFault> {
    if ns.path.is_empty() {
        return Ok(Vec::new());
    }
    let declared: BTreeSet<&str> = referenced.keys().map(String::as_str).collect();
    let fields = path_fields(ns, &declared).map_err(IdFault::UnfitPath)?;
    let names = row_id_names(id).map_err(IdFault::NotAnId)?;
    let mut readings = Vec::new();
    read(&fields, referenced, &names, &mut Vec::new(), &mut readings);
    match readings.len() {
        0 => Err(IdFault::NoReading),
        1 => Ok(readings.remove(0)),
        _ => Err(IdFault::Ambiguous(readings)),
    }
}

/// Every reading of `names` as `fields` then one leaf, appended to `out`.
fn read(
    fields: &[PathField<'_>],
    referenced: &BTreeMap<String, BTreeSet<String>>,
    names: &[&str],
    taken: &mut Vec<(String, String)>,
    out: &mut Vec<Vec<(String, String)>>,
) {
    let Some((field, rest)) = fields.split_first() else {
        if names.len() == 1 {
            out.push(taken.clone());
        }
        return;
    };
    let ids = referenced.get(field.target);
    // At least one name is left for the leaf.
    for n in 1 .. names.len() {
        let value = names[.. n].join("::");
        if ids.is_some_and(|ids| ids.contains(&value)) {
            taken.push((field.name.to_string(), value));
            read(rest, referenced, &names[n ..], taken, out);
            taken.pop();
        }
    }
    if !field.required {
        read(rest, referenced, names, taken, out);
    }
}

/// A namespace whose `path` names a field that cannot fold: undeclared, a list, a scalar, a
/// reference to no declared namespace or to itself, or named twice.
pub fn unfit_path_fields(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let declared: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
    namespaces
        .iter()
        .filter_map(|ns| path_fields(ns, &declared).err())
        .map(|message| {
            RegistryFinding {
                kind: "unfit-path-field",
                message,
                source: None,
                gate: GateScope::Always,
            }
        })
        .collect()
}

/// The namespaces whose ids a namespace in `wanted` folds, to a fixed point, so a load of some
/// namespaces reads what their ids are read against.
pub(crate) fn with_path_targets(namespaces: &[RegistryNamespace], wanted: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = wanted.clone();
    loop {
        let more: BTreeSet<String> = namespaces
            .iter()
            .filter(|ns| out.contains(&ns.key))
            .flat_map(|ns| {
                ns.path.iter().filter_map(|name| {
                    let f = ns.fields.iter().find(|f| &f.name == name)?;
                    row_reference_target(&f.r#type).map(|(t, _)| t.to_string())
                })
            })
            .filter(|t| !out.contains(t))
            .collect();
        if more.is_empty() {
            return out;
        }
        out.extend(more);
    }
}

/// Reads every row's id of a namespace declaring `path` back into its fields, and records each
/// row whose id does not fold or which wrote a folded field itself.
pub(crate) fn fold_ids(reg: &mut Registry, namespaces: &[RegistryNamespace]) {
    let mut referenced: BTreeMap<String, BTreeSet<String>> =
        namespaces.iter().map(|n| (n.key.clone(), BTreeSet::new())).collect();
    for row in reg.rows.values() {
        if let Some(ids) = referenced.get_mut(&row.namespace) {
            ids.insert(row.id.clone());
        }
    }
    for ns in namespaces.iter().filter(|ns| !ns.path.is_empty()) {
        let Some(keys) = reg.by_namespace.get(&ns.key).cloned() else { continue };
        for q in keys {
            let Some(row) = reg.rows.get_mut(&q) else { continue };
            for field in ns.path.iter().filter(|f| row.fields.contains_key(*f)) {
                reg.folds.push(FoldFault {
                    namespace: ns.key.clone(),
                    row:    q.clone(),
                    source: row.source.clone(),
                    fault:  IdFault::Written(field.clone()),
                });
            }
            match fold_row_id(ns, &referenced, &row.id) {
                Ok(values) => row.fields.extend(values),
                // the declaration's finding, said once by `unfit_path_fields`
                Err(IdFault::UnfitPath(_)) => break,
                Err(fault) => {
                    reg.folds.push(FoldFault {
                        namespace: ns.key.clone(),
                        row: q.clone(),
                        source: row.source.clone(),
                        fault,
                    });
                },
            }
        }
    }
}

/// The loader's fold faults as findings.
pub(crate) fn fold_findings(namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<RegistryFinding> {
    reg.folds
        .iter()
        .filter_map(|f| {
            let ns = namespaces.iter().find(|n| n.key == f.namespace)?;
            Some(RegistryFinding {
                kind:    f.fault.kind(),
                message: f.fault.message(ns, &f.row),
                source:  Some(f.source.clone()),
                gate:    GateScope::Always,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "fold_tests.rs"]
mod tests;
