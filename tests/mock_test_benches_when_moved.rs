//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test` builds the benches tree only when a bench is what changed or
//! what was asked for.
//!
//! The benches tree compiles every arm into its own cdylib, which is the most
//! expensive thing the command does and was done on every run, under `-p` and
//! with nothing changed alike. The tree here is the one `mock bench init`
//! makes: a root `bench.toml`, a bench with a `bench.toml`, a manifest-less arm
//! and a support crate that depends on a workspace crate by path, the way
//! kaski's do. The assertions are on the `=== benches` header, which is printed
//! exactly when the tree is run, on the note printed when it is left out, and
//! on the last line of the run.
//!
//! One scenario walks the whole life of the tree, since each run builds the
//! arms and a fixture per case would build them again.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Mutex;

/// One arm at a time, for the reason `mock_test_reaches_every_tree` gives.
static LOCK: Mutex<()> = Mutex::new(());

const RAN: &str = "=== benches";
const LEFT_OUT: &str = "benches : not run";
const DEP: &str = "crates/bsd-dep/src/lib.rs";
const DEP_OK: &str = "pub fn v() -> u8 {\n    1\n}\n";
const DEP_BROKEN: &str = "pub fn v() -> u8 {\n    2\n}\n";
const DEP_FIXED: &str = "pub fn v() -> u8 {\n    0 + 1\n}\n";

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A repository with one workspace member, one workspace crate outside `mock/`,
/// and the canonical benches tree whose support crate depends on that crate.
fn fixture(root: &Path) {
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    write(&root.join(".gitignore"), "target/\n");
    write(
        &root.join("mockspace.toml"),
        "[project]\nname = \"fixture\"\n",
    );
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"m\"]\n",
    );
    write(
        &root.join("mock/m/Cargo.toml"),
        "[package]\nname = \"a-member\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("mock/m/src/lib.rs"), "");

    write(
        &root.join("crates/bsd-dep/Cargo.toml"),
        "[package]\nname = \"bsd-dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&root.join(DEP), DEP_OK);

    let repo = env!("CARGO_MANIFEST_DIR");
    let benches = root.join("mock/benches");
    write(
        &benches.join("bench.toml"),
        &format!(
            "[build]\nmockspace = '{{ path = \"{repo}\" }}'\nopt-level = 0\nlto = \"off\"\n\
             codegen-units = 16\n\n[timing]\npasses = 1\nruns_per_pass = 20\nbatch_size = 5\n\
             harness_runs = 1\ncooldowns_ms = [0]\n"
        ),
    );
    write(
        &benches.join("sample/bench.toml"),
        "title = \"Sample\"\nworkload = \"default\"\narms = [\"a\"]\npoints = [64]\n\
         master_seed = 7\n",
    );
    write(
        &benches.join("sample/support/bsd-support/Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"bsd-support\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nbsd-dep = { path = \"../../../../../crates/bsd-dep\" }\n",
    );
    write(
        &benches.join("sample/support/bsd-support/src/lib.rs"),
        "pub fn v() -> u8 {\n    bsd_dep::v()\n}\n",
    );
    // The arm asserts on the workspace crate through the support crate, so a
    // change to that crate is a change that breaks this bench.
    write(
        &benches.join("sample/arms/a/src/lib.rs"),
        "#[test]\nfn holds() {\n    assert_eq!(bsd_support::v(), 1);\n}\n",
    );
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

/// The closing line of the run: the last one `mock test` itself prints, since
/// cargo and the notes write to the same stream after it.
fn last_line(t: &str) -> &str {
    t.lines()
        .rev()
        .find(|l| l.starts_with("mock test: "))
        .unwrap_or("")
}

#[test]
fn the_benches_tree_runs_when_it_moved_or_is_asked_and_not_otherwise() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);

    // A tree never shown green runs, and passes.
    let (o, t) = run(root, &[]);
    assert!(t.contains(RAN), "{t}");
    assert!(o.status.success(), "the fixture's benches must pass:\n{t}");

    // Nothing moved: left out, and the last line says so.
    let (o, t) = run(root, &[]);
    assert!(!t.contains(RAN), "benches ran with nothing changed:\n{t}");
    assert!(t.contains(LEFT_OUT), "{t}");
    assert!(o.status.success(), "{t}");
    assert!(
        last_line(&t).contains("benches not run"),
        "the last line must name the skipped benches: {:?}",
        last_line(&t)
    );

    // What `mock bench run` writes is output, not input.
    write(&root.join("mock/benches/results/sample/a.csv"), "1,2\n");
    write(&root.join("mock/benches/history/sample/a.tsv"), "a\n");
    let (_, t) = run(root, &[]);
    assert!(!t.contains(RAN), "generated output reran the benches:\n{t}");

    // A workspace crate a support crate depends on changes and breaks the
    // bench, which has to be seen the run it happens.
    write(&root.join(DEP), DEP_BROKEN);
    let (o, t) = run(root, &[]);
    assert!(
        t.contains(RAN),
        "a changed dependency of a bench was missed:\n{t}"
    );
    assert!(
        !o.status.success(),
        "the broken bench must fail the run:\n{t}"
    );

    // Repaired differently from how it was, it runs again and passes, then
    // stands. Put back exactly as it was when last green it would be skipped,
    // which is the same tree.
    write(&root.join(DEP), DEP_FIXED);
    let (o, t) = run(root, &[]);
    assert!(t.contains(RAN), "{t}");
    assert!(o.status.success(), "{t}");
    let (_, t) = run(root, &[]);
    assert!(!t.contains(RAN), "{t}");

    // The toolchain pin is an input to every build.
    write(
        &root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"nightly\"\n",
    );
    let (_, t) = run(root, &["--benches"]);
    assert!(t.contains(RAN), "{t}");

    // A named crate is about that crate.
    write(&root.join(DEP), DEP_BROKEN);
    let (o, t) = run(root, &["-p", "a-member"]);
    assert!(!t.contains(RAN), "benches ran under -p:\n{t}");
    assert!(t.contains(LEFT_OUT), "{t}");
    assert!(o.status.success(), "{t}");
    write(&root.join(DEP), DEP_FIXED);

    // Asking beats an unchanged record, alone and beside a named crate.
    let (_, t) = run(root, &["--benches"]);
    assert!(t.contains(RAN), "{t}");
    let (_, t) = run(root, &["-p", "a-member", "--benches"]);
    assert!(t.contains(RAN), "{t}");
    assert!(t.contains("workspace members"), "{t}");
}
