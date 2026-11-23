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
    placeholder_spans(line).into_iter().map(|(span, inner)| (line[span].to_string(), inner.to_string())).collect()
}

/// Each placeholder of `line` as the byte span of the whole placeholder, braces included, and the
/// expression inside it trimmed: what [`placeholder_exprs`] reads, with where each one stands, so a
/// rewrite edits that placeholder and no other text that happens to be spelled alike.
pub fn placeholder_spans(line: &str) -> Vec<(core::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(s) = line[from ..].find("{{").map(|s| from + s) {
        let Some(e) = line[s + 2 ..].find("}}").map(|e| s + 2 + e) else { break };
        let inner = line[s + 2 .. e].trim();
        if !inner.is_empty() {
            out.push((s .. e + 2, inner));
        }
        from = e + 2;
    }
    out
}

/// Every row reference in `text`, as `(qualified, field)` pairs: the row's `namespace::id` and the
/// field step after it, where there is one.
///
/// Explicit rather than scanned. An earlier form auto-linked any token that
/// looked like an identifier, which meant guessing whether a token was a
/// reference at all. Requiring a placeholder makes a reference something the author
/// states.
///
/// Where the id ends is the registry's to say, since an id may be a path, so the reference is read
/// by [`Registry::locate`]. A reference naming no row is returned whole, as `(namespace::rest,
/// None)`, so the check that reports it can name what was written.
pub fn find_registry_refs(
    text: &str,
    namespaces: &BTreeSet<String>,
    reg: &Registry,
) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    let mut fences = mockspace_refnames::Fences::new();
    for line in text.lines() {
        if !fences.prose(line) {
            continue;
        }
        // Only inside a placeholder. A bare `law::keys` in prose is not a
        // reference the renderer touches, so reporting it as dangling would
        // flag documentation that merely describes the syntax. The document
        // defining this syntax was the first thing to trip it.
        for (_, expr) in placeholder_exprs(line) {
            // A text the standard refuses, or one that is no plain path, is no row reference; the
            // gate that reports an expression nothing resolved is the one that says why.
            let Ok(parts) = crate::names(&expr) else { continue };
            // The prefix is optional and carries nothing, so it is skipped and both spellings
            // are the one reference they are.
            let after = &parts[usize::from(parts.first() == Some(&REGISTRY_ROOT)) ..];
            // Slot zero decides what this is. A declared namespace makes it a
            // row reference; anything else is a citation root or a crate, which
            // are checked elsewhere and by different rules.
            let Some(ns) = after.first() else { continue };
            if !namespaces.contains(*ns) || after.len() < 2 {
                continue;
            }
            match reg.locate(after) {
                Some(at) => {
                    match at.fields {
                        [] => found.push((at.row.qualified(), None)),
                        [field] => found.push((at.row.qualified(), Some(field.to_string()))),
                        // a walk through a field is resolution's to check, not this
                        _ => {},
                    }
                },
                None => found.push((after.join("::"), None)),
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
    for (qualified, field) in find_registry_refs(text, namespaces, reg) {
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
