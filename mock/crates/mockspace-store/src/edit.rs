//! An edit: the set of files one write puts or removes.

use std::collections::BTreeMap;

use crate::error::StoreError;
use crate::path::validate_path;

/// Files to put (`Some(bytes)`) or remove (`None`), keyed by store path.
/// A later call on the same path replaces an earlier one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    files: BTreeMap<String, Option<Vec<u8>>>,
}

impl Edit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put `bytes` at `path`.
    pub fn put(mut self, path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.into(), Some(bytes.into()));
        self
    }

    /// Remove the file at `path`. Removing a file that is not there is not an error.
    pub fn remove(mut self, path: impl Into<String>) -> Self {
        self.files.insert(path.into(), None);
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
                if b.starts_with(a.as_str()) && b[a.len() ..].starts_with('/') {
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
