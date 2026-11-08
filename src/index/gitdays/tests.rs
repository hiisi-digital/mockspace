use std::path::Path;
use std::process::Command;

use super::{first_days, is_shallow, last_day};
use crate::index::stamp::days_from_civil;

/// Run git in `root` as a fixed author at a fixed time, so a commit has the date a test names.
fn git(root: &Path, when: &str, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_AUTHOR_DATE", when)
        .env("GIT_COMMITTER_DATE", when)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

fn commit(root: &Path, when: &str, file: &str, body: &str) {
    std::fs::write(root.join(file), body).unwrap();
    git(root, when, &["add", file]);
    git(root, when, &["commit", "-q", "-m", "x"]);
}

fn repo() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), "2026-01-01T00:00:00Z", &["init", "-q"]);
    t
}

#[test]
fn a_file_is_dated_by_the_commit_that_added_it_not_the_one_that_changed_it() {
    let t = repo();
    commit(t.path(), "2026-03-01T12:00:00Z", "a.md", "one");
    commit(t.path(), "2026-05-09T12:00:00Z", "a.md", "two");
    commit(t.path(), "2026-04-02T12:00:00Z", "b.md", "other");

    let first = first_days(t.path());
    assert_eq!(first["a.md"], days_from_civil(2026, 3, 1));
    assert_eq!(first["b.md"], days_from_civil(2026, 4, 2));
    // THE CONTROL: the last change to the same file is the later day, so the two are not one
    // answer read twice.
    assert_eq!(last_day(t.path(), "a.md"), Some(days_from_civil(2026, 5, 9)));
}

#[test]
fn a_path_git_has_no_commit_for_has_no_day() {
    let t = repo();
    commit(t.path(), "2026-03-01T12:00:00Z", "a.md", "one");
    std::fs::write(t.path().join("new.md"), "uncommitted").unwrap();
    assert_eq!(last_day(t.path(), "new.md"), None);
    assert!(!first_days(t.path()).contains_key("new.md"));
    assert_eq!(last_day(Path::new("/no/such/dir"), "a.md"), None, "and not a repository, nothing");
}

#[test]
fn only_a_clone_with_cut_history_is_shallow() {
    let t = repo();
    commit(t.path(), "2026-03-01T12:00:00Z", "a.md", "one");
    commit(t.path(), "2026-03-02T12:00:00Z", "b.md", "two");
    assert!(!is_shallow(t.path()));
    assert!(!is_shallow(Path::new("/no/such/dir")));

    // THE CONTROL: the same repository cloned to one commit deep is.
    let into = tempfile::tempdir().unwrap();
    let dest = into.path().join("c");
    let ok = Command::new("git")
        .args(["clone", "-q", "--depth", "1"])
        .arg(format!("file://{}", t.path().display()))
        .arg(&dest)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    assert!(is_shallow(&dest));
}
