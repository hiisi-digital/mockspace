//! Git-driven staged-file detection.
//!
//! Per schema design memo §10. Spawns `git` as a subprocess and parses
//! `--name-only` output to populate a [`StagedSet`] of paths. Three gate
//! shapes:
//!
//! - `Gate::Commit`: `git diff --cached --name-only` (files in the index
//!   but not yet committed).
//! - `Gate::Push`: `git diff --name-only <base>..HEAD` where `<base>` is
//!   `MOCKSPACE_PUSH_DIFF_BASE` (loud error on bad ref) or
//!   `@{upstream}` (with merge-base fallback), or `StagedSet::Full` with
//!   a warning when neither is set.
//! - `Gate::Build`: always `StagedSet::Full`.

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace_core::lint::Gate;

/// Set of paths the current gate considers "staged".
#[derive(Debug, Clone)]
pub enum StagedSet {
    /// All documents count as staged (build gate; push-gate fallback;
    /// editor surface).
    Full,
    /// Only the listed paths.
    Paths(HashSet<PathBuf>),
}

/// Error from [`StagingFilter::new`]. The engine surfaces these through
/// the ConfigError-channel and drops staging-aware lints rather than
/// silently treating "0 staged files" as "everything passes".
#[derive(Debug)]
pub enum StagingFilterError {
    /// `MOCKSPACE_PUSH_DIFF_BASE` was set but does not resolve to a
    /// git object the worktree knows about.
    BadEnvRef {
        value: String,
    },
    /// `git` subprocess failed.
    Git(GitError),
}

impl std::fmt::Display for StagingFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadEnvRef {
                value,
            } => {
                write!(
                    f,
                    "MOCKSPACE_PUSH_DIFF_BASE = `{value}` does not resolve to a git ref"
                )
            },
            Self::Git(e) => write!(f, "git command failed: {e}"),
        }
    }
}

impl std::error::Error for StagingFilterError {}

#[derive(Debug)]
pub enum GitError {
    Spawn(std::io::Error),
    NonZeroExit {
        code:   Option<i32>,
        stderr: String,
    },
    NotUtf8(std::str::Utf8Error),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "git spawn: {e}"),
            Self::NonZeroExit {
                code,
                stderr,
            } => {
                write!(f, "git exit {code:?}: {stderr}")
            },
            Self::NotUtf8(e) => write!(f, "git output not utf-8: {e}"),
        }
    }
}

impl std::error::Error for GitError {}

/// Determines whether each document path counts as staged for the current
/// gate.
#[derive(Debug)]
pub struct StagingFilter {
    #[allow(dead_code)]
    gate:           Gate,
    workspace_root: PathBuf,
    staged:         StagedSet,
}

impl StagingFilter {
    /// Build per the current gate. Reads git via subprocess. Returns
    /// `Err` on misconfigured environment (e.g.
    /// `MOCKSPACE_PUSH_DIFF_BASE` names a non-existent ref). Detached
    /// HEAD with no env and no upstream falls back to `StagedSet::Full`
    /// with a stderr warning.
    pub fn new(gate: Gate, workspace_root: &Path) -> Result<Self, StagingFilterError> {
        let staged = match gate {
            Gate::Commit => Self::staged_for_commit(workspace_root)?,
            Gate::Push => Self::staged_for_push(workspace_root)?,
            Gate::Build => StagedSet::Full,
        };
        Ok(Self {
            gate,
            workspace_root: workspace_root.to_path_buf(),
            staged,
        })
    }

    fn staged_for_commit(root: &Path) -> Result<StagedSet, StagingFilterError> {
        let paths = run_git(root, &["diff", "--cached", "--name-only", "-z"])
            .map_err(StagingFilterError::Git)?;
        Ok(StagedSet::Paths(absolute(root, paths)))
    }

    fn staged_for_push(root: &Path) -> Result<StagedSet, StagingFilterError> {
        if let Ok(base) = env::var("MOCKSPACE_PUSH_DIFF_BASE") {
            if !git_rev_parse_verify(root, &base) {
                return Err(StagingFilterError::BadEnvRef {
                    value: base,
                });
            }
            let range = format!("{base}..HEAD");
            let paths = run_git(root, &["diff", "--name-only", "-z", &range])
                .map_err(StagingFilterError::Git)?;
            return Ok(StagedSet::Paths(absolute(root, paths)));
        }
        if let Some(upstream) = git_rev_parse_upstream(root) {
            let base = git_merge_base(root, "HEAD", &upstream).unwrap_or(upstream);
            let range = format!("{base}..HEAD");
            let paths = run_git(root, &["diff", "--name-only", "-z", &range])
                .map_err(StagingFilterError::Git)?;
            return Ok(StagedSet::Paths(absolute(root, paths)));
        }
        eprintln!(
            "warning: push gate falling back to full project; \
             set MOCKSPACE_PUSH_DIFF_BASE to gate against a specific ref"
        );
        Ok(StagedSet::Full)
    }

    /// Whether the given path counts as staged. Accepts absolute paths
    /// or paths relative to the workspace root.
    pub fn is_staged(&self, path: &Path) -> bool {
        match &self.staged {
            StagedSet::Full => true,
            StagedSet::Paths(set) => {
                let abs = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.workspace_root.join(path)
                };
                set.contains(&abs)
            },
        }
    }

    pub fn staged_set(&self) -> &StagedSet {
        &self.staged
    }
}

// =========================================================================
// Git subprocess helpers.
// =========================================================================

fn run_git(root: &Path, args: &[&str]) -> Result<HashSet<PathBuf>, GitError> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(GitError::Spawn)?;
    if !output.status.success() {
        return Err(GitError::NonZeroExit {
            code:   output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    // -z output uses NUL separators (handles non-utf8 paths gracefully).
    let stdout = output.stdout;
    let mut out = HashSet::new();
    for slice in stdout.split(|&b| b == 0) {
        if slice.is_empty() {
            continue;
        }
        match std::str::from_utf8(slice) {
            Ok(s) => {
                out.insert(PathBuf::from(s));
            },
            Err(_) => {
                eprintln!(
                    "warning: git returned a non-utf-8 path; skipping ({} bytes)",
                    slice.len()
                );
            },
        }
    }
    Ok(out)
}

fn absolute(root: &Path, paths: HashSet<PathBuf>) -> HashSet<PathBuf> {
    paths
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .collect()
}

fn git_rev_parse_verify(root: &Path, refname: &str) -> bool {
    Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", "--quiet", refname])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_rev_parse_upstream(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn git_merge_base(root: &Path, a: &str, b: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["merge-base", a, b])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_gate_always_full() {
        let tmp = tempfile::tempdir().unwrap();
        let filter = StagingFilter::new(Gate::Build, tmp.path()).unwrap();
        assert!(matches!(filter.staged_set(), StagedSet::Full));
        assert!(filter.is_staged(Path::new("anything.rs")));
    }

    #[test]
    fn commit_gate_outside_repo_returns_git_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = StagingFilter::new(Gate::Commit, tmp.path());
        // Outside a git repo, `git diff --cached` fails. The error surfaces.
        assert!(result.is_err());
    }

    #[test]
    fn commit_gate_inside_repo_returns_staged_set() {
        let tmp = tempfile::tempdir().unwrap();
        // Init a git repo.
        let _ = Command::new("git")
            .current_dir(tmp.path())
            .args(["init", "-q"])
            .output()
            .expect("git init");
        let _ = Command::new("git")
            .current_dir(tmp.path())
            .args(["config", "user.email", "test@example.com"])
            .output();
        let _ = Command::new("git")
            .current_dir(tmp.path())
            .args(["config", "user.name", "Test"])
            .output();
        // Empty repo, nothing staged.
        let filter = StagingFilter::new(Gate::Commit, tmp.path()).unwrap();
        match filter.staged_set() {
            StagedSet::Paths(set) => assert!(set.is_empty()),
            StagedSet::Full => panic!("expected Paths"),
        }
    }

    #[test]
    fn bad_env_ref_errors_loudly() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = Command::new("git")
            .current_dir(tmp.path())
            .args(["init", "-q"])
            .output()
            .expect("git init");
        std::env::set_var("MOCKSPACE_PUSH_DIFF_BASE", "totally-nonexistent-ref-zzz");
        let result = StagingFilter::new(Gate::Push, tmp.path());
        std::env::remove_var("MOCKSPACE_PUSH_DIFF_BASE");
        match result {
            Err(StagingFilterError::BadEnvRef {
                value,
            }) => {
                assert_eq!(value, "totally-nonexistent-ref-zzz");
            },
            other => panic!("expected BadEnvRef, got {other:?}"),
        }
    }
}
