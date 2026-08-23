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
//! These arms never let cargo build anything: each fixture's crates are
//! deliberately unbuildable stubs, and the assertions are on which trees the
//! command NAMES. Building them would test cargo rather than the discovery,
//! and would put minutes on a suite that is checking a directory walk.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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
    write(&root.join("mockspace.toml"), "[project]\nname = \"fixture\"\n");
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

fn run(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(["--dir", "mock", "test"])
        .current_dir(root)
        .env("CARGO_BUILD_JOBS", "2")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn it_names_every_tool_and_bench_tree() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), None, &["alpha", "zeta"], &["one_arm"]);
    let t = text(&run(tmp.path()));
    for name in ["alpha", "zeta", "one_arm"] {
        assert!(t.contains(name), "did not name `{name}`:\n{t}");
    }
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
    assert!(t.contains("alpha"), "did not name the tree that is there:\n{t}");
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
    let stripped = fs::read_to_string(&manifest).unwrap().replace("[workspace]\n\n", "");
    fs::write(&manifest, stripped).unwrap();

    let t = text(&run(tmp.path()));
    assert!(t.contains("alpha"), "the unreachable crate went unmentioned:\n{t}");
    assert!(
        t.contains("[workspace]"),
        "it did not name the one-line fix:\n{t}"
    );
}
