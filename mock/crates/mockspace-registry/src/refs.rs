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
/// One syntax then covers everything: `{{ reg::ns::id }}` for a row,
/// `{{ reg::ns::id::field }}` for a field, `{{ reg::ns }}` for a whole
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

/// Every path into the registry in `text`'s placeholders, in the standard's spelling, with the text
/// each was written as.
///
/// Explicit rather than scanned. An earlier form auto-linked any token that
/// looked like an identifier, which meant guessing whether a token was a
/// reference at all. Requiring a placeholder makes a reference something the author
/// states. Only inside a placeholder, too: a bare `law::keys` in prose is not a
/// reference the renderer touches, so reporting it as dangling would flag
/// documentation that merely describes the syntax.
fn registry_paths(text: &str, namespaces: &[RegistryNamespace]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut fences = mockspace_refnames::Fences::new();
    for line in text.lines() {
        if !fences.prose(line) {
            continue;
        }
        for (_, expr) in placeholder_exprs(line) {
            if let Some(path) = registry_path(&expr, namespaces.iter().map(|n| n.key.as_str())) {
                found.push((expr, path));
            }
        }
    }
    found
}

/// The qualified ids of every row `text`'s references read: each row a path reaches, each row whose
/// field it reads, and each row a step went on from through a reference field. What a row's
/// resolution waits on. A reference that does not resolve reads no row.
pub fn find_registry_refs(text: &str, namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<String> {
    let mut out = Vec::new();
    for (_, path) in registry_paths(text, namespaces) {
        let Ok(found) = reg.select(namespaces, &path) else { continue };
        for item in &found.items {
            match item {
                Selected::Row(r) | Selected::Field(r, _) => out.push(r.qualified()),
            }
        }
        out.extend(found.through.iter().map(|r| r.qualified()));
    }
    out
}

/// The references in `text` that resolve to nothing: a path the walker refuses, written as it was
/// with no registry root, and a plain read of a field the row does not carry, as `namespace::id::field`.
/// Both are silent without a check: the first renders as nothing useful, the second as empty.
pub fn dangling_references(text: &str, reg: &Registry, namespaces: &[RegistryNamespace]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (written, path) in registry_paths(text, namespaces) {
        match reg.select(namespaces, &path) {
            Err(_) => {
                let names = crate::names(&written).unwrap_or_default();
                let shown = if names.is_empty() { written.clone() } else { names_after_root(&names).join("::") };
                out.insert(shown);
            },
            Ok(found) if found.single => {
                if let [Selected::Field(r, f)] = found.items.as_slice() {
                    if f != "id" && f != "citation" && !r.fields.contains_key(f) {
                        out.insert(format!("{}::{f}", r.qualified()));
                    }
                }
            },
            Ok(_) => {},
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
