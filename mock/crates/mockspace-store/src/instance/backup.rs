//! A copy of the files edited here and not yet committed, kept in a ref of the repository's own.
//!
//! An instance's files are hidden from the branch, and git treats a hidden file as expendable: a
//! merge or a rebase of a commit from before the migration writes its bytes over an edit and keeps
//! no copy. So the edited files are copied into `refs/mock-backup/<namespace>`, one commit for each
//! distinct state, whenever the engine looks at the instance and from a hook that git runs before
//! it moves `ORIG_HEAD` (which a merge, a rebase and a reset do ahead of writing the work tree).
//! The ref is local and never pushed. The files of an edit are read back with
//! `git show refs/mock-backup/<namespace>:<path>`.
//!
//! What this does not cover is a write git makes with no reference moving before it and no engine
//! command since the edit: `git cherry-pick` of a commit from before the migration is one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Delta, Instance};
use crate::error::StoreError;

const ZERO_OID: &str = "0000000000000000000000000000000000000000";

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Instance<'_> {
    /// The ref the copies of this instance's edited files are kept in.
    pub fn backup_ref(&self) -> String {
        let local = self.store.pair().local();
        let name = local.strip_prefix("refs/mock/").unwrap_or_else(|| local.trim_start_matches("refs/"));
        format!("refs/mock-backup/{name}")
    }

    /// Copies the files added or changed here, against the base, into the backup ref, as a commit
    /// whose tree holds those files and whose parent is the last copy. Gives the commit, or `None`
    /// where nothing is edited, where no instance was made, or where the last copy holds exactly
    /// these files already.
    pub fn snapshot(&self) -> Result<Option<String>, StoreError> {
        if self.base()?.is_none() || !self.dir.is_dir() {
            return Ok(None);
        }
        let delta = self.delta()?;
        self.snapshot_of(&delta)
    }

    pub(super) fn snapshot_of(&self, delta: &Delta) -> Result<Option<String>, StoreError> {
        let put: Vec<(&String, &String)> =
            delta.sent.iter().filter_map(|(path, id)| id.as_ref().map(|id| (path, id))).collect();
        if put.is_empty() {
            return Ok(None);
        }
        let git = &self.store.git;
        let mut input = String::new();
        for (path, _) in &put {
            input.push_str(&format!("{}\n", self.dir.join(path.as_str()).display()));
        }
        // written to the object database now, since the ids of the delta were only computed
        let written = git.must(&["hash-object", "-w", "--no-filters", "--stdin-paths"], &[], Some(input.as_bytes()))?;
        let text = written.text();
        let blobs: Vec<&str> = text.lines().collect();
        if blobs.len() != put.len() {
            return Err(StoreError::Git {
                command: "hash-object -w --stdin-paths".to_owned(),
                detail:  format!("{} ids for {} paths", blobs.len(), put.len()),
            });
        }
        let mut lines = String::new();
        for ((path, id), blob) in put.iter().zip(blobs) {
            let mode = if crate::ref_history::split_id(id).1 { "100755" } else { "100644" };
            lines.push_str(&format!("{mode} {blob}\t{path}\0"));
        }
        let git_dir = git.must(&["rev-parse", "--absolute-git-dir"], &[], None)?.text();
        let index = PathBuf::from(git_dir).join(format!(
            "mockspace-backup-{}-{}.idx",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _guard = RemoveOnDrop(index.clone());
        let index_s = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", index_s.as_str())];
        git.must(&["update-index", "-z", "--add", "--index-info"], &env, Some(lines.as_bytes()))?;
        let tree = git.must(&["write-tree"], &env, None)?.text();
        let reference = self.backup_ref();
        let last = git.run(&["rev-parse", "--verify", "-q", &format!("{reference}^{{commit}}")], &[], None)?;
        let last = last.ok().then(|| last.text());
        if let Some(last) = &last {
            let held = git.must(&["rev-parse", &format!("{last}^{{tree}}")], &[], None)?.text();
            if held == tree {
                return Ok(None);
            }
        }
        let who = &self.store.identity;
        let ident = [
            ("GIT_AUTHOR_NAME", who.name.as_str()),
            ("GIT_AUTHOR_EMAIL", who.email.as_str()),
            ("GIT_COMMITTER_NAME", who.name.as_str()),
            ("GIT_COMMITTER_EMAIL", who.email.as_str()),
        ];
        let message = format!("backup of the edited files of {}", self.dir.display());
        let mut args = vec!["commit-tree", tree.as_str()];
        if let Some(last) = &last {
            args.extend(["-p", last.as_str()]);
        }
        args.extend(["-m", message.as_str()]);
        let commit = git.must(&args, &ident, None)?.text();
        git.must(&["update-ref", &reference, &commit, last.as_deref().unwrap_or(ZERO_OID)], &[], None)?;
        Ok(Some(commit))
    }
}
