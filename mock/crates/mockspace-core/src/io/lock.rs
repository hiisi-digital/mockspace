//! Filesystem-backed implementation of the transition lock at
//! `.git/mockspace/.lock`.
//!
//! Slice E4 of the Phase 5 IO plan. Wraps the platform's advisory
//! file lock (BSD `flock(2)` on POSIX, `LockFileEx` on Windows) via
//! `fs2`. RAII: acquire returns the guard; Drop releases.
//!
//! The lock-file body carries the holder debug payload (hostname +
//! PID + acquired-at timestamp) so a collision diagnostic can name
//! who currently holds the lock. The kernel manages the exclusion;
//! userspace just writes the payload.
//!
//! The contract surface lives in [`crate::atomicity::TransitionLock`].
//! That trait's `acquire()` constructor takes no path argument,
//! which does not fit a workspace-root-aware impl cleanly; this
//! module ships the concrete locking primitive with an inherent
//! constructor that takes the workspace root, and leaves the trait
//! integration as a follow-up (the trait can be reshaped to take a
//! path, or we extend it with a `new`-style associated fn). The
//! load-bearing piece is the lock itself; the trait dispatch shape
//! is configuration.
//!
//! See spec §24 "flock semantics and filesystem caveats" for the
//! list of unsupported substrates (NFS, sshfs, cloud-sync paths).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::atomicity::LockHolder;
use crate::io::time::current_iso8601;

/// Failure modes for [`FlockTransitionLock::acquire`].
#[derive(Debug)]
pub enum LockError {
    /// The repository's `.git/` directory was not found relative to
    /// the given workspace root. `acquire` only succeeds inside a
    /// real git repository.
    GitDirMissing {
        workspace_root: PathBuf,
    },
    /// `try_lock_exclusive` returned `WouldBlock` and the caller
    /// requested non-blocking mode. Carries the previous holder's
    /// payload if it could be read from the lock file body.
    AlreadyHeld {
        previous: Option<LockHolder>,
    },
    /// A filesystem error occurred opening or writing the lock file.
    Io {
        during: &'static str,
        path:   PathBuf,
        error:  io::Error,
    },
}

impl core::fmt::Display for LockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::GitDirMissing {
                workspace_root,
            } => {
                write!(
                    f,
                    "no .git directory under workspace root `{}`",
                    workspace_root.display()
                )
            },
            Self::AlreadyHeld {
                previous: Some(h),
            } => {
                write!(
                    f,
                    "transition lock already held by host={} pid={} at={}",
                    h.hostname, h.pid, h.acquired_at
                )
            },
            Self::AlreadyHeld {
                previous: None,
            } => {
                write!(
                    f,
                    "transition lock already held (holder payload unreadable)"
                )
            },
            Self::Io {
                during,
                path,
                error,
            } => {
                write!(
                    f,
                    "lock-file IO failed during {during} on `{}`: {error}",
                    path.display()
                )
            },
        }
    }
}

impl std::error::Error for LockError {}

/// RAII guard over the advisory lock at
/// `<workspace_root>/.git/mockspace/.lock`.
///
/// Drop releases the lock by closing the file (the kernel auto-
/// releases on file-descriptor close, which matches `flock(2)`
/// semantics). The lock-file body is preserved across acquire
/// cycles for diagnostic purposes; we only overwrite it.
#[derive(Debug)]
pub struct FlockTransitionLock {
    file:      File,
    holder:    LockHolder,
    lock_path: PathBuf,
}

impl FlockTransitionLock {
    /// Acquire the lock with blocking semantics. Waits indefinitely
    /// for any current holder to release.
    pub fn acquire(workspace_root: &Path) -> Result<Self, LockError> {
        Self::acquire_inner(workspace_root, false)
    }

    /// Acquire the lock without blocking. Returns
    /// [`LockError::AlreadyHeld`] immediately if another process or
    /// thread already holds it.
    pub fn try_acquire(workspace_root: &Path) -> Result<Self, LockError> {
        Self::acquire_inner(workspace_root, true)
    }

    fn acquire_inner(workspace_root: &Path, non_blocking: bool) -> Result<Self, LockError> {
        let git_dir = workspace_root.join(".git");
        if !git_dir.exists() {
            return Err(LockError::GitDirMissing {
                workspace_root: workspace_root.to_path_buf(),
            });
        }
        let mockspace_dir = git_dir.join("mockspace");
        std::fs::create_dir_all(&mockspace_dir).map_err(|error| {
            LockError::Io {
                during: "create-dir",
                path: mockspace_dir.clone(),
                error,
            }
        })?;
        let lock_path = mockspace_dir.join(".lock");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                LockError::Io {
                    during: "open",
                    path: lock_path.clone(),
                    error,
                }
            })?;

        if non_blocking {
            if let Err(e) = file.try_lock_exclusive() {
                if e.kind() == io::ErrorKind::WouldBlock {
                    let previous = read_holder(&lock_path).ok();
                    return Err(LockError::AlreadyHeld {
                        previous,
                    });
                }
                return Err(LockError::Io {
                    during: "try-lock",
                    path:   lock_path,
                    error:  e,
                });
            }
        } else {
            file.lock_exclusive().map_err(|error| {
                LockError::Io {
                    during: "lock",
                    path: lock_path.clone(),
                    error,
                }
            })?;
        }

        // We now hold the lock. Write our holder payload over the
        // file body. Truncate first so a previous longer holder's
        // bytes do not bleed through.
        let holder = current_holder();
        let body = format_holder(&holder);
        // Drop the read pointer; rewrite from offset 0.
        let mut writer = file.try_clone().map_err(|error| {
            LockError::Io {
                during: "clone",
                path: lock_path.clone(),
                error,
            }
        })?;
        writer.set_len(0).map_err(|error| {
            LockError::Io {
                during: "truncate",
                path: lock_path.clone(),
                error,
            }
        })?;
        writer.write_all(body.as_bytes()).map_err(|error| {
            LockError::Io {
                during: "write-holder",
                path: lock_path.clone(),
                error,
            }
        })?;
        writer.flush().map_err(|error| {
            LockError::Io {
                during: "flush",
                path: lock_path.clone(),
                error,
            }
        })?;

        Ok(Self {
            file,
            holder,
            lock_path,
        })
    }

    /// The holder payload captured at acquire time. Never re-read
    /// from the lock-file body afterward.
    pub fn holder(&self) -> &LockHolder {
        &self.holder
    }

    /// The path to the lock file. Useful for diagnostics.
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }
}

impl Drop for FlockTransitionLock {
    fn drop(&mut self) {
        // Best-effort unlock. The kernel releases on FD close anyway,
        // so failure here is not actionable.
        let _ = FileExt::unlock(&self.file);
    }
}

/// Compose the current process's [`LockHolder`].
fn current_holder() -> LockHolder {
    let hostname = match hostname_lookup() {
        Some(h) => h,
        None => "unknown".to_owned(),
    };
    let pid = std::process::id();
    let acquired_at = current_iso8601();
    LockHolder {
        hostname,
        pid,
        acquired_at,
    }
}

/// Render a [`LockHolder`] as a three-line plaintext payload.
fn format_holder(h: &LockHolder) -> String {
    format!(
        "hostname: {}\npid: {}\nacquired_at: {}\n",
        h.hostname, h.pid, h.acquired_at
    )
}

/// Parse the lock-file body back into a [`LockHolder`]. Used only
/// at collision time to surface who holds the lock.
fn read_holder(path: &Path) -> Result<LockHolder, io::Error> {
    let text = std::fs::read_to_string(path)?;
    let mut hostname: Option<String> = None;
    let mut pid: Option<u32> = None;
    let mut acquired_at: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("hostname: ") {
            hostname = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("pid: ") {
            pid = rest.parse().ok();
        } else if let Some(rest) = line.strip_prefix("acquired_at: ") {
            acquired_at = Some(rest.to_owned());
        }
    }
    Ok(LockHolder {
        hostname:    hostname.unwrap_or_else(|| "unknown".to_owned()),
        pid:         pid.unwrap_or(0),
        acquired_at: acquired_at.unwrap_or_else(|| "unknown".to_owned()),
    })
}

/// Best-effort hostname lookup. Uses the `HOSTNAME` environment
/// variable on Linux/macOS where it is commonly set; falls back to
/// the `hostname(1)` binary; falls back to `None` if neither yields
/// a result. mockspace-core has no network primitives and we do not
/// want to add a hostname crate dependency for a debug payload.
fn hostname_lookup() -> Option<String> {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return Some(h);
        }
    }
    let output = std::process::Command::new("hostname").output().ok()?;
    let trimmed = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn init_repo(dir: &Path) {
        let out = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .output()
            .expect("git should run");
        assert!(out.status.success(), "git init failed");
    }

    #[test]
    fn acquire_creates_lock_file_and_writes_holder() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire succeeds");
        let body =
            std::fs::read_to_string(lock.lock_path()).expect("lock file is readable while held");
        assert!(body.contains("hostname:"));
        assert!(body.contains(&format!("pid: {}", lock.holder().pid)));
        // The lock file lives under .git/mockspace/.lock.
        let expected = dir
            .path()
            .canonicalize()
            .unwrap()
            .join(".git")
            .join("mockspace")
            .join(".lock");
        let actual = lock.lock_path().canonicalize().unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn acquire_errors_when_git_dir_missing() {
        let dir = TempDir::new().unwrap();
        // No `git init`; no .git/ directory.
        let err = FlockTransitionLock::acquire(dir.path()).unwrap_err();
        assert!(
            matches!(err, LockError::GitDirMissing { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn try_acquire_when_already_held_returns_already_held_with_previous_holder() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let first = FlockTransitionLock::acquire(dir.path()).expect("first acquire");

        let err = FlockTransitionLock::try_acquire(dir.path()).unwrap_err();
        match err {
            LockError::AlreadyHeld {
                previous: Some(h),
            } => {
                // The reported previous holder should match the
                // first guard's payload (same PID, same hostname).
                assert_eq!(h.pid, first.holder().pid);
                assert_eq!(h.hostname, first.holder().hostname);
            },
            other => panic!("expected AlreadyHeld with previous, got {other:?}"),
        }
    }

    #[test]
    fn drop_releases_lock_so_a_second_acquire_succeeds() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        {
            let _first = FlockTransitionLock::acquire(dir.path()).expect("first acquire");
            // _first drops at end of this block; lock released.
        }
        let second = FlockTransitionLock::try_acquire(dir.path()).expect("second acquire");
        assert!(second.lock_path().exists());
    }
}
