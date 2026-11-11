//! Reading one commit's tree with `git ls-tree` and `git cat-file`; nothing is checked out.

use std::path::PathBuf;

use crate::error::StoreError;
use crate::git::Git;
use crate::path::{under, validate_path};
use crate::reader::Reader;

/// The files of one commit. A ref store reads its local tip through one, and a
/// merge hook is handed one for the tip that won the race.
pub(crate) struct CommitReader {
    git:    Git,
    commit: Option<String>,
}

impl CommitReader {
    /// `commit` of `None` is the empty tree.
    pub fn new(repo: PathBuf, commit: Option<String>) -> Self {
        Self {
            git: Git::new(repo),
            commit,
        }
    }
}

impl Reader for CommitReader {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        validate_path(path)?;
        let Some(commit) = &self.commit else { return Ok(None) };
        let entry = self
            .git
            .must(&["ls-tree", "-z", commit, "--", path], &[], None)?;
        let text = String::from_utf8_lossy(&entry.stdout).into_owned();
        let Some(line) = text.split('\0').find(|l| !l.is_empty()) else {
            return Ok(None);
        };
        // `<mode> SP <type> SP <object> TAB <path>`
        let (meta, _) = line.split_once('\t').unwrap_or((line, ""));
        let mut parts = meta.split(' ');
        let (_mode, kind, object) = (parts.next(), parts.next(), parts.next());
        match (kind, object) {
            (Some("blob"), Some(object)) => {
                Ok(Some(
                    self.git
                        .must(&["cat-file", "blob", object], &[], None)?
                        .stdout,
                ))
            },
            _ => Ok(None),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let Some(commit) = &self.commit else { return Ok(Vec::new()) };
        let out = self
            .git
            .must(&["ls-tree", "-r", "-z", "--name-only", commit], &[], None)?;
        // a store names its files as text; a lossy name would name no file and read as absent
        let mut paths = Vec::new();
        for raw in out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            let path = std::str::from_utf8(raw).map_err(|_| {
                StoreError::InvalidPath {
                    path: String::from_utf8_lossy(raw).into_owned(),
                    why:  "is not text, which a store cannot name",
                }
            })?;
            if under(path, prefix) {
                paths.push(path.to_owned());
            }
        }
        paths.sort();
        Ok(paths)
    }
}
