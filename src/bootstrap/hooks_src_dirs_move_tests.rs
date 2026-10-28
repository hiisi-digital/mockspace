//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A file moved from one crate to another changes both crates.
//!
//! With git's rename detection on, `git diff --name-only` names only the
//! destination of a move: the crate that lost the file is never linted, and a
//! lint that the move broke there (a `mod` line left behind, a re-export gone)
//! is found by the next commit that happens to touch it. The generated hooks list
//! changed paths with `--no-renames`, so the source of a move counts as the
//! deletion it is.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`.

use std::fs;
use std::path::Path;

use super::hooks_src_dirs_tests::{KASKI, fixture, git, run_full, scope_call, stage};
use super::*;

/// Long enough, and moved whole, so git reads the move as a rename.
const BODY: &str = "pub fn moved() -> u32 {\n    let a = 1;\n    let b = 2;\n    a + b\n}\n\n\
                    pub fn other() -> u32 {\n    let c = 3;\n    c * 2\n}\n";

fn move_file(root: &Path, from: &str, to: &str) {
    fs::create_dir_all(root.join(to).parent().unwrap()).unwrap();
    git(root, &["mv", from, to]);
    // the control for every arm: git does read this as one rename
    let status = git(root, &["diff", "--cached", "--name-status", "-M"]);
    assert!(
        status.starts_with("R100"),
        "the fixture is not a rename:\n{status}"
    );
}

/// What the pre-commit hook prints with `from` moved to `to`, and the arguments
/// it passes to `cargo mock`.
fn commit_after_move(src_dirs: &[&str], from: &str, to: &str) -> (String, String) {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[(from, BODY)]);
    git(root, &["commit", "-q", "-m", "base"]);
    move_file(root, from, to);
    let (printed, calls) = run_full(
        &gen_pre_commit(
            "mock",
            &HookLayout::of(src_dirs),
            &root.join("no-user-hook"),
        ),
        root,
        "",
    );
    (printed, scope_call(&calls))
}

/// What the pre-push hook prints for a push of the move of `from` to `to`.
fn push_after_move(src_dirs: &[&str], from: &str, to: &str) -> (String, String) {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[(from, BODY)]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    move_file(root, from, to);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let (printed, calls) = run_full(
        &gen_pre_push(
            "mock",
            &HookLayout::of(src_dirs),
            &root.join("no-user-hook"),
        ),
        root,
        &format!("refs/heads/m {head} refs/heads/m {base}\n"),
    );
    (printed, scope_call(&calls))
}

const BETWEEN_CRATES: (&str, &str) = ("mock/crates/a/src/x.rs", "mock/crates/b/src/x.rs");
const BETWEEN_DIRECTORIES: (&str, &str) = (
    "mock/crates/a/src/x.rs",
    "mock/applets/console/crates/c/src/x.rs",
);

#[test]
fn a_commit_moving_a_file_between_crates_scopes_to_both() {
    let (printed, args) = commit_after_move(KASKI, BETWEEN_CRATES.0, BETWEEN_CRATES.1);
    assert!(printed.contains("crates: a,b"), "hook said:\n{printed}");
    assert!(args.contains("--scope a,b"), "arguments: {args}");
}

#[test]
fn a_commit_moving_a_file_between_declared_directories_scopes_to_both() {
    let (printed, args) = commit_after_move(KASKI, BETWEEN_DIRECTORIES.0, BETWEEN_DIRECTORIES.1);
    assert!(printed.contains("crates: a,c"), "hook said:\n{printed}");
    assert!(args.contains("--scope a,c"), "arguments: {args}");
}

#[test]
fn a_push_of_a_move_between_crates_scopes_to_both() {
    let (printed, args) = push_after_move(KASKI, BETWEEN_CRATES.0, BETWEEN_CRATES.1);
    assert!(printed.contains("scope: a,b"), "hook said:\n{printed}");
    assert!(args.contains("--scope a,b"), "arguments: {args}");
}

#[test]
fn a_push_of_a_move_between_declared_directories_scopes_to_both() {
    let (printed, args) = push_after_move(KASKI, BETWEEN_DIRECTORIES.0, BETWEEN_DIRECTORIES.1);
    assert!(printed.contains("scope: a,c"), "hook said:\n{printed}");
    assert!(args.contains("--scope a,c"), "arguments: {args}");
}

/// The control: where only the destination is declared, only the destination is
/// a crate, so the arms above are about the source being counted and not about
/// every move being read as two crates.
#[test]
fn a_move_out_of_an_undeclared_directory_scopes_to_the_destination_alone() {
    let (printed, _) = commit_after_move(
        &["crates"],
        "mock/applets/console/crates/c/src/x.rs",
        "mock/crates/b/src/x.rs",
    );
    assert!(printed.contains("crates: b"), "hook said:\n{printed}");
    assert!(!printed.contains("crates: b,"), "hook said:\n{printed}");
}
