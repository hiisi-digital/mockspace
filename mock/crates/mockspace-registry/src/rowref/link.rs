//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Row links held in a `ref` or `ref[]` field.
//!
//! A namespace declares one relation field typed `ref[]` and every row it
//! relates to goes in it, whichever namespace the other row lives in. Naming a
//! field per target namespace (`ikiuni`, `bevy`, `canon`) made the schema a
//! list of places a relation could be written, and one relation written in two
//! of them said the same thing twice.
//!
//! An item of such a field is one of two shapes that cannot be mistaken for each
//! other. A file citation opens on a root: `research::dir::file::heading`. A row
//! link opens on a declared namespace, and every name after it is the row's id:
//! `tenet::engine_time`, or `ticket::question::engine::allowed` where ids are
//! paths. `namespace-root-collision` keeps a root from carrying a namespace's
//! name, so an item is a link exactly when its first name is a namespace.
//!
//! The link is written once, on one row. The other end is derived: `links_of`
//! lists a row's links from both ends, and `refsto` finds the rows linking to
//! one, through the same edge walk the typed row-reference fields use.

use super::*;

/// The namespace and id of an item that links a row, when it is one: a path of two names or more,
/// the first a declared namespace, in either spelling and returned as declared, and every name
/// after it the row's id, `::` joined. A link names a
/// row and never a field of one, so nothing after the id is read.
///
/// An item the standard refuses is no link. It is a citation that does not parse, and the finding
/// for it carries the rule it broke, so `slot::Not-A-Slug` is reported as the uppercase it is.
pub fn link_target<'k>(item: &str, namespaces: &BTreeSet<&'k str>) -> Option<(&'k str, String)> {
    match crate::names(item.trim()).ok()?.as_slice() {
        [namespace, id @ ..] if !id.is_empty() => {
            let key = crate::namespace_named(namespace, namespaces.iter().copied())?;
            Some((key, id.join("::")))
        },
        _ => None,
    }
}

/// The declared namespace keys, which is what makes an item a link.
pub fn namespace_keys(namespaces: &[RegistryNamespace]) -> BTreeSet<&str> {
    namespaces.iter().map(|n| n.key.as_str()).collect()
}

/// A `ref` cell with each row link rewritten as a reference to the row, and
/// every file citation left as it was.
///
/// The cell emits the reference and the pass after it resolves it, which is how
/// a typed row-reference cell works too, so there is one implementation of what
/// a row link renders as and a link to a missing row stays visibly unresolved.
pub fn render_links(raw: &str, namespaces: &BTreeSet<&str>) -> String {
    raw.split(", ")
        .map(|item| {
            match link_target(item, namespaces) {
                Some((namespace, slug)) => format!("{{{{ {namespace}::{slug} }}}}"),
                None => item.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check every row link against the rows that exist.
///
/// A link naming no row is reported as `unknown-row-reference` and one whose
/// slug is not a slug as `malformed-row-reference`, the two kinds a typed
/// row-reference field reports.
///
/// Not meant to be called alone. [`validate_row_references`] ends with it, and
/// that is what its two callers (the gate in `entry/dispatch.rs` and the
/// writer's check in `registry/writer.rs`) run, so a link is checked wherever a
/// typed row reference is and neither caller has to remember it. Calling this
/// instead of that checks links and skips the typed fields.
pub fn validate_links(reg: &Registry, namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let keys = namespace_keys(namespaces);
    let citing = reference_fields(namespaces);
    let mut out = Vec::new();
    for row in reg.rows.values() {
        let empty = Vec::new();
        for name in citing.get(&row.namespace).unwrap_or(&empty) {
            let Some(raw) = row.fields.get(name) else {
                continue;
            };
            if raw.trim().is_empty() {
                continue;
            }
            for item in raw.split(", ").map(str::trim) {
                let Some((target, slug)) = link_target(item, &keys) else {
                    continue;
                };
                out.extend(super::dangling(reg, row, name, target, &slug));
            }
        }
    }
    out
}

/// What `cargo mock links <namespace>::<slug>` prints: the row's links from both
/// ends, each with the field that states it.
///
/// An argument that is not a row of a declared namespace is an error saying
/// which of the two it failed, with the nearest slug when one is close, since a
/// typo and an absent row call for different next steps.
pub fn links_report(
    reg: &Registry,
    namespaces: &[RegistryNamespace],
    id: &str,
) -> Result<String, String> {
    let id = id.trim();
    let keys = namespace_keys(namespaces);
    let Some((namespace, slug)) = link_target(id, &keys) else {
        let known: Vec<&str> = keys.iter().copied().collect();
        return Err(format!(
            "`{id}` is not `namespace::id` of a declared namespace ({}).",
            known.join(", ")
        ));
    };
    // The row as the registry keys it, whichever spelling of the namespace was written.
    let qualified = format!("{namespace}::{slug}");
    if !reg.rows.contains_key(&qualified) {
        let near = super::nearest(reg, namespace, &slug);
        return Err(format!("no row `{id}`.{near}"));
    }
    let links = links_of(reg, namespaces, &qualified);
    let mut out = format!("{qualified}\n");
    for (title, side) in [("links to", &links.outgoing), ("linked from", &links.incoming)] {
        if side.is_empty() {
            out.push_str(&format!("  {title}: nothing\n"));
            continue;
        }
        out.push_str(&format!("  {title}:\n"));
        let width = side.iter().map(|l| l.field.len()).max().unwrap_or(0);
        for l in side {
            out.push_str(&format!("    {:width$}  {}\n", l.field, l.other));
        }
    }
    Ok(out)
}

/// One end of a link as seen from a row: the field that holds it and the row at
/// the other end.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    /// The field, on the row that states the link.
    pub field: String,
    /// The row at the other end, as `namespace::slug`.
    pub other: String,
}

impl Link {
    pub fn new(field: &str, other: &str) -> Self {
        Self {
            field: field.to_string(),
            other: other.to_string(),
        }
    }
}

/// A row's links from both ends.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Links {
    /// What the row states: its own fields, and the row each points at.
    pub outgoing: Vec<Link>,
    /// What states a link to the row, and the field each does it in.
    pub incoming: Vec<Link>,
}

/// Every link of `qualified`, in both directions, from the typed row-reference
/// fields and the row links in `ref` fields alike.
///
/// Sorted and without repeats. A link stated in two fields of one row is two
/// entries, since the fields differ, and the same field listing the same row
/// twice is one.
pub fn links_of(reg: &Registry, namespaces: &[RegistryNamespace], qualified: &str) -> Links {
    let mut outgoing = BTreeSet::new();
    let mut incoming = BTreeSet::new();
    super::for_each_edge_by_field(reg, namespaces, |target, source, field| {
        if source == qualified {
            outgoing.insert(Link::new(field, &target));
        }
        if target == qualified {
            incoming.insert(Link::new(field, &source));
        }
    });
    Links {
        outgoing: outgoing.into_iter().collect(),
        incoming: incoming.into_iter().collect(),
    }
}

/// `cargo mock links <namespace>::<slug>`: print a row's links from both ends.
///
/// The registry stores each link once, on the row that states it, so there is
/// no second place to keep in step. This is the tool that shows the other end.
pub fn cmd_links(cfg: &dyn RegistryEnv, id: &str) -> std::process::ExitCode {
    use std::process::ExitCode;

    if id.trim().is_empty() {
        eprintln!("usage: cargo mock links <namespace>::<id>");
        return ExitCode::FAILURE;
    }
    let registry = match load_registry_for(cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    match links_report(&registry, cfg.namespaces(), id) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        },
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        },
    }
}
