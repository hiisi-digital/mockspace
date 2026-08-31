//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test` finds the trees a plain `cargo test` cannot reach.
//!
//! The whole point of the subcommand is discovery: bench crates, tool crates
//! and the generated lint crate are compiled by mockspace outside the
//! consumer's workspace, so a `cargo test` run there reaches the members and
//! nothing else. A repository whose `members` list is empty runs no tests at
//! all while appearing to.
//!
//! So what must not go wrong is the finding. A version that reported only the
//! workspace, or that silently skipped a tree it could not enter, would read
//! exactly like one that works.
//!
//! The fixtures are minimal crates, and cargo does build them: an earlier
//! version of this comment claimed they were unbuildable stubs and that no arm
//! let cargo build anything, which was simply false, and the runs leave a
//! `target/` behind to prove it. They are kept minimal so that stays cheap.
//!
//! The assertions are on which trees the command NAMES and on the status it
//! exits with. The second was missing entirely at first: mutating the failure
//! path to `SUCCESS` left all five arms green, which for a test runner is the
//! one property that must not be able to break silently.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Mutex;

/// One arm at a time.
///
/// Every arm spawns cargo against one shared `CARGO_TARGET_DIR`, with fixture
/// crates that share package names across arms at different paths. Run
/// concurrently they contend, and the arm asserting a failing tree fails the
/// run passed alone and failed in the suite, which is the worst way for a test
/// to be wrong: it accuses the code and the fault is in the harness.
static LOCK: Mutex<()> = Mutex::new(());

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A mock workspace with the trees named in `tools` and `benches`, and a
/// `members` list that is empty unless `member` is given.
fn fixture(root: &Path, member: Option<&str>, tools: &[&str], benches: &[&str]) {
    let members = member.map(|m| format!("\"{m}\"")).unwrap_or_default();
    write(
        &root.join("mock/Cargo.toml"),
        &format!("[workspace]\nresolver = \"2\"\nmembers = [{members}]\n"),
    );
    write(
        &root.join("mockspace.toml"),
        "[project]\nname = \"fixture\"\n",
    );
    fs::create_dir_all(root.join(".git")).unwrap();
    if let Some(m) = member {
        write(
            &root.join("mock").join(m).join("Cargo.toml"),
            "[package]\nname = \"a-member\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        );
        write(&root.join("mock").join(m).join("src/lib.rs"), "");
    }
    // Each carries the empty `[workspace]` that makes it its own root, which is
    // the shape the command tells a consumer to adopt. Without it the crate is
    // reported as unreachable instead, which is the arm below.
    for (dir, names) in [("tools", tools), ("benches", benches)] {
        for n in names.iter() {
            write(
                &root.join("mock").join(dir).join(n).join("Cargo.toml"),
                &format!(
                    "[workspace]\n\n[package]\nname = \"{n}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"
                ),
            );
            write(&root.join("mock").join(dir).join(n).join("src/lib.rs"), "");
        }
    }
}

/// Run it, returning the whole `Output` so an arm can assert the status.
///
/// `text()` alone discards `Output::status`, which is how every arm here came
/// to assert what was printed and none to assert whether the command
/// succeeded.
fn run(root: &Path) -> Output {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(["--dir", "mock", "test"])
        .current_dir(root)
        .env("CARGO_BUILD_JOBS", "2")
        .output()
        .unwrap()
}

fn ok(o: &Output) -> bool {
    o.status.success()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn it_names_every_tool_tree() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &["alpha", "zeta"], &[]);
    let o = run(tmp.path());
    let t = text(&o);
    for name in ["alpha", "zeta"] {
        assert!(t.contains(name), "did not name `{name}`:\n{t}");
    }
    assert!(ok(&o), "every tree is green and it did not exit zero:\n{t}");
}

#[test]
fn it_exits_non_zero_when_a_tree_fails() {
    // The property a test runner exists for, and the one no arm asserted. With
    // the failure path mutated to SUCCESS every other arm here stayed green.
    let tmp = tempfile::tempdir().unwrap();
    // A crate name no other arm uses. Every arm builds into one shared
    // CARGO_TARGET_DIR, so two crates called `alpha` at different temporary
    // paths collide on cargo's fingerprint, and this arm reused the artifact
    // another arm had built from an EMPTY lib.rs: the tests ran, zero of them
    // failed, and the tree passed. It passed alone and failed in the suite,
    // which is the worst shape for a wrong test, because it accuses the code.
    fixture(tmp.path(), None, &["gamma_that_fails"], &[]);
    // A test that fails rather than a crate that does not build, so the failure
    // is the suite's rather than the compiler's.
    fs::write(
        tmp.path().join("mock/tools/gamma_that_fails/src/lib.rs"),
        "#[test]\nfn fails() { panic!(\"this arm must make the run fail\"); }\n",
    )
    .unwrap();
    let o = run(tmp.path());
    assert!(
        !ok(&o),
        "a failing tree did not fail the run:\n{}",
        text(&o)
    );
}

#[test]
fn it_does_not_claim_a_tree_that_is_not_there() {
    // The control for the arm above. Without it, a version printing every name
    // it was ever given, or matching too loosely, satisfies that arm and is
    // wrong. `zeta` is absent from this fixture and must be absent from the
    // output.
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &["alpha"], &[]);
    let t = text(&run(tmp.path()));
    // `=== tool :` rather than the bare name, because the same name appears in
    // the orphan report below and one substring cannot mean both "discovered"
    // and "rejected". A regression classifying every tool as an orphan kept the
    // bare-name form of this arm green.
    assert!(
        t.contains("=== tool :") && t.contains("alpha"),
        "did not discover the tree that is there:\n{t}"
    );
    assert!(!t.contains("zeta"), "named a tree that is not there:\n{t}");
}

#[test]
fn a_memberless_workspace_is_a_note_and_not_a_failure() {
    // A virtual manifest with no members does not run zero tests, it errors:
    // "the manifest is virtual, and the workspace has no members". Passing that
    // through means the one tree a plain `cargo test` was supposed to cover
    // reports a hard failure caused by nothing being there.
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &["alpha"], &[]);
    let t = text(&run(tmp.path()));
    assert!(
        t.contains("declares no workspace member"),
        "it did not say why the workspace tree was skipped:\n{t}"
    );
    assert!(
        !t.contains("manifest is virtual"),
        "cargo's own error reached the caller:\n{t}"
    );
}

#[test]
fn a_member_is_reached_when_there_is_one() {
    // The control for the arm above: without it, always skipping the workspace
    // would satisfy it. A declared member must put the workspace tree back.
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), Some("crates/a-member"), &[], &[]);
    let t = text(&run(tmp.path()));
    assert!(
        t.contains("workspace members"),
        "a workspace with a member was still skipped:\n{t}"
    );
    assert!(
        !t.contains("declares no workspace member"),
        "it reported no member while one is declared:\n{t}"
    );
}

#[test]
fn a_crate_that_is_neither_member_nor_root_is_named_with_its_fix() {
    // Cargo refuses this outright, and its own message suggests adding the
    // crate to `members`. That is the wrong direction: membership puts a cdylib
    // into the consumer's dependency graph. The one-line fix is reported
    // instead, and the crate is not silently dropped from the run.
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &["alpha"], &[]);
    let manifest = tmp.path().join("mock/tools/alpha/Cargo.toml");
    let stripped = fs::read_to_string(&manifest)
        .unwrap()
        .replace("[workspace]\n\n", "");
    fs::write(&manifest, stripped).unwrap();

    let t = text(&run(tmp.path()));
    assert!(
        t.contains("cannot be tested where they sit") && t.contains("alpha"),
        "the unreachable crate was not reported as one:\n{t}"
    );
    assert!(
        t.contains("[workspace]"),
        "it did not name the one-line fix:\n{t}"
    );
    assert!(
        !t.contains("=== tool : ") || !t.contains("/alpha ==="),
        "an orphan was also reported as a discovered tree:\n{t}"
    );
}

#[test]
fn it_reaches_a_bench_tree_that_has_no_manifest_anywhere() {
    // The defect this arm exists for. `mock bench init` scaffolds
    // `benches/<bench>/arms/<arm>/` with no Cargo.toml anywhere, because the
    // arm manifests are generated on demand under `target/`. A one-level walk
    // for a directory containing a manifest therefore finds nothing on the
    // canonical layout and reports nothing to run, on a tree that plainly has
    // an arm in it, which `src/bench.rs` records having already fixed once.
    //
    // So the bench third is delegated to `mock bench test` rather than walked,
    // and this asserts the tree is reached at all rather than silently absent.
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &[], &[]);
    write(
        &tmp.path()
            .join("mock/benches/sample/arms/sample/src/lib.rs"),
        "pub fn nothing() {}\n",
    );
    let t = text(&run(tmp.path()));
    assert!(
        t.contains("benches"),
        "a bench tree with no manifest was not reached:\n{t}"
    );
}

#[test]
fn it_says_there_is_nothing_when_there_is_nothing() {
    // The control for the arm above: without it, a version that always prints
    // the bench heading would satisfy it.
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &[], &[]);
    let t = text(&run(tmp.path()));
    assert!(
        t.contains("no tree to test"),
        "an empty workspace did not say so:\n{t}"
    );
}
