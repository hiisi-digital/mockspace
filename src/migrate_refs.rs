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
    /// Whether a file of a data ref's instance is among the writes, which no branch commit carries.
    pub instance_written: bool,
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

    // One listing of each directory a citation walks through, for the whole run.
    let _scope = mockspace_registry::cache_scope();
    let templates = match templates(cfg) {
        Ok(t) => t,
        Err(e) => return Err(vec![e]),
    };
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
                    Ok(Some(b)) => match String::from_utf8(b) {
                        Ok(text) => text,
                        Err(e) => {
                            why.push(format!(
                                "migrate: {rel} is not UTF-8 ({e}), so a rewrite would write other bytes than it holds; \
                                 nothing is written"
                            ));
                            continue;
                        },
                    },
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

    // The preset shorthands, in every file the loaders read.
    for path in config_files(cfg) {
        let rel = rel_of(&path);
        match fs::read_to_string(&path) {
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
    }

    // A file to rewrite that is a symlink, or lies below one, would be written through the link to
    // wherever it points, so the run refuses it before it writes anything, as a data ref does.
    why.extend(symlinks_on(&cfg.repo_root, writes.iter().map(|(rel, _)| rel.as_str())));
    if !why.is_empty() {
        return Err(why);
    }
    // A migration already made is refused. What it could not rewrite is named in the refusal.
    if writes.is_empty() && on_refs.is_empty() {
        let mut out = vec![
            "migrate: no reference is in a spelling this migration rewrites, so the tree is already migrated"
                .to_string(),
        ];
        out.extend(lines);
        return Err(out);
    }
    let left = lines.iter().filter(|l| l.starts_with("left ")).count();
    lines.push(format!(
        "{} file(s) rewritten{}, {left} reference(s) left as written and named above",
        writes.len() + on_refs.len(),
        if on_refs.is_empty() { String::new() } else { format!(" ({} of them on a registry ref)", on_refs.len()) }
    ));
    let instances = instance_dirs(cfg)?;
    let instance_written = writes.iter().any(|(rel, _)| {
        let abs = cfg.repo_root.join(rel);
        instances.iter().any(|d| abs.starts_with(d))
    });
    Ok(Planned {
        lines,
        writes,
        on_refs,
        instance_written,
    })
}

/// The refusal of each of `paths`, from the repository root, that is a symlink or lies below one.
pub(crate) fn symlinks_on<'a>(repo_root: &Path, paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let paths: Vec<&str> = paths.collect();
    mockspace_store::symlinks_on(repo_root, &paths)
        .into_iter()
        .map(|link| {
            format!(
                "migrate: {link} is a symlink, so a file at or below it would be written to wherever it points; \
                 nothing is written"
            )
        })
        .collect()
}

/// Every config file the loaders read for the preset shorthands that exists, the one this run loaded
/// among them.
fn config_files(cfg: &Config) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = mockspace_refnames::CONFIG_FILES.iter().map(|f| cfg.repo_root.join(f)).filter(|p| p.is_file()).collect();
    if cfg.config_path.is_file() && !out.contains(&cfg.config_path) {
        out.push(cfg.config_path.clone());
    }
    out
}

/// The directories of the declared data refs' instances, every one of which must exist in this clone.
fn instance_dirs(cfg: &Config) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut out = Vec::new();
    let mut why = Vec::new();
    for d in &cfg.data_refs {
        let ns = d.namespace();
        let held = d
            .open(&cfg.repo_root)
            .and_then(|store| d.instance(&store, &cfg.mock_dir).and_then(|inst| inst.base().map_err(|e| e.to_string())));
        match held {
            Ok(Some(_)) => out.push(d.instance_dir(&cfg.mock_dir)),
            Ok(None) => {
                why.push(format!(
                    "migrate: the data ref `{ns}` has no instance in this clone, so the files it holds cannot be read \
                     and a run would call the tree migrated without them; `cargo mock data-refs instantiate` makes it"
                ));
            },
            Err(e) => why.push(format!("migrate: the data ref `{ns}` could not be read: {e}")),
        }
    }
    if why.is_empty() { Ok(out) } else { Err(why) }
}

/// Every `*.md.tmpl` of the repository: what git lists of it, tracked or not yet added and not
/// ignored, and the files the declared data refs hold, which no branch lists.
fn templates(cfg: &Config) -> Result<Vec<PathBuf>, String> {
    instance_dirs(cfg).map_err(|why| why.join("\n"))?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&cfg.repo_root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.md.tmpl"])
        .output()
        .map_err(|e| format!("migrate: running git to list the templates: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "migrate: git does not list the files of {}, so the templates are not known: {}",
            cfg.repo_root.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut found = BTreeSet::new();
    let mut odd = Vec::new();
    for name in out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        match std::str::from_utf8(name) {
            Ok(p) => {
                found.insert(cfg.repo_root.join(p));
            },
            Err(_) => odd.push(String::from_utf8_lossy(name).into_owned()),
        }
    }
    if !odd.is_empty() {
        return Err(format!(
            "migrate: {} is a template whose name is not UTF-8, so it cannot be rewritten; rename it and run again; \
             nothing is written",
            odd.join(", ")
        ));
    }
    let held = crate::data_refs::ref_files(cfg).map_err(|e| format!("migrate: the files of the data refs: {e}"))?;
    found.extend(held.held.iter().filter(|p| p.ends_with(".md.tmpl")).map(|p| cfg.mock_dir.join(p)));
    Ok(found.into_iter().filter(|p| p.is_file()).collect())
}

/// The text of a config file with each `extends` of a `[lints.<name>]` (a string, or an array of
/// them) written as the standard spells a preset, edited by span so the rest of the file stays as
/// written.
fn in_config(text: &str) -> Result<(String, Vec<Found>), String> {
    let doc = ImDocument::parse(text.to_string()).map_err(|e| e.to_string())?;
    let (mut edits, mut found) = (Vec::new(), Vec::new());
    if let Some(lints) = doc.get("lints").and_then(Item::as_table) {
        for (_, lint) in lints.iter() {
            let Some(value) = lint.get("extends").and_then(Item::as_value) else {
                continue;
            };
            let mut strings = Vec::new();
            match value {
                Value::String(s) => strings.push(s),
                Value::Array(a) => strings.extend(a.iter().filter_map(|v| if let Value::String(s) = v { Some(s) } else { None })),
                _ => {},
            }
            for s in strings {
                let (Some(span), old) = (s.span(), s.value()) else { continue };
                let mut outcome = legacy_refs::preset_shorthand(old);
                let raw = &text[span.clone()];
                if let Outcome::Rewritten(new) = &outcome {
                    let quote = &raw[.. 1];
                    if raw.len() == old.len() + 2 && raw[1 .. raw.len() - 1] == *old && (quote == "\"" || quote == "'") {
                        edits.push((span.clone(), format!("{quote}{new}{quote}")));
                    } else {
                        outcome = Outcome::Left("it is written with escapes the rewrite does not read".to_string());
                    }
                }
                if outcome != Outcome::Same {
                    found.push(Found {
                        span,
                        token: old.clone(),
                        old: old.clone(),
                        outcome,
                    });
                }
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
        let mut here = legacy_refs::in_text(&h.value, ctx);
        if h.reference {
            here.extend(legacy_refs::in_list(&h.value, ctx));
        }
        // Each rewrite is made at the bytes of the file that write its reference, and a reference
        // any byte of which is written as an escape is left.
        let at = written_at(raw, &h.value);
        let mut in_raw = Vec::new();
        for f in &mut here {
            if f.rewritten().is_none() {
                continue;
            }
            match at.as_deref().and_then(|at| raw_span(at, &f.span)) {
                Some(span) => in_raw.push(Found { span, ..f.clone() }),
                None => f.outcome = Outcome::Left("it is written with escapes the rewrite does not read".to_string()),
            }
        }
        let new_raw = legacy_refs::apply(raw, &in_raw);
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

/// For each byte of a string's value, the byte of the string as the file writes it (`raw`, its
/// quotes included) that writes it, or `None` for a byte an escape writes. `None` for the whole
/// where the bytes so read are not the value, which is a string this does not read.
fn written_at(raw: &str, value: &str) -> Option<Vec<Option<usize>>> {
    let bytes = raw.as_bytes();
    let (delim, basic) = match bytes {
        [b'"', b'"', b'"', ..] => (3, true),
        [b'\'', b'\'', b'\'', ..] => (3, false),
        [b'"', ..] => (1, true),
        [b'\'', ..] => (1, false),
        _ => return None,
    };
    let end = raw.len().checked_sub(delim)?;
    let mut i = delim;
    // a multi-line string drops the line break straight after its opening quotes
    if delim == 3 {
        if raw[i ..].starts_with("\r\n") {
            i += 2;
        } else if raw[i ..].starts_with('\n') {
            i += 1;
        }
    }
    let mut at = Vec::with_capacity(value.len());
    while i < end {
        if !basic || bytes[i] != b'\\' {
            at.push(Some(i));
            i += 1;
            continue;
        }
        let next = *bytes.get(i + 1)?;
        if delim == 3 && matches!(next, b' ' | b'\t' | b'\r' | b'\n') {
            // a line-ending backslash: it and the whitespace after it write nothing
            i += 1;
            while i < end && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }
            continue;
        }
        let (len, written) = match next {
            b'u' => (6, char::from_u32(u32::from_str_radix(raw.get(i + 2 .. i + 6)?, 16).ok()?)?.len_utf8()),
            b'U' => (10, char::from_u32(u32::from_str_radix(raw.get(i + 2 .. i + 10)?, 16).ok()?)?.len_utf8()),
            _ => (2, 1),
        };
        at.extend(std::iter::repeat_n(None, written));
        i += len;
    }
    let same = at.len() == value.len()
        && at.iter().zip(value.bytes()).all(|(r, v)| r.is_none_or(|r| bytes[r] == v));
    same.then_some(at)
}

/// Where the bytes `span` of the value stand in the file's string, where each of them is written
/// as itself and in order.
fn raw_span(at: &[Option<usize>], span: &Range<usize>) -> Option<Range<usize>> {
    let first = (*at.get(span.start)?)?;
    let all_plain = (span.start .. span.end).all(|v| at.get(v).copied().flatten() == Some(first + (v - span.start)));
    all_plain.then(|| first .. first + span.len())
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
