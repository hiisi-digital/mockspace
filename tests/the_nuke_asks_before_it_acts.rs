//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `--nuke` end to end, through the binary and against a real repository.
//!
//! The unit tests decide what a plan holds and what applying one does. Neither
//! reaches the thing somebody actually types, and the thing somebody actually
//! types is what changed: the flag used to be refused on its own and to demand
//! a second word before it would act.
//!
//! Every check here was run by hand first, in a throwaway repository, while the
//! behaviour was being built. Kept so nobody runs it by hand again.
//!
//! The fixture points `core.hooksPath` at an empty directory. The engine
//! installs a durable pre-commit gate on its first run in any tree, and that
//! gate refuses a commit until the pin it reads resolves to a released engine,
//! which a temporary directory has no way to satisfy. What is under test here
//! is the nuke, not the gate.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git could not be run");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A committed repository with one crate, three modules and two designs.
fn a_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    let mock = root.join("mock");
    let src = mock.join("crates/alpha/src");
    std::fs::create_dir_all(src.join("inner")).unwrap();

    std::fs::write(mock.join("mockspace.toml"), "project_name = \"alpha\"\n").unwrap();
    std::fs::write(mock.join("DESIGN.md.tmpl"), "# design\n").unwrap();
    std::fs::write(mock.join("crates/alpha/DESIGN.md.tmpl"), "# crate\n").unwrap();
    std::fs::write(
        mock.join("crates/alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\n",
    )
    .unwrap();
    std::fs::write(src.join("lib.rs"), "pub mod other;\n").unwrap();
    std::fs::write(src.join("other.rs"), "pub fn other() {}\n").unwrap();
    std::fs::write(src.join("inner/mod.rs"), "pub fn i() {}\n").unwrap();

    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.invalid"]);
    git(root, &["config", "user.name", "t"]);
    // Once to let the engine write whatever it writes on a first run, then
    // commit the lot, because the nuke refuses a tree holding anything git does
    // not. Running the tool is what makes the tree dirty, so a fixture that
    // committed first would be dirty by the time the nuke looked.
    run(root, &["--check"]);

    // Only now, because the engine points `core.hooksPath` at its own durable
    // gate on every run, so an override set before this one does not survive it.
    std::fs::create_dir_all(root.join(".nohooks")).unwrap();
    git(root, &[
        "config",
        "core.hooksPath",
        &root.join(".nohooks").display().to_string(),
    ]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "one"]);
    d
}

/// The engine, on this repository, with nothing on standard input.
///
/// A closed input is what a pipe and a script both give, and it has to read as
/// a no rather than as an answer.
fn run(root: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .arg("--dir")
        .arg(root.join("mock"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("the engine could not be run");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("mock/crates/alpha/src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn the_flag_alone_names_every_file_and_then_asks() {
    // The whole point of the change. It used to print a sentence about a second
    // flag and exit non-zero without saying what was in the tree.
    let d = a_repo();
    let said = run(d.path(), &["--nuke"]);

    assert!(said.contains("mock/crates/alpha/src/other.rs"), "{said}");
    assert!(
        said.contains("mock/crates/alpha/src/inner/mod.rs"),
        "a nested module goes unnamed: {said}"
    );
    assert!(
        said.contains("stub"),
        "and says which one is stubbed: {said}"
    );
    assert!(said.contains("go ahead?"), "{said}");
    assert!(
        !said.contains("--i-mean-it"),
        "it still asks for a second flag: {said}"
    );
}

#[test]
fn a_closed_input_is_a_no_and_nothing_moves() {
    let d = a_repo();
    let before = sources(d.path());
    let said = run(d.path(), &["--nuke"]);

    assert!(said.contains("left alone"), "{said}");
    assert_eq!(before, sources(d.path()));
}

#[test]
fn the_design_tier_names_the_designs_as_well_as_the_source() {
    let d = a_repo();
    let said = run(d.path(), &["--nuke=docs"]);

    assert!(said.contains("mock/DESIGN.md.tmpl"), "{said}");
    assert!(said.contains("mock/crates/alpha/DESIGN.md.tmpl"), "{said}");
    assert!(
        said.contains("mock/crates/alpha/src/other.rs"),
        "the source too: {said}"
    );
    assert!(said.contains("go ahead?"), "{said}");
}

#[test]
fn a_source_nuke_leaves_a_stub_and_takes_the_empty_directory_with_it() {
    let d = a_repo();
    run(d.path(), &["--nuke", "--y"]);

    assert_eq!(sources(d.path()), vec![PathBuf::from(
        "mock/crates/alpha/src/lib.rs"
    )]);
    let stub = std::fs::read_to_string(d.path().join("mock/crates/alpha/src/lib.rs")).unwrap();
    assert!(stub.contains("Nuked by"), "{stub}");
    assert!(
        !d.path().join("mock/crates/alpha/src/inner").exists(),
        "an empty module directory reads as one somebody forgot to write"
    );
    // The tier held: the designs are what it is rewritten from.
    assert!(d.path().join("mock/DESIGN.md.tmpl").exists());
    assert!(d.path().join("mock/crates/alpha/DESIGN.md.tmpl").exists());
}

#[test]
fn a_tier_nobody_named_refuses_rather_than_falling_back_to_the_source() {
    // The control. Falling back on an unreadable tier turns a typo into a wipe.
    let d = a_repo();
    let before = sources(d.path());
    let said = run(d.path(), &["--nuke=deisgn", "--y"]);

    assert!(said.contains("deisgn"), "{said}");
    assert_eq!(before, sources(d.path()));
}

#[test]
fn a_dirty_tree_is_refused_whatever_the_answer_would_have_been() {
    // A separate guard from the question, and `--y` does not carry past it.
    // What the nuke deletes is only recoverable from git.
    let d = a_repo();
    std::fs::write(
        d.path().join("mock/crates/alpha/src/unsaved.rs"),
        "fn u() {}\n",
    )
    .unwrap();
    let said = run(d.path(), &["--nuke", "--y"]);

    assert!(said.contains("refused"), "{said}");
    assert!(said.contains("unsaved.rs"), "and names it: {said}");
    assert!(d.path().join("mock/crates/alpha/src/other.rs").exists());
}

/// The engine, with `answer` on standard input.
///
/// The whole safeguard is a question, and until this existed nothing in the suite
/// ever answered it: the tests above cover a closed input and `--y`, which are the
/// two paths that skip the reading entirely.
fn run_answering(root: &Path, args: &[&str], answer: &str) -> String {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .arg("--dir")
        .arg(root.join("mock"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the engine could not be run");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(answer.as_bytes())
        .expect("could not write the answer");
    let out = child.wait_with_output().expect("the engine did not finish");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_yes_on_the_prompt_carries_the_plan_out() {
    let d = a_repo();
    let said = run_answering(d.path(), &["--nuke"], "y\n");

    assert!(said.contains("go ahead?"), "it did not ask: {said}");
    assert!(said.contains("NUKE complete"), "{said}");
    assert_eq!(sources(d.path()), vec![PathBuf::from(
        "mock/crates/alpha/src/lib.rs"
    )]);
}

#[test]
fn anything_but_a_yes_leaves_it_alone() {
    // The control for the test above, and the direction that matters: a prompt
    // reading any answer as consent is worse than no prompt, because it looks
    // like one.
    for answer in ["n\n", "\n", "yes please\n", "Y E S\n"] {
        let d = a_repo();
        let before = sources(d.path());
        let said = run_answering(d.path(), &["--nuke"], answer);

        assert!(
            said.contains("left alone"),
            "{answer:?} was taken as yes: {said}"
        );
        assert_eq!(before, sources(d.path()), "{answer:?} deleted something");
    }
}
