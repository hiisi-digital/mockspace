//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::resolved::{Resolved, row_fields, table_cells};
use super::*;
use super::chain::{apply_methods, split_methods};
use super::resolve_ops::{resolve_pathof, resolve_refsto, resolve_sourcesof};

/// Resolve every `{{ ... }}` reference in a rendered document.
///
/// One pass over one syntax, replacing what were three: row references, field
/// references, whole-table embeds, and file citations are all placeholder
/// expressions distinguished by their shape rather than by their delimiters.
///
/// An expression that resolves to nothing is left exactly as written, braces
/// and all, so the validator reports it and the reader sees an obvious
/// unresolved reference rather than a plausible-looking wrong one.
pub fn resolve_all(
    text: &str,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> String {
    let _scope = crate::refpath::cache_scope();
    // One pass. Data is resolved bottom-up before any document renders, so a
    // reference inside a row's field is already inlined by the time a table
    // carrying it expands, and re-running over the output would only find
    // references a document wrote about itself.
    resolve_once(text, namespaces, reg, roots, repo_root, docs_dir, cfg)
}

pub(crate) fn resolve_once(
    text: &str,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> String {
    let by_key: BTreeMap<&str, &RegistryNamespace> =
        namespaces.iter().map(|n| (n.key.as_str(), n)).collect();

    let mut out = String::with_capacity(text.len());
    let mut fences = mockspace_refnames::Fences::new();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !fences.prose(line) {
            out.push_str(line);
            continue;
        }
        let mut rewritten = line.to_string();
        let mut dropped_any = false;
        for (token, expr) in placeholder_exprs(line) {
            if let Some(rep) = resolve_expr(&expr, &by_key, reg, roots, repo_root, docs_dir, cfg) {
                if rep.is_empty() {
                    dropped_any = true;
                }
                rewritten = rewritten.replace(&token, &rep);
            }
        }
        if dropped_any {
            rewritten = tidy_after_drop(&rewritten);
        }
        out.push_str(&rewritten);
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Clean up what dropping a reference left behind.
///
/// A citation is usually the whole of a parenthetical, or the tail of one. When
/// it renders as nothing, the punctuation that framed it is still there, and a
/// document reading "the split becomes structural ()." is worse than one that
/// never carried the citation. Only lines that actually dropped something are
/// touched, so ordinary prose containing empty parentheses is left alone.
fn tidy_after_drop(line: &str) -> String {
    let mut s = line.to_string();
    // Collapse first, so a parenthetical left holding only the whitespace
    // between several dropped citations reads as empty.
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s = regex_lite_replace(&s, "( )", "()");
    // A parenthetical emptied entirely, including one holding only separators
    // left by several dropped citations.
    s = regex_lite_replace(&s, " ()", "");
    s = regex_lite_replace(&s, "()", "");
    // Separators orphaned by a drop inside a parenthetical that kept something.
    s = regex_lite_replace(&s, "(, ", "(");
    s = regex_lite_replace(&s, ", )", ")");
    s = regex_lite_replace(&s, " ,", ",");
    // A drop that was not inside parentheses leaves the space before the
    // sentence's own punctuation.
    for mark in [".", ";", ":"] {
        s = regex_lite_replace(&s, &format!(" {mark}"), mark);
    }
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim_end().to_string()
}

/// Replace every occurrence, without a regex dependency.
fn regex_lite_replace(s: &str, from: &str, to: &str) -> String {
    let mut out = s.to_string();
    while out.contains(from) {
        out = out.replace(from, to);
    }
    out
}

/// Resolve `crates::<name>` to a link to that crate's generated document, which the project
/// answers: the registry holds no crates of its own.
fn resolve_crate_ref(name: &str, cfg: &dyn RegistryEnv, docs_dir: &Path) -> Option<String> {
    cfg.crate_link(name, docs_dir)
}

/// A path as the repository sees it, which is how a document should name one.
pub(crate) fn rel_to_repo(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn resolve_expr(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<String> {
    resolve_typed(expr, by_key, reg, roots, repo_root, docs_dir, cfg).map(|r| r.to_markdown())
}

/// A namespace's whole table, as a value.
fn table_of(ns: &RegistryNamespace, reg: &Registry) -> Resolved {
    let (columns, rows) = table_cells(ns, reg);
    Resolved::Table {
        namespace: ns.key.clone(),
        columns,
        rows,
    }
}

/// What an expression resolves to, before anything decides how to show it.
///
/// The markdown wrapper below is what documents use. `cargo mock query` uses
/// this directly, so a table can be aligned for a terminal instead of piped for
/// a parser, and both are answering the identical lookup.
pub fn resolve_typed(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &dyn RegistryEnv,
) -> Option<Resolved> {
    // A postfix chain narrows whatever the expression resolved to:
    // `pathof(crates::store).dir()`. Split off first, so the base expression is
    // resolved by the ordinary path and the methods only ever see a string.
    if let Some((base, methods)) = split_methods(expr) {
        let value = resolve_typed(&base, by_key, reg, roots, repo_root, docs_dir, cfg)?;
        return apply_methods(value, &methods);
    }

    // Two questions that are not the same one.
    //
    // `pathof(x)` is where x is DECLARED: the file to open to change it. For a
    // registry row that is the TOML it sits in, which is what a rule telling an
    // agent where to edit actually needs.
    //
    // `sourcesof(x)` is what x RESTS ON: its provenance. Plural, because
    // provenance is an array and a claim usually has several sources.
    if let Some(inner) = crate::operation(expr, "pathof") {
        return resolve_pathof(inner, by_key, reg, repo_root, cfg);
    }
    if let Some(inner) = crate::operation(expr, "refsto") {
        return resolve_refsto(inner, by_key, reg, roots, repo_root, docs_dir, cfg)
            .map(Resolved::List);
    }

    if let Some(inner) = crate::operation(expr, "sourcesof") {
        return resolve_sourcesof(inner, by_key, reg, roots, repo_root, docs_dir, cfg).map(Resolved::List);
    }

    // A query that widens, or a plain path naming a branch of a namespace's id tree, selects rows:
    // `ticket::question::**`, `ticket::{task, question}::engine`. The one reader of every path into
    // the registry that is not one row or one field of it.
    if let Some(found) = crate::resolve_ops::resolve_selection(expr, by_key, reg, cfg) {
        return found;
    }

    let parts = crate::names(expr).unwrap_or_default();

    if parts.first() == Some(&CRATE_ROOT) && parts.len() == 2 {
        // A crate reference. Checked like any other: a crate that does not
        // exist is reported rather than linked into nothing, which is what
        // turns a crate mention in prose from a string into something the
        // build verifies.
        return resolve_crate_ref(parts[1], cfg, docs_dir).map(|link| {
            Resolved::Citation {
                // The crate as the project spells it, which the reference does not: it names the
                // crate in the standard's character set, so `my_crate` shows as `my-crate`.
                text: link
                    .strip_prefix('[')
                    .and_then(|l| l.split_once("]("))
                    .map_or_else(|| parts[1].to_string(), |(shown, _)| shown.to_string()),
                link: link
                    .rsplit_once('(')
                    .and_then(|(_, l)| l.strip_suffix(')'))
                    .map(str::to_string),
            }
        });
    }

    // A namespace addressed directly: `law::keys` rather than `reg::law::keys`.
    //
    // The `reg::` prefix carried no information. Slot zero is either a declared
    // root or a declared namespace, and the two cannot collide (a collision is
    // reported as a configuration error), so the prefix only made every
    // reference four characters longer and read as ceremony. It still resolves,
    // because thousands of references were written with it.
    //
    // Rewritten into the prefixed form rather than handled separately, so there
    // is one resolution path and the two spellings cannot diverge.
    // A namespace alone renders its whole table: `{{ task }}`. Placeholders
    // have already been substituted by the time this runs, so a single segment
    // that names a namespace can only mean the namespace.
    if parts.len() == 1 {
        return by_key.get(parts[0]).map(|ns| table_of(ns, reg));
    }

    if parts.first().is_some_and(|p| by_key.contains_key(p)) && parts.len() >= 2 {
        let rewritten = format!("{}::{}", REGISTRY_ROOT, parts.join("::"));
        return resolve_typed(&rewritten, by_key, reg, roots, repo_root, docs_dir, cfg);
    }

    if parts.first() == Some(&REGISTRY_ROOT) {
        // A whole namespace: its table, inline.
        if parts.len() == 2 {
            return by_key.get(parts[1]).map(|ns| table_of(ns, reg));
        }
        // A row, or one field on it. Where the id ends is the registry's to say, since an id
        // may be a path: `locate` finds the row, and what is left is the field step.
        let found = reg.locate(&parts)?;
        let row = found.row;
        let field = match found.fields {
            [] => None,
            [one] => Some(*one),
            _ => return None,
        };
        if row.namespace == "reference" {
            if !by_key.contains_key("reference") {
                return None;
            }

            // `::citation` is a computed field, not one the row declares:
            // the full form. Everything else is a real field.
            if field.is_some_and(|f| f != "citation") {
                return row.fields.get(field?).cloned().map(Resolved::Field);
            }

            // A short form by default, the full one on request. They differ
            // in width: a sources column carrying four hundred full citations
            // is unreadable, while the same citation in prose wants its
            // title. Default to the form that fits everywhere. There is no
            // page for either to link to, so the citation is its text alone.
            let text =
                if field.is_some() { format_citation(row) } else { short_citation(row) };
            return Some(Resolved::Citation {
                text,
                link: None,
            });
        }
        let ns = by_key.get(row.namespace.as_str())?;
        if let Some(field) = field {
            if field == "id" {
                return Some(Resolved::Field(row.id.clone()));
            }
            // An internal field does not resolve. Returning None leaves
            // the reference visibly unresolved and reports it, which is
            // the same treatment a field that does not exist gets. A
            // field declared as never reaching the documentation, with
            // a documented way to put it there anyway, would not be
            // one.
            if ns
                .fields
                .iter()
                .any(|f| f.name == field && f.visibility == FieldVisibility::Internal)
            {
                return None;
            }
            // A field typed as a namespace holds slugs, and each is resolved
            // through the same path as the field's table cell, so a field
            // reference and the cell agree, and a slug that names no row is
            // reported rather than passed through.
            if let Some(target) = ns
                .fields
                .iter()
                .find(|f| f.name == field)
                .and_then(|f| super::row_reference_target(&f.r#type))
                .map(|(t, _)| t)
                .filter(|t| cfg.namespaces().iter().any(|n| n.key == *t))
            {
                let raw = row.fields.get(field)?;
                return Some(Resolved::List(
                    crate::refpath::list(raw)
                        .map(|slug| {
                            let expr = format!("{target}::{slug}");
                            // A slug that resolves to nothing keeps its
                            // reference form rather than vanishing. The
                            // table cell for the same datum emits the
                            // reference and is caught by the
                            // unresolved-token scan; dropping it here
                            // made one path loud and the other silent
                            // about the identical bad value, and a
                            // field of two slugs rendered as one.
                            resolve_expr(
                                &expr, by_key, reg, roots, repo_root, docs_dir, cfg,
                            )
                            .unwrap_or_else(|| format!("{{{{ {expr} }}}}"))
                        })
                        .collect(),
                ));
            }
            return row.fields.get(field).cloned().map(Resolved::Field);
        }
        // A namespace declaring a value field renders that instead of a
        // slug, so a constant states its number wherever it is named.
        if let Some(v) = ns.value_field.as_ref().and_then(|f| row.fields.get(f)) {
            return Some(Resolved::Field(v.clone()));
        }
        return Some(Resolved::Row {
            namespace: row.namespace.clone(),
            id:        row.id.clone(),
            fields:    row_fields(ns, row),
        });
    }

    // Otherwise a file citation.
    let r = FileRef::parse(expr)?;

    // A citation into an internal root renders as nothing. The reference stays
    // in the source, where it is checked and greppable and is the provenance
    // record; it does not reach a reader who cannot open what it names.
    //
    // Empty rather than absent, so the parenthetical it may sit in is left for
    // `resolve_all` to tidy: a citation that was the only thing in its
    // parentheses would otherwise leave `()` behind.
    if cfg.internal_roots().contains(&r.root) {
        return Some(Resolved::Hidden);
    }

    // A root that does not render as a link becomes prose. The citation still
    // appears, because a generated document is also read internally and the
    // provenance is the point; what does not appear is the path, which is the
    // only part that leaks.
    if let Some(label) = cfg.prose_roots().get(&r.root) {
        let path = r.path_text();
        return Some(Resolved::Citation {
            text: match r.anchor() {
                Anchor::Heading(h) => format!("{label}, {path} ({h})"),
                Anchor::Lines(lo, hi) if lo == hi => format!("{label}, {path} line {lo}"),
                Anchor::Lines(lo, hi) => format!("{label}, {path} lines {lo} to {hi}"),
            },
            link: None,
        });
    }

    let rel = roots.get(&r.root)?;
    let Location::Found(target) = r.locate(&repo_root.join(rel)) else { return None };
    let link = relative_from(docs_dir, &target);
    let shown = format!("{}/{}", r.root, r.path_text());
    Some(match r.anchor() {
        // The link carries the slug a forge gives the heading it found, so the anchor a reader
        // follows is the one the forge has, and a heading the file does not hold resolves to
        // nothing rather than to a link that lands at the top.
        Anchor::Heading(h) => {
            let (_, forge) = heading_in(&target, &h)?;
            Resolved::Citation {
                text: format!("{shown}#{forge}"),
                link: Some(format!("{link}#{forge}")),
            }
        },
        Anchor::Lines(lo, hi) if lo == hi => {
            Resolved::Citation {
                text: format!("{shown}:{lo}"),
                link: Some(format!("{link}#L{lo}")),
            }
        },
        Anchor::Lines(lo, hi) => {
            Resolved::Citation {
                text: format!("{shown}:{lo}-{hi}"),
                link: Some(format!("{link}#L{lo}-L{hi}")),
            }
        },
    })
}

