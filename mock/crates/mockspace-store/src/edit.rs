//! An edit: the set of files one write puts or removes.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::StoreError;
use crate::path::validate_path;

/// Files to put (`Some(bytes)`) or remove (`None`), keyed by store path.
/// A later call on the same path replaces an earlier one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    files: BTreeMap<String, Option<Vec<u8>>>,
    /// The puts that are executable (mode 100755); every other put is a plain file (100644).
    exec:  BTreeSet<String>,
}

impl Edit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put `bytes` at `path`.
    pub fn put(mut self, path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let path = path.into();
        self.exec.remove(&path);
        self.files.insert(path, Some(bytes.into()));
        self
    }

    /// Put `bytes` at `path` as an executable file.
    pub fn put_exec(mut self, path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let path = path.into();
        self.exec.insert(path.clone());
        self.files.insert(path, Some(bytes.into()));
        self
    }

    /// Whether the put at `path` is executable.
    pub fn is_exec(&self, path: &str) -> bool {
        self.exec.contains(path)
    }

    /// Remove the file at `path`. Removing a file that is not there is not an error.
    pub fn remove(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        self.exec.remove(&path);
        self.files.insert(path, None);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The entries in path order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, Option<&[u8]>)> {
        self.files.iter().map(|(p, b)| (p.as_str(), b.as_deref()))
    }

    /// Every path must be a valid store path, and no path may sit under
    /// another one that is also an entry (a file and a directory of one name).
    pub fn validate(&self) -> Result<(), StoreError> {
        for path in self.files.keys() {
            validate_path(path)?;
        }
        let paths: Vec<&String> = self.files.keys().collect();
        for (i, a) in paths.iter().enumerate() {
            for b in &paths[i + 1 ..] {
                let a_is_put = matches!(self.files.get(a.as_str()), Some(Some(_)));
                let b_is_put = matches!(self.files.get(b.as_str()), Some(Some(_)));
                if a_is_put
                    && b_is_put
                    && b.starts_with(a.as_str())
                    && b[a.len() ..].starts_with('/')
                {
                    return Err(StoreError::InvalidPath {
                        path: (*b).clone(),
                        why:  "lies under another path of the same edit",
                    });
                }
            }
        }
        Ok(())
    }
}

impl Edit {
    /// The edit against the state it is to be written on: a file may not be put where a
    /// directory of files stands, nor below a path that is a file, unless the same edit
    /// removes what is in the way. Both backends refuse the same edits, a path beginning with a double quote included.
    pub fn check_against(&self, base: &dyn crate::reader::Reader) -> Result<(), StoreError> {
        let existing: std::collections::BTreeSet<String> = base.list("")?.into_iter().collect();
        let removed = |p: &str| matches!(self.files.get(p), Some(None));
        let clash = |path: &str, why| {
            Err(StoreError::InvalidPath {
                path: path.to_owned(),
                why,
            })
        };
        for (path, bytes) in &self.files {
            if bytes.is_none() {
                continue;
            }
            for (i, _) in path.match_indices('/') {
                let ancestor = &path[.. i];
                if existing.contains(ancestor) && !removed(ancestor) {
                    return clash(path, "a parent of it is a file");
                }
            }
            let dir = format!("{path}/");
            if existing
                .range(dir.clone() ..)
                .take_while(|e| e.starts_with(&dir))
                .any(|e| !removed(e))
            {
                return clash(path, "it is a directory holding files");
            }
        }
        Ok(())
    }
}
