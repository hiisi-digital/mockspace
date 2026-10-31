//! What the tests of the store's parts have in common: a git command that cannot fail quietly, and
//! a consumer clone with a second worktree.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A consumer clone with a remote, and a second worktree of it.
pub(super) struct Consumer {
    pub(super) _tmp:  tempfile::TempDir,
    pub(super) main:  PathBuf,
    pub(super) other: PathBuf,
}

pub(super) fn consumer(remote: &str) -> Consumer {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("clone");
    std::fs::create_dir_all(main.join("mock")).unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &["remote", "add", "origin", remote]);
    std::fs::write(main.join("mock").join("keep"), "").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "one"]);
    let other = tmp.path().join("wt").join("elsewhere");
    git(&main, &[
        "worktree",
        "add",
        "-q",
        other.to_str().unwrap(),
        "-b",
        "other",
    ]);
    Consumer {
        _tmp: tmp,
        main,
        other,
    }
}
