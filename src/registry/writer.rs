//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one way registry files are written to a store.
//!
//! A write to a ref skips the commit gate, because nothing is committed in a work tree, so the
//! checks the gate runs over the registry data run here instead, before the write: the files
//! must read as TOML with an `id` on every row, and the registry they make must raise no finding
//! of [`validate`], [`validate_row_references`] or [`validate_provenance`] that the registry
//! before the write did not already raise. A write that fails them is refused and no store
//! is touched. A finding the registry already had is not the write's, and does not stop it.
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
pub struct RegistryWriter<'a> {
    cfg:    &'a Config,
    stores: &'a RegistryStores,
}

impl<'a> RegistryWriter<'a> {
    pub fn new(cfg: &'a Config, stores: &'a RegistryStores) -> Self {
        Self { cfg, stores }
    }

    /// Put (`Some`) or remove (`None`) the files at these paths under the registry root, as one
    /// write with `message`. Every path must be held by the same store.
    pub fn write(
        &self,
        changes: &[(String, Option<Vec<u8>>)],
        message: &str,
    ) -> Result<Applied, RegistryStoreError> {
        let (store, held, edit) = self.stores.edit_in(changes)?;
        let before = self.current(None)?;
        let changed: BTreeSet<String> = changes.iter().map(|(p, _)| p.clone()).collect();
        let base: BTreeMap<String, Option<String>> = before
            .iter()
            .filter(|(r, _)| changed.contains(r))
            .cloned()
            .collect();
        let after = overlay(before.clone(), changes);
        refuse_new(&self.problems(&before, &BTreeSet::new()), self.problems(&after, &changed))?;
        let hook = Race {
            writer: self,
            held: held.clone(),
            base,
            changed,
        };
        store
            .apply(&edit, message, &hook)
            .map_err(|source| RegistryStoreError::Store { held, source })
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
            let text = self.stores.read(&rel)?.and_then(|b| String::from_utf8(b).ok());
            out.push((rel, text));
        }
        if let Some((held, reader)) = tip.filter(|(h, _)| *h != ".") {
            let fail = |source| RegistryStoreError::Store { held: held.to_string(), source };
            for inner in reader.list("").map_err(fail)? {
                let rel = format!("{held}/{inner}");
                if is_registry_file(&rel) {
                    let text = reader.read(&inner).map_err(fail)?.and_then(|b| String::from_utf8(b).ok());
                    out.push((rel, text));
                }
            }
            out.sort_by(|a, b| a.0.split('/').cmp(b.0.split('/')));
        }
        Ok(out)
    }

    /// What the checks find in `files`. `changed` names the files this write touches, which
    /// must also read as TOML and give every row an `id`: the loader skips such a file or row
    /// and says so, and a skipped row is a row the write silently lost.
    fn problems(&self, files: &Files, changed: &BTreeSet<String>) -> Vec<String> {
        let cfg = self.cfg;
        let mut out = Vec::new();
        let known: BTreeSet<&str> = cfg.registry_namespaces.iter().map(|n| n.key.as_str()).collect();
        for (rel, text) in files.iter().filter(|(r, _)| changed.contains(r)) {
            let Some(text) = text else { continue };
            match text.parse::<toml_edit::DocumentMut>() {
                Err(e) => out.push(format!("[not-toml] {rel} is not valid TOML: {e}")),
                Ok(doc) => {
                    for (key, item) in doc.iter().filter(|(k, _)| known.contains(k)) {
                        let rows = item.as_array_of_tables().into_iter().flat_map(|t| t.iter());
                        if rows.into_iter().any(|t| t.get("id").and_then(|v| v.as_str()).is_none()) {
                            out.push(format!("[row-without-id] a [[{key}]] row in {rel} has no id"));
                        }
                    }
                },
            }
        }
        let reg = rows_of_texts(self.stores.root(), files.clone(), &cfg.registry_namespaces, None);
        let ns = &cfg.registry_namespaces;
        let findings = validate(ns, &reg)
            .into_iter()
            .chain(validate_row_references(&reg, ns))
            .chain(validate_provenance(&cfg.repo_root, &cfg.registry_roots, &cfg.frozen_roots, &reg, ns));
        out.extend(findings.map(|f| format!("[{}] {}", f.kind, f.message)));
        out
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
    let fresh: Vec<String> = new.into_iter().filter(|p| !old.contains(p)).collect();
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
        if self.held == "." { inner.to_string() } else { format!("{}/{inner}", self.held) }
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
                (Some(_), None) => return Err(refuse(format!("{rel} was removed by the other writer"))),
                (Some(b), Some(t)) if base.as_deref() == Some(t.as_str()) => Some(b.to_vec()),
                (Some(b), Some(t)) => {
                    let ours = std::str::from_utf8(b).map_err(|_| refuse(format!("{rel} is not UTF-8")))?;
                    let out = crate::registry_merge::merge_text(base.as_deref().unwrap_or(""), ours, t, 7)
                        .map_err(|why| refuse(format!("{rel}: {}", why.0)))?;
                    if out.conflicts > 0 {
                        return Err(refuse(format!("{rel}: {} block(s) changed on both sides", out.conflicts)));
                    }
                    Some(out.text.into_bytes())
                },
                (None, t) if t.as_deref() == base.as_deref() || t.is_none() => None,
                (None, _) => return Err(refuse(format!("{rel} was changed by the other writer, and this write removes it"))),
            };
            next = match &merged {
                Some(b) => next.put(inner, b.clone()),
                None => next.remove(inner),
            };
            changes.push((rel, merged));
        }
        let held = (self.held.as_str(), tip);
        let on_tip = self.writer.current(Some(held)).map_err(|e| refuse(e.to_string()))?;
        let after = overlay(on_tip.clone(), &changes);
        refuse_new(&self.writer.problems(&on_tip, &BTreeSet::new()), self.writer.problems(&after, &self.changed))
            .map_err(|e| refuse(e.to_string()))?;
        Ok(next)
    }
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod tests;
