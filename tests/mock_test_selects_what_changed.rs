//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test` under nextest, end to end, over one repository through the
//! sequence a piece of work goes through: a first run, a run after nothing, a
//! change in a dependency, a full run, a change in content a test reads, and a
//! cheap pass.
//!
//! The unit tests under `src/suite/` pin each decision. This is the arm that
//! fails if the decisions are never consulted, if the history is never written
//! back, or if nextest is handed a config or a filterset it refuses.
//!
//! Needs cargo-nextest, and says so and passes without it, since a machine
//! without it runs the members under `cargo test` and that path is
//! `mock_test_reaches_every_tree.rs`'s.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

fn append(p: &Path, s: &str) {
    let mut t = fs::read_to_string(p).unwrap();
    t.push_str(s);
    fs::write(p, t).unwrap();
}

/// `base` is a library, `uses` depends on it, reads `content/rows.txt` with
/// `include_str!` and has one test that sleeps past the heavy threshold, and
/// `alone` depends on nothing.
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
        "pub const ROWS: &str = include_str!(\"../../../../content/rows.txt\");\n\
         #[test]\nfn quick_uses() { assert_eq!(base::four(), 4); }\n\
         #[test]\nfn slow_drawn() {\n\
         \x20   std::thread::sleep(std::time::Duration::from_millis(900));\n\
         \x20   assert!(!ROWS.is_empty());\n}\n",
    );
    write(
        &root.join("mock/crates/alone/Cargo.toml"),
        &pkg("alone", ""),
    );
    write(
        &root.join("mock/crates/alone/src/lib.rs"),
        "#[test]\nfn quick_alone() {}\n",
    );
    write(&root.join("content/rows.txt"), "one row\n");
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

#[test]
fn a_piece_of_work_runs_only_what_it_reaches() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);

    // First run: no history, everything, and the timings recorded.
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");
    for m in ["alone", "base", "uses"] {
        assert!(
            t.contains(&format!("run      {m}: no earlier pass on record")),
            "first run, {m}:\n{t}"
        );
    }
    assert!(
        t.contains("uses slow_drawn"),
        "the slowest list names the slow test:\n{t}"
    );

    // Nothing changed: nothing runs.
    let (ok, t) = mock(root, &[]);
    assert!(ok, "second run:\n{t}");
    assert!(t.contains("nothing to run"), "after none:\n{t}");

    // A one-line change in a dependency: it and its dependent, not `alone`.
    append(
        &root.join("mock/crates/base/src/lib.rs"),
        "pub fn three() -> u32 { 3 }\n",
    );
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after a dependency changed:\n{t}");
    assert!(t.contains("run      base: its own inputs moved"), "{t}");
    assert!(t.contains("run      uses: moved: base"), "{t}");
    assert!(
        t.contains("settled  1 member(s) passed at what they are now: alone"),
        "{t}"
    );
    assert!(
        t.contains("heavy    1 test(s)"),
        "the slow test is known heavy now:\n{t}"
    );
    assert!(
        !t.contains("cached"),
        "its pass was at the old fingerprint:\n{t}"
    );

    // A full run: every member, and the heavy test's pass still stands.
    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "full:\n{t}");
    assert!(t.contains("run      alone: asked for"), "{t}");
    assert!(t.contains("cached   1 heavy test(s) not run"), "{t}");
    assert!(t.contains("uses slow_drawn"), "{t}");
    // ... unless the cache is off.
    let (ok, t) = mock(root, &["--full", "--no-cache"]);
    assert!(ok, "full without cache:\n{t}");
    assert!(!t.contains("cached"), "{t}");

    // Content the dependent includes: only it runs, and the heavy test with it.
    append(&root.join("content/rows.txt"), "two rows\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after content changed:\n{t}");
    assert!(
        t.contains("run      uses: its own inputs moved (reads 1 path(s) outside itself)"),
        "{t}"
    );
    assert!(t.contains("settled  2 member(s)"), "{t}");
    assert!(!t.contains("cached"), "{t}");

    // A cheap pass after the dependent moved defers its heavy test, and leaves
    // it owed: the next run runs `uses` again, heavy test included.
    append(&root.join("mock/crates/uses/src/lib.rs"), "// one line\n");
    let (ok, t) = mock(root, &["--cheap"]);
    assert!(ok, "cheap:\n{t}");
    assert!(t.contains("deferred 1 heavy test(s) not run"), "{t}");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after cheap:\n{t}");
    assert!(
        t.contains("run      uses"),
        "a cheap pass must not leave its member green:\n{t}"
    );
    assert!(!t.contains("deferred"), "{t}");

    // A failing test fails the run and leaves its member owed.
    append(
        &root.join("mock/crates/alone/src/lib.rs"),
        "#[test]\nfn breaks() { panic!(\"this arm must fail the run\"); }\n",
    );
    let (ok, t) = mock(root, &[]);
    assert!(!ok, "a failing test did not fail the run:\n{t}");
    let (ok, t) = mock(root, &[]);
    assert!(
        !ok && t.contains("run      alone"),
        "a failed member must run again:\n{t}"
    );
}
