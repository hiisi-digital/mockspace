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

[[registry.namespace]]
key = "law"
title = "Laws"
description = "A statement that holds."

[[registry.namespace.field]]
name = "rests_on"
type = "ref[]"
required = false
description = "What it rests on."

[[registry.namespace.field]]
name = "text"
type = "string"
required = false
description = "The statement."
"#;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A repository with `cites.toml` citing `slot::nosuch`, committed.
/// The scratch directory and the repository inside it, so a bare remote and a home can sit
/// beside the working tree rather than in it.
struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Fixture {
    fn path(&self) -> &Path {
        &self.root
    }

    fn beside(&self, name: &str) -> std::path::PathBuf {
        self.root.parent().unwrap().join(name)
    }
}

fn repo() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    let root = root.as_path();
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
    Fixture {
        root: root.to_path_buf(),
        _dir: dir,
    }
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
fn push_base(tmp: &Fixture) {
    let remote = tmp.beside("remote.git");
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

fn commit_touching(tmp: &Fixture, file: &str) {
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

/// A pre-commit hook that runs the binary as the generated hook does, from the mock directory.
/// The log is kept beside the working tree so the hook's output can be read back.
fn install_hook(tmp: &Fixture) {
    let home = tmp.beside("home");
    fs::create_dir_all(&home).unwrap();
    let script = format!(
        "#!/bin/sh\ncd \"$(git rev-parse --show-toplevel)/mock\" && HOME='{}' exec '{}' \
         --lint-only --commit --scope infra >'{}' 2>&1\n",
        home.display(),
        common::BIN,
        tmp.beside("hook.log").display()
    );
    common::write_executable(&tmp.path().join(".git/hooks/pre-commit"), script);
}

fn hook_log(tmp: &Fixture) -> String {
    fs::read_to_string(tmp.beside("hook.log")).unwrap_or_default()
}

/// `git commit -a` hands the hook a temporary index holding the edit, while `.git/index` still
/// holds the old file. The gate has to read the one git is committing.
#[test]
fn commit_dash_a_of_the_citing_file_fails_through_a_real_hook() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    install_hook(&tmp);
    touch(tmp.path(), "cites.toml");
    let out = common::git_out_dir(tmp.path(), &["commit", "-q", "-a", "-m", "chore: edit"]);
    assert!(
        !out.status.success(),
        "committed past the gate:\n{}",
        hook_log(&tmp)
    );
    assert!(
        hook_log(&tmp).contains("ERROR [unknown-row-reference]"),
        "{}",
        hook_log(&tmp)
    );
}

#[test]
fn commit_of_a_path_naming_the_citing_file_fails_through_a_real_hook() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    install_hook(&tmp);
    touch(tmp.path(), "cites.toml");
    let out = common::git_out_dir(tmp.path(), &[
        "commit",
        "-q",
        "-m",
        "chore: edit",
        "mock/registry/cites.toml",
    ]);
    assert!(
        !out.status.success(),
        "committed past the gate:\n{}",
        hook_log(&tmp)
    );
    assert!(
        hook_log(&tmp).contains("ERROR [unknown-row-reference]"),
        "{}",
        hook_log(&tmp)
    );
}

/// The control: the same `commit -a`, with another file edited, goes through with the warning.
#[test]
fn commit_dash_a_of_another_file_passes_through_a_real_hook() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    install_hook(&tmp);
    touch(tmp.path(), "other.toml");
    let out = common::git_out_dir(tmp.path(), &["commit", "-q", "-a", "-m", "chore: edit"]);
    assert!(out.status.success(), "blocked:\n{}", hook_log(&tmp));
    assert!(
        hook_log(&tmp).contains("warning [unknown-row-reference]"),
        "{}",
        hook_log(&tmp)
    );
}

/// Commits the unpushed `feature` branch's file by a local merge that has a second parent already
/// on a remote. The merge commit lists no file by default, so the scope was empty.
#[test]
fn a_push_of_a_merge_changing_the_citing_file_fails() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    push_base(&tmp);
    common::git_in_dir(tmp.path(), &["checkout", "-q", "-b", "feature"]);
    touch(tmp.path(), "cites.toml");
    common::git_in_dir(tmp.path(), &["commit", "-qam", "chore: edit"]);
    common::git_in_dir(tmp.path(), &["push", "-q", "origin", "feature"]);
    common::git_in_dir(tmp.path(), &["fetch", "-q", "origin"]);
    common::git_in_dir(tmp.path(), &["checkout", "-q", "dev"]);
    touch(tmp.path(), "other.toml");
    common::git_in_dir(tmp.path(), &["commit", "-qam", "chore: other"]);
    common::git_in_dir(tmp.path(), &["push", "-q", "origin", "dev"]);
    common::git_in_dir(tmp.path(), &[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "chore: merge",
        "feature",
    ]);
    let out = gate(tmp.path(), "--strict");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR [unknown-row-reference]"),
        "{}",
        stderr(&out)
    );
}

/// A provenance finding names a file nobody staged and still blocks.
#[test]
fn a_provenance_finding_blocks_a_commit_staging_another_file() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check fails every arm without it"
    );
    let tmp = repo();
    write(
        &tmp.path().join("mock/registry/law.toml"),
        "[[law]]\nid = \"one\"\nrests_on = [\"nosuchroot::design::1\"]\n",
    );
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    common::git_in_dir(tmp.path(), &["commit", "-q", "-m", "chore: law"]);
    touch(tmp.path(), "other.toml");
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    let out = gate(tmp.path(), "--commit");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR [unknown-provenance-root]"),
        "{}",
        stderr(&out)
    );
}

/// A reference cycle is found by resolution, which the generation run does, and it blocks a commit
/// staging another file.
#[test]
fn a_reference_cycle_blocks_a_commit_staging_another_file() {
    let tmp = repo();
    write(
        &tmp.path().join("mock/registry/law.toml"),
        "[[law]]\nid = \"a\"\ntext = \"{{ law::b::text }}\"\n\n[[law]]\nid = \"b\"\ntext = \"{{ law::a::text }}\"\n",
    );
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    common::git_in_dir(tmp.path(), &["commit", "-q", "-m", "chore: law"]);
    touch(tmp.path(), "other.toml");
    common::git_in_dir(tmp.path(), &["add", "-A"]);
    let out = common::mockspace()
        .current_dir(tmp.path().join("mock"))
        .args(["--generate-only", "--commit"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR [reference-cycle]"),
        "{}",
        stderr(&out)
    );
}

#[path = "common/ambient.rs"]
mod ambient;
