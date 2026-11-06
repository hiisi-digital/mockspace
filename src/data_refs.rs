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

use mockspace_store::{RefPair, RefStore, RemoteForm};
use serde::Deserialize;

/// One `[[data_ref]]` of `mockspace.toml`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct DataRefDecl {
    /// The ref namespace, `refs/mock/<namespace>`. Required, and unique among the declarations.
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
        match d.path.as_deref() {
            None => why.push(format!("{at}: `path` is missing, so the ref holds nothing")),
            Some(p) => {
                if let Some(bad) = path_problem(p) {
                    why.push(format!("{at}: path `{p}` {bad}"));
                } else {
                    let p = p.trim_matches('/');
                    for (j, o) in decls.iter().enumerate().take(i) {
                        let Some(other) = o.path.as_deref().map(|o| o.trim_matches('/')) else {
                            continue;
                        };
                        if p == other || p.starts_with(&format!("{other}/")) || other.starts_with(&format!("{p}/")) {
                            why.push(format!("{at}: path `{p}` overlaps `{other}` of [[data_ref]] #{}", j + 1));
                        }
                    }
                }
            },
        }
        if let Some(inst) = d.instance.as_deref() {
            if let Some(bad) = path_problem(inst) {
                why.push(format!("{at}: instance `{inst}` {bad}"));
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
        rel.split('/').fold(mock_dir.to_path_buf(), |p, part| p.join(part))
    }
}

/// `cargo mock data-refs <status|verify|instantiate|commit> [namespace]`. Prints one line per ref it
/// touched, and a refusal on stderr; exits non-zero when any ref could not be done.
pub fn run(cfg: &crate::config::Config, args: &[&str]) -> std::process::ExitCode {
    use std::process::ExitCode;

    use mockspace_store::{Instance, Refuse, Store};

    let Some((verb, rest)) = args.split_first() else {
        eprintln!("data-refs: status, verify, instantiate or commit [namespace]");
        return ExitCode::FAILURE;
    };
    if !matches!(*verb, "status" | "verify" | "instantiate" | "commit") {
        eprintln!("data-refs: `{verb}` is none of status, verify, instantiate or commit");
        return ExitCode::FAILURE;
    }
    let wanted = rest.first().copied();
    let decls: Vec<&DataRefDecl> = cfg.data_refs.iter().filter(|d| wanted.is_none_or(|w| d.namespace() == w)).collect();
    if decls.is_empty() {
        match wanted {
            Some(w) => eprintln!("data-refs: no [[data_ref]] has the namespace `{w}`"),
            None => println!("data-refs: no [[data_ref]] is declared"),
        }
        return if wanted.is_some() { ExitCode::FAILURE } else { ExitCode::SUCCESS };
    }
    let mut failed = false;
    for d in decls {
        let ns = d.namespace();
        let outcome = (|| -> Result<String, String> {
            let store = d.open(&cfg.repo_root)?;
            let dir = d.instance_dir(&cfg.mock_dir);
            let inst = Instance::new(&store, &dir).map_err(|e| e.to_string())?;
            match *verb {
                "status" => {
                    let tip = store.local_tip().map_err(|e| e.to_string())?;
                    let base = inst.base().map_err(|e| e.to_string())?;
                    let dirty = if base.is_some() { inst.is_dirty().map_err(|e| e.to_string())? } else { false };
                    Ok(format!(
                        "tip {} base {} {}",
                        tip.as_deref().unwrap_or("none"),
                        base.as_deref().unwrap_or("none"),
                        if dirty { "changed" } else { "clean" }
                    ))
                },
                "verify" => {
                    let tip = store.local_tip().map_err(|e| e.to_string())?;
                    let base = inst.base().map_err(|e| e.to_string())?;
                    let Some(base) = base else {
                        return Err("has no instance, so `instantiate` it".into());
                    };
                    if inst.is_dirty().map_err(|e| e.to_string())? {
                        return Err("has changes the ref does not, so `commit` them".into());
                    }
                    if tip.as_deref() != Some(base.as_str()) {
                        return Err(format!("is at {base} and the ref is at {}", tip.as_deref().unwrap_or("none")));
                    }
                    Ok(format!("clean at {base}"))
                },
                "instantiate" => {
                    store.refresh().map_err(|e| e.to_string())?;
                    let tip = inst.materialize().map_err(|e| e.to_string())?;
                    Ok(format!("instantiated at {tip}"))
                },
                _ => {
                    let message = format!("data: {ns}");
                    let first = inst.base().map_err(|e| e.to_string())?.is_none()
                        && store.local_tip().map_err(|e| e.to_string())?.is_none();
                    let applied = if first { inst.seed(&message, &Refuse) } else { inst.apply(&message, &Refuse) }
                        .map_err(|e| e.to_string())?;
                    Ok(match applied.commit {
                        Some(c) if applied.changed => format!("committed {c}"),
                        _ => "nothing changed".to_string(),
                    })
                },
            }
        })();
        match outcome {
            Ok(line) => println!("{ns}: {line}"),
            Err(why) => {
                eprintln!("{ns}: {why}");
                failed = true;
            },
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

#[cfg(test)]
#[path = "data_refs_tests.rs"]
mod tests;
