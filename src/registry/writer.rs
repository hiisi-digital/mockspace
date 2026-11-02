//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one way registry files are written to a store.
//!
//! A write to a ref skips the commit gate, because nothing is committed in a work tree, so the
//! checks the gate runs over the registry data run here instead, before the write (and, for a
//! directory kept on a ref, the schema check of `taplo` over a copy laid out as the tree would
//! be): the files
//! must read as TOML with an `id` on every row, and the registry they make must raise no finding
//! of [`validate`], [`validate_row_references`] or [`validate_provenance`] that the registry
//! before the write raised fewer times. A write that fails them is refused and no store
//! is touched. A finding the registry already had, as often as it had it, is not the write's,
//! and does not stop it. The dangling-reference scan
//! (`refs_unresolved`) reads the documents a generation run wrote, which the writer has no way to
//! render, so a row a document names can be removed by a write. A generation run (`cargo mock`)
//! reports it; the pre-commit and pre-push hooks run lint-only and never reach the scan, and a
//! write to a ref makes no commit. FIXME(9bf0671ca0e0): nothing checks it before a generation run.
//! (`a_row_a_document_names_can_be_removed_by_a_write_the_writer_cannot_see_the_documents`)
//!
//! When another writer moves a ref first, the merge driver of registry files
//! (`registry_merge`) is the race handler: it merges the file as it was when this write began,
//! as this write left it and as the winner left it, by row. The checks then run again on the
//! merged result, so a rebase cannot land what the first attempt would have been refused.

use std::collections::{BTreeMap, BTreeSet};

use mockspace_store::{Applied, Edit, MergeHook, MergeRefusal, Reader};

use super::stores::is_registry_file;
use super::*;
use crate::config::Config;

/// One file of the registry and its text, `None` where it is not text.
type Files = Vec<(String, Option<String>)>;

/// A writer of one project's registry, with the checks of that project's config.
/// The one finding a failed schema check raises.
const SCHEMA_FINDING: &str = "[schema] the registry does not match its schema";

pub struct RegistryWriter<'a> {
    cfg:           &'a Config,
    stores:        &'a RegistryStores,
    /// The lines the last schema check failed with, for the refusal that names it.
    schema_detail: std::cell::RefCell<Vec<String>>,
}

impl<'a> RegistryWriter<'a> {
    pub fn new(cfg: &'a Config, stores: &'a RegistryStores) -> Self {
        Self {
            cfg,
            stores,
            schema_detail: Default::default(),
        }
    }

    /// Put (`Some`) or remove (`None`) the files at these paths under the registry root, as one
    /// write with `message`. Every path must be held by the same store.
    pub fn write(
        &self,
        changes: &[(String, Option<Vec<u8>>)],
        message: &str,
    ) -> Result<Applied, RegistryStoreError> {
        let (store, held, edit) = self.stores.edit_in(changes)?;
        let on_ref = held != ".";
        let changed: BTreeSet<String> = changes.iter().map(|(p, _)| p.clone()).collect();
        // A directory in the tree is checked by the commit gate over the whole registry, so a write
        // to it reads and checks only the files it touches, as `registry-append` always cost.
        let before = if on_ref {
            self.current(None)?
        } else {
            let mut files = Files::new();
            for rel in &changed {
                let text = self
                    .stores
                    .read(rel)?
                    .and_then(|b| String::from_utf8(b).ok());
                files.push((rel.clone(), text));
            }
            files
        };
        let base: BTreeMap<String, Option<String>> = before
            .iter()
            .filter(|(r, _)| changed.contains(r))
            .cloned()
            .collect();
        let after = overlay(before.clone(), changes);
        let old = self.problems(&before, &BTreeSet::new(), on_ref);
        let now = self.problems(&after, &changed, on_ref);
        // a check that could not run is not a check that found nothing, new finding or not
        let unrun: Vec<String> = now
            .iter()
            .filter(|p| p.starts_with("[schema-unavailable]"))
            .cloned()
            .collect();
        if !unrun.is_empty() {
            return Err(RegistryStoreError::Refused(unrun));
        }
        self.refuse_new(&old, now)?;
        let hook = Race {
            writer: self,
            held: held.clone(),
            base,
            changed,
        };
        store.apply(&edit, message, &hook).map_err(|source| {
            RegistryStoreError::Store {
                held,
                source,
            }
        })
    }

    /// Every file of the registry. Where `tip` is given, the files it holds replace those the
    /// directory `held` stood for: the state a race was lost to.
    fn current(&self, tip: Option<(&str, &dyn Reader)>) -> Result<Files, RegistryStoreError> {
        let under = |held: &str, rel: &str| rel.starts_with(&format!("{held}/"));
        let mut out = Files::new();
        for rel in self.stores.files()? {
            if tip.is_some_and(|(held, _)| held != "." && under(held, &rel)) {
                continue;
            }
            let text = self
                .stores
                .read(&rel)?
                .and_then(|b| String::from_utf8(b).ok());
            out.push((rel, text));
        }
        if let Some((held, reader)) = tip.filter(|(h, _)| *h != ".") {
            let fail = |source| {
                RegistryStoreError::Store {
                    held: held.to_string(),
                    source,
                }
            };
            for inner in reader.list("").map_err(fail)? {
                let rel = format!("{held}/{inner}");
                if is_registry_file(&rel) {
                    let text = reader
                        .read(&inner)
                        .map_err(fail)?
                        .and_then(|b| String::from_utf8(b).ok());
                    out.push((rel, text));
                }
            }
            out.sort_by(|a, b| a.0.split('/').cmp(b.0.split('/')));
        }
        Ok(out)
    }

    /// What `taplo` finds in `files` laid out as the registry is in the tree. The gate checks the
    /// working tree, which does not hold the rows kept on a ref, so a write to a ref runs the same
    /// check over a copy of the registry as it would stand: the generated config and schemas of the
    /// tree beside the files.
    fn schema_problems(&self, files: &Files) -> Vec<String> {
        let cfg = self.cfg;
        let scratch = Scratch::new();
        let mock_rel = cfg
            .mock_dir
            .strip_prefix(&cfg.repo_root)
            .unwrap_or(std::path::Path::new("mock"));
        let laid = (|| -> std::io::Result<()> {
            std::fs::copy(
                cfg.repo_root.join(".taplo.toml"),
                scratch.0.join(".taplo.toml"),
            )?;
            let schemas = |root: &std::path::Path| {
                crate::build_dir::target_dir(root)
                    .join("mockspace")
                    .join("registry-schemas")
            };
            let to = schemas(&scratch.0);
            std::fs::create_dir_all(&to)?;
            for e in std::fs::read_dir(schemas(&cfg.repo_root))? {
                let e = e?;
                std::fs::copy(e.path(), to.join(e.file_name()))?;
            }
            for (rel, text) in files {
                let Some(text) = text else { continue };
                let at = scratch.0.join(mock_rel).join("registry").join(rel);
                std::fs::create_dir_all(at.parent().unwrap_or(&scratch.0))?;
                std::fs::write(at, text)?;
            }
            Ok(())
        })();
        if let Err(e) = laid {
            return vec![format!(
                "[schema-unavailable] the registry could not be laid out for the schema check: {e}"
            )];
        }
        match check_schemas(&scratch.0, &cfg.registry_namespaces, None) {
            SchemaCheck::Ran { failures } => {
                // taplo's log is not the same twice: a line it writes in one run is missing from the
                // next, and it names the copy it read. Only whether it failed is stable, so that is the
                // finding, and the lines are kept aside to be shown with a refusal.
                let (scratch_root, tree_root) = (scratch.0.to_string_lossy(), cfg.repo_root.to_string_lossy());
                let mut seen = BTreeSet::new();
                let lines: Vec<String> = failures
                    .into_iter()
                    .filter(|f| !f.contains("operation failed"))
                    .map(|f| f.replace(scratch_root.as_ref(), tree_root.as_ref()))
                    .filter(|f| seen.insert(f.clone()))
                    .collect();
                if lines.is_empty() {
                    return Vec::new();
                }
                *self.schema_detail.borrow_mut() = lines;
                vec![SCHEMA_FINDING.to_string()]
            },
            SchemaCheck::Unavailable if files.is_empty() => Vec::new(),
            SchemaCheck::Unavailable => vec!["[schema-unavailable] taplo is not installed, so the shape of rows on a ref is unverified".to_string()],
        }
    }

    /// [`refuse_new`], with the lines of a failed schema check added to what it names.
    fn refuse_new(&self, old: &[String], new: Vec<String>) -> Result<(), RegistryStoreError> {
        refuse_new(old, new).map_err(|e| {
            match e {
                RegistryStoreError::Refused(mut found)
                    if found.iter().any(|p| p == SCHEMA_FINDING) =>
                {
                    found.extend(
                        self.schema_detail
                            .borrow()
                            .iter()
                            .map(|l| format!("  taplo: {l}")),
                    );
                    RegistryStoreError::Refused(found)
                },
                other => other,
            }
        })
    }

    /// What the checks find in `files`. `changed` names the files this write touches, which
    /// must also read as TOML and give every row an `id`: the loader skips such a file or row
    /// and says so, and a skipped row is a row the write silently lost.
    fn problems(&self, files: &Files, changed: &BTreeSet<String>, on_ref: bool) -> Vec<String> {
        let cfg = self.cfg;
        let mut out = Vec::new();
        let known: BTreeSet<&str> = cfg
            .registry_namespaces
            .iter()
            .map(|n| n.key.as_str())
            .collect();
        for (rel, text) in files.iter().filter(|(r, _)| changed.contains(r)) {
            let Some(text) = text else { continue };
            match text.parse::<toml_edit::DocumentMut>() {
                Err(e) => out.push(format!("[not-toml] {rel} is not valid TOML: {e}")),
                Ok(doc) => {
                    for (key, item) in doc.iter().filter(|(k, _)| known.contains(k)) {
                        let rows = item.as_array_of_tables().into_iter().flat_map(|t| t.iter());
                        if rows
                            .into_iter()
                            .any(|t| t.get("id").and_then(|v| v.as_str()).is_none())
                        {
                            out.push(format!(
                                "[row-without-id] a [[{key}]] row in {rel} has no id"
                            ));
                        }
                    }
                },
            }
        }
        if !on_ref {
            return out;
        }
        let reg = rows_of_texts(
            self.stores.root(),
            files.clone(),
            &cfg.registry_namespaces,
            None,
        );
        let ns = &cfg.registry_namespaces;
        let findings = validate(ns, &reg)
            .into_iter()
            .chain(validate_row_references(&reg, ns))
            .chain(validate_provenance(
                &cfg.repo_root,
                &cfg.registry_roots,
                &cfg.frozen_roots,
                &reg,
                ns,
            ));
        out.extend(findings.map(|f| format!("[{}] {}", f.kind, f.message)));
        if on_ref {
            // the gate counts the cycles `resolve_data` names among the registry findings
            let (_, cycles) = resolve_data(
                ns,
                &reg,
                &cfg.registry_roots,
                &cfg.repo_root,
                &cfg.docs_dir,
                cfg,
            );
            out.extend(
                cycles
                    .into_iter()
                    .map(|f| format!("[{}] {}", f.kind, f.message)),
            );
            out.extend(self.schema_problems(files));
        }
        out
    }
}

/// A directory under the system temp directory, removed when it goes out of scope.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mockspace-schema-{}-{n}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn overlay(mut files: Files, changes: &[(String, Option<Vec<u8>>)]) -> Files {
    for (rel, bytes) in changes {
        files.retain(|(r, _)| r != rel);
        if let Some(b) = bytes {
            files.push((rel.clone(), String::from_utf8(b.clone()).ok()));
        }
    }
    files.sort_by(|a, b| a.0.split('/').cmp(b.0.split('/')));
    files
}

fn refuse_new(old: &[String], new: Vec<String>) -> Result<(), RegistryStoreError> {
    // counted, not looked up: a write that adds a second row failing the way a standing one does
    // raises one finding more than before, and that one is the write's
    let mut had: BTreeMap<&str, usize> = BTreeMap::new();
    for p in old {
        *had.entry(p.as_str()).or_default() += 1;
    }
    let fresh: Vec<String> = new
        .into_iter()
        .filter(|p| match had.get_mut(p.as_str()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            },
            _ => true,
        })
        .collect();
    if fresh.is_empty() { Ok(()) } else { Err(RegistryStoreError::Refused(fresh)) }
}

/// The race handler: `registry_merge` over each file the write touched, then the checks again.
struct Race<'a, 'b> {
    writer:  &'b RegistryWriter<'a>,
    /// The directory the store holds, `.` for the tree.
    held:    String,
    /// The touched files as the write found them.
    base:    BTreeMap<String, Option<String>>,
    changed: BTreeSet<String>,
}

impl Race<'_, '_> {
    fn rel(&self, inner: &str) -> String {
        if self.held == "." {
            inner.to_string()
        } else {
            format!("{}/{inner}", self.held)
        }
    }

    fn text_of(bytes: Option<Vec<u8>>) -> Option<String> {
        bytes.and_then(|b| String::from_utf8(b).ok())
    }
}

impl MergeHook for Race<'_, '_> {
    fn rebase(&self, tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal> {
        let refuse = |why: String| MergeRefusal(why);
        let mut next = Edit::new();
        let mut changes: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        for (inner, bytes) in ours.entries() {
            let rel = self.rel(inner);
            let base = self.base.get(&rel).cloned().flatten();
            let theirs = Self::text_of(tip.read(inner).map_err(|e| refuse(e.to_string()))?);
            let merged: Option<Vec<u8>> = match (bytes, &theirs) {
                (Some(b), None) if base.is_none() => Some(b.to_vec()),
                (Some(_), None) => {
                    return Err(refuse(format!("{rel} was removed by the other writer")));
                },
                (Some(b), Some(t)) if base.as_deref() == Some(t.as_str()) => Some(b.to_vec()),
                (Some(b), Some(t)) => {
                    let ours = std::str::from_utf8(b)
                        .map_err(|_| refuse(format!("{rel} is not UTF-8")))?;
                    let out = crate::registry_merge::merge_text(
                        base.as_deref().unwrap_or(""),
                        ours,
                        t,
                        crate::registry_merge::DEFAULT_MARKER,
                    )
                    .map_err(|why| refuse(format!("{rel}: {}", why.0)))?;
                    if out.conflicts > 0 {
                        return Err(refuse(format!(
                            "{rel}: {} block(s) changed on both sides",
                            out.conflicts
                        )));
                    }
                    Some(out.text.into_bytes())
                },
                (None, t) if t.as_deref() == base.as_deref() || t.is_none() => None,
                (None, _) => {
                    return Err(refuse(format!(
                        "{rel} was changed by the other writer, and this write removes it"
                    )));
                },
            };
            next = match &merged {
                Some(b) => next.put(inner, b.clone()),
                None => next.remove(inner),
            };
            changes.push((rel, merged));
        }
        let held = (self.held.as_str(), tip);
        let on_tip = self
            .writer
            .current(Some(held))
            .map_err(|e| refuse(e.to_string()))?;
        let after = overlay(on_tip.clone(), &changes);
        let on_ref = self.held != ".";
        let old = self.writer.problems(&on_tip, &BTreeSet::new(), on_ref);
        let now = self.writer.problems(&after, &self.changed, on_ref);
        self.writer
            .refuse_new(&old, now)
            .map_err(|e| refuse(e.to_string()))?;
        Ok(next)
    }
}

#[cfg(test)]
#[path = "writer_race_tests.rs"]
mod race_tests;

#[cfg(test)]
#[path = "writer_checks_tests.rs"]
mod checks_tests;
