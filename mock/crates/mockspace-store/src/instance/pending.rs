//! A copy made before git writes that the backup ref could not take, kept beside the record.
//!
//! The copy before git writes is made from git's own `reference-transaction` hook, in its
//! `prepared` state, while the transaction that moves `ORIG_HEAD` holds its locks. The files
//! backend locks each ref apart, so the backup ref is free then. The reftable backend locks the
//! whole stack the transaction writes to (`tables.list.lock`, taken in `transaction_prepare` and
//! let go only once the transaction is committed, after the hook), and the backup ref is in the
//! stack of the repository, which holds `ORIG_HEAD` too for the main worktree: a write to it from
//! the hook waits `reftable.lockTimeout` (100 ms by default) and fails, every time.
//!
//! So the copy is made as objects, which need no lock, and the commit that holds it is named in a
//! file beside the instance's record. The next look takes that commit for the last copy of this
//! instance and folds it into the backup ref, as a parent of the next commit there, so its history
//! keeps it. Under the reftable backend a guard is kept this way straight away, since trying the
//! ref first costs the person's git command the lock timeout for each attempt; under the files
//! backend it is kept this way only where every attempt at the ref failed.
//!
//! The objects are named by nothing git reads until then, so a `git gc` that prunes them before the
//! next engine command would lose the copy: git prunes only an object older than
//! `gc.pruneExpire`, two weeks by default.

use std::path::PathBuf;

use super::Instance;
use crate::error::StoreError;
use crate::ids::Oid;

impl Instance<'_> {
    fn pending_file(&self) -> Result<PathBuf, StoreError> {
        Ok(self.record_file()?.with_extension("guard"))
    }

    /// The commit of a copy the backup ref has not taken yet, where there is one. A file naming
    /// an object the database does not hold is an error, since that copy is then lost.
    pub(super) fn pending(&self) -> Result<Option<Oid>, StoreError> {
        match self.pending_named()? {
            None => Ok(None),
            Some((_, commit, true)) => Ok(Some(commit)),
            Some((file, _, false)) => {
                Err(self.fail(
                    "names a copy whose commit the repository no longer holds (a `git gc` pruned it before an engine \
                     command took it)",
                    vec![file.display().to_string()],
                ))
            },
        }
    }

    /// Whether the file beside the record names a copy whose commit the repository no longer holds.
    pub fn pending_is_lost(&self) -> Result<bool, StoreError> {
        Ok(matches!(self.pending_named()?, Some((_, _, false))))
    }

    /// The file beside the record, the commit it names, and whether the repository holds it.
    fn pending_named(&self) -> Result<Option<(PathBuf, Oid, bool)>, StoreError> {
        let file = self.pending_file()?;
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.io("reading the copy kept beside the record of", e)),
        };
        let commit = Oid::parse(text.trim())?;
        let out = self.store.git.run(&["cat-file", "-e", commit.as_str()], &[], None)?;
        // `cat-file -e` says a missing object by exit 1 with nothing on stderr; anything else is git
        // failing, which is no answer that the copy is gone
        let held = match (out.code, out.stderr.is_empty()) {
            (Some(0), _) => true,
            (Some(1), true) => false,
            _ => {
                return Err(StoreError::Git {
                    command: format!("cat-file -e {commit}"),
                    detail:  out.stderr,
                });
            },
        };
        Ok(Some((file, commit, held)))
    }

    /// Names `commit` as the copy the backup ref has not taken yet, replacing any earlier one,
    /// which is a parent of it.
    pub(super) fn keep_pending(&self, commit: &Oid) -> Result<(), StoreError> {
        let file = self.pending_file()?;
        let staged = file.with_extension(format!("guard.{}.tmp", std::process::id()));
        let done = std::fs::write(&staged, format!("{commit}\n")).and_then(|()| std::fs::rename(&staged, &file));
        if done.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        done.map_err(|e| self.io("keeping the copy beside the record of", e))
    }

    /// Forgets the copy `commit` once the backup ref holds it, unless a later one replaced it.
    pub(super) fn drop_pending(&self, commit: &Oid) {
        let Ok(file) = self.pending_file() else { return };
        if std::fs::read_to_string(&file).is_ok_and(|t| t.trim() == commit.as_str()) {
            let _ = std::fs::remove_file(&file);
        }
    }

    /// Whether the repository keeps its refs in the reftable backend.
    pub(super) fn refs_in_reftable(&self) -> bool {
        self.store
            .git
            .run(&["config", "--get", "extensions.refStorage"], &[], None)
            .is_ok_and(|o| o.ok() && o.text().eq_ignore_ascii_case("reftable"))
    }
}
