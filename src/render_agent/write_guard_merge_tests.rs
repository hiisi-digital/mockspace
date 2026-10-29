//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A merge's conflict can be resolved whatever phase the round is in.
//!
//! The guard gates a crate's design templates to the DOC phase and its source to IMPL. A merge that
//! conflicts in such a file leaves the conflict markers in the working tree, and resolving them is an
//! edit the phase refuses: the merge cannot be finished, and routing around it is a bypass. While a
//! merge is in progress the guard lets an edit through to a path git lists as unmerged, and to no other.
//!
//! The fixture is a repository in TOPIC, where nothing under a crate is writable, with a template and a
//! source file that `main` and `side` each changed, so merging `side` conflicts in both, and a third file
//! neither touched.

use super::write_guard_phase_tests::{
    git,
    guard_for,
    refused,
    repo_with_rounds,
    run_from,
    write_payload,
};

/// A repository in TOPIC with a merge of `side` into `main` begun and conflicted.
fn merging() -> std::path::PathBuf {
    let d = repo_with_rounds("merge", &["202609062237_topic.a-thing.md"]);
    let krate = d.join("mock/crates/x");
    std::fs::create_dir_all(krate.join("src")).unwrap();
    std::fs::write(krate.join("src/lib.rs"), "fn a() {}\n").unwrap();
    std::fs::write(krate.join("README.md.tmpl"), "# x\n").unwrap();
    git(&d, &["add", "mock"]);
    git(&d, &["commit", "-q", "--no-verify", "-m", "chore: a crate"]);
    git(&d, &["switch", "-q", "-c", "side"]);
    std::fs::write(krate.join("DESIGN.md.tmpl"), "# side\n").unwrap();
    std::fs::write(krate.join("src/lib.rs"), "fn side() {}\n").unwrap();
    git(&d, &["add", "mock"]);
    git(&d, &["commit", "-q", "--no-verify", "-m", "chore: side"]);
    git(&d, &["switch", "-q", "main"]);
    std::fs::write(krate.join("DESIGN.md.tmpl"), "# main\n").unwrap();
    std::fs::write(krate.join("src/lib.rs"), "fn main_side() {}\n").unwrap();
    git(&d, &["add", "mock"]);
    git(&d, &["commit", "-q", "--no-verify", "-m", "chore: main"]);
    // the merge conflicts, so git exits non-zero and `git()` would panic on it
    let _ = std::process::Command::new("git")
        .args(["merge", "-q", "--no-commit", "--no-ff", "side"])
        .current_dir(&d)
        .output()
        .unwrap();
    d
}

fn edit(repo: &std::path::Path, rel: &str) -> String {
    let guard = guard_for(repo);
    run_from(repo, &guard, &write_payload(&repo.join(rel)))
}

#[test]
fn a_template_in_conflict_can_be_resolved_in_topic() {
    let d = merging();
    let out = edit(&d, "mock/crates/x/DESIGN.md.tmpl");
    assert!(!refused(&out), "the conflicted template was refused: {out}");
}

#[test]
fn a_source_file_in_conflict_can_be_resolved_outside_impl() {
    let d = merging();
    let out = edit(&d, "mock/crates/x/src/lib.rs");
    assert!(!refused(&out), "the conflicted source was refused: {out}");
}

#[test]
fn a_file_the_merge_did_not_conflict_in_is_still_gated() {
    // The control: a merge in progress does not open the crate to editing.
    let d = merging();
    let out = edit(&d, "mock/crates/x/README.md.tmpl");
    assert!(
        refused(&out),
        "an unconflicted template was writable in TOPIC: {out}"
    );
    assert!(out.contains("TOPIC"), "{out}");
}

#[test]
fn a_file_is_gated_again_once_its_conflict_is_staged() {
    // Resolving is what the exception is for; an edit after the resolution was staged is not that.
    let d = merging();
    std::fs::write(d.join("mock/crates/x/DESIGN.md.tmpl"), "# both\n").unwrap();
    git(&d, &["add", "mock/crates/x/DESIGN.md.tmpl"]);
    let out = edit(&d, "mock/crates/x/DESIGN.md.tmpl");
    assert!(refused(&out), "a resolved file stayed writable: {out}");
}

#[test]
fn the_same_file_is_gated_when_no_merge_is_in_progress() {
    // The control for the first two: it is the merge that excuses them.
    let d = merging();
    git(&d, &["merge", "--abort"]);
    let out = edit(&d, "mock/crates/x/DESIGN.md.tmpl");
    assert!(refused(&out), "a template was writable in TOPIC: {out}");
}
