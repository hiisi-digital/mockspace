//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one walker of a path into the registry, over the tree of row ids.
//!
//! The root is a branch whose members are the declared namespaces, each under its key folded into
//! the standard's set. A namespace is a branch whose members are the first names of its rows' ids,
//! each proper prefix of an id is a branch, and each id is a record whose members are its fields: a
//! field its namespace declares, or one the row writes (which the schema allows only where it is
//! declared), and `id`. A field holding references to rows is a node (§7), so a step past it goes on
//! into each row it names. What a walk reaches, and whether anything widened on the way, is all it
//! answers; what that is shown as is the caller's.
//!
//! The tree is never built. A node is a namespace and the names below it, and its members are read
//! off the store's keys in order when a step asks for them, so a plain path costs a few lookups
//! however many rows the registry holds, and a wildcard costs the rows below where it stands.

use std::ops::Bound;

use letkajenkka::{Query, Selector, Step};

use super::*;

/// Where the walker reads rows from: their qualified keys in order, their fields and values.
pub(crate) trait Store {
    /// Every declared namespace key.
    fn namespaces(&self) -> Vec<&str>;
    /// The keys from `from` on, in key order.
    fn keys_from<'s>(&'s self, from: &str) -> Box<dyn Iterator<Item = &'s str> + 's>;
    /// Whether `key` is a row.
    fn is_row(&self, key: &str) -> bool;
    /// The fields a step from the row `key` may name, with their types.
    fn fields(&self, key: &str) -> Vec<(String, String)>;
    /// The value the row `key` holds in `field`.
    fn value(&self, key: &str, field: &str) -> Option<&str>;
}

/// A node: the root, or a namespace and the names below it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Node {
    ns:    Option<String>,
    names: Vec<String>,
}

impl Node {
    const ROOT: Self = Self {
        ns:    None,
        names: Vec::new(),
    };

    /// The node's qualified key: `namespace` or `namespace::name::name`.
    fn key(&self) -> String {
        let mut k = self.ns.clone().unwrap_or_default();
        for n in &self.names {
            k.push_str("::");
            k.push_str(n);
        }
        k
    }

    fn child(&self, name: &str) -> Self {
        let mut names = self.names.clone();
        names.push(name.to_string());
        Self {
            ns: self.ns.clone(),
            names,
        }
    }
}

/// One thing a query reached, by the qualified key of its row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Hit {
    Row(String),
    Field(String, String),
}

/// What a query reached, and whether it named one record or one field with nothing widening: a
/// plain read, shown as that row or that value.
pub(crate) struct Answer {
    pub hits:    Vec<Hit>,
    pub single:  bool,
    /// The rows a step went on from through a reference field, each once.
    pub through: Vec<String>,
}

/// What a walk reached, and whether a step before it widened.
enum Reached {
    Node(Node, bool),
    Field(String, String, bool),
}

/// Why a walk stopped. The variants are [`crate::SelectRefusal`]'s, which says them.
pub(crate) use crate::SelectRefusal as Stop;

/// The walker over one store.
pub(crate) struct Walker<'s, S: Store> {
    store:   &'s S,
    through: std::cell::RefCell<Vec<String>>,
}

impl<'s, S: Store> Walker<'s, S> {
    pub(crate) fn new(store: &'s S) -> Self {
        Self {
            store,
            through: std::cell::RefCell::default(),
        }
    }

    /// What `query` reaches, each expansion in turn, in the order the standard fixes: within an
    /// expansion each row and each field once, however many `**` paths reach it (§8.4), and across
    /// expansions everything each one reaches, a set's repeated member read twice (§8.3).
    pub(crate) fn run(&self, query: &str) -> Result<Answer, Stop> {
        let refused = |e| Stop::Syntax(crate::Refusal::Syntax(e).say(query.trim()));
        let text = crate::standard_spelling(query.trim()).map_err(refused)?;
        let parsed = Query::parse(&text).map_err(refused)?;
        let path = parsed.path().ok_or(Stop::Operation)?;
        let expansion = path.expand().ok_or(Stop::TooWide)?;
        let mut hits = Vec::new();
        let mut count = 0;
        let mut one = false;
        for variant in expansion {
            let steps: Vec<Step<'_>> = variant.steps().collect();
            let steps = after_registry_root(&steps, |s| s.selector == Selector::Ident(REGISTRY_ROOT) && s.constraints.is_empty());
            let mut reached = Vec::new();
            self.walk(&Node::ROOT, steps, false, &mut reached)?;
            count += reached.len();
            one = match reached.as_slice() {
                [Reached::Node(node, widened)] => !widened && self.is_record(node),
                [Reached::Field(_, _, widened)] => !widened,
                _ => false,
            };
            let mut seen = BTreeSet::new();
            for r in reached {
                match r {
                    Reached::Node(node, _) => {
                        hits.extend(self.rows_under(&node).into_iter().map(Hit::Row).filter(|h| seen.insert(h.clone())));
                    },
                    Reached::Field(k, f, _) => {
                        let hit = Hit::Field(k, f);
                        if seen.insert(hit.clone()) {
                            hits.push(hit);
                        }
                    },
                }
            }
        }
        let mut through = self.through.take();
        through.sort_unstable();
        through.dedup();
        Ok(Answer {
            hits,
            single: !path.widens() && count == 1 && one,
            through,
        })
    }

    /// The namespaces a name at the root names: every key folding to it.
    fn namespaces_named(&self, name: &str) -> Vec<String> {
        self.store
            .namespaces()
            .into_iter()
            .filter(|k| mockspace_refnames::fold(k).is_some_and(|f| f == name))
            .map(String::from)
            .collect()
    }

    /// The names of the row `key` of namespace `ns` after the namespace, where its id is a path.
    fn id_names<'k>(ns: &str, key: &'k str) -> Option<Vec<&'k str>> {
        row_id_names(key.get(ns.len() + 2 ..)?).ok()
    }

    /// The ids at or below `node`, each as its names, the node's own included, in key order.
    fn ids_under(&self, node: &Node) -> Vec<Vec<String>> {
        let Some(ns) = &node.ns else { return Vec::new() };
        let key = node.key();
        let stem = format!("{key}::");
        let from = if node.names.is_empty() { stem.clone() } else { key.clone() };
        self.store
            .keys_from(&from)
            .take_while(|k| k.starts_with(&from))
            .filter(|k| *k == key || k.starts_with(&stem))
            .filter_map(|k| Self::id_names(ns, k))
            .map(|n| n.into_iter().map(String::from).collect())
            .collect()
    }

    /// Whether any row lies strictly below `node`.
    fn has_below(&self, node: &Node) -> bool {
        let stem = format!("{}::", node.key());
        self.store.keys_from(&stem).next().is_some_and(|k| k.starts_with(&stem))
    }

    fn is_record(&self, node: &Node) -> bool {
        node.ns.is_some() && !node.names.is_empty() && self.store.is_row(&node.key()) && !self.has_below(node)
    }

    /// The members of a branch: the namespaces at the root, by folded name, else the next names.
    fn children(&self, node: &Node) -> Vec<Node> {
        let Some(ns) = &node.ns else {
            let mut keys: Vec<(String, &str)> = self
                .store
                .namespaces()
                .into_iter()
                .filter_map(|k| Some((mockspace_refnames::fold(k)?.to_string(), k)))
                .collect();
            keys.sort();
            return keys
                .into_iter()
                .map(|(_, k)| {
                    Node {
                        ns:    Some(k.to_string()),
                        names: Vec::new(),
                    }
                })
                .collect();
        };
        let depth = node.names.len();
        let names: BTreeSet<String> = self.ids_under(node).into_iter().filter_map(|n| n.get(depth).cloned()).collect();
        names
            .into_iter()
            .map(|n| {
                Node {
                    ns:    Some(ns.clone()),
                    names: [node.names.clone(), vec![n]].concat(),
                }
            })
            .collect()
    }

    /// `node` and every node below it, branches and records, members in byte order step by step.
    fn deep(&self, node: &Node) -> Vec<Node> {
        let Some(ns) = &node.ns else {
            let mut out = vec![Node::ROOT];
            for child in self.children(node) {
                out.extend(self.deep(&child));
            }
            return out;
        };
        let depth = node.names.len();
        let mut below: BTreeSet<Vec<String>> = BTreeSet::new();
        for names in self.ids_under(node) {
            for n in depth + 1 ..= names.len() {
                below.insert(names[.. n].to_vec());
            }
        }
        let mut out = vec![node.clone()];
        out.extend(below.into_iter().map(|names| {
            Node {
                ns: Some(ns.clone()),
                names,
            }
        }));
        out
    }

    /// The rows at or below `node`, by key, members in byte order step by step.
    fn rows_under(&self, node: &Node) -> Vec<String> {
        if node.ns.is_none() {
            return self.children(node).iter().flat_map(|c| self.rows_under(c)).collect();
        }
        let mut ids = self.ids_under(node);
        ids.sort();
        ids.into_iter()
            .map(|names| {
                Node {
                    ns: node.ns.clone(),
                    names,
                }
                .key()
            })
            .filter(|k| self.store.is_row(k))
            .collect()
    }

    fn walk(&self, node: &Node, steps: &[Step<'_>], widened: bool, out: &mut Vec<Reached>) -> Result<(), Stop> {
        let Some((step, rest)) = steps.split_first() else {
            out.push(Reached::Node(node.clone(), widened));
            return Ok(());
        };
        if !step.constraints.is_empty() {
            return Err(Stop::Constraint(step.to_string()));
        }
        let row = node.ns.is_some() && !node.names.is_empty() && self.store.is_row(&node.key());
        match step.selector {
            Selector::Ordinal(_) | Selector::Range(..) => Err(Stop::Ordered(step.to_string())),
            Selector::Ident(name) if node.ns.is_none() => {
                match self.namespaces_named(name).as_slice() {
                    [one] => {
                        let ns = Node {
                            ns:    Some(one.clone()),
                            names: Vec::new(),
                        };
                        self.walk(&ns, rest, widened, out)
                    },
                    [] if widened => Ok(()),
                    [] => Err(Stop::NoSuch(step.to_string())),
                    many => Err(Stop::Ambiguous(name.to_string(), many.to_vec())),
                }
            },
            Selector::Ident(name) => {
                let child = node.child(name);
                // A row whose id is a prefix of another's (`id-is-a-branch`) is still a row: the
                // longer id wins a name, and a name no child takes is its field.
                if self.store.is_row(&child.key()) || self.has_below(&child) {
                    self.walk(&child, rest, widened, out)
                } else if row {
                    self.field_steps(&node.key(), &[name], rest, widened, out)
                } else if widened {
                    Ok(())
                } else {
                    Err(Stop::NoSuch(step.to_string()))
                }
            },
            Selector::Any => {
                for child in self.children(node) {
                    self.walk(&child, rest, true, out)?;
                }
                if row { self.every_field(&node.key(), rest, out) } else { Ok(()) }
            },
            Selector::Deep => {
                for n in self.deep(node) {
                    self.walk(&n, rest, true, out)?;
                }
                // Taken from a record, `**` covers what `*` does, its fields, as well as the record
                // at no depth (§8.4). Taken from a branch it reaches the records below as rows and
                // does not open them, so `ticket::**` is rows.
                if self.is_record(node) { self.every_field(&node.key(), rest, out) } else { Ok(()) }
            },
            // An expansion holds no set.
            Selector::Set(_) => Ok(()),
        }
    }

    /// Every field of the row `key`, each walked on with `rest`.
    fn every_field(&self, key: &str, rest: &[Step<'_>], out: &mut Vec<Reached>) -> Result<(), Stop> {
        let fields = self.store.fields(key);
        let mut names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        self.field_steps(key, &names, rest, true, out)
    }

    /// The fields `names` of the row `key`: each reached where the path ends, each a node to go on
    /// from where it holds references, and a stop where it holds a value and the path goes on.
    fn field_steps(&self, key: &str, names: &[&str], rest: &[Step<'_>], widened: bool, out: &mut Vec<Reached>) -> Result<(), Stop> {
        let fields = self.store.fields(key);
        for name in names {
            let Some((_, ty)) = fields.iter().find(|(n, _)| n == name) else {
                if widened {
                    continue;
                }
                return Err(Stop::NoSuch((*name).to_string()));
            };
            if rest.is_empty() {
                out.push(Reached::Field(key.to_string(), (*name).to_string(), widened));
                continue;
            }
            match self.targets(key, name, ty) {
                Some((targets, many)) => {
                    self.through.borrow_mut().push(key.to_string());
                    for t in targets {
                        self.walk(&t, rest, widened || many, out)?;
                    }
                },
                None if widened => {},
                None => {
                    return Err(Stop::PastField {
                        row:   key.to_string(),
                        field: (*name).to_string(),
                        ty:    ty.clone(),
                    });
                },
            }
        }
        Ok(())
    }

    /// The nodes of the rows a reference field of the row `key` names, and whether it may name
    /// several; `None` where the field holds no references. A row the field names and the store
    /// does not hold is left out: the reference check reports it.
    fn targets(&self, key: &str, name: &str, ty: &str) -> Option<(Vec<Node>, bool)> {
        let keys = self.store.namespaces();
        let declared: BTreeSet<&str> = keys.iter().copied().collect();
        let raw = self.store.value(key, name).unwrap_or("");
        let items = crate::refpath::list(raw);
        let ids: Vec<(String, String)> = match ty {
            "ref" | "ref[]" => items.filter_map(|item| link_target(item, &declared).map(|(n, id)| (n.to_string(), id))).collect(),
            _ => {
                let (target, _) = row_reference_target(ty)?;
                if !declared.contains(target) {
                    return None;
                }
                items.map(|id| (target.to_string(), id.to_string())).collect()
            },
        };
        let nodes = ids
            .into_iter()
            .filter_map(|(ns, id)| {
                let names = row_id_names(&id).ok()?.into_iter().map(String::from).collect();
                let node = Node {
                    ns: Some(ns),
                    names,
                };
                self.store.is_row(&node.key()).then_some(node)
            })
            .collect();
        Some((nodes, ty.ends_with("[]")))
    }
}

/// The keys of a sorted map from `from` on, for a [`Store`] over one.
pub(crate) fn keys_from<'m, V>(map: &'m BTreeMap<String, V>, from: &str) -> Box<dyn Iterator<Item = &'m str> + 'm> {
    Box::new(map.range::<str, _>((Bound::Included(from), Bound::Unbounded)).map(|(k, _)| k.as_str()))
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
