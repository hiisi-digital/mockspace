//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo mock migrate references-to-letkajenkka`: the files of a repository that hold a
//! reference, read and rewritten.
//!
//! The templates are every `*.md.tmpl` of the repository, and the data is every file of the registry as
//! the loader reads it (the directory of the mock directory and every directory kept on a ref), and
//! the preset shorthands are the `extends` of the `[lints.<name>]` tables of `mockspace.toml`. Which spelling is old and what it becomes is the
//! registry crate's `legacy_refs`; this reads the files, finds where a reference stands in each
//! and hands back the text it would write.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use mockspace_registry::legacy_refs::{self, Ctx, Found, Outcome};
use toml_edit::{ImDocument, Item, Value};

use crate::config::Config;

/// What a run would do: the lines it prints and the files it writes.
pub(crate) struct Planned {
    pub lines:    Vec<String>,
    /// `(path from the repository root, text)` for each file of the tree.
    pub writes:   Vec<(String, String)>,
    /// `(path under the registry root, text)` for each registry file kept on a ref, which is written by
    /// the registry's own writer and not as a file.
    pub on_refs:  Vec<(String, String)>,
}

/// The lines a run prints, and the files it writes.
pub(crate) fn plan(cfg: &Config) -> Result<Planned, Vec<String>> {
    let namespaces: BTreeSet<&str> = cfg.registry_namespaces.iter().map(|n| n.key.as_str()).collect();
    let ref_fields = mockspace_registry::reference_fields(&cfg.registry_namespaces);
    let ctx = Ctx {
        repo_root:  &cfg.repo_root,
        roots:      &cfg.registry_roots,
        namespaces: &namespaces,
    };
    let (mut lines, mut writes, mut on_refs, mut why) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let report = |rel: &str, found: &[Found], lines: &mut Vec<String>| {
        for f in found {
            lines.push(match &f.outcome {
                Outcome::Rewritten(new) => format!("rewrite {rel}: {} -> {new}", f.old.trim()),
                Outcome::Left(why) => format!("left {rel}: {} ({why})", f.old.trim()),
                Outcome::Same => continue,
            });
        }
    };
    let rel_of = |p: &Path| p.strip_prefix(&cfg.repo_root).unwrap_or(p).to_string_lossy().replace('\\', "/");

    let mut templates = Vec::new();
    walk(&cfg.repo_root, &mut |p| p.to_string_lossy().ends_with(".md.tmpl"), &mut templates);
    for path in &templates {
        let rel = rel_of(path);
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                why.push(format!("migrate: reading {rel}: {e}"));
                continue;
            },
        };
        let found = legacy_refs::in_text(&text, &ctx);
        let new = legacy_refs::apply(&text, &found);
        report(&rel, &found, &mut lines);
        if new != text {
            writes.push((rel, new));
        }
    }

    // The registry as the loader reads it: the directory of the tree and every directory kept on a ref.
    let stores = mockspace_registry::RegistryStores::from_decls(&cfg.mock_dir, &cfg.repo_root, &cfg.registry_stores)
        .and_then(|s| s.refresh().map(|()| s));
    match stores.and_then(|s| s.files().map(|f| (s, f))) {
        Err(e) => why.push(format!("migrate: the registry could not be read: {e}")),
        Ok((stores, files)) => {
            let mounts: Vec<String> =
                cfg.registry_stores.iter().filter_map(|d| d.dir.as_deref()).map(|d| d.trim_matches('/').to_string()).collect();
            for file in files {
                let on_ref = mounts.iter().any(|m| file == *m || file.starts_with(&format!("{m}/")));
                let rel = if on_ref {
                    format!("{} (on its ref)", rel_of(&stores.source_of(&file)))
                } else {
                    rel_of(&stores.source_of(&file))
                };
                let text = match stores.read(&file) {
                    Ok(Some(b)) => String::from_utf8_lossy(&b).into_owned(),
                    Ok(None) => continue,
                    Err(e) => {
                        why.push(format!("migrate: reading {rel}: {e}"));
                        continue;
                    },
                };
                match in_toml(&text, &ref_fields, &ctx) {
                    Err(e) => {
                        why.push(format!("migrate: {rel} is not read as TOML, so nothing in it is touched: {e}"));
                    },
                    Ok((new, found)) => {
                        report(&rel, &found, &mut lines);
                        if new != text {
                            if on_ref { on_refs.push((file, new)) } else { writes.push((rel_of(&stores.source_of(&file)), new)) }
                        }
                    },
                }
            }
        },
    }

    // The preset shorthands of `mockspace.toml`.
    let rel = rel_of(&cfg.config_path);
    match fs::read_to_string(&cfg.config_path) {
        Err(e) => why.push(format!("migrate: reading {rel}: {e}")),
        Ok(text) => match in_config(&text) {
            Err(e) => why.push(format!("migrate: {rel} is not read as TOML, so nothing in it is touched: {e}")),
            Ok((new, found)) => {
                report(&rel, &found, &mut lines);
                if new != text {
                    writes.push((rel, new));
                }
            },
        },
    }

    if !why.is_empty() {
        return Err(why);
    }
    if lines.is_empty() {
        lines.push("no reference is in the spelling before the standard, so there is nothing to rewrite".to_string());
    } else {
        let left = lines.iter().filter(|l| l.starts_with("left ")).count();
        lines.push(format!(
            "{} file(s) rewritten{}, {left} reference(s) left as written and named above",
            writes.len() + on_refs.len(),
            if on_refs.is_empty() { String::new() } else { format!(" ({} of them on a registry ref)", on_refs.len()) }
        ));
    }
    Ok(Planned {
        lines,
        writes,
        on_refs,
    })
}

/// The text of `mockspace.toml` with each `extends` of a `[lints.<name>]` written as the standard
/// spells a preset, edited by span so the rest of the file stays as written.
fn in_config(text: &str) -> Result<(String, Vec<Found>), String> {
    let doc = ImDocument::parse(text.to_string()).map_err(|e| e.to_string())?;
    let (mut edits, mut found) = (Vec::new(), Vec::new());
    if let Some(lints) = doc.get("lints").and_then(Item::as_table) {
        for (_, lint) in lints.iter() {
            let Some(Value::String(s)) = lint.get("extends").and_then(Item::as_value) else {
                continue;
            };
            let (Some(span), old) = (s.span(), s.value()) else { continue };
            let mut outcome = legacy_refs::preset_shorthand(old);
            let raw = &text[span.clone()];
            if let Outcome::Rewritten(new) = &outcome {
                let quote = &raw[.. 1];
                if raw.len() == old.len() + 2 && raw[1 .. raw.len() - 1] == *old && (quote == "\"" || quote == "'") {
                    edits.push((span, format!("{quote}{new}{quote}")));
                } else {
                    outcome = Outcome::Left("it is written with escapes the rewrite does not read".to_string());
                }
            }
            if outcome != Outcome::Same {
                found.push(Found {
                    token: old.clone(),
                    old: old.clone(),
                    outcome,
                });
            }
        }
    }
    edits.sort_by_key(|(s, _): &(Range<usize>, String)| s.start);
    let mut out = text.to_string();
    for (span, new) in edits.into_iter().rev() {
        out.replace_range(span, &new);
    }
    ImDocument::parse(out.clone()).map_err(|e| format!("the rewrite would not read as TOML: {e}"))?;
    Ok((out, found))
}

/// Every file under `dir` the filter takes, in path order, leaving out what is not the repository's
/// own: `.git`, a build directory, the worktrees, and any hidden directory.
fn walk(dir: &Path, take: &mut dyn FnMut(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.is_dir() {
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                walk(&path, take, out);
            }
        } else if meta.is_file() && take(&path) {
            out.push(path);
        }
    }
}

/// A string of the data and where it stands in the file.
struct Held {
    span:      Range<usize>,
    value:     String,
    /// Whether it is an item of a field declared to hold references.
    reference: bool,
}

/// The text of a registry file with each reference rewritten in the string that holds it, edited
/// by byte span so the rest of the file stays as written.
fn in_toml(
    text: &str,
    ref_fields: &BTreeMap<String, Vec<String>>,
    ctx: &Ctx<'_>,
) -> Result<(String, Vec<Found>), String> {
    let doc = ImDocument::parse(text.to_string()).map_err(|e| e.to_string())?;
    let mut held = Vec::new();
    for (ns, item) in doc.iter() {
        let fields = ref_fields.get(ns).map(Vec::as_slice).unwrap_or_default();
        collect(item, fields, false, &mut held);
    }
    let (mut edits, mut found) = (Vec::new(), Vec::new());
    for h in held {
        let raw = &text[h.span.clone()];
        let mut changes: Vec<(String, String)> = Vec::new();
        let mut here = legacy_refs::in_text(&h.value, ctx);
        if h.reference {
            for part in h.value.split(", ") {
                let outcome = legacy_refs::item(part, ctx);
                if outcome != Outcome::Same && !part.contains("{{") {
                    here.push(Found {
                        token: part.to_string(),
                        old: part.to_string(),
                        outcome,
                    });
                }
            }
        }
        let mut new_raw = raw.to_string();
        for f in &mut here {
            if let Outcome::Rewritten(new) = &f.outcome {
                let new_token = f.token.replacen(f.old.trim(), new, 1);
                if raw.contains(&f.token) {
                    changes.push((f.token.clone(), new_token));
                } else {
                    f.outcome = Outcome::Left("it is written with escapes the rewrite does not read".to_string());
                }
            }
        }
        for (old, new) in &changes {
            new_raw = new_raw.replace(old, new);
        }
        if new_raw != raw {
            edits.push((h.span, new_raw));
        }
        found.extend(here);
    }
    edits.sort_by_key(|(s, _)| s.start);
    let mut out = text.to_string();
    for (span, new) in edits.into_iter().rev() {
        out.replace_range(span, &new);
    }
    ImDocument::parse(out.clone()).map_err(|e| format!("the rewrite would not read as TOML: {e}"))?;
    Ok((out, found))
}

fn collect(item: &Item, fields: &[String], inside: bool, out: &mut Vec<Held>) {
    match item {
        Item::Value(v) => collect_value(v, inside, out),
        Item::Table(t) => {
            for (k, v) in t.iter() {
                collect(v, fields, inside || fields.iter().any(|f| f == k), out);
            }
        },
        Item::ArrayOfTables(a) => {
            for t in a {
                for (k, v) in t.iter() {
                    collect(v, fields, inside || fields.iter().any(|f| f == k), out);
                }
            }
        },
        Item::None => {},
    }
}

fn collect_value(v: &Value, reference: bool, out: &mut Vec<Held>) {
    match v {
        Value::String(s) => {
            if let Some(span) = s.span() {
                out.push(Held {
                    span,
                    value: s.value().clone(),
                    reference,
                });
            }
        },
        Value::Array(a) => a.iter().for_each(|x| collect_value(x, reference, out)),
        Value::InlineTable(t) => t.iter().for_each(|(_, x)| collect_value(x, reference, out)),
        _ => {},
    }
}

#[cfg(test)]
#[path = "migrate_refs_tests.rs"]
mod tests;
