//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::fs;
use std::path::Path;

use super::*;

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

fn arm(root: &Path) -> std::path::PathBuf {
    let benches = root.join("benches");
    write(&benches.join("sample/bench.toml"), "name = \"sample\"\n");
    write(&benches.join("sample/arms/a/src/lib.rs"), "pub fn f() {}\n");
    benches
}

/// A `-p` naming other crates asks about those crates, so the benches tree
/// stays out even when its inputs have never been recorded green.
#[test]
fn a_named_crate_leaves_the_benches_out() {
    assert_eq!(
        decide(false, true, "x", None),
        Verdict::Skip(Skip::OtherCratesNamed)
    );
    assert_eq!(
        decide(false, true, "x", Some("y")),
        Verdict::Skip(Skip::OtherCratesNamed)
    );
}

/// Control for the arm above: without a `-p` and with no record, the tree has
/// never been shown green and runs.
#[test]
fn a_tree_never_green_runs() {
    assert_eq!(
        decide(false, false, "x", None),
        Verdict::Run(Run::NeverGreen)
    );
}

#[test]
fn inputs_that_match_the_green_record_are_left_out() {
    assert_eq!(
        decide(false, false, "x", Some("x")),
        Verdict::Skip(Skip::Unchanged)
    );
}

#[test]
fn inputs_that_moved_since_the_green_record_run() {
    assert_eq!(
        decide(false, false, "x", Some("y")),
        Verdict::Run(Run::Moved)
    );
}

/// The explicit request beats an unchanged record and a `-p` alike.
#[test]
fn asking_runs_it_whatever_else_holds() {
    assert_eq!(
        decide(true, false, "x", Some("x")),
        Verdict::Run(Run::Asked)
    );
    assert_eq!(decide(true, true, "x", Some("x")), Verdict::Run(Run::Asked));
}

#[test]
fn the_digest_is_stable_over_an_unchanged_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let b = arm(tmp.path());
    assert_eq!(digest(&b), digest(&b));
}

#[test]
fn an_edited_arm_moves_the_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let b = arm(tmp.path());
    let before = digest(&b);
    write(&b.join("sample/arms/a/src/lib.rs"), "pub fn f() { 1; }\n");
    assert_ne!(before, digest(&b));
}

#[test]
fn an_added_support_file_moves_the_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let b = arm(tmp.path());
    let before = digest(&b);
    write(&b.join("support/src/lib.rs"), "");
    assert_ne!(before, digest(&b));
}

/// What the bench build itself writes under a `target/` is output, not input;
/// counting it would move the digest on every run and never let a skip stand.
#[test]
fn build_output_does_not_move_the_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let b = arm(tmp.path());
    let before = digest(&b);
    write(&b.join("sample/arms/a/target/debug/junk"), "built");
    assert_eq!(before, digest(&b));
}

/// A rename with the same bytes is a different tree.
#[test]
fn a_renamed_file_moves_the_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let b = arm(tmp.path());
    let before = digest(&b);
    fs::rename(
        b.join("sample/arms/a/src/lib.rs"),
        b.join("sample/arms/a/src/other.rs"),
    )
    .unwrap();
    assert_ne!(before, digest(&b));
}

#[test]
fn a_green_record_reads_back_and_a_missing_one_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(recorded(tmp.path()), None);
    record_green(tmp.path(), "abc");
    assert_eq!(recorded(tmp.path()).as_deref(), Some("abc"));
    record_green(tmp.path(), "def");
    assert_eq!(recorded(tmp.path()).as_deref(), Some("def"));
}
