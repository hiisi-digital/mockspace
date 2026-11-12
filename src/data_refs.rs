//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Directories of the mock directory kept on a git ref, declared in `mockspace.toml`.
//!
//! A `[[data_ref]]` names a ref namespace and the one directory of the mock directory it holds. The
//! files of that directory live in the ref, outside every branch, and an instance in the working
//! tree (by default in place, under the mock directory) holds them for the tools that read files.
//! Absent means none, so a project that declares nothing is read as it always was.
//!
//! This module holds the declaration and what is checked of it before anything is opened. The ref
//! and its instance are `mockspace_store::RefStore` and `Instance`, which a [`DataRefDecl`] opens
//! through the same [`RefDecl`] a registry store does, so a misspelt key refuses the config here as
//! it does there.

use std::path::{Component, Path};

use mockspace_store::RefStore;
use serde::Deserialize;

use crate::registry::RefDecl;

/// One `[[data_ref]]` of `mockspace.toml`.
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DataRefDecl {
    /// The ref namespace: the ref is `refs/heads/mock/<namespace>` in the branch form and
    /// `refs/mock/<namespace>` in the ref form. Required, and unique among the declarations.
    pub namespace: Option<String>,
    /// The directory under the mock directory the ref holds, relative to it. The root of the ref is
    /// this directory, so one ref holds one directory and writers of different ones never meet.
    pub path:      Option<String>,
    /// Where the instance is placed, relative to the mock directory. Defaults to `path`, so the
    /// instance is where the directory is read today.
    pub instance:  Option<String>,
    /// The remote the ref is fetched from and pushed to. Defaults to `origin`.
    pub remote:    Option<String>,
    /// `branch` (the default) or `ref`, as for a registry store.
    pub form:      Option<String>,
    /// The files of `path` the ref holds, as patterns relative to it (`**/*.md.tmpl`, `*/**`).
    /// Absent, the ref holds every file of the directory and the directory is the ref's alone.
    /// Given, it holds only the files a pattern matches, and the branch keeps the others, so a
    /// crate's templates leave the tree while its sources stay.
    pub include:   Option<Vec<String>>,
}

impl DataRefDecl {
    /// The namespace, which [`validate`] has checked is there.
    pub fn namespace(&self) -> &str {
        self.namespace.as_deref().unwrap_or("")
    }

    /// The store of this ref over the repository at `repo_root`.
    pub fn open(&self, repo_root: &Path) -> Result<RefStore, String> {
        RefDecl {
            dir:       None,
            namespace: self.namespace.clone(),
            remote:    self.remote.clone(),
            form:      self.form.clone(),
        }
        .open(repo_root, self.namespace())
    }
}

/// The first components of a path that belong to a store of their own and are never a data ref's.
const OWN_STORES: [&str; 3] = ["registry", ROUNDS, "test-history"];

/// The directory of the design rounds under the mock directory.
const ROUNDS: &str = "design_rounds";

/// Whether directories `a` and `b` (relative, trimmed of `/`) are the same or one holds the other.
fn nested(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// Every refusal of `decls` at once, so a config with several mistakes names them in one run.
pub fn validate(decls: &[DataRefDecl]) -> Result<(), Vec<String>> {
    validate_against(decls, &[])
}

/// [`validate`], and the namespaces `taken` by the registry's stores and the test history, which a
/// data ref may not share since two writers on one ref would each commit over the other.
pub fn validate_against(decls: &[DataRefDecl], taken: &[String]) -> Result<(), Vec<String>> {
    let mut why = Vec::new();
    for (i, d) in decls.iter().enumerate() {
        let at = format!("[[data_ref]] #{}", i + 1);
        match d.namespace.as_deref() {
            None | Some("") => why.push(format!("{at}: `namespace` is missing or empty")),
            Some(ns) => {
                if !crate::pairing::is_ref_name(ns) {
                    why.push(format!("{at}: the namespace `{ns}` is not one a ref and a `Mock-Ref` line can be named by"));
                }
                if decls.iter().take(i).any(|o| o.namespace.as_deref() == Some(ns)) {
                    why.push(format!("{at}: the namespace `{ns}` is declared twice"));
                }
                if taken.iter().any(|t| t == ns) {
                    why.push(format!("{at}: the namespace `{ns}` is a registry store's or the test history's"));
                }
            },
        }
        let place = |d: &DataRefDecl| d.instance.as_deref().or(d.path.as_deref()).map(|p| p.trim_matches('/').to_string());
        if let (Some(mine), true) = (place(d), d.instance.as_deref().is_none_or(|p| path_problem(p).is_none())) {
            for (j, o) in decls.iter().enumerate().take(i) {
                // a ref over the whole mock directory lies around every other
                if place(o).is_some_and(|theirs| mine == WHOLE || theirs == WHOLE || nested(&mine, &theirs)) {
                    why.push(format!("{at}: its instance `{mine}` overlaps the instance of [[data_ref]] #{}", j + 1));
                }
            }
        }
        match d.path.as_deref() {
            None => why.push(format!("{at}: `path` is missing, so the ref holds nothing")),
            Some(WHOLE) => {
                match d.include.as_deref() {
                    None => {
                        why.push(format!(
                            "{at}: path `.` is the mock directory itself, which a ref holds files of only by `include`"
                        ));
                    },
                    Some(patterns) => {
                        for pat in patterns {
                            if let Some(bad) = whole_pattern_problem(pat) {
                                why.push(format!("{at}: pattern `{pat}` {bad}"));
                            }
                        }
                    },
                }
            },
            Some(p) => {
                if let Some(bad) = path_problem(p) {
                    why.push(format!("{at}: path `{p}` {bad}"));
                } else {
                    let p = p.trim_matches('/');
                    match OWN_STORES.iter().find(|s| nested(p, s)) {
                        // The closed rounds are directories of `design_rounds` and belong to no
                        // branch, where the flat files of an open round are the branch's own. A
                        // ref may hold the first by patterns that reach only into a directory.
                        Some(&ROUNDS) if p == ROUNDS && d.include.is_some() => {
                            for pat in d.include.iter().flatten() {
                                let first = pat.split('/').next().unwrap_or("");
                                if !pat.contains('/') || first == "**" {
                                    why.push(format!(
                                        "{at}: pattern `{pat}` could hold a flat file of `{ROUNDS}`, which is an open \
                                         round's and stays on its branch"
                                    ));
                                }
                            }
                        },
                        Some(own) => {
                            why.push(format!("{at}: path `{p}` is under `{own}`, which has a store of its own"));
                        },
                        None => {},
                    }
                    for (j, o) in decls.iter().enumerate().take(i) {
                        let Some(other) = o.path.as_deref().map(|o| o.trim_matches('/')) else {
                            continue;
                        };
                        if nested(p, other) {
                            why.push(format!("{at}: path `{p}` overlaps `{other}` of [[data_ref]] #{}", j + 1));
                        }
                    }
                }
            },
        }
        if let Some(patterns) = d.include.as_deref() {
            if let Err(e) = mockspace_store::Held::new(patterns) {
                why.push(format!("{at}: `include` {e}"));
            }
            if d.instance.is_some() {
                why.push(format!(
                    "{at}: `instance` and `include` are both given, and files held by pattern stay where the \
                     branch's own files are"
                ));
            }
        }
        if let Some(inst) = d.instance.as_deref() {
            if let Some(bad) = path_problem(inst) {
                why.push(format!("{at}: instance `{inst}` {bad}"));
            } else if let Some(own) = OWN_STORES.iter().find(|s| nested(inst.trim_matches('/'), s)) {
                why.push(format!("{at}: instance `{inst}` is under `{own}`, which has a store of its own"));
            }
        }
        if d.namespace.as_deref().is_some_and(|n| !n.is_empty()) {
            if let Err(e) = d.open(Path::new(".")) {
                why.push(format!("{at}: {e}"));
            }
        }
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}

/// The `path` that names the mock directory itself. A ref may hold files of it only by pattern.
const WHOLE: &str = ".";

/// What is wrong with `pat` as a pattern of a ref over the whole mock directory, if anything. Such
/// a ref lies among everything the branch and the engine keep there, so each pattern names where
/// it reaches: a file of the mock directory itself, or a path under a directory named in full.
fn whole_pattern_problem(pat: &str) -> Option<String> {
    let mut parts = pat.split('/');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        let config = mockspace_store::Held::new(&[pat]).is_ok_and(|h| h.holds("mockspace.toml"));
        return config.then(|| "could hold `mockspace.toml`, which a clone reads before it has any ref".to_string());
    }
    if first.contains('*') {
        return Some("starts with a wildcard, and so could reach into any directory; name the directory".to_string());
    }
    if first == ROUNDS {
        let into_a_directory = rest.len() >= 2 && rest[0] != "**";
        return (!into_a_directory).then(|| {
            format!("could hold a flat file of `{ROUNDS}`, which is an open round's and stays on its branch")
        });
    }
    OWN_STORES.contains(&first).then(|| format!("is under `{first}`, which has a store of its own"))
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

impl DataRefDecl {
    /// The directory the instance is placed in: `instance` if given, else `path`, under `mock_dir`.
    pub fn instance_dir(&self, mock_dir: &Path) -> std::path::PathBuf {
        let rel = self.instance.as_deref().or(self.path.as_deref()).unwrap_or("").trim_matches('/');
        if rel == WHOLE {
            return mock_dir.to_path_buf();
        }
        rel.split('/').fold(mock_dir.to_path_buf(), |p, part| p.join(part))
    }

    /// The instance of this ref over `store`, holding the files `include` names where it is given.
    pub fn instance<'a>(
        &self,
        store: &'a RefStore,
        mock_dir: &Path,
    ) -> Result<mockspace_store::Instance<'a>, String> {
        let inst = mockspace_store::Instance::new(store, self.instance_dir(mock_dir)).map_err(|e| e.to_string())?;
        match self.include.as_deref() {
            None => Ok(inst),
            Some(patterns) => Ok(inst.holding(mockspace_store::Held::new(patterns).map_err(|e| e.to_string())?)),
        }
    }
}

impl DataRefDecl {
    /// The files under this ref's instance directory that the index of the branch holds and the ref would hold
    /// too, as paths from the repository root.
    pub fn tracked(&self, repo_root: &Path, mock_dir: &Path) -> Result<Vec<String>, String> {
        let dir = self.instance_dir(mock_dir);
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["--literal-pathspecs", "ls-files", "-z", "--full-name", "--"])
            .arg(&dir)
            .output()
            .map_err(|e| format!("running git ls-files: {e}"))?;
        if !out.status.success() {
            return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let held = match self.include.as_deref() {
            None => None,
            Some(patterns) => Some(mockspace_store::Held::new(patterns).map_err(|e| e.to_string())?),
        };
        let lead = match dir.strip_prefix(repo_root) {
            Ok(rel) => {
                rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
            },
            Err(_) => return Err(format!("{} is not under the repository", dir.display())),
        };
        Ok(String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            .filter(|p| {
                let inside = p.strip_prefix(lead.as_str()).map(|r| r.trim_start_matches('/'));
                match (&held, inside) {
                    (None, _) => true,
                    (Some(h), Some(rel)) => h.holds(rel),
                    (Some(_), None) => false,
                }
            })
            .map(str::to_owned)
            .collect())
    }
}

mod gate;
mod hook;
mod landing;
mod sparse;
mod standing;
pub use gate::{repo_lints_pass, run_gated};
pub use sparse::{check as git_is_kept_off_the_instances, install as keep_git_off_the_instances};
pub use standing::standing;
pub use landing::trailer_lines;

mod verbs;
pub use verbs::{ref_files, run};

#[cfg(test)]
#[path = "data_refs_tests.rs"]
mod tests;
