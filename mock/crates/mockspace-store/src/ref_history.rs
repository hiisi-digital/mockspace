//! What a ref store can say about the commits already on its local ref.

use std::collections::BTreeMap;

use crate::error::StoreError;
use crate::reader::Reader;
use crate::ref_store::RefStore;

/// What follows a blob id in an id map when the file is executable (mode 100755). A plain file's
/// id is the bare blob id, so one comparison of two ids compares content and mode together.
pub(crate) const EXEC: &str = "+x";

/// The blob id and whether the file is executable, from an id of the maps `tree_ids` and
/// `local_ids` give.
pub(crate) fn split_id(id: &str) -> (&str, bool) {
    match id.strip_suffix(EXEC) {
        Some(blob) => (blob, true),
        None => (id, false),
    }
}

/// How many paths or ids go to one git process, so that neither pipe fills while the other is
/// being written.
pub(crate) const BATCH: usize = 200;

/// One commit of the local ref: its id and the time it was authored, in seconds since the epoch.
/// The author time is the one a rebase or a cherry-pick keeps, which is why a pairing records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub id:          String,
    pub author_time: i64,
}

impl RefStore {
    /// The files of `commit`, read without a checkout. `commit` is an object id, and anything
    /// that git would take for an option or a path is refused.
    pub fn reader_of(&self, commit: &str) -> Result<impl Reader, StoreError> {
        if commit.is_empty() || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StoreError::InvalidRef {
                name: commit.to_owned(),
                why:  "not an object id",
            });
        }
        Ok(self.reader_at(Some(commit.to_owned())))
    }

    /// Every commit reachable from the local ref, newest first, and none when there is no local
    /// ref. A shallow clone lists only the history it holds; see [`RefStore::is_shallow`].
    pub fn history(&self) -> Result<Vec<CommitInfo>, StoreError> {
        if self.local_tip()?.is_none() {
            return Ok(Vec::new());
        }
        self.history_at(self.pair.local())
    }

    /// Every commit reachable from `tip`, newest first.
    pub fn history_at(&self, tip: &str) -> Result<Vec<CommitInfo>, StoreError> {
        let out = self.git.must(
            &["log", "--format=%H %at", tip, "--"],
            &[],
            None,
        )?;
        let mut commits = Vec::new();
        for line in out.text().lines() {
            let parsed = line
                .split_once(' ')
                .and_then(|(id, at)| Some((id, at.parse::<i64>().ok()?)));
            let Some((id, author_time)) = parsed else {
                return Err(StoreError::Git {
                    command: "log --format=%H %at".to_owned(),
                    detail:  format!("a line that is not `<id> <seconds>`: {line:?}"),
                });
            };
            commits.push(CommitInfo {
                id: id.to_owned(),
                author_time,
            });
        }
        Ok(commits)
    }

    /// Whether the repository holds a cut history, so that a commit missing from
    /// [`RefStore::history`] may only be one that was not fetched.
    pub fn is_shallow(&self) -> Result<bool, StoreError> {
        let out = self
            .git
            .must(&["rev-parse", "--is-shallow-repository"], &[], None)?;
        Ok(out.text() == "true")
    }

    /// The blob id of each file of `commit`, by path. A tree entry that is not a regular file
    /// (a symbolic link, a submodule) or whose name is not text is refused, because each would
    /// be read as a file it is not, or as another file of the same lossy name.
    pub(crate) fn tree_ids(&self, commit: &str) -> Result<BTreeMap<String, String>, StoreError> {
        // the base of an instance of a ref not born yet holds no file
        if commit == crate::instance::UNBORN {
            return Ok(BTreeMap::new());
        }
        let out = self.git.must(&["ls-tree", "-r", "-z", commit], &[], None)?;
        let mut ids = BTreeMap::new();
        for entry in out.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            // `<mode> SP <type> SP <object> TAB <path>`
            let Some(tab) = entry.iter().position(|b| *b == b'\t') else {
                return Err(StoreError::Git {
                    command: "ls-tree -r -z".to_owned(),
                    detail:  format!(
                        "an entry without a path: {:?}",
                        String::from_utf8_lossy(entry)
                    ),
                });
            };
            let (meta, raw) = (String::from_utf8_lossy(&entry[.. tab]), &entry[tab + 1 ..]);
            let path = std::str::from_utf8(raw).map_err(|_| {
                StoreError::InvalidPath {
                    path: String::from_utf8_lossy(raw).into_owned(),
                    why:  "is not text",
                }
            })?;
            match meta.split(' ').collect::<Vec<_>>()[..] {
                ["100644", "blob", id] => {
                    ids.insert(path.to_owned(), id.to_owned());
                },
                ["100755", "blob", id] => {
                    ids.insert(path.to_owned(), format!("{id}{EXEC}"));
                },
                _ => {
                    return Err(StoreError::InvalidPath {
                        path: path.to_owned(),
                        why:  "is not a regular file (a symbolic link or a submodule)",
                    });
                },
            }
        }
        Ok(ids)
    }

    /// The bytes of each blob in `ids`, in the order given.
    pub(crate) fn blobs(&self, ids: &[&str]) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut input = String::new();
        for id in ids {
            input.push_str(split_id(id).0);
            input.push('\n');
        }
        let out = self
            .git
            .must(&["cat-file", "--batch"], &[], Some(input.as_bytes()))?;
        let (mut blobs, mut at) = (Vec::new(), 0usize);
        for id in ids {
            let bad = |why: &str| {
                StoreError::Git {
                    command: "cat-file --batch".to_owned(),
                    detail:  format!("{why} for {id}"),
                }
            };
            let rest = &out.stdout[at ..];
            let end = rest
                .iter()
                .position(|b| *b == b'\n')
                .ok_or_else(|| bad("no header"))?;
            let header = String::from_utf8_lossy(&rest[.. end]).into_owned();
            let size = match header.split(' ').collect::<Vec<_>>()[..] {
                [_, "blob", size] => size.parse::<usize>().map_err(|_| bad("a bad size"))?,
                _ => return Err(bad(&format!("`{header}`"))),
            };
            let body = rest
                .get(end + 1 .. end + 1 + size)
                .ok_or_else(|| bad("a short body"))?;
            blobs.push(body.to_vec());
            at += end + 1 + size + 1;
        }
        Ok(blobs)
    }

    /// Every file of `commit` with its bytes, by path, in one pass of a few git processes rather
    /// than two for each file. A tree too large to hold in memory is not what this is for.
    pub fn files_of(&self, commit: &str) -> Result<BTreeMap<String, Vec<u8>>, StoreError> {
        let _checked = self.reader_of(commit)?;
        let ids = self.tree_ids(commit)?;
        let paths: Vec<&String> = ids.keys().collect();
        let mut files = BTreeMap::new();
        for chunk in paths.chunks(BATCH) {
            let wanted: Vec<&str> = chunk.iter().map(|p| ids[*p].as_str()).collect();
            for (path, bytes) in chunk.iter().zip(self.blobs(&wanted)?) {
                files.insert((*path).clone(), bytes);
            }
        }
        Ok(files)
    }
}
