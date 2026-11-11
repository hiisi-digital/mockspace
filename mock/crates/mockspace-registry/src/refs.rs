//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Strip the placeholder delimiters from a line, yielding each expression.
///
/// References in prose are written `{{ reg::vocab::xpbd }}`, in the same
/// placeholder vocabulary the rest of the template system uses. The braces are
/// not ceremony: they make a reference something the author states rather than
/// something the renderer guesses at from a pattern. Without them a project
/// with a root named `core` would silently link `core::mem::12`, and prose
/// about code would be rewritten by accident.
///
/// One syntax then covers everything: `{{ reg::ns::slug }}` for a row,
/// `{{ reg::ns::slug::field }}` for a field, `{{ reg::ns }}` for a whole
/// table, and `{{ root::path::line }}` for a file citation. What stands inside
/// the braces is read by [`crate::names`], [`crate::FileRef`] and
/// [`crate::operation`], which are `letkajenkka`'s parser, and not here.
///
/// Provenance fields in the data need no braces: the field is already declared
/// to hold references, so there is nothing to disambiguate.
pub fn placeholder_exprs(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(s) = rest.find("{{") {
        let after = &rest[s + 2 ..];
        let Some(e) = after.find("}}") else { break };
        let inner = after[.. e].trim().to_string();
        if !inner.is_empty() {
            out.push((format!("{{{{{}}}}}", &after[.. e]), inner));
        }
        rest = &after[e + 2 ..];
    }
    out
}

/// Every `reg::` reference in `text`, as `(qualified, field)` pairs.
///
/// Explicit rather than scanned. An earlier form auto-linked any token that
/// looked like an identifier, which meant guessing whether a token was a
/// reference at all. Requiring `reg::` makes a reference something the author
/// states.
pub fn find_registry_refs(
    text: &str,
    namespaces: &BTreeSet<String>,
) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Only inside a placeholder. A bare `law::keys` in prose is not a
        // reference the renderer touches, so reporting it as dangling would
        // flag documentation that merely describes the syntax. The document
        // defining this syntax was the first thing to trip it.
        for (_, expr) in placeholder_exprs(line) {
            // A text the standard refuses, or one that is no plain path, is no row reference; the
            // gate that reports an expression nothing resolved is the one that says why.
            let Ok(mut parts) = crate::names(&expr) else { continue };
            // The prefix is optional and carries nothing, so drop it and treat
            // both spellings as the one reference they are.
            if parts.first() == Some(&REGISTRY_ROOT) {
                parts.remove(0);
            }
            // Slot zero decides what this is. A declared namespace makes it a
            // row reference; anything else is a citation root or a crate, which
            // are checked elsewhere and by different rules.
            let Some(ns) = parts.first() else { continue };
            if !namespaces.contains(*ns) {
                continue;
            }
            match parts.len() {
                3 => {
                    found.push((
                        format!("{}::{}", parts[0], parts[1]),
                        Some(parts[2].to_string()),
                    ))
                },
                2 => found.push((format!("{}::{}", parts[0], parts[1]), None)),
                _ => {},
            }
        }
    }
    found
}
/// References naming a row that does not exist, or a field that row does not
/// carry. Both are silent without a check: the first renders as nothing
/// useful, the second renders as empty.
pub fn dangling_references(
    text: &str,
    reg: &Registry,
    namespaces: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (qualified, field) in find_registry_refs(text, namespaces) {
        match reg.get(&qualified) {
            None => {
                out.insert(qualified.clone());
            },
            Some(row) => {
                if let Some(f) = &field {
                    if f != "id" && !row.fields.contains_key(f) {
                        out.insert(format!("{qualified}::{f}"));
                    }
                }
            },
        }
    }
    out
}


/// A relative path from `from` (a directory) to `to`, in forge-link form.
pub(crate) fn relative_from(from: &Path, to: &Path) -> String {
    let f: Vec<_> = from.components().collect();
    let t: Vec<_> = to.components().collect();
    let common = f.iter().zip(t.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); f.len() - common];
    parts.extend(
        t[common ..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().to_string()),
    );
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}
