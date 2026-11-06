//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Directories of the mock directory kept on a git ref, declared in `mockspace.toml`.
//!
//! A `[[data_ref]]` names a ref namespace and the directories of the mock directory it holds. The
//! files of those directories live in the ref, outside every branch, and an instance in the working
//! tree (by default in place, under the mock directory) holds them for the tools that read files.
//! Absent means none, so a project that declares nothing is read as it always was.
//!
//! This module holds the declaration and what is checked of it before anything is opened. The ref
//! and its instance are `mockspace_store::RefStore` and `Instance`, which a [`DataRefDecl`] opens
//! through the same [`RefDecl`] a registry store does, so a misspelt key refuses the config here as
//! it does there.

use std::path::{Component, Path};

use mockspace_store::{RefPair, RefStore, RemoteForm};
use serde::Deserialize;

/// One `[[data_ref]]` of `mockspace.toml`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct DataRefDecl {
    /// The ref namespace, `refs/mock/<namespace>`. Required, and unique among the declarations.
    pub namespace: Option<String>,
    /// The directories under the mock directory the ref holds, as paths relative to it.
    pub paths:     Vec<String>,
    /// Where the instance is placed, relative to the mock directory. Defaults to the mock directory
    /// itself, so each path of `paths` is instantiated where it is read today.
    pub instance:  Option<String>,
    /// The remote the ref is fetched from and pushed to. Defaults to `origin`.
    pub remote:    Option<String>,
    /// `branch` (the default) or `ref`, as for a registry store.
    pub form:      Option<String>,
}

impl DataRefDecl {
    /// The namespace, which [`validate`] has checked is there.
    pub fn namespace(&self) -> &str {
        self.namespace.as_deref().unwrap_or("")
    }

    /// The store of this ref over the repository at `repo_root`.
    pub fn open(&self, repo_root: &Path) -> Result<RefStore, String> {
        let form = match self.form.as_deref() {
            None | Some("branch") => RemoteForm::Branch,
            Some("ref") => RemoteForm::Ref,
            Some(other) => return Err(format!("form `{other}` is neither `branch` nor `ref`")),
        };
        let pair = RefPair::for_namespace(self.namespace(), form).map_err(|e| e.to_string())?;
        let remote = self.remote.clone().unwrap_or_else(|| "origin".into());
        Ok(RefStore::new(repo_root, remote, pair))
    }
}

/// Every refusal of `decls` at once, so a config with several mistakes names them in one run.
pub fn validate(decls: &[DataRefDecl]) -> Result<(), Vec<String>> {
    let mut why = Vec::new();
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for (i, d) in decls.iter().enumerate() {
        let at = format!("[[data_ref]] #{}", i + 1);
        match d.namespace.as_deref() {
            None | Some("") => why.push(format!("{at}: `namespace` is missing or empty")),
            Some(ns) => {
                if decls.iter().take(i).any(|o| o.namespace.as_deref() == Some(ns)) {
                    why.push(format!("{at}: the namespace `{ns}` is declared twice"));
                }
            },
        }
        if d.paths.is_empty() {
            why.push(format!("{at}: `paths` is empty, so the ref holds nothing"));
        }
        for p in &d.paths {
            if let Some(bad) = path_problem(p) {
                why.push(format!("{at}: path `{p}` {bad}"));
                continue;
            }
            let p = p.trim_matches('/');
            for (other_ns, other) in &seen {
                if p == *other || p.starts_with(&format!("{other}/")) || other.starts_with(&format!("{p}/")) {
                    why.push(format!("{at}: path `{p}` overlaps `{other}` of `{other_ns}`"));
                }
            }
            seen.push((d.namespace.as_deref().unwrap_or(""), p));
        }
        if let Some(inst) = d.instance.as_deref() {
            if let Some(bad) = path_problem(inst) {
                why.push(format!("{at}: instance `{inst}` {bad}"));
            }
        }
        if let Err(e) = d.open(Path::new(".")).map(|_| ()) {
            if d.namespace.as_deref().is_some_and(|n| !n.is_empty()) {
                why.push(format!("{at}: {e}"));
            }
        }
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}

/// What is wrong with `p` as a path under the mock directory, if anything.
fn path_problem(p: &str) -> Option<&'static str> {
    let t = p.trim_matches('/');
    if t.is_empty() {
        return Some("is empty");
    }
    if Path::new(p).is_absolute() {
        return Some("is absolute, and a path is relative to the mock directory");
    }
    if Path::new(t).components().any(|c| !matches!(c, Component::Normal(_))) {
        return Some("holds `.` or `..`");
    }
    None
}

#[cfg(test)]
#[path = "data_refs_tests.rs"]
mod tests;
