//! `RepoHandle`: a thin newtype over [`gix::Repository`] scoped to
//! mockspace's local-ref operations.
//!
//! Slice E1 of the Phase 5 IO plan (see
//! `mock/research/202605220843_phase-5-io-slice-plan.md`). The handle
//! is the entry point that subsequent slices (ref reader, ref writer,
//! flock, anchor capture) compose against. This slice ships only the
//! constructor and the underlying `gix::Repository` accessor; richer
//! operations land in their own slices so each can be reviewed and
//! tested in isolation.
//!
//! Discovery follows gix's own walk-up-from-cwd behaviour, which
//! matches spec §20 "Discovery and root resolution": stop at the
//! nearest `.git` directory or at a filesystem boundary.

use std::path::{Path, PathBuf};

/// A handle on the workspace's git repository.
///
/// Opening succeeds for any git working tree or bare repo; mockspace
/// only requires that `refs/mock/*` writes work, which they do on any
/// non-shallow repo.
#[derive(Debug)]
pub struct RepoHandle {
    repo: gix::Repository,
}

/// Failure modes for [`RepoHandle::open`] and friends.
#[derive(Debug)]
pub enum RepoError {
    /// `gix::discover` could not locate a `.git` directory walking up
    /// from the given path. The path may be outside any repo, or the
    /// caller may be running from a tempdir without `git init`.
    NotFound {
        searched_from: PathBuf,
    },
    /// `gix::open` / `gix::discover` failed for a non-not-found reason
    /// (permissions, malformed repo, etc.).
    Open(Box<gix::discover::Error>),
}

impl core::fmt::Display for RepoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound {
                searched_from,
            } => {
                write!(
                    f,
                    "no git repository found walking up from `{}`",
                    searched_from.display()
                )
            },
            Self::Open(e) => write!(f, "failed to open repository: {e}"),
        }
    }
}

impl std::error::Error for RepoError {}

impl RepoHandle {
    /// Discover a git repository starting from `path` and walking up.
    ///
    /// Matches spec §20: discovery stops at the nearest `.git`
    /// directory or at a filesystem boundary. `path` may be the
    /// workspace root, a subdirectory, or anywhere under the repo.
    pub fn open(path: &Path) -> Result<Self, RepoError> {
        match gix::discover(path) {
            Ok(repo) => {
                Ok(Self {
                    repo,
                })
            },
            Err(e) => {
                // gix::discover::Error does not expose a structural
                // "not found" discriminant; match on the Display
                // surface, which carries "could not find a git
                // repository" for the discovery-failure case.
                let msg = e.to_string();
                if msg.contains("Could not find a git repository")
                    || msg.contains("did not find a git repository")
                {
                    Err(RepoError::NotFound {
                        searched_from: path.to_path_buf(),
                    })
                } else {
                    Err(RepoError::Open(Box::new(e)))
                }
            },
        }
    }

    /// Access the underlying `gix::Repository`. Crate-visible so
    /// later slices in the same module hierarchy can call gix APIs
    /// the `RepoHandle` does not yet wrap. Crate-internal so the
    /// gix dep does not leak through `mockspace-core`'s public API;
    /// external consumers go through `RepoHandle`'s wrappers.
    pub(crate) fn repo(&self) -> &gix::Repository {
        &self.repo
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn init_repo(dir: &Path) {
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .status()
            .expect("git init should run");
        assert!(status.success(), "git init failed");
    }

    #[test]
    fn open_succeeds_for_a_fresh_git_init() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());

        let handle = RepoHandle::open(dir.path()).expect("open should succeed");
        // Round-trip via the accessor: the repo's git_dir resolves
        // under the tempdir we just initialised.
        let git_dir = handle.repo().git_dir().to_path_buf();
        let dir_canonical = std::fs::canonicalize(dir.path()).expect("canonicalize tempdir");
        let git_dir_canonical = std::fs::canonicalize(&git_dir).expect("canonicalize .git");
        assert!(
            git_dir_canonical.starts_with(&dir_canonical),
            "expected git_dir `{}` under tempdir `{}`",
            git_dir_canonical.display(),
            dir_canonical.display()
        );
    }

    #[test]
    fn open_discovers_from_a_subdirectory() {
        // Per spec §20 walk-up discovery: an inner subdirectory still
        // resolves the repo at the parent.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let inner = dir.path().join("crates").join("inner");
        std::fs::create_dir_all(&inner).unwrap();

        let handle = RepoHandle::open(&inner).expect("open from inner should succeed");
        let git_dir = handle.repo().git_dir().to_path_buf();
        let dir_canonical = std::fs::canonicalize(dir.path()).expect("canonicalize tempdir");
        let git_dir_canonical = std::fs::canonicalize(&git_dir).expect("canonicalize .git");
        assert!(git_dir_canonical.starts_with(&dir_canonical));
    }

    #[test]
    fn open_returns_not_found_for_a_path_outside_any_repo() {
        // A bare tempdir with no git init at it or any ancestor up
        // to the tempfile mount point. gix discovery walks up to the
        // mount point and stops without finding a `.git`.
        let dir = TempDir::new().unwrap();
        // Sanity: if the test host happens to have a `.git` at /tmp
        // or similar (rare, but conceivable on CI runners), this
        // test cannot pass. The assertion below tolerates that case
        // by accepting either NotFound (the expected branch on a
        // clean runner) or skipping (if a parent repo exists).
        match RepoHandle::open(dir.path()) {
            Err(RepoError::NotFound {
                ..
            }) => {},
            Ok(_) => {
                // Host has a parent .git that swallowed the test
                // path. Skip rather than fail; the NotFound branch
                // is exercised on any normal dev/CI environment.
                eprintln!("skipped: tempdir was inside a parent git repo");
            },
            Err(other) => panic!("expected NotFound, got {other:?}"),
        }
    }
}
