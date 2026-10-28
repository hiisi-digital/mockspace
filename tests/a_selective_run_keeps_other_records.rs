//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A `mock test` run for some members leaves the test-history record of every
//! other member exactly as it was, and removes a record only where its member is
//! no longer in the workspace.
//!
//! The records are tracked, so a run that deletes or rewrites the file of a
//! member it never ran shows up as a change in every clone's tree, and from
//! there into commits that were about something else. The unit tests under
//! `src/suite/` pin the save; this is the arm that fails if the run hands the
//! save the wrong idea of whose files are its own.
//!
//! Needs cargo-nextest, and says so and passes without it, as
//! `mock_test_selects_what_changed.rs` does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// `base` is a library, `uses` depends on it and has one test that sleeps past
/// the heavy threshold, and `alone` depends on nothing.
fn fixture(root: &Path) {
    write(
        &root.join("mockspace.toml"),
        "project_name = \"fixture\"\nmock_dir = \"mock\"\n\n[test]\nheavy_after_secs = 0.5\n",
    );
    write(&root.join(".gitignore"), "target/\n");
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/base\", \"crates/uses\", \"crates/alone\"]\n",
    );
    let pkg = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\n{deps}"
        )
    };
    write(&root.join("mock/crates/base/Cargo.toml"), &pkg("base", ""));
    write(
        &root.join("mock/crates/base/src/lib.rs"),
        "pub fn four() -> u32 { 4 }\n#[test]\nfn quick_base() { assert_eq!(four(), 4); }\n",
    );
    write(
        &root.join("mock/crates/uses/Cargo.toml"),
        &pkg("uses", "base = { path = \"../base\" }\n"),
    );
    write(
        &root.join("mock/crates/uses/src/lib.rs"),
        "#[test]\nfn quick_uses() { assert_eq!(base::four(), 4); }\n\
         #[test]\nfn slow_drawn() {\n\
         \x20   std::thread::sleep(std::time::Duration::from_millis(900));\n}\n",
    );
    write(
        &root.join("mock/crates/alone/Cargo.toml"),
        &pkg("alone", ""),
    );
    write(
        &root.join("mock/crates/alone/src/lib.rs"),
        "#[test]\nfn quick_alone() {}\n",
    );
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init");
}

fn mock(root: &Path, args: &[&str]) -> (bool, String) {
    let o: Output = Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(["--dir", "mock", "test"])
        .args(args)
        .current_dir(root)
        // The fixture's own `target/`, whatever the caller shares.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env("CARGO_BUILD_JOBS", "2")
        .output()
        .unwrap();
    let t = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.success(), t)
}

fn nextest_here() -> bool {
    Command::new("cargo")
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn record(root: &Path, member: &str) -> PathBuf {
    root.join("mock/test-history/default")
        .join(format!("{member}.json"))
}

/// A record file with nothing in it: the shape a member's file has once its
/// green record was dropped, as a merge of two moved greens does.
fn blank(member: &str) -> String {
    format!(
        "{{\n  \"version\": 2,\n  \"flavour\": \"\",\n  \"member\": \"{member}\",\n  \"green\": null,\n  \"tests\": {{}},\n  \"bins\": {{}}\n}}\n"
    )
}

/// Every run is made after `uses` changed, so it has something to run, and
/// after the history holds a record of every member.
fn prepared() -> Option<tempfile::TempDir> {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");
    for m in ["alone", "base", "uses"] {
        assert!(
            record(root, m).exists(),
            "no record of {m} after a full run:\n{t}"
        );
    }
    let lib = root.join("mock/crates/uses/src/lib.rs");
    let mut text = fs::read_to_string(&lib).unwrap();
    text.push_str("// one line\n");
    fs::write(&lib, text).unwrap();
    Some(tmp)
}

const RUNS: &[&[&str]] = &[&["-p", "uses"], &["--cheap", "-p", "uses"]];

/// The other members' files stand byte for byte: one a merge left blank, one of
/// the earlier version, which a save would otherwise write out as version 2 with
/// its green gone.
#[test]
fn a_run_for_one_member_leaves_the_others_files_as_they_are() {
    for args in &RUNS[.. 2] {
        let Some(tmp) = prepared() else { return };
        let root = tmp.path();
        let older = fs::read_to_string(record(root, "base"))
            .unwrap()
            .replace("\"version\": 2", "\"version\": 1");
        fs::write(record(root, "base"), &older).unwrap();
        fs::write(record(root, "alone"), blank("alone")).unwrap();

        let (ok, t) = mock(root, args);
        assert!(ok, "{args:?}:\n{t}");
        assert!(t.contains("run      uses"), "{args:?} ran uses:\n{t}");

        assert_eq!(
            fs::read_to_string(record(root, "alone")).ok().as_deref(),
            Some(blank("alone").as_str()),
            "{args:?}: the blank record of a member the run did not cover"
        );
        assert_eq!(
            fs::read_to_string(record(root, "base")).ok().as_deref(),
            Some(older.as_str()),
            "{args:?}: the older record of a member the run did not cover"
        );
        // A cheap run leaves the member owed its heavy test, so only the other
        // run settles it green.
        if args[0] != "--cheap" {
            assert!(
                fs::read_to_string(record(root, "uses")).is_ok_and(|t| t.contains("\"full\"")),
                "{args:?}: the member that ran is recorded"
            );
        }
    }
}

/// A file the run cannot read is passed over when it loads, and stays when it
/// saves: it may be another version's, or a merge's half, and neither is the
/// run's to delete.
#[test]
fn a_file_the_run_could_not_read_stays() {
    let Some(tmp) = prepared() else { return };
    let root = tmp.path();
    let newer = blank("alone").replace("\"version\": 2", "\"version\": 3");
    let half = "<<<<<<< ours\n{}\n=======\n{}\n>>>>>>> theirs\n";
    fs::write(record(root, "alone"), &newer).unwrap();
    fs::write(record(root, "base"), half).unwrap();

    let (ok, t) = mock(root, RUNS[0]);
    assert!(ok, "{t}");
    assert_eq!(fs::read_to_string(record(root, "alone")).unwrap(), newer);
    assert_eq!(fs::read_to_string(record(root, "base")).unwrap(), half);
}

/// A record goes when its member is no longer in the workspace, whichever run
/// finds it, and in every flavour's directory.
#[test]
fn a_record_goes_only_with_its_member() {
    let Some(tmp) = prepared() else { return };
    let root = tmp.path();
    fs::write(record(root, "gone"), blank("gone")).unwrap();
    let other = root.join("mock/test-history/features-editor/gone.json");
    write(
        &other,
        &blank("gone").replace("\"flavour\": \"\"", "\"flavour\": \"--features editor\""),
    );
    fs::write(record(root, "alone"), blank("alone")).unwrap();

    let (ok, t) = mock(root, RUNS[0]);
    assert!(ok, "{t}");
    assert!(
        !record(root, "gone").exists(),
        "the removed member's record"
    );
    assert!(
        !other.exists(),
        "the removed member's record under another flavour"
    );
    assert!(
        record(root, "alone").exists(),
        "a member still in the workspace"
    );
}

/// The member that ran has files in other flavours too, from other runs, and a
/// run in the default flavour leaves those as they are.
#[test]
fn a_run_leaves_its_own_members_files_in_other_flavours() {
    let Some(tmp) = prepared() else { return };
    let root = tmp.path();
    let older = fs::read_to_string(record(root, "uses"))
        .unwrap()
        .replace("\"version\": 2", "\"version\": 1")
        .replace("\"flavour\": \"\"", "\"flavour\": \"--features editor\"");
    let editor = root.join("mock/test-history/features-editor/uses.json");
    write(&editor, &older);
    let release = root.join("mock/test-history/release/uses.json");
    let empty = blank("uses").replace("\"flavour\": \"\"", "\"flavour\": \"--release\"");
    write(&release, &empty);

    let (ok, t) = mock(root, RUNS[0]);
    assert!(ok, "{t}");
    assert_eq!(
        fs::read_to_string(&editor).ok().as_deref(),
        Some(older.as_str())
    );
    assert_eq!(
        fs::read_to_string(&release).ok().as_deref(),
        Some(empty.as_str())
    );
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
