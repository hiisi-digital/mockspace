//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Removing a file git does not track passes in any phase.
//!
//! A lint can ask for an untracked source file to be reverted, which for a file never committed means
//! removing it. In the DOC phase the guard refuses any `rm` under the mock directory, so the doc window
//! could never lock. Removing such a file only returns the tree to its committed state, so a plain `rm`
//! of paths that all exist and are all untracked passes, and every other shape keeps the phase rule.

use super::write_guard_phase_tests::{
    bash_payload,
    git,
    guard_for,
    refused,
    repo_with_rounds,
    run_from,
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

#[test]
fn every_other_shape_keeps_the_phase_rule() {
    let d = doc_round();
    let s = p(&d, "mock/crates/x/src/stray.rs");
    let dir = p(&d, "mock/crates/x/src");
    for cmd in [
        format!("rm {dir}/*.rs"),
        format!("rm {s} ; rm {}", p(&d, "mock/crates/x/src/tracked.rs")),
        format!("rm {s} && cp {s} {}", p(&d, "mock/crates/x/src/tracked.rs")),
        format!("rm {}", p(&d, "mock/crates/x/src/missing.rs")),
        format!("mv {s} {}", p(&d, "mock/crates/x/src/moved.rs")),
    ] {
        assert!(refused(&out_of(&d, &cmd)), "passed in DOC: {cmd}");
    }
}
