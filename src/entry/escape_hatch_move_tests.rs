//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A staged move changes the crate it left as well as the one it reached.
//!
//! `--scope` and `--doc-only` are checked against what is staged. With git's
//! rename detection on, a staged `git mv` names only its destination, so a scope
//! naming the destination crate alone passed, and the crate that lost the file
//! was never linted.

use std::path::Path;
use std::process::Command;

use mockspace_lint_rules::src_layout::SrcLayout;

use super::escape_hatch::{Refusal, staged_crates, verify_doc_only, verify_scope};

/// Long enough, and moved whole, so git reads the move as a rename.
const BODY: &str = "pub fn moved() -> u32 {\n    let a = 1;\n    let b = 2;\n    a + b\n}\n\n\
                    pub fn other() -> u32 {\n    let c = 3;\n    c * 2\n}\n";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repository with `from` committed and then staged as moved to `to`.
fn moved(from: &str, to: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    std::fs::create_dir_all(root.join(from).parent().unwrap()).unwrap();
    std::fs::write(root.join(from), BODY).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    std::fs::create_dir_all(root.join(to).parent().unwrap()).unwrap();
    git(root, &["mv", from, to]);
    let status = git(root, &["diff", "--cached", "--name-status", "-M"]);
    assert!(
        status.starts_with("R100"),
        "the fixture is not a rename:\n{status}"
    );
    tmp
}

fn grouped(root: &Path) -> SrcLayout {
    let mock = root.join("mock");
    SrcLayout::new(&mock, &[
        mock.join("crates"),
        mock.join("applets/console/crates"),
    ])
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

const BETWEEN_CRATES: (&str, &str) = ("mock/crates/a/src/x.rs", "mock/crates/b/src/x.rs");
const BETWEEN_DIRECTORIES: (&str, &str) = (
    "mock/crates/a/src/x.rs",
    "mock/applets/console/crates/c/src/x.rs",
);

#[test]
fn a_move_between_crates_stages_both() {
    let tmp = moved(BETWEEN_CRATES.0, BETWEEN_CRATES.1);
    let got = staged_crates(tmp.path(), "mock", &grouped(tmp.path()));
    assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn a_move_between_declared_directories_stages_both() {
    let tmp = moved(BETWEEN_DIRECTORIES.0, BETWEEN_DIRECTORIES.1);
    let got = staged_crates(tmp.path(), "mock", &grouped(tmp.path()));
    assert_eq!(got, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn a_scope_naming_the_destination_alone_is_refused_for_the_crate_the_file_left() {
    for (from, to, dest, left) in [
        (BETWEEN_CRATES.0, BETWEEN_CRATES.1, "b", "a"),
        (BETWEEN_DIRECTORIES.0, BETWEEN_DIRECTORIES.1, "c", "a"),
    ] {
        let tmp = moved(from, to);
        let r = verify_scope(tmp.path(), "mock", &grouped(tmp.path()), dest);
        match r {
            Some(Refusal::ScopeOmitsStagedCrates {
                omitted,
                ..
            }) => assert_eq!(omitted, vec![left.to_string()], "{from} to {to}"),
            other => panic!("{from} to {to}: expected a refusal, got {other:?}"),
        }
    }
}

#[test]
fn a_scope_naming_both_passes() {
    let tmp = moved(BETWEEN_DIRECTORIES.0, BETWEEN_DIRECTORIES.1);
    assert_eq!(
        verify_scope(tmp.path(), "mock", &grouped(tmp.path()), "a,c"),
        None
    );
}

#[test]
fn doc_only_is_refused_over_a_moved_source_file() {
    let tmp = moved(BETWEEN_CRATES.0, BETWEEN_CRATES.1);
    let r = verify_doc_only(tmp.path(), "mock", &grouped(tmp.path()));
    match r {
        Some(Refusal::DocOnlyWithStagedSource {
            staged,
        }) => {
            assert_eq!(sorted(staged), vec![
                BETWEEN_CRATES.0.to_string(),
                BETWEEN_CRATES.1.to_string()
            ])
        },
        other => panic!("expected a refusal naming both sides, got {other:?}"),
    }
}
