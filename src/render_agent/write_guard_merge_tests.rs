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
    bash_payload,
    git,
    guard_for,
    refused,
    repo_with_rounds,
    run_from,
    write_payload,
};

/// A repository in TOPIC with a merge of `side` into `main` begun and conflicted.
fn merging() -> std::path::PathBuf {
    merging_in("merge", "202609062237_topic.a-thing.md")
}

/// The same merge in a repository whose one round file is `round`, which sets the phase.
fn merging_in(tag: &str, round: &str) -> std::path::PathBuf {
    let d = repo_with_rounds(tag, &[round]);
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

/// Ends the merge the way a squash or a rebase leaves it: the unmerged stages stay in the index and
/// `MERGE_HEAD` is gone. `merge --abort` would clear the stages too and prove nothing about the check.
fn end_merge_keeping_conflicts(repo: &std::path::Path) {
    std::fs::remove_file(repo.join(".git/MERGE_HEAD")).unwrap();
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
    // The control for the first two: it is the merge in progress (`MERGE_HEAD`) that excuses them, with the conflict still listed.
    let d = merging();
    end_merge_keeping_conflicts(&d);
    let out = edit(&d, "mock/crates/x/DESIGN.md.tmpl");
    assert!(refused(&out), "a template was writable in TOPIC: {out}");
}

/// An Edit of `path`, the other tool that carries a path.
fn edit_payload(path: &std::path::Path) -> String {
    serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {
            "file_path": path.display().to_string(),
            "old_string": "a",
            "new_string": "b",
        }
    })
    .to_string()
}

#[test]
fn a_directory_holding_a_conflict_is_not_opened_whole() {
    // The exception is for the file git lists as unmerged, not for a directory that holds one.
    let d = merging();
    let guard = guard_for(&d);
    let krate = d.join("mock/crates/x");
    let with_slash = format!("{}/", krate.display());
    let during = run_from(&d, &guard, &write_payload(&krate));
    let during2 = run_from(&d, &guard, &write_payload(std::path::Path::new(&with_slash)));
    end_merge_keeping_conflicts(&d);
    let after = run_from(&d, &guard, &write_payload(&krate));
    assert!(refused(&after), "control: no merge refuses");
    assert!(refused(&during), "a merge opened the whole crate directory: {during}");
    assert!(refused(&during2), "a merge opened the whole crate directory: {during2}");
}

#[test]
fn a_command_naming_a_conflicted_file_is_not_opened() {
    // Write and Edit carry one path and can be matched exactly; a command's targets are not parsed.
    let d = merging();
    let guard = guard_for(&d);
    let cmd = format!("sed -i s/a/b/ {}", d.join("mock/crates/x/DESIGN.md.tmpl").display());
    let during = run_from(&d, &guard, &bash_payload(&cmd));
    assert!(refused(&during), "a command write was let through by the merge exception: {during}");
}

#[test]
fn an_edit_of_a_file_in_conflict_is_let_through_and_of_another_is_not() {
    let d = merging();
    let guard = guard_for(&d);
    let conflicted = run_from(&d, &guard, &edit_payload(&d.join("mock/crates/x/DESIGN.md.tmpl")));
    assert!(!refused(&conflicted), "the conflicted template was refused to an Edit: {conflicted}");
    let other = run_from(&d, &guard, &edit_payload(&d.join("mock/crates/x/README.md.tmpl")));
    assert!(refused(&other), "an unconflicted template was open to an Edit: {other}");
    end_merge_keeping_conflicts(&d);
    let after = run_from(&d, &guard, &edit_payload(&d.join("mock/crates/x/DESIGN.md.tmpl")));
    assert!(refused(&after), "control: no merge refuses an Edit: {after}");
}

#[test]
fn a_conflicted_source_file_is_open_in_the_doc_phase() {
    // DOC gates source; the exception holds there as in TOPIC, and only while merging.
    let d = merging_in("merge-doc", "202609062251_changelist.doc.md");
    let out = edit(&d, "mock/crates/x/src/lib.rs");
    assert!(!refused(&out), "the conflicted source was refused in DOC: {out}");
    end_merge_keeping_conflicts(&d);
    let after = edit(&d, "mock/crates/x/src/lib.rs");
    assert!(refused(&after), "control: source is gated in DOC without a merge: {after}");
}

#[test]
fn a_conflicted_template_is_open_in_the_impl_phase() {
    // IMPL gates the design templates; the exception holds there as in TOPIC, and only while merging.
    let d = merging_in("merge-impl", "202609062251_changelist.src.md");
    let out = edit(&d, "mock/crates/x/DESIGN.md.tmpl");
    assert!(!refused(&out), "the conflicted template was refused in IMPL: {out}");
    end_merge_keeping_conflicts(&d);
    let after = edit(&d, "mock/crates/x/DESIGN.md.tmpl");
    assert!(refused(&after), "control: a template is gated in IMPL without a merge: {after}");
}
