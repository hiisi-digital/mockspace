//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Selecting in the registry: every path into it, a plain one included, read by the one walker.
//!
//! A plain path is a selection that names one member; `{{ ticket::question::engine::allowed }}`,
//! `{{ ticket::{task}::engine::b::title }}`, `{{ ticket::question::** }}` and `cargo mock query`
//! all go through [`Registry::select`].
//!
//! - **A node stands for the rows at or below it.** A record is its row; a branch is every row
//!   under it, so `ticket::question::engine` and `ticket::question::engine::**` select alike.
//! - **An ident** names one member. Where no earlier step widened, one naming nothing fails the
//!   query (§7); after a widening step it yields nothing for that member.
//! - **A set** is its expansion, by `letkajenkka`: each member's path, in written order, a repeated
//!   member read twice (§8.3) and reported by [`crate::repeated_set_members`].
//! - **`*`** is every member of the node, **`**`** the node and every branch and record below it,
//!   and from a record its fields too. Members come in byte order of their names, step by step
//!   (§11): `a::b` before `a0`, since `a` is before `a0`. A node reached by several `**` paths
//!   comes once (§8.4).
//! - **A step from a record** names a field its namespace declares (`id` included, internal fields
//!   not), or one the row writes. A namespace is a uniform branch (§6) and this host yields: after
//!   a widening step a row with no such field is passed over, so `ticket::**::internals::**` keeps
//!   the rows with `internals` in their ids.
//! - **A step past a field** goes on into the rows the field names, where it holds references
//!   (§7): `ticket::*::**::needs::title`. Past a field holding a value it fails, naming the field
//!   and its type.
//! - **Refused**: an ordinal or a range (a branch has no order and a record no lines), a constraint
//!   (the registry declares no shapes), an operation, and a namespace name two namespaces fold to.

use super::*;
use crate::walk::{Hit, Store, Walker, keys_from};

/// Why a query selects nothing it can say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectRefusal {
    /// The standard refuses the text, as the finding says.
    Syntax(String),
    /// An operation, `refsto(...)` and the like, which is no path over the ids.
    Operation,
    /// More combinations of set members than can be counted.
    TooWide,
    /// A step naming nothing where no earlier step widened (§7).
    NoSuch(String),
    /// A name two namespaces fold to, with their keys.
    Ambiguous(String, Vec<String>),
    /// An ordinal or a range, which no branch and no record takes.
    Ordered(String),
    /// A constraint, naming a shape the registry does not declare.
    Constraint(String),
    /// A step past a field holding a value, with the row, the field and its type.
    PastField { row: String, field: String, ty: String },
}

impl SelectRefusal {
    /// The refusal as the person reads it.
    pub fn say(&self, query: &str) -> String {
        match self {
            Self::Syntax(why) => why.clone(),
            Self::Operation => format!("`{query}` is an operation, no path over the row ids"),
            Self::TooWide => format!("`{query}` expands to more paths than can be counted"),
            Self::NoSuch(step) => {
                format!("`{query}`: `{step}` names nothing there, and no step before it widened")
            },
            Self::Ambiguous(name, keys) => {
                format!("`{query}`: `{name}` is the folded name of {}; rename one", keys.join(" and "))
            },
            Self::Ordered(step) => {
                format!("`{query}`: `{step}` selects by position, and a namespace's rows have no order to select by")
            },
            Self::Constraint(step) => format!("`{query}`: `{step}` names a shape, and the registry declares none"),
            Self::PastField {
                row,
                field,
                ty,
            } => {
                format!(
                    "`{query}`: `{field}` of {row} is a `{ty}`, a value and no row, so no step goes on from it"
                )
            },
        }
    }
}

/// What a path into the registry reached: a row, or one field of one.
#[derive(Debug, Clone)]
pub enum Selected<'r> {
    Row(&'r RegistryRow),
    Field(&'r RegistryRow, String),
}

/// What [`Registry::select`] answers.
#[derive(Debug, Clone)]
pub struct Selection<'r> {
    /// What was reached, in the order the standard fixes.
    pub items:   Vec<Selected<'r>>,
    /// Whether the path named one record or one field and nothing widened: a plain read of a row
    /// or a field, which shows as that row or that value and not as a table or a list.
    pub single:  bool,
    /// The rows a step went on from through a reference field, whose fields were read on the way.
    pub through: Vec<&'r RegistryRow>,
}

impl<'r> Selection<'r> {
    /// The rows alone, a row of a field included once for each field.
    pub fn rows(&self) -> Vec<&'r RegistryRow> {
        self.items
            .iter()
            .filter_map(|s| {
                match s {
                    Selected::Row(r) => Some(*r),
                    Selected::Field(..) => None,
                }
            })
            .collect()
    }
}

/// The qualified ids of `ids` a query selects, in the order the standard fixes.
///
/// `ids` are qualified, `namespace::id`, the namespace in either spelling; a text that is no
/// namespace and path id is left out. No fields are declared over a plain list, so a step from a row
/// fails where nothing widened and passes the row over where something did. The same walker
/// [`Registry::select`] and resolution use, for a tool that holds its rows its own way.
///
/// # Errors
///
/// The [`SelectRefusal`] saying why the query selects nothing it can say. No ids is no refusal.
pub fn select_ids<'a>(query: &str, ids: &[&'a str]) -> Result<Vec<&'a str>, SelectRefusal> {
    let mut store = ListStore::default();
    for id in ids {
        let Ok(names) = crate::names(id) else { continue };
        let Some((namespace, rest)) = names.split_first() else { continue };
        if rest.is_empty() || !rest.iter().all(|n| is_valid_slug(n)) {
            continue;
        }
        if !store.namespaces.iter().any(|n| n == namespace) {
            store.namespaces.push((*namespace).to_string());
        }
        store.keys.entry(format!("{namespace}::{}", rest.join("::"))).or_insert(id);
    }
    Ok(Walker::new(&store)
        .run(query)?
        .hits
        .into_iter()
        .filter_map(|h| {
            match h {
                Hit::Row(k) => store.keys.get(&k).copied(),
                Hit::Field(..) => None,
            }
        })
        .collect())
}

/// A plain list of qualified ids, as [`select_ids`] walks it: no fields, no values.
#[derive(Default)]
struct ListStore<'a> {
    namespaces: Vec<String>,
    keys:       BTreeMap<String, &'a str>,
}

impl Store for ListStore<'_> {
    fn namespaces(&self) -> Vec<&str> {
        self.namespaces.iter().map(String::as_str).collect()
    }

    fn keys_from<'s>(&'s self, from: &str) -> Box<dyn Iterator<Item = &'s str> + 's> {
        keys_from(&self.keys, from)
    }

    fn is_row(&self, key: &str) -> bool {
        self.keys.contains_key(key)
    }

    fn fields(&self, _: &str) -> Vec<(String, String)> {
        Vec::new()
    }

    fn value(&self, _: &str, _: &str) -> Option<&str> {
        None
    }
}

/// The registry as the walker reads it: its rows, and the namespaces they are declared by.
struct RegistryStore<'r, 'n> {
    reg:  &'r Registry,
    decl: BTreeMap<&'n str, &'n RegistryNamespace>,
}

impl Store for RegistryStore<'_, '_> {
    fn namespaces(&self) -> Vec<&str> {
        self.decl.keys().copied().collect()
    }

    fn keys_from<'s>(&'s self, from: &str) -> Box<dyn Iterator<Item = &'s str> + 's> {
        keys_from(&self.reg.rows, from)
    }

    fn is_row(&self, key: &str) -> bool {
        self.reg.rows.contains_key(key)
    }

    fn fields(&self, key: &str) -> Vec<(String, String)> {
        let Some(row) = self.reg.rows.get(key) else { return Vec::new() };
        self.decl.get(row.namespace.as_str()).map(|ns| fields_of(ns, row)).unwrap_or_default()
    }

    fn value(&self, key: &str, field: &str) -> Option<&str> {
        self.reg.rows.get(key)?.fields.get(field).map(String::as_str)
    }
}

/// The fields a step from a row of `ns` may name, with their types: what it declares and is not
/// internal, what the row writes beyond that, and `id`, and for `reference` its computed `citation`.
fn fields_of(ns: &RegistryNamespace, row: &RegistryRow) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = ns
        .fields
        .iter()
        .filter(|f| f.visibility != FieldVisibility::Internal)
        .map(|f| (f.name.clone(), f.r#type.clone()))
        .collect();
    for written in row.fields.keys() {
        if !ns.fields.iter().any(|f| &f.name == written) {
            out.push((written.clone(), "string".to_string()));
        }
    }
    if !out.iter().any(|(n, _)| n == "id") {
        out.push(("id".to_string(), "string".to_string()));
    }
    if ns.key == "reference" {
        out.push(("citation".to_string(), "string".to_string()));
    }
    out
}

impl Registry {
    /// What a path into the registry reaches, rows and fields, in the order the standard fixes.
    /// The first step is the namespace, after the optional registry root, in either spelling.
    ///
    /// # Errors
    ///
    /// The [`SelectRefusal`] saying why the path reaches nothing it can say. An answer of nothing
    /// after a widening step is no refusal.
    pub fn select<'r>(&'r self, namespaces: &[RegistryNamespace], query: &str) -> Result<Selection<'r>, SelectRefusal> {
        self.select_in(&namespaces.iter().map(|n| (n.key.as_str(), n)).collect(), query)
    }

    /// [`Self::select`] under declarations already keyed by namespace, as resolution holds them.
    pub(crate) fn select_in<'r>(&'r self, decl: &BTreeMap<&str, &RegistryNamespace>, query: &str) -> Result<Selection<'r>, SelectRefusal> {
        let store = RegistryStore {
            reg:  self,
            decl: decl.clone(),
        };
        let answer = Walker::new(&store).run(query)?;
        let row = |k: &str| self.rows.get(k);
        let items = answer
            .hits
            .into_iter()
            .filter_map(|h| {
                match h {
                    Hit::Row(k) => row(&k).map(Selected::Row),
                    Hit::Field(k, f) => row(&k).map(|r| Selected::Field(r, f)),
                }
            })
            .collect();
        Ok(Selection {
            items,
            single: answer.single,
            through: answer.through.iter().filter_map(|k| row(k)).collect(),
        })
    }

    /// The rows alone of [`Self::select`], for a caller asking which rows and not what they say.
    ///
    /// # Errors
    ///
    /// As [`Self::select`].
    pub fn select_rows<'r>(&'r self, namespaces: &[RegistryNamespace], query: &str) -> Result<Vec<&'r RegistryRow>, SelectRefusal> {
        Ok(self.select(namespaces, query)?.rows())
    }
}

/// Two declared namespaces whose keys fold to one name, which a path could not tell apart.
pub fn namespace_fold_collisions(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let mut by_name: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for ns in namespaces {
        if let Some(name) = mockspace_refnames::fold(&ns.key).map(|f| f.to_string()) {
            let keys = by_name.entry(name).or_default();
            if !keys.contains(&ns.key.as_str()) {
                keys.push(&ns.key);
            }
        }
    }
    by_name
        .into_iter()
        .filter(|(_, keys)| keys.len() > 1)
        .map(|(name, keys)| {
            RegistryFinding {
                kind:    "namespace-fold-collision",
                message: format!(
                    "namespaces {} all fold to `{name}`, the name a path writes, so a path cannot tell \
                     them apart. Rename all but one.",
                    keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
                ),
                source:  None,
                gate:    GateScope::Always,
            }
        })
        .collect()
}

/// Why `expr` did not resolve, where the reason is the walker's: its path, out of any methods and
/// operation, is a path into the registry and is refused. `None` for anything else, a citation
/// among them, whose failure this does not explain. What a document's unresolved reference and
/// `cargo mock query` both say.
pub fn unresolved_reason(namespaces: &[RegistryNamespace], reg: &Registry, expr: &str) -> Option<String> {
    let base = crate::chain::split_methods(expr).map_or_else(|| expr.to_string(), |(b, _)| b);
    let (_, inner) = peel(&base);
    let text = registry_path(inner, namespaces.iter().map(|n| n.key.as_str()))?;
    reg.select(namespaces, &text).err().map(|r| r.say(inner.trim()))
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
