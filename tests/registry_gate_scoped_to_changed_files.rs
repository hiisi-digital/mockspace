//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A reference to a row gates only the run that changes the file holding it.
//!
//! One real git repository holds a row file citing a row that no row declares (`cites.toml`) and
//! an unrelated clean one (`other.toml`). The commit gate judges the staged files and the push
//! gate the files of the commits no remote has, so each pair below is the same registry with
//! the citing file in the change and with another file in it. The first of each pair is the
//! case that must fail.

mod common;

use std::fs;
use std::path::Path;
use std::process::Output;

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "slot"
title = "Slots"
description = "Something a person needs."

[[registry.namespace]]
key = "answer"
title = "Answers"
description = "What answers a slot."

[[registry.namespace.field]]
name = "slot"
type = "slot"
required = true
description = "The slot this answers."
"#;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A repository with `cites.toml` citing `slot::nosuch`, committed.
fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    common::git_init(root);
    let mock = root.join("mock");
    write(
        &mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    write(&mock.join("mockspace.toml"), CONFIG);
    write(
        &mock.join("registry/slots.toml"),
        "[[slot]]\nid = \"display\"\n",
    );
    write(
        &mock.join("registry/cites.toml"),
        "[[answer]]\nid = \"niri\"\nslot = \"nosuch\"\n",
    );
    write(
        &mock.join("registry/other.toml"),
        "[[answer]]\nid = \"sway\"\nslot = \"display\"\n",
    );
    common::git_in_dir(root, &["add", "-A"]);
    common::git_in_dir(root, &["commit", "-q", "-m", "chore: base"]);
    tmp
}

/// Appends a comment to `file`, so the file is part of the next change.
fn touch(root: &Path, file: &str) {
    let path = root.join("mock/registry").join(file);
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("# touched\n");
    fs::write(&path, text).unwrap();
}

fn gate(root: &Path, flag: &str) -> Output {
    common::mockspace()
        .current_dir(root.join("mock"))
        .args(["--lint-only", flag, "--scope", "infra"])
        .output()
        .unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn taplo_present() -> bool {
    std::process::Command::new("taplo")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_commit_staging_the_citing_file_fails() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    touch(tmp.path(), "cites.toml");
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    let out = gate(tmp.path(), "--commit");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR [unknown-row-reference]"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_commit_staging_another_file_passes_with_the_warning() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    touch(tmp.path(), "other.toml");
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    let out = gate(tmp.path(), "--commit");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("warning [unknown-row-reference]"),
        "{}",
        stderr(&out)
    );
}

/// A bare repository as `origin`, with everything committed so far pushed to it.
fn push_base(tmp: &tempfile::TempDir) {
    let remote = tmp.path().join("remote.git");
    common::git_in_dir(tmp.path(), &[
        "init",
        "-q",
        "--bare",
        remote.to_str().unwrap(),
    ]);
    common::git_in_dir(tmp.path(), &[
        "remote",
        "add",
        "origin",
        remote.to_str().unwrap(),
    ]);
    common::git_in_dir(tmp.path(), &["push", "-q", "origin", "HEAD:refs/heads/dev"]);
    common::git_in_dir(tmp.path(), &["fetch", "-q", "origin"]);
}

fn commit_touching(tmp: &tempfile::TempDir, file: &str) {
    touch(tmp.path(), file);
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    common::git_in_dir(tmp.path(), &["commit", "-q", "-m", "chore: touch"]);
}

#[test]
fn a_push_of_a_commit_changing_the_citing_file_fails() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    push_base(&tmp);
    commit_touching(&tmp, "cites.toml");
    let out = gate(tmp.path(), "--strict");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR [unknown-row-reference]"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_push_whose_commits_leave_the_citing_file_alone_passes_with_the_warning() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    push_base(&tmp);
    commit_touching(&tmp, "other.toml");
    let out = gate(tmp.path(), "--strict");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("warning [unknown-row-reference]"),
        "{}",
        stderr(&out)
    );
}

/// The citing file was changed in a commit a remote already has, so this push does not carry it.
#[test]
fn a_push_after_the_citing_commit_is_on_a_remote_passes_with_the_warning() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    commit_touching(&tmp, "cites.toml");
    push_base(&tmp);
    commit_touching(&tmp, "other.toml");
    let out = gate(tmp.path(), "--strict");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

/// A duplicate is not a reference: it blocks the push although the commits change another file.
#[test]
fn a_duplicate_still_blocks_a_run_that_does_not_change_its_file() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    write(
        &tmp.path().join("mock/registry/dup.toml"),
        "[[slot]]\nid = \"display\"\n",
    );
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    common::git_in_dir(tmp.path(), &["commit", "-q", "-m", "chore: duplicate"]);
    push_base(&tmp);
    commit_touching(&tmp, "other.toml");
    let out = gate(tmp.path(), "--strict");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR [duplicate-id]"),
        "{}",
        stderr(&out)
    );
}

#[path = "common/ambient.rs"]
mod ambient;
