//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::path::{Path, PathBuf};

use mockspace_lint_rules::Touched;

use super::{Marker, Plan, reduce};
use crate::entry::registry_scope::Changed;

const REPO: &str = "/r";

fn files(list: &[&str]) -> Changed {
    Changed::Files(list.iter().map(|f| Path::new(REPO).join(f)).collect::<Vec<PathBuf>>())
}

fn reduced(list: &[&str]) -> Option<Vec<String>> {
    reduce(&files(list), Path::new(REPO), Path::new("/r/mock"))
}

#[test]
fn the_files_come_back_relative_to_the_repository() {
    assert_eq!(
        reduced(&["content/a.toml", "mock/crates/x/src/lib.rs"]),
        Some(vec!["content/a.toml".to_string(), "mock/crates/x/src/lib.rs".to_string()])
    );
}

#[test]
fn a_change_to_the_lint_set_runs_every_lint() {
    for set in [
        "mockspace.toml",
        "mock/mockspace.toml",
        "mock/lints/a_lint.rs",
        "mock/tools/t/src/main.rs",
        "mock/Cargo.toml",
        "mock/Cargo.lock",
        "mock/rust-toolchain.toml",
        "mock/.cargo/config.toml",
        "rust-toolchain.toml",
        ".cargo/config.toml",
    ] {
        assert_eq!(reduced(&["content/a.toml", set]), None, "{set} is the lint set");
    }
}

#[test]
fn a_crate_beside_the_lint_set_is_not_the_lint_set() {
    assert!(reduced(&["mock/lints_notes.md", "mock/crates/lints/src/lib.rs"]).is_some());
}

#[test]
fn a_run_git_could_not_list_runs_every_lint() {
    assert_eq!(reduce(&Changed::Unknown, Path::new(REPO), Path::new("/r/mock")), None);
    assert_eq!(reduce(&Changed::NotCommit, Path::new(REPO), Path::new("/r/mock")), None);
}

fn marker(dir: &Path, key: &str, head: &str) -> Option<Marker> {
    Some(Marker {
        path: dir.join("last-full-run/tag-commit"),
        key:  key.to_string(),
        head: Some(head.to_string()),
        root: dir.to_path_buf(),
    })
}

fn everything(plan: &Plan) -> bool {
    matches!(plan.touched(), Touched::Everything)
}

fn files_of() -> Option<Vec<String>> {
    Some(vec!["notes/b.txt".to_string()])
}

/// A tree whose history is `h1`, then `h2`: `h1` is an ancestor of `h2` and nothing else is.
fn descends(kept: &str) -> bool {
    kept == "h1"
}

fn kept(dir: &Path, key: &str, head: &str) {
    std::fs::create_dir_all(dir.join("last-full-run")).unwrap();
    std::fs::write(dir.join("last-full-run/tag-commit"), format!("{key}\n{head}\n/r\n")).unwrap();
}

#[test]
fn a_moved_lint_set_key_makes_a_run_touching_nothing_full() {
    let dir = tempfile::tempdir().unwrap();
    kept(dir.path(), "old", "h1");
    assert!(everything(&Plan::decide(files_of(), marker(dir.path(), "new", "h1"), &descends)));
    // The control: the key the last full run kept narrows.
    assert!(!everything(&Plan::decide(files_of(), marker(dir.path(), "old", "h1"), &descends)));
}

#[test]
fn a_key_kept_over_a_commit_that_is_not_an_ancestor_of_head_is_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    kept(dir.path(), "k", "h1");
    assert!(!everything(&Plan::decide(files_of(), marker(dir.path(), "k", "h2"), &descends)), "a descendant");
    kept(dir.path(), "k", "other");
    assert!(everything(&Plan::decide(files_of(), marker(dir.path(), "k", "h2"), &descends)), "a branch elsewhere");
    assert!(everything(&Plan::decide(files_of(), Some(Marker { head: None, ..marker(dir.path(), "k", "h2").unwrap() }), &descends)));
}

#[test]
fn only_a_clean_full_run_keeps_the_lint_set_key_with_the_commit_and_the_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let first = Plan::decide(files_of(), marker(dir.path(), "k", "h1"), &descends);
    assert!(everything(&first), "no key kept yet");
    first.settle(1, false);
    assert!(everything(&Plan::decide(files_of(), marker(dir.path(), "k", "h1"), &descends)), "a blocked run keeps no key");
    first.settle(0, true);
    assert!(everything(&Plan::decide(files_of(), marker(dir.path(), "k", "h1"), &descends)), "a doc-only run was not full");
    first.settle(0, false);
    assert!(!everything(&Plan::decide(files_of(), marker(dir.path(), "k", "h1"), &descends)));
    let text = std::fs::read_to_string(dir.path().join("last-full-run/tag-commit")).unwrap();
    assert_eq!(text.lines().collect::<Vec<_>>(), ["k", "h1", dir.path().to_str().unwrap()]);
}

#[test]
fn a_run_with_no_store_is_always_full() {
    assert!(everything(&Plan::decide(files_of(), None, &descends)));
}
