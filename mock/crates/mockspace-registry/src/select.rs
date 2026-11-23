//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Selecting rows by a query over the tree of their ids.
//!
//! The registry read as the letkajenkka standard reads it: a root branch whose members are the
//! declared namespaces, each namespace a branch whose members are the first names of its rows'
//! ids, each proper prefix of an id a branch, and each id a record. `ticket::question::engine`
//! names a branch and `ticket::question::engine::allowed` a record.
//!
//! What a query selects:
//!
//! - **A node stands for the rows at or below it.** A record is its row; a branch is every row
//!   under it, so `ticket::question::engine` and `ticket::question::engine::**` select alike.
//! - **An ident** names one member. Under a declared namespace every path is a branch, empty where
//!   no id begins with it, so an ident naming nothing selects nothing. A first step naming no
//!   declared namespace is refused, as a namespace is declared and a typo in one is no answer.
//! - **A set** is its expansion, by `letkajenkka`: each member's path in written order.
//! - **`*`** is every member of the node, **`**`** the node itself and every node below it, each
//!   in byte order of the matched names.
//! - **Each row once**, at its first place, however many paths and set members reach it.
//! - **Refused**: an ordinal or a range (a branch has no order, and a record no lines), a
//!   constraint (the registry declares no shapes), an operation, and a field step. A step taken
//!   from a record names one of its fields, which is resolution's (`cargo mock query`) and not
//!   selection's. The tree is mixed, holding branches and records together, and after a widening
//!   step this host yields only the members for which the step resolves (§6): a field step
//!   resolves on none, so `ticket::**::internals::**` passes over a row whose id ends before an
//!   `internals`. A query that does not widen names one node, and a field step from it is refused.

use std::process::ExitCode;

use letkajenkka::{Query, Selector, Step};

use super::*;

/// Why a query selects nothing it can say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectRefusal {
    /// The standard refuses the text, as the finding says.
    Syntax(String),
    /// An operation, `refsto(...)` and the like, which is resolution's.
    Operation,
    /// More combinations of set members than can be counted.
    TooWide,
    /// A first step naming no declared namespace.
    UnknownNamespace(String),
    /// An ordinal or a range, which no branch and no record takes.
    Ordered(String),
    /// A constraint, naming a shape the registry does not declare.
    Constraint(String),
    /// A step from the named row, which is a field step.
    FieldStep { row: String, step: String },
}

impl SelectRefusal {
    /// The refusal as the person reads it.
    pub fn say(&self, query: &str) -> String {
        match self {
            Self::Syntax(why) => why.clone(),
            Self::Operation => format!("`{query}` is an operation; a selection is a path of rows (`cargo mock query` resolves it)"),
            Self::TooWide => format!("`{query}` expands to more paths than can be counted"),
            Self::UnknownNamespace(ns) => format!("`{query}` opens on `{ns}`, which is no declared namespace"),
            Self::Ordered(step) => {
                format!("`{query}`: `{step}` selects by position, and a namespace's rows have no order to select by")
            },
            Self::Constraint(step) => format!("`{query}`: `{step}` names a shape, and the registry declares none"),
            Self::FieldStep {
                row,
                step,
            } => {
                format!(
                    "`{query}`: `{step}` is a field step from the row {row}; a selection names rows, and `cargo mock query` resolves a field"
                )
            },
        }
    }
}

/// One node of the id tree: its members by name, and the row it is, where it is one.
#[derive(Default)]
struct Node<'r> {
    children: BTreeMap<String, Node<'r>>,
    row:      Option<&'r RegistryRow>,
}

impl<'r> Node<'r> {
    /// Every row at or below this node, in byte order of their ids.
    fn rows(&self, out: &mut Vec<&'r RegistryRow>) {
        out.extend(self.row);
        for child in self.children.values() {
            child.rows(out);
        }
    }

    /// This node and every node below it, the node first and then its members in byte order.
    fn deep<'a>(&'a self, out: &mut Vec<&'a Node<'r>>) {
        out.push(self);
        for child in self.children.values() {
            child.deep(out);
        }
    }

    /// A record: a row with nothing below it.
    fn is_record(&self) -> bool {
        self.row.is_some() && self.children.is_empty()
    }
}

/// The tree of `reg`'s row ids under the declared namespaces, keyed by each namespace's name as
/// the standard spells it (a host's hyphen folded to an underscore).
fn tree<'r>(reg: &'r Registry, namespaces: &[RegistryNamespace]) -> Node<'r> {
    let mut root = Node::default();
    for ns in namespaces {
        let key = mockspace_refnames::fold_separators(&ns.key).unwrap_or_else(|| ns.key.clone());
        let branch = root.children.entry(key).or_default();
        for q in reg.by_namespace.get(&ns.key).into_iter().flatten() {
            let Some(row) = reg.rows.get(q) else { continue };
            // An id that is no path is the schema's finding, and no node here.
            let Ok(names) = row_id_names(&row.id) else { continue };
            let mut at = &mut *branch;
            for name in names {
                at = at.children.entry(name.to_string()).or_default();
            }
            at.row = Some(row);
        }
    }
    root
}

/// The nodes `steps` reach from `node`, appended to `out` in the order the standard fixes.
fn walk<'a, 'r>(
    node: &'a Node<'r>,
    steps: &[Step<'_>],
    widened: bool,
    out: &mut Vec<&'a Node<'r>>,
) -> Result<(), SelectRefusal> {
    let Some((step, rest)) = steps.split_first() else {
        out.push(node);
        return Ok(());
    };
    if !step.constraints.is_empty() {
        return Err(SelectRefusal::Constraint(step.to_string()));
    }
    if matches!(step.selector, Selector::Ordinal(_) | Selector::Range(..)) {
        return Err(SelectRefusal::Ordered(step.to_string()));
    }
    // `**` includes no depth at all, which a record has too: it is the record itself.
    if node.is_record() && step.selector == Selector::Deep {
        return walk(node, rest, true, out);
    }
    if node.is_record() {
        return match (widened, node.row) {
            (true, _) => Ok(()),
            (false, Some(row)) => {
                Err(SelectRefusal::FieldStep {
                    row:  row.qualified(),
                    step: step.to_string(),
                })
            },
            (false, None) => Ok(()),
        };
    }
    match step.selector {
        Selector::Ident(name) => {
            if let Some(child) = node.children.get(name) {
                walk(child, rest, widened, out)?;
            }
        },
        Selector::Any => {
            for child in node.children.values() {
                walk(child, rest, true, out)?;
            }
        },
        Selector::Deep => {
            let mut below = Vec::new();
            node.deep(&mut below);
            for n in below {
                walk(n, rest, true, out)?;
            }
        },
        // An expansion holds no set, and the two above were refused.
        Selector::Set(_) | Selector::Ordinal(_) | Selector::Range(..) => {},
    }
    Ok(())
}

impl Registry {
    /// The rows a query over the id tree selects, in the order the standard fixes and each once.
    ///
    /// The first step is the namespace, or the registry root `reg` before it. See the module
    /// documentation for what each selector selects and what is refused.
    ///
    /// # Errors
    ///
    /// A [`SelectRefusal`] saying why the query selects nothing it can say. An answer of no rows
    /// is no refusal.
    pub fn select(&self, namespaces: &[RegistryNamespace], query: &str) -> Result<Vec<&RegistryRow>, SelectRefusal> {
        let query = query.trim();
        let parsed = match Query::parse(query) {
            Ok(q) => q,
            // A namespace named after a directory may carry its hyphen in the first step, as a
            // reference's root may, and nowhere after it.
            Err(e) => {
                let hosted = namespaces.iter().find_map(|ns| {
                    let folded = mockspace_refnames::fold_separators(&ns.key)?;
                    let rest = query.strip_prefix(ns.key.as_str())?;
                    (rest.is_empty() || rest.starts_with("::")).then(|| format!("{folded}{rest}"))
                });
                return match hosted {
                    Some(folded) if Query::parse(&folded).is_ok() => self.select(namespaces, &folded),
                    _ => Err(SelectRefusal::Syntax(crate::Refusal::Syntax(e).say(query))),
                };
            },
        };
        let Some(path) = parsed.path() else { return Err(SelectRefusal::Operation) };
        let expansion = path.expand().ok_or(SelectRefusal::TooWide)?;
        let root = tree(self, namespaces);
        let mut nodes = Vec::new();
        for variant in expansion {
            let mut steps: Vec<Step<'_>> = variant.steps().collect();
            if steps.len() > 1 && steps[0].selector == Selector::Ident(REGISTRY_ROOT) && steps[0].constraints.is_empty() {
                steps.remove(0);
            }
            if let Some(Step {
                selector: Selector::Ident(ns),
                ..
            }) = steps.first()
            {
                if !root.children.contains_key(*ns) {
                    return Err(SelectRefusal::UnknownNamespace((*ns).to_string()));
                }
            }
            walk(&root, &steps, false, &mut nodes)?;
        }
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        for node in nodes {
            let mut under = Vec::new();
            node.rows(&mut under);
            rows.extend(under.into_iter().filter(|r| seen.insert(r.qualified())));
        }
        Ok(rows)
    }
}

/// `cargo mock select <query>`: the qualified id of each row the query selects, one a line.
///
/// Silent with exit status 0 on an empty answer, since a pattern matching nothing is an answer; a
/// refused query is said on stderr with status 1.
pub fn cmd_select(cfg: &dyn RegistryEnv, query: &str) -> ExitCode {
    if query.trim().is_empty() {
        eprintln!("usage: cargo mock select '<query>'");
        eprintln!();
        eprintln!("  ticket::question::engine          every row whose id begins question::engine");
        eprintln!("  ticket::**::internals::**         every row with `internals` anywhere in its id");
        eprintln!("  ticket::{{task, question}}::*       the rows under either, in that order");
        eprintln!("  ticket::question::engine::allowed one row");
        return ExitCode::FAILURE;
    }
    let registry = match load_registry_for(cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    match registry.select(cfg.namespaces(), query) {
        Ok(rows) => {
            for row in rows {
                println!("{}", row.qualified());
            }
            ExitCode::SUCCESS
        },
        Err(refusal) => {
            eprintln!("{}", refusal.say(query.trim()));
            ExitCode::FAILURE
        },
    }
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
