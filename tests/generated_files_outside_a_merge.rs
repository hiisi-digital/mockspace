//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A cherry-pick, a revert, `git am`, `git checkout -m` and a stash application over a generated
//! document.
//!
//! None of them is a `git merge` or a rebase, and no hook of ours runs after
//! one. The driver must not keep ours there, or the other side's edit is
//! dropped with nothing to put it back; the file conflicts as it would without
//! a driver.

mod common;

use std::fs;

use common::Repo;

#[test]
fn a_cherry_pick_over_a_generated_document_change_conflicts_and_drops_no_edit() {
    let r = Repo::new();
    r.two_sides();
    let picked = r.git_out(&["cherry-pick", "a"]);
    assert!(!picked.status.success(), "{}", Repo::failure(&picked));
    assert!(
        r.unmerged().contains(&"docs/ALPHA.md".to_string()),
        "{:?}",
        r.unmerged()
    );
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

#[test]
fn a_revert_over_a_generated_document_change_conflicts_and_drops_no_edit() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["merge", "-q", "a", "-m", "chore: merge a"]);
    // a later edit to the line the reverted commit changed, made in the
    // document only, so the sources revert cleanly and the document is what is
    // left to reconcile
    let path = r.root.join("docs/ALPHA.md");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("five, edited on dev", "five, hand-edited");
    fs::write(&path, text).unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: edit alpha by hand"]);
    let reverted = r.git_out(&["revert", "--no-edit", ":/chore: edit on dev"]);
    assert!(!reverted.status.success(), "{}", Repo::failure(&reverted));
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

#[test]
fn applying_a_stash_over_a_generated_document_change_conflicts_and_drops_no_edit() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["switch", "-q", "a"]);
    let path = r.root.join("docs/ALPHA.md");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("five", "five, stashed");
    fs::write(&path, text).unwrap();
    r.git(&["stash", "push", "-q", "-m", "mine"]);
    r.git(&["switch", "-q", "dev"]);
    let applied = r.git_out(&["stash", "apply"]);
    assert!(!applied.status.success(), "{}", Repo::failure(&applied));
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

#[test]
fn applying_a_patch_over_a_generated_document_change_conflicts_and_drops_no_edit() {
    // `git am` keeps its state in `rebase-apply`, the directory a rebase with
    // the apply backend uses, and is told apart from it by the file inside
    let r = Repo::new();
    r.two_sides();
    let patch = r.git(&["format-patch", "-1", "--stdout", "a"]);
    let patch_file = r.root.join("..").join("a.patch");
    fs::write(&patch_file, patch).unwrap();
    let applied = r.git_out(&["am", "--3way", patch_file.to_str().unwrap()]);
    assert!(!applied.status.success(), "{}", Repo::failure(&applied));
    assert!(
        r.unmerged().contains(&"docs/ALPHA.md".to_string()),
        "{:?}\n{}",
        r.unmerged(),
        Repo::failure(&applied)
    );
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

#[test]
fn checking_out_with_merge_over_a_generated_document_change_conflicts_and_drops_no_edit() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["switch", "-q", "a"]);
    let path = r.root.join("docs/ALPHA.md");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("five", "five, local");
    fs::write(&path, text).unwrap();
    // git leaves the conflict in the file and exits 0 for this one
    let switched = r.git_out(&["checkout", "-m", "dev"]);
    assert!(switched.status.success(), "{}", Repo::failure(&switched));
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
