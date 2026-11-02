//! The ref backend: a namespace kept on a git ref, written with history,
//! fetched and pushed by explicit refspec with a lease.
//!
//! The model is `tools/msg/lib/store.sh` of clause-dev. Nothing is checked out.
//! The remote's ref is fetched into the private local ref of the pair, read with
//! `cat-file` and `ls-tree`, and written by `hash-object`, a temporary index and
//! `commit-tree`. The push carries `--force-with-lease` on the tip the commit was
//! built on. A push that loses is answered by fetching again and asking the
//! merge hook what the edit is on the tip that won; whether the push lost is
//! decided by what the remote holds afterwards. Git's output is read for one thing only:
//! a push that changed nothing on the remote is retried when its words name a dropped
//! connection or a locked ref, and is final otherwise.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::commit_reader::CommitReader;
use crate::edit::Edit;
use crate::error::StoreError;
use crate::git::Git;
use crate::identity::Identity;
use crate::merge::MergeHook;
use crate::reader::Reader;
use crate::refs::RefPair;
use crate::retry::{RetryPolicy, transient_push_output};
use crate::store::{Applied, Freshness, Store};

const ZERO_OID: &str = "0000000000000000000000000000000000000000";
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
static INDEX_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A namespace on a remote's ref, mirrored in a private local ref of `repo`.
pub struct RefStore {
    repo:            PathBuf,
    git:             Git,
    remote:          String,
    pair:            RefPair,
    identity:        Identity,
    retry:           RetryPolicy,
    depth:           Option<u32>,
    /// Set by a fetch that asked the remote and was told it has no such ref.
    confirmed_empty: AtomicBool,
}

impl RefStore {
    /// `repo` is any directory of the repository (a work tree or a bare one),
    /// `remote` a remote name or a URL.
    pub fn new(repo: impl Into<PathBuf>, remote: impl Into<String>, pair: RefPair) -> Self {
        let repo = repo.into();
        Self {
            git: Git::new(repo.clone()),
            repo,
            remote: remote.into(),
            pair,
            identity: Identity::default(),
            retry: RetryPolicy::default(),
            depth: None,
            confirmed_empty: AtomicBool::new(false),
        }
    }

    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Fetch at most `depth` commits of history, for a shallow clone.
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = Some(depth);
        self
    }

    /// Run this program in place of `git` for fetch, push and writes: a wrapper, or a git kept elsewhere.
    pub fn with_git_program(mut self, program: impl Into<std::ffi::OsString>) -> Self {
        self.git = self.git.with_program(program);
        self
    }

    pub fn pair(&self) -> &RefPair {
        &self.pair
    }

    /// The commit at the local ref, if there is one.
    pub fn local_tip(&self) -> Result<Option<String>, StoreError> {
        let spec = format!("{}^{{commit}}", self.pair.local());
        let out = self
            .git
            .run(&["rev-parse", "-q", "--verify", &spec], &[], None)?;
        match (out.code, out.stderr.is_empty()) {
            (Some(0), _) => Ok(Some(out.text())),
            (Some(1), true) => Ok(None),
            _ => {
                Err(StoreError::Git {
                    command: format!("rev-parse --verify {spec}"),
                    detail:  out.stderr,
                })
            },
        }
    }

    fn reader_at(&self, commit: Option<String>) -> CommitReader {
        CommitReader::new(self.repo.clone(), commit)
    }

    fn fetch_error(&self, detail: String) -> StoreError {
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

    /// Run `git -C repo remote-add`-free fetch: remote and refspec are arguments.
    fn fetch_once(&self) -> Result<Freshness, StoreError> {
        let spec = self.pair.fetch_refspec();
        let depth = self.depth.map(|d| d.to_string());
        let mut args = vec!["fetch", "-q"];
        if let Some(d) = &depth {
            args.extend(["--depth", d]);
        }
        args.extend([self.remote.as_str(), spec.as_str()]);
        let out = self.git.run(&args, &[], None)?;
        if out.ok() {
            self.confirmed_empty.store(false, Ordering::SeqCst);
            return match self.local_tip()? {
                Some(tip) => {
                    Ok(Freshness::Fetched {
                        tip,
                    })
                },
                None => {
                    Err(self.fetch_error("fetch reported success but left no local ref".into()))
                },
            };
        }
        // The fetch failed. Either the remote answered and has no such ref (a store
        // not yet written), or it did not answer. Only the remote can say which.
        match self.remote_holds_ref() {
            Ok(false) => {
                match self.local_tip()? {
                    None => {
                        self.confirmed_empty.store(true, Ordering::SeqCst);
                        Ok(Freshness::Empty)
                    },
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

    /// Build one commit holding `edit` on `tip`. `None` when it changes nothing.
    fn build_commit(
        &self,
        tip: Option<&str>,
        edit: &Edit,
        message: &str,
    ) -> Result<Option<String>, StoreError> {
        let git_dir = self
            .git
            .must(&["rev-parse", "--absolute-git-dir"], &[], None)?
            .text();
        let index = PathBuf::from(git_dir).join(format!(
            "mockspace-store-{}-{}.idx",
            std::process::id(),
            INDEX_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
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
                    lines.push_str(&format!("100644 {blob}\t{path}\n"));
                },
                None => lines.push_str(&format!("0 {ZERO_OID}\t{path}\n")),
            }
        }
        self.git.must(
            &["update-index", "--index-info"],
            &env,
            Some(lines.as_bytes()),
        )?;
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

    /// Whether the remote's tip has `commit` in its history. The tip is fetched into a
    /// scratch ref without the depth limit, so a shallow store, whose own ref is cut short,
    /// can still tell a commit that is held from one that is not.
    fn remote_holds(&self, commit: &str) -> Result<bool, StoreError> {
        let scratch = format!(
            "refs/mockspace-store/probe-{}-{}",
            std::process::id(),
            INDEX_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
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

    /// After an accepted push: the remote must now hold the commit.
    fn verify_pushed(&self, commit: &str) -> Result<(), StoreError> {
        let verify = |detail: String| {
            StoreError::Verify {
                remote_ref: self.pair.remote().to_owned(),
                commit: commit.to_owned(),
                detail,
            }
        };
        match self.remote_holds(commit) {
            Ok(true) => Ok(()),
            Ok(false) => {
                Err(verify(
                    "the remote's tip does not have it in its history".into(),
                ))
            },
            Err(e) => Err(verify(e.to_string())),
        }
    }
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Reader for RefStore {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.committed()?.read(path)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        self.committed()?.list(prefix)
    }
}

impl RefStore {
    /// The reader for the local tip. A missing ref is an empty store only when a
    /// fetch has asked the remote and heard that it has none.
    fn committed(&self) -> Result<CommitReader, StoreError> {
        match self.local_tip()? {
            Some(tip) => Ok(self.reader_at(Some(tip))),
            None if self.confirmed_empty.load(Ordering::SeqCst) => Ok(self.reader_at(None)),
            None => {
                Err(StoreError::NotFetched {
                    local_ref: self.pair.local().to_owned(),
                })
            },
        }
    }
}

impl Store for RefStore {
    fn refresh(&self) -> Result<Freshness, StoreError> {
        self.fetch_once()
    }

    fn apply(
        &self,
        edit: &Edit,
        message: &str,
        hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError> {
        edit.validate()?;
        let attempts = self.retry.attempts.max(1);
        let mut current = edit.clone();
        // The tip the edit was computed on: the local ref as it stands now, which is what the
        // caller's reads saw. Any other tip, after the refresh or after a lost race, sends
        // the edit through the hook.
        let mut built_on: Option<String> = self.local_tip()?;
        let mut refreshed = false;
        for attempt in 1 ..= attempts {
            let is_last = attempt == attempts;
            if !refreshed {
                match self.fetch_once() {
                    Ok(_) => {},
                    Err(
                        e @ StoreError::Fetch {
                            ..
                        },
                    ) if is_last => return Err(e),
                    Err(StoreError::Fetch {
                        ..
                    }) => {
                        self.retry.sleep_after(attempt);
                        continue;
                    },
                    Err(e) => return Err(e),
                }
            }
            refreshed = false;
            let tip = self.local_tip()?;
            if built_on != tip {
                current = hook.rebase(&self.reader_at(tip.clone()), edit)?;
                current.validate()?;
            }
            built_on = tip.clone();
            current.check_against(&self.reader_at(tip.clone()))?;
            let Some(commit) = self.build_commit(tip.as_deref(), &current, message)? else {
                return Ok(Applied {
                    commit:   tip,
                    changed:  false,
                    attempts: attempt,
                });
            };
            let lease = format!(
                "--force-with-lease={}:{}",
                self.pair.remote(),
                tip.as_deref().unwrap_or("")
            );
            let refspec = format!("{commit}:{}", self.pair.remote());
            let out = self
                .git
                .run(&["push", "-q", &lease, &self.remote, &refspec], &[], None)?;
            if out.ok() {
                self.git
                    .must(&["update-ref", self.pair.local(), &commit], &[], None)?;
                self.verify_pushed(&commit)?;
                return Ok(Applied {
                    commit:   Some(commit),
                    changed:  true,
                    attempts: attempt,
                });
            }
            // The push reported a failure. What the remote holds says what happened: our
            // commit already there (accepted, then the connection dropped), a tip that moved
            // (lost), or the tip we built on (refused, or a transient fault).
            match self.fetch_once() {
                Ok(_) => {
                    refreshed = true;
                    if self.remote_holds(&commit)? {
                        return Ok(Applied {
                            commit:   Some(commit),
                            changed:  true,
                            attempts: attempt,
                        });
                    }
                    let lost = self.local_tip()? != tip;
                    if !lost && !transient_push_output(&out.stderr) {
                        return Err(StoreError::PushRefused {
                            remote:     self.remote.clone(),
                            remote_ref: self.pair.remote().to_owned(),
                            detail:     out.stderr,
                        });
                    }
                },
                Err(
                    e @ StoreError::Fetch {
                        ..
                    },
                ) if is_last => return Err(e),
                Err(StoreError::Fetch {
                    ..
                }) => {},
                Err(e) => return Err(e),
            }
            if !is_last {
                self.retry.sleep_after(attempt);
            }
        }
        Err(StoreError::RaceLost {
            attempts,
        })
    }
}
