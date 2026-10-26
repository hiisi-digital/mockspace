//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two branches that each add to a file that only grows merge.
//!
//! Every scenario runs the real binary and real git in a scratch repository
//! that `cargo mock` has wired, with `cargo mock` shimmed to this build, so what
//! is asserted is what a consumer's clone does at `git merge`: the
//! `.gitattributes` lines, the driver in the clone's config, and the
//! `merge-additions` subcommand git calls through it.
//!
//! The control carries the rest. With the driver's config removed the same two
//! branches conflict as text, which is what shows the fixture makes a conflict
//! at all. And the negative the whole rule rests on: a side that edited a line
//! already there is not an append, and the file conflicts with conflict markers.

mod common;

use std::fs;

use common::{Repo, write};

/// A file the project declares, and a package's SHAME template, which mockspace names itself.
const DECLARED: &str = "content/strings/en.toml";
const SHAME: &str = "mock/crates/foo/SHAME.md.tmpl";

/// A repository with both files committed and the wiring in place, on `dev`.
fn repo() -> Repo {
    let r = Repo::new();
    write(
        &r.root.join("mock/mockspace.toml"),
        "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\nappend_only = [\"content/strings/en.toml\"]\n",
    );
    r.generate();
    write(&r.root.join(DECLARED), "\"a\" = \"one\"\n\"b\" = \"two\"\n");
    write(&r.root.join(SHAME), "# shame\n\n## first\nthe first gap\n");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add the files"]);
    r
}

fn append(r: &Repo, file: &str, branch: &str, add: &str) {
    let path = r.root.join(file);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}{add}")).unwrap();
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: edit on {branch}")]);
}

/// Two branches off `dev`, each appending to `file` at the same spot, the end.
fn two_additions(r: &Repo, file: &str, on_a: &str, on_dev: &str) {
    r.git(&["switch", "-q", "-c", "a"]);
    append(r, file, "a", on_a);
    r.git(&["switch", "-q", "dev"]);
    append(r, file, "dev", on_dev);
}

fn merge(r: &Repo) -> std::process::Output {
    r.git_out(&["merge", "a", "-m", "chore: merge a"])
}

#[test]
fn the_wiring_names_each_file_and_no_directory_and_is_written_once() {
    let r = repo();
    let attrs = fs::read_to_string(r.root.join(".gitattributes")).unwrap();
    for line in [
        "/content/strings/en.toml merge=mockspace-append",
        "/mock/crates/*/SHAME.md.tmpl merge=mockspace-append",
        "/mock/crates/*/BACKLOG.md.tmpl merge=mockspace-append",
    ] {
        assert_eq!(attrs.matches(line).count(), 1, "{line} in {attrs}");
    }
    let driver = r.git(&["config", "--local", "merge.mockspace-append.driver"]);
    assert!(
        driver.contains("cargo mock merge-additions %O %A %B %P %L"),
        "{driver}"
    );
    r.generate();
    assert_eq!(
        fs::read_to_string(r.root.join(".gitattributes")).unwrap(),
        attrs,
        "a second run changes nothing"
    );
}

#[test]
fn control_without_the_driver_two_additions_at_the_end_conflict() {
    let r = repo();
    two_additions(
        &r,
        DECLARED,
        "\"from-a\" = \"x\"\n",
        "\"from-dev\" = \"y\"\n",
    );
    r.git(&["config", "--local", "--remove-section", "merge.mockspace-append"]);
    let merged = merge(&r);
    assert!(
        !merged.status.success(),
        "the fixture must conflict without the driver: {}",
        Repo::failure(&merged)
    );
    assert_eq!(r.unmerged(), [DECLARED]);
}

#[test]
fn two_additions_to_a_declared_file_both_stay_ours_first_with_no_conflict() {
    let r = repo();
    two_additions(
        &r,
        DECLARED,
        "\"from-a\" = \"x\"\n",
        "\"from-dev\" = \"y\"\n",
    );
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.unmerged().is_empty());
    // `dev` is ours, `a` is theirs
    assert_eq!(
        fs::read_to_string(r.root.join(DECLARED)).unwrap(),
        "\"a\" = \"one\"\n\"b\" = \"two\"\n\"from-dev\" = \"y\"\n\"from-a\" = \"x\"\n"
    );
    assert_eq!(r.git(&["status", "--porcelain", "--", DECLARED]), "");
}

#[test]
fn two_sections_appended_to_a_package_shame_both_stay() {
    let r = repo();
    two_additions(
        &r,
        SHAME,
        "\n## second\nthe gap a found\n",
        "\n## third\nthe gap dev found\n",
    );
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(
        fs::read_to_string(r.root.join(SHAME)).unwrap(),
        "# shame\n\n## first\nthe first gap\n\n## third\nthe gap dev found\n\n## second\nthe gap a found\n"
    );
}

#[test]
fn a_file_no_pattern_names_still_conflicts() {
    let r = repo();
    write(&r.root.join("notes.txt"), "one\n");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add notes"]);
    two_additions(&r, "notes.txt", "from a\n", "from dev\n");
    assert!(!merge(&r).status.success());
    assert_eq!(r.unmerged(), ["notes.txt"]);
}

/// The negative: a side that edited a line already there is not an append, so
/// the file is git's to merge, with conflict markers and nothing selected.
#[test]
fn one_side_editing_a_base_line_still_conflicts_and_keeps_both_texts() {
    let r = repo();
    r.git(&["switch", "-q", "-c", "a"]);
    let path = r.root.join(DECLARED);
    let base = fs::read_to_string(&path).unwrap();
    fs::write(&path, base.replace("\"two\"", "\"two, edited on a\"")).unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit on a"]);
    r.git(&["switch", "-q", "dev"]);
    append(&r, DECLARED, "dev", "\"from-dev\" = \"y\"\n");

    let merged = merge(&r);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(r.unmerged(), [DECLARED]);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("<<<<<<< ours").count(), 1, "{text}");
    assert!(
        text.contains("two, edited on a") && text.contains("from-dev"),
        "neither side was dropped: {text}"
    );
}

/// Both sides change the file, so git does call the driver: `a` edits the first
/// line and `dev` appends past the second, in hunks that do not touch. The
/// driver leaves it to the text merge, which takes both as git would.
#[test]
fn an_edit_and_an_append_in_apart_hunks_are_taken_as_git_would() {
    let r = repo();
    r.git(&["switch", "-q", "-c", "a"]);
    let path = r.root.join(DECLARED);
    let base = fs::read_to_string(&path).unwrap();
    fs::write(&path, base.replace("\"one\"", "\"one, edited on a\"")).unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit on a"]);
    r.git(&["switch", "-q", "dev"]);
    append(&r, DECLARED, "dev", "\"from-dev\" = \"y\"\n");
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let stderr = String::from_utf8_lossy(&merged.stderr);
    assert!(
        stderr.contains("a side changed a line that was already there"),
        "the driver was not called: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "\"a\" = \"one, edited on a\"\n\"b\" = \"two\"\n\"from-dev\" = \"y\"\n"
    );
}
