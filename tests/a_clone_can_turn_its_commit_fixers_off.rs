//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A clone turns the commit fixers off for itself, in its own git config.
//!
//! `auto_fmt` and `auto_clippy_fix` are keys in `mockspace.toml`, so they are
//! the project's and every clone carries the same answer. A machine that should
//! not compile the project at commit time needs a switch of its own that the
//! repository does not see, and `git config mockspace.autoClippyFix false` is
//! that switch. These arms plant a real clone per case, since the thing being
//! read is git's own config rather than a file the loader opens.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace::config::Config;

/// A clone with `mockspace.toml` at its root carrying `extra` beneath the two
/// keys every config names. Returns the tempdir and the clone's root.
fn clone(extra: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    git(&root, &["init", "-q"]);
    fs::write(
        root.join("mockspace.toml"),
        format!("project_name = \"probe\"\nmock_dir = \"mock\"\n{extra}"),
    )
    .expect("write config");
    fs::create_dir_all(root.join("mock")).expect("create mock dir");
    (tmp, root)
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {args:?} in {}", root.display());
}

fn load(root: &Path) -> Config {
    Config::from_dir(&root.join("mock"))
}

#[test]
fn a_clone_that_turns_clippy_off_has_it_off_and_leaves_fmt_alone() {
    let (_tmp, root) = clone("");
    git(&root, &["config", "mockspace.autoClippyFix", "false"]);
    let cfg = load(&root);
    assert!(!cfg.auto_clippy_fix, "the clone said false");
    assert!(
        cfg.auto_fmt,
        "nothing was said about fmt, so it keeps its default"
    );
}

#[test]
fn a_clone_that_turns_fmt_off_has_it_off_and_leaves_clippy_alone() {
    let (_tmp, root) = clone("");
    git(&root, &["config", "mockspace.autoFmt", "false"]);
    let cfg = load(&root);
    assert!(!cfg.auto_fmt, "the clone said false");
    assert!(cfg.auto_clippy_fix, "nothing was said about clippy");
}

/// The control for both arms above: the same clone with nothing set has both
/// fixers on, so the arms are reading the setting and not a default of false.
#[test]
fn a_clone_that_says_nothing_has_both_on() {
    let (_tmp, root) = clone("");
    let cfg = load(&root);
    assert!(cfg.auto_fmt && cfg.auto_clippy_fix);
}

#[test]
fn the_clone_beats_the_file_in_both_directions() {
    let (_tmp, root) = clone("auto_clippy_fix = false\nauto_fmt = true\n");
    git(&root, &["config", "mockspace.autoClippyFix", "true"]);
    git(&root, &["config", "mockspace.autoFmt", "false"]);
    let cfg = load(&root);
    assert!(
        cfg.auto_clippy_fix,
        "the file said false and the clone said true"
    );
    assert!(!cfg.auto_fmt, "the file said true and the clone said false");
}

/// The control on the one above: the file's own value holds where the clone
/// says nothing, so the arm above is an override and not the file being lost.
#[test]
fn the_file_holds_where_the_clone_says_nothing() {
    let (_tmp, root) = clone("auto_clippy_fix = false\nauto_fmt = false\n");
    let cfg = load(&root);
    assert!(!cfg.auto_clippy_fix && !cfg.auto_fmt);
}

/// git's other spellings of a boolean are its business, and the loader takes
/// whatever git reads them as.
#[test]
fn every_spelling_git_reads_as_false_turns_it_off() {
    for v in ["false", "no", "off", "0", "FALSE"] {
        let (_tmp, root) = clone("");
        git(&root, &["config", "mockspace.autoClippyFix", v]);
        assert!(!load(&root).auto_clippy_fix, "`{v}` is false to git");
    }
}

#[test]
#[should_panic(expected = "mockspace.autoClippyFix")]
fn a_value_that_is_not_a_boolean_is_refused() {
    let (_tmp, root) = clone("");
    git(&root, &["config", "mockspace.autoClippyFix", "maybe"]);
    let _ = load(&root);
}

/// A tree found by its `mockspace.toml` rather than by `.git` has no clone to
/// ask. The file's value stands, and the load does not reach for git at all.
#[test]
fn a_tree_with_no_git_takes_the_files_value() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("mockspace.toml"),
        "project_name = \"probe\"\nmock_dir = \"mock\"\nauto_clippy_fix = false\n",
    )
    .expect("write config");
    fs::create_dir_all(root.join("mock")).expect("create mock dir");
    assert!(!load(root).auto_clippy_fix);
    assert!(load(root).auto_fmt);
}
