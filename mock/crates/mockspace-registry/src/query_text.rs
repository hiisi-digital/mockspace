//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a reference's text is before it is walked: whether it is a path into the registry, the
//! one spelling rule for the namespace it opens on, the operation around it, and a set member it
//! repeats.
//!
//! A namespace is written as declared or folded into the standard's set: `web-renderer::pass` and
//! `web_renderer::pass` name one namespace. The host's spelling is taken in the first step of a
//! path of two names or more and nowhere else, as `crate::names` takes it for every reference.
//! The standard has a host fold its naming and not accept the unfolded spelling (letkajenkka
//! `SPEC.md` 3.2); this allowance predates the registry's paths and keeps citations written with
//! it resolving, and settling the two is not the registry's to do here.

use std::borrow::Cow;

use letkajenkka::{Query, Selector};

use super::*;

/// The namespace a written name names, by the one rule: the name and the key fold alike.
pub fn namespace_named<'a>(written: &str, keys: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let written = mockspace_refnames::fold(written)?.to_string();
    keys.into_iter().find(|k| mockspace_refnames::fold(k).is_some_and(|f| f == written.as_str()))
}

/// `text` in the standard's spelling: as written where the standard reads it, or with the hyphens
/// of its first step after the registry root folded, where they stand there and nowhere else.
///
/// # Errors
///
/// The standard's refusal of the text as written.
pub fn standard_spelling(text: &str) -> Result<Cow<'_, str>, letkajenkka::Error> {
    let refused = match Query::parse(text) {
        Ok(_) => return Ok(Cow::Borrowed(text)),
        Err(e) => e,
    };
    let hosted = mockspace_refnames::fold_separators(text).filter(|folded| {
        let Ok(query) = Query::parse(folded) else { return false };
        let Some(path) = query.path() else { return false };
        let steps: Vec<_> = path.steps().collect();
        let steps = after_registry_root(&steps, |s| s.selector == Selector::Ident(REGISTRY_ROOT));
        let (Some(first), true) = (steps.first(), steps.len() > 1) else { return false };
        let Selector::Ident(name) = first.selector else { return false };
        text.match_indices('-').all(|(at, _)| (first.at .. first.at + name.len()).contains(&at))
    });
    hosted.map(Cow::Owned).ok_or(refused)
}

/// The text of `expr` to walk the registry with, where it is a path into the registry: one opening,
/// after the optional root, on a declared namespace in either spelling, or on a set or a wildcard.
/// `None` for anything else: a citation, a crate, an operation.
pub fn registry_path<'k>(expr: &str, keys: impl IntoIterator<Item = &'k str>) -> Option<String> {
    let text = standard_spelling(expr.trim()).ok()?;
    let path = Query::parse(&text).ok()?.path()?;
    let steps: Vec<_> = path.steps().collect();
    let steps = after_registry_root(&steps, |s| s.selector == Selector::Ident(REGISTRY_ROOT));
    let opens = match steps.first()?.selector {
        Selector::Ident(name) => namespace_named(name, keys).is_some(),
        Selector::Ordinal(_) | Selector::Range(..) => false,
        Selector::Set(_) | Selector::Any | Selector::Deep => true,
    };
    opens.then(|| text.into_owned())
}

/// The declared namespace a reference opens on, after the optional registry root, by the one
/// spelling rule; `None` where it opens on anything else.
pub fn reference_namespace<'k>(text: &str, keys: impl IntoIterator<Item = &'k str>) -> Option<&'k str> {
    let text = standard_spelling(text.trim()).ok()?;
    let path = Query::parse(&text).ok()?.path()?;
    let steps: Vec<_> = path.steps().collect();
    let steps = after_registry_root(&steps, |s| s.selector == Selector::Ident(REGISTRY_ROOT));
    match steps.first()?.selector {
        Selector::Ident(name) => namespace_named(name, keys),
        _ => None,
    }
}

/// The operations that take a reference and ask something of what it names.
pub const OPERATIONS: &[&str] = &["pathof", "refsto", "sourcesof"];

/// `expr` without the operation around it, and the operation's name where there is one: the one
/// place a reference is taken out of `pathof`, `refsto` or `sourcesof`.
pub fn peel(expr: &str) -> (Option<&'static str>, &str) {
    OPERATIONS
        .iter()
        .find_map(|op| crate::operation(expr, op).map(|inner| (Some(*op), inner)))
        .unwrap_or((None, expr))
}

/// Each set member `query` writes twice in one set, as written. The standard keeps both (8.3), so
/// a repeated member is read twice and reported, never dropped.
pub fn repeated_set_members(query: &str) -> Vec<String> {
    let Ok(text) = standard_spelling(query.trim()) else { return Vec::new() };
    let Some(path) = Query::parse(&text).ok().and_then(Query::path) else { return Vec::new() };
    let mut out = Vec::new();
    for step in path.steps() {
        let Selector::Set(inner) = step.selector else { continue };
        let members: Vec<&str> = letkajenkka::Arguments::of(inner).map(str::trim).collect();
        for (i, m) in members.iter().enumerate() {
            if members[.. i].contains(m) && !out.iter().any(|o: &String| o == m) {
                out.push((*m).to_string());
            }
        }
    }
    out
}

/// What a reference's repeated set members are reported as, one line each, its methods and
/// operation taken off first: what a document's template and `cargo mock query` both print.
pub fn repeated_set_warnings(expr: &str) -> Vec<String> {
    let base = crate::chain::split_methods(expr.trim()).map_or_else(|| expr.trim().to_string(), |(b, _)| b);
    repeated_set_members(peel(&base).1)
        .into_iter()
        .map(|m| format!("`{}` names `{m}` twice in one set; both are read, as the set is exactly its expansion (letkajenkka 8.3)", expr.trim()))
        .collect()
}

#[cfg(test)]
#[path = "query_text_tests.rs"]
mod tests;
