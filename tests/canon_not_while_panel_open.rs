//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Canon staged while a panel is open fails the commit gate.
//!
//! The claim this replaces was in a pull request body and in no file: "the
//! whole path is verified against a real repository and the real binary". Six
//! unit tests over the pure matcher existed and were real; the end-to-end run
//! did not. Under `evidence-lives-in-the-repo-or-it-never-happened.md` that is
//! void rather than unverified, so here it is.
//!
//! Every arm runs the binary with the arguments the generated pre-commit hook
//! uses, `--lint-only --commit`, which is the invocation that actually gates a
//! commit and the one that skips the build.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
}

fn write(path: &Path, content: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A repo declaring `mock/canon/**` as its canon, with a canon file present.
/// `panel_open` decides whether a seat has been minted and not consolidated.
fn fixture(root: &Path, panel_open: bool) {
    git(root, &["init", "-q"]);
    let mock = root.join("mock");
    write(
        &mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    write(
        &mock.join("mockspace.toml"),
        "project_name = \"fixture\"\ncrate_prefix = \"fx\"\ncanon_paths = [\"mock/canon/**\"]\n",
    );
    if panel_open {
        write(
            &mock.join("panel/kickoff.toml"),
            "slug = \"kickoff\"\nconsolidation = []\n\n[[seat]]\nnumber = 1\n\
             persona = \"leroy\"\ntopic = \"the thing\"\nminted_at_unix = 0\n",
        );
    }
    write(&mock.join("canon/law.md"), "# a law\n");
}

fn commit_gate(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(["--lint-only", "--commit", "--scope", "infra"])
        .current_dir(root.join("mock"))
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
fn staging_canon_while_a_panel_is_open_fails_the_gate() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), true);
    git(tmp.path(), &["add", "mock/canon/law.md"]);
    let out = commit_gate(tmp.path());
    let t = text(&out);
    assert!(
        t.contains("canon-not-while-panel-open") || t.contains("canon is staged"),
        "the gate did not report it: {t}"
    );
    assert_ne!(out.status.code(), Some(0), "and it must not pass: {t}");
}

/// The control on the panel: the same staged file with no open panel passes.
/// Without it, the arm above is equally consistent with a gate that refuses
/// every canon edit.
#[test]
fn staging_canon_with_no_panel_open_passes() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), false);
    git(tmp.path(), &["add", "mock/canon/law.md"]);
    let t = text(&commit_gate(tmp.path()));
    assert!(
        !t.contains("canon is staged"),
        "no panel is open, so nothing is forbidden: {t}"
    );
}

/// The control on the path: an open panel and a staged file outside the canon
/// globs passes. Without it, the first arm is equally consistent with a gate
/// that refuses every commit while a panel is open.
#[test]
fn staging_something_that_is_not_canon_passes_with_a_panel_open() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), true);
    write(&tmp.path().join("mock/notes.md"), "not canon\n");
    git(tmp.path(), &["add", "mock/notes.md"]);
    let t = text(&commit_gate(tmp.path()));
    assert!(
        !t.contains("canon is staged"),
        "the file is outside the declared canon: {t}"
    );
}

/// The control on the surface: a canon file changed in the working tree but
/// **not staged** passes, because a commit carries what is staged. This is the
/// arm that distinguishes this lint from the readiness row, which reads the
/// working tree on purpose.
#[test]
fn an_unstaged_canon_change_passes_the_commit_gate() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), true);
    git(tmp.path(), &["add", "mock/panel/kickoff.toml"]);
    write(&tmp.path().join("mock/canon/law.md"), "# edited, not staged\n");
    let t = text(&commit_gate(tmp.path()));
    assert!(
        !t.contains("canon is staged"),
        "nothing of the canon is staged, so the commit does not carry it: {t}"
    );
}
