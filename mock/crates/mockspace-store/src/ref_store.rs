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
//!
//! Each store remembers the tip it last saw (a refresh, or its own write), reads from
//! that commit, and takes it as the base of its next write. Another store on the same
//! clone moving the local ref does not change what this one read.

use std::path::PathBuf;
use std::sync::Mutex;

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

/// What this store knows of the remote.
pub(crate) enum Seen {
    /// Nothing yet: no refresh and no write.
    Unknown,
    /// The remote was asked and has no such ref.
    Empty,
    /// The commit this store last saw at the tip.
    Tip(String),
}

/// A namespace on a remote's ref, mirrored in a private local ref of `repo`.
pub struct RefStore {
    pub(crate) repo:     PathBuf,
    pub(crate) git:      Git,
    pub(crate) remote:   String,
    pub(crate) pair:     RefPair,
    pub(crate) identity: Identity,
    pub(crate) retry:    RetryPolicy,
    pub(crate) depth:    Option<u32>,
    seen:                Mutex<Seen>,
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
            seen: Mutex::new(Seen::Unknown),
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

    /// The commit at the local ref, if there is one. Shared by every store on the clone,
    /// so it is not what this store's reads saw; `read` and `list` use the tip this store
    /// last refreshed to or wrote.
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

    fn set_seen(&self, seen: Seen) {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = seen;
    }

    fn seen_tip(&self) -> Option<String> {
        match &*self.seen.lock().unwrap_or_else(|e| e.into_inner()) {
            Seen::Tip(t) => Some(t.clone()),
            _ => None,
        }
    }

    /// The reader for what this store last saw. A store that has not heard from the
    /// remote does not know whether the namespace is empty, and says so.
    fn committed(&self) -> Result<CommitReader, StoreError> {
        match &*self.seen.lock().unwrap_or_else(|e| e.into_inner()) {
            Seen::Tip(t) => Ok(self.reader_at(Some(t.clone()))),
            Seen::Empty => Ok(self.reader_at(None)),
            Seen::Unknown => {
                Err(StoreError::NotFetched {
                    local_ref: self.pair.local().to_owned(),
                })
            },
        }
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

impl Store for RefStore {
    fn refresh(&self) -> Result<Freshness, StoreError> {
        let fresh = self.fetch_once()?;
        match &fresh {
            Freshness::Fetched {
                tip,
            } => self.set_seen(Seen::Tip(tip.clone())),
            Freshness::Empty => self.set_seen(Seen::Empty),
            Freshness::Local => {},
        }
        Ok(fresh)
    }

    fn apply(
        &self,
        edit: &Edit,
        message: &str,
        hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError> {
        self.apply_from(self.seen_tip().as_deref(), edit, message, hook)
    }
}

impl RefStore {
    /// [`Store::apply`] for an edit built on `built_on`, the commit its author read the files
    /// from, or `None` for one built on a ref with no commits. Any other tip on the remote,
    /// whatever this store has read since, sends the edit through the hook, so a writer whose
    /// files are older than the tip cannot write over a file the tip changed. `apply` is this
    /// with the tip the store last saw.
    pub fn apply_from(
        &self,
        built_on: Option<&str>,
        edit: &Edit,
        message: &str,
        hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError> {
        edit.validate()?;
        let attempts = self.retry.attempts.max(1);
        let mut current = edit.clone();
        // Any tip other than this one, after the refresh or after a lost race, sends the edit
        // through the hook.
        let mut built_on: Option<String> = built_on.map(str::to_owned);
        let mut refreshed = false;
        // commits pushed whose outcome the remote could not be asked about
        let mut unconfirmed: Vec<String> = Vec::new();
        // the words of the last push that failed without a race, for the end of the attempts
        let mut stuck: Option<String> = None;
        for attempt in 1 ..= attempts {
            let is_last = attempt == attempts;
            if !refreshed {
                match self.fetch_once() {
                    Ok(_) => {},
                    Err(
                        e @ StoreError::Fetch {
                            ..
                        },
                    ) if is_last && unconfirmed.is_empty() => return Err(e),
                    Err(StoreError::Fetch {
                        detail,
                        ..
                    }) if is_last => {
                        return Err(StoreError::OutcomeUnknown {
                            remote_ref: self.pair.remote().to_owned(),
                            commits: unconfirmed,
                            detail,
                        });
                    },
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
            for commit in &unconfirmed {
                if self.remote_holds(commit)? {
                    return Ok(self.landed(commit.clone(), attempt, true));
                }
            }
            let tip = self.local_tip()?;
            if built_on != tip {
                current = hook.rebase(&self.reader_at(tip.clone()), edit)?;
                current.validate()?;
            }
            built_on = tip.clone();
            current.check_against(&self.reader_at(tip.clone()))?;
            let Some(commit) = self.build_commit(tip.as_deref(), &current, message)? else {
                if let Some(t) = &tip {
                    self.set_seen(Seen::Tip(t.clone()));
                }
                return Ok(Applied {
                    commit:    tip,
                    changed:   false,
                    attempts:  attempt,
                    confirmed: true,
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
                let confirmed = self.verify_pushed(&commit)?;
                return Ok(self.landed(commit, attempt, confirmed));
            }
            // The push reported a failure. What the remote holds says what happened: our
            // commit already there (accepted, then the connection dropped), a tip that moved
            // (lost), or the tip we built on (refused, or a transient fault).
            match self.fetch_once() {
                Ok(_) => {
                    refreshed = true;
                    if self.remote_holds(&commit)? {
                        return Ok(self.landed(commit, attempt, true));
                    }
                    if self.local_tip()? != tip {
                        stuck = None;
                    } else if transient_push_output(&out.stderr) {
                        stuck = Some(out.stderr);
                    } else {
                        return Err(StoreError::PushRefused {
                            remote:     self.remote.clone(),
                            remote_ref: self.pair.remote().to_owned(),
                            detail:     out.stderr,
                        });
                    }
                },
                Err(StoreError::Fetch {
                    detail,
                    ..
                }) if is_last => {
                    unconfirmed.push(commit);
                    return Err(StoreError::OutcomeUnknown {
                        remote_ref: self.pair.remote().to_owned(),
                        commits: unconfirmed,
                        detail,
                    });
                },
                Err(StoreError::Fetch {
                    ..
                }) => unconfirmed.push(commit),
                Err(e) => return Err(e),
            }
            if !is_last {
                self.retry.sleep_after(attempt);
            }
        }
        match stuck {
            Some(detail) => {
                Err(StoreError::PushRefused {
                    remote: self.remote.clone(),
                    remote_ref: self.pair.remote().to_owned(),
                    detail,
                })
            },
            None => {
                Err(StoreError::RaceLost {
                    attempts,
                })
            },
        }
    }
}

impl RefStore {
    fn landed(&self, commit: String, attempts: u32, confirmed: bool) -> Applied {
        self.set_seen(Seen::Tip(commit.clone()));
        Applied {
            commit: Some(commit),
            changed: true,
            attempts,
            confirmed,
        }
    }
}
