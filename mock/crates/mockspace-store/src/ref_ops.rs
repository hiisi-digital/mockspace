//! The git steps of the ref backend: fetch, ask the remote, build a commit, check a push.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::commit_reader::CommitReader;
use crate::edit::Edit;
use crate::error::StoreError;
use crate::ref_store::RefStore;
use crate::store::Freshness;

const ZERO_OID: &str = "0000000000000000000000000000000000000000";
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl RefStore {
    pub(crate) fn reader_at(&self, commit: Option<String>) -> CommitReader {
        CommitReader::new(self.repo.clone(), commit)
    }

    pub(crate) fn fetch_error(&self, detail: String) -> StoreError {
        StoreError::Fetch {
            remote: self.remote.clone(),
            refspec: self.pair.fetch_refspec(),
            detail,
        }
    }

    /// Whether the remote, which answered, holds the ref. Asked of `ls-remote`
    /// and compared by exact name; the words of a failed fetch are not parsed.
    fn remote_holds_ref(&self) -> Result<bool, String> {
        let out = self
            .git
            .run(&["ls-remote", &self.remote, self.pair.remote()], &[], None)
            .map_err(|e| e.to_string())?;
        if !out.ok() {
            return Err(out.stderr);
        }
        let want = self.pair.remote();
        Ok(out
            .text()
            .lines()
            .any(|l| l.split('\t').nth(1) == Some(want)))
    }

    /// Bring the remote's ref into the local one. The remote may only have moved
    /// forward: a tip that does not descend from the one held here is an error and
    /// the local ref is put back, so a rewritten remote is never read as the data.
    pub(crate) fn fetch_once(&self) -> Result<Freshness, StoreError> {
        let old = self.local_tip()?;
        let spec = self.pair.fetch_refspec();
        let depth = self.depth.map(|d| d.to_string());
        let mut args = vec!["fetch", "-q"];
        // Depth only for the first fetch. A later depth-limited fetch makes the new tip a
        // shallow root, and the old tip is then no longer reachable from it.
        if let (Some(d), None) = (&depth, &old) {
            args.extend(["--depth", d]);
        }
        args.extend([self.remote.as_str(), spec.as_str()]);
        let out = self.git.run(&args, &[], None)?;
        if out.ok() {
            let Some(new) = self.local_tip()? else {
                return Err(self.fetch_error("fetch reported success but left no local ref".into()));
            };
            if let Some(old) = &old {
                if *old != new && !self.descends(old, &new)? {
                    self.git
                        .must(&["update-ref", self.pair.local(), old], &[], None)?;
                    return Err(StoreError::RemoteRewrote {
                        remote_ref: self.pair.remote().to_owned(),
                        local_tip:  old.clone(),
                        remote_tip: new,
                    });
                }
            }
            return Ok(Freshness::Fetched {
                tip: new,
            });
        }
        // The fetch failed. Either the remote answered and has no such ref (a store
        // not yet written), or it did not answer. Only the remote can say which.
        match self.remote_holds_ref() {
            Ok(false) => {
                match old {
                    None => Ok(Freshness::Empty),
                    Some(local_tip) => {
                        Err(StoreError::RemoteLostRef {
                            remote_ref: self.pair.remote().to_owned(),
                            local_tip,
                        })
                    },
                }
            },
            Ok(true) => Err(self.fetch_error(out.stderr)),
            Err(ls) => Err(self.fetch_error(format!("{}; ls-remote: {ls}", out.stderr))),
        }
    }

    /// Whether `newer` has `older` in its history. A shallow store may lack the
    /// history to say, so what the local repository cannot answer is asked of the
    /// remote's whole tip.
    fn descends(&self, older: &str, newer: &str) -> Result<bool, StoreError> {
        let local = self
            .git
            .run(&["merge-base", "--is-ancestor", older, newer], &[], None)?;
        match local.code {
            Some(0) => Ok(true),
            _ => self.remote_holds(older),
        }
    }

    /// Whether the remote's tip has `commit` in its history. The tip is fetched into a
    /// scratch ref without the depth limit, so a shallow store, whose own ref is cut short,
    /// can still tell a commit that is held from one that is not.
    pub(crate) fn remote_holds(&self, commit: &str) -> Result<bool, StoreError> {
        let scratch = format!("refs/mockspace-store/probe-{}", unique());
        let spec = format!("+{}:{scratch}", self.pair.remote());
        let fetched = self
            .git
            .run(&["fetch", "-q", &self.remote, &spec], &[], None)?;
        if !fetched.ok() {
            return match self.remote_holds_ref() {
                Ok(false) => Ok(false),
                _ => Err(self.fetch_error(fetched.stderr)),
            };
        }
        let held = self.git.run(
            &["merge-base", "--is-ancestor", commit, &scratch],
            &[],
            None,
        );
        let _ = self.git.run(&["update-ref", "-d", &scratch], &[], None);
        match held?.code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => {
                Err(StoreError::Git {
                    command: format!("merge-base --is-ancestor {commit} {scratch}"),
                    detail:  "neither an answer nor a refusal".into(),
                })
            },
        }
    }

    /// After an accepted push: whether the remote is seen to hold the commit. A remote
    /// that cannot be reached for the check leaves it unconfirmed (`false`) and not an
    /// error, since the push itself was accepted under a lease. A remote that answers
    /// and does not hold the commit is an error.
    pub(crate) fn verify_pushed(&self, commit: &str) -> Result<bool, StoreError> {
        match self.remote_holds(commit) {
            Ok(true) => Ok(true),
            Ok(false) => {
                Err(StoreError::Verify {
                    remote_ref: self.pair.remote().to_owned(),
                    commit:     commit.to_owned(),
                    detail:     "the remote's tip does not have it in its history".into(),
                })
            },
            Err(StoreError::Fetch {
                ..
            }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Build one commit holding `edit` on `tip`. `None` when it changes nothing.
    pub(crate) fn build_commit(
        &self,
        tip: Option<&str>,
        edit: &Edit,
        message: &str,
    ) -> Result<Option<String>, StoreError> {
        let git_dir = self
            .git
            .must(&["rev-parse", "--absolute-git-dir"], &[], None)?
            .text();
        let index = PathBuf::from(git_dir).join(format!("mockspace-store-{}.idx", unique()));
        let _guard = RemoveOnDrop(index.clone());
        let index_s = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", index_s.as_str())];
        if let Some(tip) = tip {
            self.git.must(&["read-tree", tip], &env, None)?;
        }
        let mut lines = String::new();
        for (path, bytes) in edit.entries() {
            match bytes {
                Some(bytes) => {
                    let blob = self
                        .git
                        .must(&["hash-object", "-w", "--stdin"], &[], Some(bytes))?
                        .text();
                    lines.push_str(&format!("100644 {blob}\t{path}\0"));
                },
                None => lines.push_str(&format!("0 {ZERO_OID}\t{path}\0")),
            }
        }
        // NUL-terminated, so a path git would read as C-quoted (one that begins with `"`) is a path
        self.git.must(
            &["update-index", "-z", "--index-info"],
            &env,
            Some(lines.as_bytes()),
        )?;
        // git skips, and says nothing of, a path it will not store; every put must be in the index
        let held = self.git.must(&["ls-files", "-z"], &env, None)?;
        let held: std::collections::HashSet<&[u8]> = held.stdout.split(|b| *b == 0).collect();
        for (path, bytes) in edit.entries() {
            if bytes.is_some() && !held.contains(path.as_bytes()) {
                return Err(StoreError::InvalidPath {
                    path: path.to_owned(),
                    why:  "git would not store it",
                });
            }
        }
        let tree = self.git.must(&["write-tree"], &env, None)?.text();
        let tip_tree = match tip {
            Some(t) => {
                self.git
                    .must(&["rev-parse", &format!("{t}^{{tree}}")], &[], None)?
                    .text()
            },
            None => EMPTY_TREE.to_owned(),
        };
        if tree == tip_tree {
            return Ok(None);
        }
        let ident = [
            ("GIT_AUTHOR_NAME", self.identity.name.as_str()),
            ("GIT_AUTHOR_EMAIL", self.identity.email.as_str()),
            ("GIT_COMMITTER_NAME", self.identity.name.as_str()),
            ("GIT_COMMITTER_EMAIL", self.identity.email.as_str()),
        ];
        let mut args = vec!["commit-tree", tree.as_str()];
        if let Some(tip) = tip {
            args.extend(["-p", tip]);
        }
        args.extend(["-m", message]);
        Ok(Some(self.git.must(&args, &ident, None)?.text()))
    }
}
