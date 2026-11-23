//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A query over the tree of row ids: the core resolution walks when a reference widens or names a
//! branch, so `{{ ticket::question::** }}`, `cargo mock query 'ticket::*::engine'` and
//! `refsto(ticket::{a, b})` are read by the one reader.
//!
//! The registry as the letkajenkka standard reads it: a root branch whose members are the declared
//! namespaces, each under its name folded into the standard's set (`web-renderer` is
//! `web_renderer`, and the hyphen is refused), each namespace a branch whose members are the
//! first names of its rows' ids, each proper prefix of an id a branch, and each id a record whose
//! members are the fields its namespace declares.
//!
//! - **A node stands for the rows at or below it.** A record is its row; a branch is every row
//!   under it, so `ticket::question::engine` and `ticket::question::engine::**` select alike.
//! - **An ident** names one member. Where no earlier step widened, one naming nothing fails the
//!   query (§7); after a widening step it yields nothing for that member, as below.
//! - **A set** is its expansion, by `letkajenkka`: each member's path, in written order.
//! - **`*`** is every member of the node, **`**`** the node and every branch and record below it.
//!   Under each, members come in byte order of their name, step by step (§11): `a::b` before `a0`,
//!   since `a` is before `a0`.
//! - **Each row once**, and each field of a row once, at its first place, however many paths and
//!   set members reach it.
//! - **A step from a record** names a field exactly when the namespace declares it (`id` included,
//!   internal fields not). A namespace is a uniform branch (§6), and this host yields: after a
//!   widening step, a row whose namespace declares no such field is passed over, so
//!   `ticket::**::internals::**` keeps the rows with `internals` in their ids and nothing fails.
//!   A field is where the walk ends; a step past one is refused.
//! - **Refused**: an ordinal or a range (a branch has no order and a record no lines), a constraint
//!   (the registry declares no shapes), an operation, and a namespace name two namespaces fold to.

use letkajenkka::{Query, Selector, Step};

use super::*;

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
    /// A step past a field of the named row.
    PastField { row: String, field: String },
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
            } => format!("`{query}`: a step past the field `{field}` of {row}, where a selection ends"),
        }
    }
}

/// What a selection reached, by the index of a leaf in the list the tree was built from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Hit {
    Row(usize),
    Field(usize, String),
}

/// One node of the id tree.
#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    /// The leaf this node is, where it is one.
    leaf:     Option<usize>,
    /// At a namespace node, every namespace key folding to its name; two or more is a collision.
    keys:     Vec<String>,
}

impl Node {
    fn rows(&self, out: &mut Vec<usize>) {
        out.extend(self.leaf);
        for child in self.children.values() {
            child.rows(out);
        }
    }

    fn deep<'a>(&'a self, out: &mut Vec<&'a Node>) {
        out.push(self);
        for child in self.children.values() {
            child.deep(out);
        }
    }

    fn is_record(&self) -> bool {
        self.leaf.is_some() && self.children.is_empty()
    }
}

/// A leaf of the tree: its namespace's key, its id's names, its qualified id, and the fields a step
/// from it may name.
struct Leaf<'a> {
    namespace: &'a str,
    names:     Vec<&'a str>,
    qualified: String,
    fields:    Vec<&'a str>,
}

/// The tree of `leaves` under `namespaces`, each namespace under its folded name.
fn tree(namespaces: &[&str], leaves: &[Leaf<'_>]) -> Node {
    let mut root = Node::default();
    for key in namespaces {
        // A key no fold makes a name of cannot be reached by a query, and is left out.
        let Some(name) = mockspace_refnames::fold(key).map(|f| f.to_string()) else { continue };
        let branch = root.children.entry(name).or_default();
        if !branch.keys.iter().any(|k| k == key) {
            branch.keys.push((*key).to_string());
        }
    }
    for (i, leaf) in leaves.iter().enumerate() {
        let Some(name) = mockspace_refnames::fold(leaf.namespace).map(|f| f.to_string()) else { continue };
        let Some(mut at) = root.children.get_mut(&name) else { continue };
        for n in &leaf.names {
            at = at.children.entry((*n).to_string()).or_default();
        }
        at.leaf = Some(i);
    }
    root
}

/// What a walk reached: a node, or a field of a leaf.
enum Reached<'n> {
    Node(&'n Node),
    Field(usize, String),
}

fn walk<'n>(
    node: &'n Node,
    steps: &[Step<'_>],
    widened: bool,
    leaves: &[Leaf<'_>],
    out: &mut Vec<Reached<'n>>,
) -> Result<(), SelectRefusal> {
    let Some((step, rest)) = steps.split_first() else {
        out.push(Reached::Node(node));
        return Ok(());
    };
    if !step.constraints.is_empty() {
        return Err(SelectRefusal::Constraint(step.to_string()));
    }
    if matches!(step.selector, Selector::Ordinal(_) | Selector::Range(..)) {
        return Err(SelectRefusal::Ordered(step.to_string()));
    }
    if let (true, Some(i)) = (node.is_record(), node.leaf) {
        return from_record(i, step, rest, widened, leaves, node, out);
    }
    match step.selector {
        Selector::Ident(name) => {
            match node.children.get(name) {
                Some(child) if child.keys.len() > 1 => Err(SelectRefusal::Ambiguous(name.to_string(), child.keys.clone())),
                Some(child) => walk(child, rest, widened, leaves, out),
                None if widened => Ok(()),
                None => Err(SelectRefusal::NoSuch(step.to_string())),
            }
        },
        Selector::Any => node.children.values().try_for_each(|c| walk(c, rest, true, leaves, out)),
        Selector::Deep => {
            let mut below = Vec::new();
            node.deep(&mut below);
            below.into_iter().try_for_each(|n| walk(n, rest, true, leaves, out))
        },
        // An expansion holds no set, and the two above were refused.
        Selector::Set(_) | Selector::Ordinal(_) | Selector::Range(..) => Ok(()),
    }
}

/// A step from the record of leaf `i`: a field its namespace declares, every field for `*`, the
/// record itself for `**` at no depth.
fn from_record<'n>(
    i: usize,
    step: &Step<'_>,
    rest: &[Step<'_>],
    widened: bool,
    leaves: &[Leaf<'_>],
    node: &'n Node,
    out: &mut Vec<Reached<'n>>,
) -> Result<(), SelectRefusal> {
    let leaf = &leaves[i];
    let fields: Vec<&str> = match step.selector {
        Selector::Deep => return walk(node, rest, true, leaves, out),
        Selector::Ident(name) if leaf.fields.contains(&name) => vec![name],
        Selector::Ident(_) if widened => return Ok(()),
        Selector::Ident(_) => return Err(SelectRefusal::NoSuch(step.to_string())),
        Selector::Any => {
            let mut all = leaf.fields.clone();
            all.sort_unstable();
            all
        },
        Selector::Set(_) | Selector::Ordinal(_) | Selector::Range(..) => return Ok(()),
    };
    if let Some(next) = rest.first() {
        // `**` past a field is no depth at all, which is the field.
        if !(rest.len() == 1 && next.selector == Selector::Deep && next.constraints.is_empty()) {
            return Err(SelectRefusal::PastField {
                row:   leaf.qualified.clone(),
                field: fields.first().map_or_else(|| step.to_string(), |f| (*f).to_string()),
            });
        }
    }
    out.extend(fields.into_iter().map(|f| Reached::Field(i, f.to_string())));
    Ok(())
}

/// The hits of `query` over `leaves`, in the order the standard fixes and each once.
fn run(query: &str, namespaces: &[&str], leaves: &[Leaf<'_>]) -> Result<Vec<Hit>, SelectRefusal> {
    let query = query.trim();
    let parsed = Query::parse(query).map_err(|e| SelectRefusal::Syntax(crate::Refusal::Syntax(e).say(query)))?;
    let path = parsed.path().ok_or(SelectRefusal::Operation)?;
    let expansion = path.expand().ok_or(SelectRefusal::TooWide)?;
    let root = tree(namespaces, leaves);
    let mut reached = Vec::new();
    for variant in expansion {
        let steps: Vec<Step<'_>> = variant.steps().collect();
        let steps = after_registry_root(&steps, |s| s.selector == Selector::Ident(REGISTRY_ROOT) && s.constraints.is_empty());
        walk(&root, steps, false, leaves, &mut reached)?;
    }
    let mut seen = BTreeSet::new();
    let mut hits = Vec::new();
    for r in reached {
        match r {
            Reached::Node(node) => {
                let mut under = Vec::new();
                node.rows(&mut under);
                hits.extend(under.into_iter().map(Hit::Row).filter(|h| seen.insert(h.clone())));
            },
            Reached::Field(i, f) => {
                let hit = Hit::Field(i, f);
                if seen.insert(hit.clone()) {
                    hits.push(hit);
                }
            },
        }
    }
    Ok(hits)
}

/// The qualified ids of `ids` a query selects, in the order the standard fixes and each once.
///
/// `ids` are qualified, `namespace::id`, the namespace as declared; a text that is no namespace and
/// path id is left out. No fields are declared over a plain list, so a step from a row fails where
/// nothing widened and passes the row over where something did. The same reader [`Registry::select`]
/// and resolution use, for a tool that holds its rows its own way.
///
/// # Errors
///
/// The [`SelectRefusal`] saying why the query selects nothing it can say. No ids is no refusal.
pub fn select_ids<'a>(query: &str, ids: &[&'a str]) -> Result<Vec<&'a str>, SelectRefusal> {
    let mut kept = Vec::new();
    let mut leaves = Vec::new();
    for id in ids {
        let Ok(names) = crate::names(id) else { continue };
        let Some((namespace, rest)) = names.split_first() else { continue };
        if rest.is_empty() || !rest.iter().all(|n| is_valid_slug(n)) {
            continue;
        }
        kept.push(*id);
        leaves.push(Leaf {
            namespace,
            names: rest.to_vec(),
            qualified: (*id).to_string(),
            fields: Vec::new(),
        });
    }
    let mut namespaces: Vec<&str> = leaves.iter().map(|l| l.namespace).collect();
    namespaces.sort_unstable();
    namespaces.dedup();
    let hits = run(query, &namespaces, &leaves)?;
    Ok(hits
        .into_iter()
        .filter_map(|h| {
            match h {
                Hit::Row(i) => Some(kept[i]),
                Hit::Field(..) => None,
            }
        })
        .collect())
}

/// What a query over the registry reached: a row, or one field of one.
#[derive(Debug, Clone)]
pub enum Selected<'r> {
    Row(&'r RegistryRow),
    Field(&'r RegistryRow, String),
}

impl Registry {
    /// What a query over the id tree selects, rows and fields, in the order the standard fixes and
    /// each once. The first step is the namespace, after the optional registry root.
    ///
    /// # Errors
    ///
    /// The [`SelectRefusal`] saying why the query selects nothing it can say. An answer of nothing
    /// after a widening step is no refusal.
    pub fn select(&self, namespaces: &[RegistryNamespace], query: &str) -> Result<Vec<Selected<'_>>, SelectRefusal> {
        let declared: BTreeMap<&str, Vec<&str>> = namespaces
            .iter()
            .map(|ns| {
                let mut fields: Vec<&str> = ns
                    .fields
                    .iter()
                    .filter(|f| f.visibility != FieldVisibility::Internal)
                    .map(|f| f.name.as_str())
                    .collect();
                fields.push("id");
                (ns.key.as_str(), fields)
            })
            .collect();
        let mut rows = Vec::new();
        let mut leaves = Vec::new();
        for ns in namespaces {
            for q in self.by_namespace.get(&ns.key).into_iter().flatten() {
                let Some(row) = self.rows.get(q) else { continue };
                // An id that is no path is the schema's finding, and no node here.
                let Ok(names) = row_id_names(&row.id) else { continue };
                rows.push(row);
                leaves.push(Leaf {
                    namespace: &ns.key,
                    names,
                    qualified: row.qualified(),
                    fields: declared.get(ns.key.as_str()).cloned().unwrap_or_default(),
                });
            }
        }
        let keys: Vec<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
        Ok(run(query, &keys, &leaves)?
            .into_iter()
            .map(|h| {
                match h {
                    Hit::Row(i) => Selected::Row(rows[i]),
                    Hit::Field(i, f) => Selected::Field(rows[i], f),
                }
            })
            .collect())
    }

    /// The rows alone of [`Self::select`], for a caller asking which rows and not what they say.
    ///
    /// # Errors
    ///
    /// As [`Self::select`].
    pub fn select_rows(&self, namespaces: &[RegistryNamespace], query: &str) -> Result<Vec<&RegistryRow>, SelectRefusal> {
        Ok(self
            .select(namespaces, query)?
            .into_iter()
            .filter_map(|s| {
                match s {
                    Selected::Row(r) => Some(r),
                    Selected::Field(..) => None,
                }
            })
            .collect())
    }
}

/// Two declared namespaces whose keys fold to one name, which a query could not tell apart.
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
                    "namespaces {} all fold to `{name}`, the name a query writes, so a query cannot tell \
                     them apart. Rename all but one.",
                    keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
                ),
                source:  None,
                gate:    GateScope::Always,
            }
        })
        .collect()
}

/// Why an expression that did not resolve was refused as a query over the row ids, where it is one:
/// its path, under any methods and operation, widens or opens on a namespace. `None` for anything
/// else, a citation among them, whose failure this does not explain.
pub(crate) fn refusal(namespaces: &[RegistryNamespace], reg: &Registry, expr: &str) -> Option<String> {
    let base = crate::chain::split_methods(expr).map_or_else(|| expr.to_string(), |(b, _)| b);
    let inner = ["pathof", "refsto", "sourcesof"].iter().find_map(|op| crate::operation(&base, op)).unwrap_or(&base);
    let path = Query::parse(inner).ok()?.path()?;
    let opens_on_a_namespace = path.steps().next().is_some_and(|s| {
        match s.selector {
            Selector::Ident(n) => n == REGISTRY_ROOT || namespaces.iter().any(|ns| mockspace_refnames::fold(&ns.key).is_some_and(|f| f == n)),
            _ => true,
        }
    });
    if !(opens_on_a_namespace || path.widens()) {
        return None;
    }
    reg.select(namespaces, inner).err().map(|r| r.say(inner))
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
