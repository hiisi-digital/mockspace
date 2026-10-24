//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test` builds the benches tree only when a bench is what changed or
//! what was asked for.
//!
//! The benches tree compiles every arm into its own cdylib, which is the most
//! expensive thing the command does and was done on every run, under `-p` and
//! with nothing changed alike. The arms here assert on the `=== benches`
//! header, which is printed exactly when the tree is run, and on the note
//! printed when it is left out.

mod common;

use std::fs;
use std::path::Path;
use std::process::Output;
use std::sync::Mutex;

/// One arm at a time, for the reason `mock_test_reaches_every_tree` gives.
static LOCK: Mutex<()> = Mutex::new(());

const RAN: &str = "=== benches";
const LEFT_OUT: &str = "benches : not run";
const ARM: &str = "mock/benches/sample/src/lib.rs";
const ARM_SRC: &str = "#[test]\nfn passes() {}\n";

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A workspace with one member, `a-member`, and one bench crate that carries a
/// passing test, since a green record is only written for a tree that passed.
fn fixture(root: &Path) {
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"m\"]\n",
    );
    write(
        &root.join("mockspace.toml"),
        "[project]\nname = \"fixture\"\n",
    );
    fs::create_dir_all(root.join(".git")).unwrap();
    write(
        &root.join("mock/m/Cargo.toml"),
        "[package]\nname = \"a-member\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("mock/m/src/lib.rs"), "");
    write(
        &root.join("mock/benches/sample/Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"sample\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&root.join(ARM), ARM_SRC);
}

fn run(root: &Path, extra: &[&str]) -> (Output, String) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let o = common::mockspace()
        .args(["--dir", "mock", "test"])
        .args(extra)
        .current_dir(root)
        .env("CARGO_BUILD_JOBS", "2")
        .output()
        .unwrap();
    let t = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o, t)
}

/// A run that names a crate is about that crate, so the benches tree stays out
/// even though it has never passed. This is the arm that fails on the old code.
#[test]
fn a_named_crate_does_not_build_the_benches() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let (o, t) = run(tmp.path(), &["-p", "a-member"]);
    assert!(!t.contains(RAN), "benches ran under -p:\n{t}");
    assert!(t.contains(LEFT_OUT), "the omission was not reported:\n{t}");
    assert!(o.status.success(), "{t}");
}

/// Control for the arm above: the same fixture with no `-p` does run them, so
/// the absence above is the selection and not a fixture with no benches.
#[test]
fn a_bare_run_builds_a_tree_that_never_passed() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let (_, t) = run(tmp.path(), &[]);
    assert!(t.contains(RAN), "{t}");
}

#[test]
fn a_second_run_with_nothing_changed_leaves_the_benches_out() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let (first, t1) = run(tmp.path(), &[]);
    assert!(t1.contains(RAN), "{t1}");
    assert!(
        first.status.success(),
        "the fixture's benches must pass:\n{t1}"
    );
    let (_, t2) = run(tmp.path(), &[]);
    assert!(
        !t2.contains(RAN),
        "benches ran again with nothing changed:\n{t2}"
    );
    assert!(t2.contains(LEFT_OUT), "{t2}");
}

#[test]
fn a_changed_arm_builds_the_benches_again() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let (first, t1) = run(tmp.path(), &[]);
    assert!(first.status.success(), "{t1}");
    write(
        &tmp.path().join(ARM),
        "#[test]\nfn passes() { let _ = 1; }\n",
    );
    let (_, t2) = run(tmp.path(), &[]);
    assert!(
        t2.contains(RAN),
        "an edited arm did not run the benches:\n{t2}"
    );
}

#[test]
fn asking_for_them_builds_the_benches_with_nothing_changed() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let (first, t1) = run(tmp.path(), &[]);
    assert!(first.status.success(), "{t1}");
    let (_, t2) = run(tmp.path(), &["--benches"]);
    assert!(t2.contains(RAN), "--benches did not run them:\n{t2}");
}

#[test]
fn asking_for_them_beside_a_named_crate_builds_both() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let (_, t) = run(tmp.path(), &["-p", "a-member", "--benches"]);
    assert!(t.contains(RAN), "{t}");
    assert!(t.contains("workspace members"), "{t}");
}
