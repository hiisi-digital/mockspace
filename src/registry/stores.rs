//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where the registry's files live: a directory by default, a git ref for any directory a project
//! mounts one on.
//!
//! The registry is the `*.toml` files under `<mock>/registry/`. A [`RegistryStores`] holds the
//! store of that whole tree (a [`DirStore`] on the directory, which is what every project has today)
//! and, for each directory the project declares in `[[registry.store]]`, a store of its own
//! (a [`RefStore`]) that holds the files under that directory instead. A file is named by its path
//! under the registry root whichever store holds it, so a row's `source` reads the same in both,
//! and a directory mounted on a ref hides what the tree still holds under the same name.
//!
//! Reading asks the stores, and a ref store that cannot be fetched is an error. It is never read
//! as a registry with no rows in it, which is how a fetch that failed would otherwise look.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mockspace_store::{DirStore, Edit, RefPair, RefStore, RemoteForm, Store, StoreError};
use serde::Deserialize;

/// Why a registry could not be read or written.
#[derive(Debug)]
pub enum RegistryStoreError {
    /// A store failed. Carries which directory of the registry it holds.
    Store {
        held:   String,
        source: StoreError,
    },
    /// The checks of the writer refused the write; nothing reached any store.
    Refused(Vec<String>),
    /// A `[[registry.store]]` declaration is not one the engine can open.
    Declared(String),
    /// A write that would have to land in two stores at once, which no store can make atomic.
    SpansStores(Vec<String>),
}

impl std::fmt::Display for RegistryStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store {
                held,
                source,
            } => write!(f, "the registry store of `{held}`: {source}"),
            Self::Refused(why) => {
                write!(
                    f,
                    "the registry checks refused the write, nothing was stored:"
                )?;
                why.iter().try_for_each(|w| write!(f, "\n  {w}"))
            },
            Self::Declared(why) => write!(f, "[[registry.store]]: {why}"),
            Self::SpansStores(held) => {
                write!(
                    f,
                    "one write may land in one store, and this one touches {}",
                    held.join(", ")
                )
            },
        }
    }
}

impl std::error::Error for RegistryStoreError {}

struct Mount {
    dir:   String,
    store: Box<dyn Store>,
}

/// The stores that together hold one registry.
pub struct RegistryStores {
    root:      PathBuf,
    default:   Box<dyn Store>,
    mounts:    Vec<Mount>,
    refreshed: Mutex<bool>,
}

impl RegistryStores {
    /// The registry as every project has it today: one directory, `<mock>/registry`.
    pub fn directory(mock_dir: &Path) -> Self {
        let root = mock_dir.join("registry");
        Self {
            default: Box::new(DirStore::new(root.clone())),
            root,
            mounts: Vec::new(),
            refreshed: Mutex::new(false),
        }
    }

    /// The registry of `mock_dir` with each declared directory mounted on its ref.
    pub fn from_decls(
        mock_dir: &Path,
        repo_root: &Path,
        decls: &[RefDecl],
    ) -> Result<Self, RegistryStoreError> {
        let mut out = Self::directory(mock_dir);
        for d in decls {
            let dir = d.dir.as_deref().unwrap_or("").trim_matches('/');
            if dir.is_empty() {
                return Err(RegistryStoreError::Declared(
                    "`dir` is missing or empty".into(),
                ));
            }
            let store = d
                .open(repo_root, &format!("registry/{dir}"))
                .map_err(|why| RegistryStoreError::Declared(format!("`{dir}`: {why}")))?;
            out = out.mount(dir, Box::new(store))?;
        }
        Ok(out)
    }

    /// Keep the files under `dir` in `store` instead of the tree.
    pub fn mount(mut self, dir: &str, store: Box<dyn Store>) -> Result<Self, RegistryStoreError> {
        let dir = dir.trim_matches('/').to_string();
        mockspace_store::validate_path(&dir).map_err(|source| {
            RegistryStoreError::Store {
                held: dir.clone(),
                source,
            }
        })?;
        if self.mounts.iter().any(|m| {
            m.dir == dir
                || m.dir.starts_with(&format!("{dir}/"))
                || dir.starts_with(&format!("{}/", m.dir))
        }) {
            return Err(RegistryStoreError::Declared(format!(
                "`{dir}` overlaps a directory already mounted"
            )));
        }
        self.mounts.push(Mount {
            dir,
            store,
        });
        Ok(self)
    }

    /// The registry's root in the tree, which a row's `source` is relative to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bring every store up to date, once. A ref store whose fetch fails fails this.
    pub fn refresh(&self) -> Result<(), RegistryStoreError> {
        let mut done = self.refreshed.lock().unwrap_or_else(|e| e.into_inner());
        if *done {
            return Ok(());
        }
        let held = |h: &str| h.to_string();
        self.default.refresh().map_err(|source| {
            RegistryStoreError::Store {
                held: held("."),
                source,
            }
        })?;
        for m in &self.mounts {
            m.store.refresh().map_err(|source| {
                RegistryStoreError::Store {
                    held: m.dir.clone(),
                    source,
                }
            })?;
        }
        *done = true;
        Ok(())
    }

    fn mount_of(&self, rel: &str) -> Option<&Mount> {
        self.mounts
            .iter()
            .find(|m| rel == m.dir || rel.starts_with(&format!("{}/", m.dir)))
    }

    /// The store holding `rel`, the directory it is held under (`.` for the tree), and `rel`
    /// as that store names it.
    pub(crate) fn route<'a>(&'a self, rel: &str) -> (&'a dyn Store, String, String) {
        match self.mount_of(rel) {
            Some(m) => {
                (
                    m.store.as_ref(),
                    m.dir.clone(),
                    rel[m.dir.len() ..].trim_start_matches('/').to_string(),
                )
            },
            None => (self.default.as_ref(), ".".into(), rel.to_string()),
        }
    }

    /// Every `*.toml` of the registry as a path under the root, in the order the directory walk
    /// has always met them: by name at every level, a directory in place among its siblings, and
    /// no directory whose name begins with a dot.
    pub fn files(&self) -> Result<Vec<String>, RegistryStoreError> {
        self.refresh()?;
        let mut out: Vec<String> = Vec::new();
        let listed = |store: &dyn Store, held: &str| {
            store.list("").map_err(|source| {
                RegistryStoreError::Store {
                    held: held.to_string(),
                    source,
                }
            })
        };
        for p in listed(self.default.as_ref(), ".")? {
            if self.mount_of(&p).is_none() {
                out.push(p);
            }
        }
        for m in &self.mounts {
            for p in listed(m.store.as_ref(), &m.dir)? {
                out.push(format!("{}/{p}", m.dir));
            }
        }
        out.retain(|p| is_registry_file(p));
        out.sort_by(|a, b| a.split('/').cmp(b.split('/')));
        Ok(out)
    }

    /// The bytes of the file at `rel`, or `None` where there is none.
    pub fn read(&self, rel: &str) -> Result<Option<Vec<u8>>, RegistryStoreError> {
        self.refresh()?;
        let (store, held, inner) = self.route(rel);
        store.read(&inner).map_err(|source| {
            RegistryStoreError::Store {
                held,
                source,
            }
        })
    }

    /// The path a file has in the tree, which is what a row says it was declared in.
    pub fn source_of(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root.clone(), |p, part| p.join(part))
    }

    /// The path under the root a `source` names, if it lies under it.
    pub fn rel_of(&self, source: &Path) -> Option<String> {
        let rel = source.strip_prefix(&self.root).ok()?;
        Some(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// An edit as the store holding `rel` names its paths. Every entry must be held by the same store.
    pub(crate) fn edit_in(
        &self,
        entries: &[(String, Option<Vec<u8>>)],
    ) -> Result<(&dyn Store, String, Edit), RegistryStoreError> {
        let mut held: Vec<String> = entries.iter().map(|(p, _)| self.route(p).1).collect();
        held.sort();
        held.dedup();
        if held.len() > 1 {
            return Err(RegistryStoreError::SpansStores(held));
        }
        let first = entries.first().map(|(p, _)| p.as_str()).unwrap_or("");
        let (store, held, _) = self.route(first);
        let mut edit = Edit::new();
        for (p, bytes) in entries {
            let inner = self.route(p).2;
            edit = match bytes {
                Some(b) => edit.put(inner, b.clone()),
                None => edit.remove(inner),
            };
        }
        Ok((store, held, edit))
    }
}

/// Whether `rel` is a file the registry loads: a `*.toml` not under a directory whose name begins with a dot.
pub(crate) fn is_registry_file(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    rel.ends_with(".toml")
        && parts[.. parts.len() - 1]
            .iter()
            .all(|d| !d.starts_with('.'))
}

impl crate::config::Config {
    /// The stores of this project's registry: its directory, with each `[[registry.store]]`
    /// directory mounted on its ref.
    pub fn registry_stores(&self) -> Result<RegistryStores, RegistryStoreError> {
        RegistryStores::from_decls(&self.mock_dir, &self.repo_root, &self.registry_stores)
    }
}

/// Where a namespace is kept on a git ref: the one declaration of a store on a ref, read with the
/// same strictness whether it holds registry files (`[[registry.store]]`) or test history
/// (`[test.history]`), so a misspelt key refuses the config in both.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct RefDecl {
    /// The directory under `<mock>/registry/` whose files the ref holds. A `[[registry.store]]`
    /// names one and a `[test.history]` has none, which is refused where the history opens.
    pub dir:       Option<String>,
    /// The ref namespace, `refs/mock/<namespace>`. Each user of it has its own default.
    pub namespace: Option<String>,
    /// The remote the ref is fetched from and pushed to. Defaults to `origin`.
    pub remote:    Option<String>,
    /// `branch` (the default: `refs/heads/mock/<namespace>` on the remote, which a host that
    /// takes no ref outside `refs/heads` accepts) or `ref` (`refs/mock/<namespace>` on both sides).
    pub form:      Option<String>,
}

impl RefDecl {
    /// The store this declares, over the repository at `repo_root`, for `default_namespace` where
    /// the declaration names none.
    pub fn open(&self, repo_root: &Path, default_namespace: &str) -> Result<RefStore, String> {
        let form = match self.form.as_deref() {
            None | Some("branch") => RemoteForm::Branch,
            Some("ref") => RemoteForm::Ref,
            Some(other) => return Err(format!("form `{other}` is neither `branch` nor `ref`")),
        };
        let namespace = self.namespace.as_deref().unwrap_or(default_namespace);
        let pair = RefPair::for_namespace(namespace, form).map_err(|e| e.to_string())?;
        let remote = self.remote.clone().unwrap_or_else(|| "origin".into());
        Ok(RefStore::new(repo_root, remote, pair))
    }
}


#[cfg(test)]
#[path = "stores_tests.rs"]
mod tests;
