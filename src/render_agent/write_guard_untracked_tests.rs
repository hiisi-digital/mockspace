//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Removing a file git does not track passes in any phase.
//!
//! A lint can ask for an untracked source file to be reverted, which for a file never committed means
//! removing it. In the DOC phase the guard refuses any `rm` under the mock directory, so the doc window
//! could never lock. Removing such a file only returns the tree to its committed state, so a plain `rm`
//! of paths that are all existing files, untracked and not held by HEAD, passes, and every other shape
//! keeps the phase rule.

use super::write_guard_phase_tests::{
    bash_payload,
    git,
    guard_for,
    refused,
    repo_with_rounds,
    run_from,
    scratch,
};

/// A repository in DOC with a committed source file and two untracked ones.
fn doc_round() -> std::path::PathBuf {
    let d = repo_with_rounds("untracked", &["202609062237_changelist.doc.md"]);
    let src = d.join("mock/crates/x/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("tracked.rs"), "// t\n").unwrap();
    git(&d, &["add", "mock/crates/x/src/tracked.rs"]);
    git(&d, &[
        "commit",
        "-q",
        "--no-verify",
        "-m",
        "chore: a source file",
    ]);
    std::fs::write(src.join("stray.rs"), "// s\n").unwrap();
    std::fs::write(src.join("stray2.rs"), "// s\n").unwrap();
    d
}

fn out_of(d: &std::path::Path, command: &str) -> String {
    run_from(d, &guard_for(d), &bash_payload(command))
}

fn p(d: &std::path::Path, rel: &str) -> String {
    d.join(rel).display().to_string()
}

#[test]
fn rm_of_an_untracked_source_file_passes_in_doc() {
    let d = doc_round();
    let out = out_of(&d, &format!("rm {}", p(&d, "mock/crates/x/src/stray.rs")));
    assert!(
        !refused(&out),
        "an untracked file could not be removed in DOC: {out}"
    );
    let cmd = format!(
        "rm -f {} {}",
        p(&d, "mock/crates/x/src/stray.rs"),
        p(&d, "mock/crates/x/src/stray2.rs")
    );
    assert!(
        !refused(&out_of(&d, &cmd)),
        "rm -f of two untracked files was refused"
    );
}

#[test]
fn rm_of_an_untracked_source_file_passes_by_a_relative_path() {
    let d = doc_round();
    let out = out_of(&d, "rm mock/crates/x/src/stray.rs");
    assert!(
        !refused(&out),
        "a relative untracked path was refused: {out}"
    );
}

#[test]
fn rm_of_a_tracked_source_file_is_still_refused_in_doc() {
    let d = doc_round();
    let out = out_of(&d, &format!("rm {}", p(&d, "mock/crates/x/src/tracked.rs")));
    assert!(refused(&out), "a tracked file was removable in DOC: {out}");
}

#[test]
fn rm_of_one_untracked_and_one_tracked_path_is_refused() {
    let d = doc_round();
    let cmd = format!(
        "rm {} {}",
        p(&d, "mock/crates/x/src/stray.rs"),
        p(&d, "mock/crates/x/src/tracked.rs")
    );
    assert!(refused(&out_of(&d, &cmd)), "a mixed rm passed");
}

/// An untracked file whose name is shell syntax, so that a command split on whitespace names only
/// existing untracked files and nothing but the metacharacter filter can refuse it.
fn untracked_named(d: &std::path::Path, name: &str) {
    std::fs::write(d.join("mock/crates/x/src").join(name), "// s\n").unwrap();
}

#[test]
fn shell_syntax_keeps_the_phase_rule_though_every_word_is_an_untracked_file() {
    let d = doc_round();
    let dir = p(&d, "mock/crates/x/src");
    for name in ["*.rs", ";.rs", "&&.rs", "|.rs", "$HOME.rs", "a\\b.rs"] {
        untracked_named(&d, name);
        let cmd = format!("rm {dir}/{name}");
        assert!(refused(&out_of(&d, &cmd)), "passed in DOC: {cmd}");
    }
}

#[test]
fn other_shapes_keep_the_phase_rule() {
    let d = doc_round();
    let s = p(&d, "mock/crates/x/src/stray.rs");
    std::fs::create_dir_all(d.join("mock/crates/x/src/dir.rs")).unwrap();
    std::fs::write(d.join("mock/crates/x/src/dir.rs/inner"), "x\n").unwrap();
    for cmd in [
        format!("rm {}", p(&d, "mock/crates/x/src/missing.rs")),
        format!("mv {s} {}", p(&d, "mock/crates/x/src/moved.rs")),
        format!("rm -- {s}"),
        format!("rm -r {s}"),
        format!("rm {}", p(&d, "mock/crates/x/src/dir.rs")),
    ] {
        assert!(refused(&out_of(&d, &cmd)), "passed in DOC: {cmd}");
    }
}

#[test]
fn a_path_outside_any_repository_is_refused() {
    let d = doc_round();
    let outside = scratch("outside");
    let f = outside.join("mock/crates/x/src/stray.rs");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "// s\n").unwrap();
    let out = out_of(&d, &format!("rm {}", f.display()));
    assert!(
        refused(&out),
        "a file outside any repository was removable: {out}"
    );
}

#[test]
fn a_file_dropped_from_the_index_but_held_by_head_is_refused() {
    let d = doc_round();
    git(&d, &[
        "rm",
        "-q",
        "--cached",
        "mock/crates/x/src/tracked.rs",
    ]);
    let out = out_of(&d, &format!("rm {}", p(&d, "mock/crates/x/src/tracked.rs")));
    assert!(refused(&out), "a file HEAD holds was removable: {out}");
}

#[test]
fn a_tracked_file_named_in_another_case_is_refused() {
    let d = doc_round();
    git(&d, &["config", "core.ignorecase", "true"]);
    let src = d.join("mock/crates/x/src");
    std::fs::hard_link(src.join("tracked.rs"), src.join("Tracked.rs")).unwrap();
    let out = out_of(&d, &format!("rm {}", p(&d, "mock/crates/x/src/Tracked.rs")));
    assert!(
        refused(&out),
        "a case variant of a tracked file was removable: {out}"
    );
}

#[test]
fn a_corrupt_index_does_not_read_as_untracked() {
    let d = doc_round();
    std::fs::write(d.join(".git/index"), "garbage").unwrap();
    let out = out_of(&d, &format!("rm {}", p(&d, "mock/crates/x/src/tracked.rs")));
    assert!(
        refused(&out),
        "a tracked file was removable past a corrupt index: {out}"
    );
}

#[test]
fn a_relative_path_is_read_against_the_cwd_the_payload_names() {
    let d = doc_round();
    let elsewhere = scratch("cwd");
    let mut payload: serde_json::Value =
        serde_json::from_str(&bash_payload("rm mock/crates/x/src/stray.rs")).unwrap();
    // The command has to name the repository to be in this guard's scope; its cwd is the payload's.
    payload["cwd"] = d.display().to_string().into();
    let out = run_from(&d, &guard_for(&d), &payload.to_string());
    assert!(!refused(&out), "the payload's cwd was not used: {out}");
    let out = run_from(&elsewhere, &guard_for(&d), &payload.to_string());
    assert!(
        !refused(&out),
        "the process cwd was used over the payload's: {out}"
    );
    payload["cwd"] = elsewhere.display().to_string().into();
    let out = run_from(&d, &guard_for(&d), &payload.to_string());
    assert!(
        refused(&out),
        "the process cwd was used over the payload's: {out}"
    );
}
