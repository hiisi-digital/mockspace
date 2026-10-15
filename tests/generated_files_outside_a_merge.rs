//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A rebase, a cherry-pick, a revert and a stash application over a generated
//! document.
//!
//! None of them is a `git merge`, and git sets `GITHEAD_<sha>` only for that.
//! Where no hook regenerates afterwards the driver must not keep ours, or the
//! other side's edit is dropped with nothing to put it back.

mod common;

use std::fs;

use common::Repo;

#[test]
fn a_rebase_over_a_generated_document_change_conflicts_and_drops_no_edit() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["switch", "-q", "a"]);
    let rebased = r.git_out(&["rebase", "dev"]);
    assert!(
        !rebased.status.success(),
        "no hook regenerates during a rebase, so the document is left to conflict: {}",
        Repo::failure(&rebased)
    );
    assert!(
        r.unmerged().contains(&"docs/ALPHA.md".to_string()),
        "{:?}",
        r.unmerged()
    );
    let doc = r.doc();
    assert!(
        doc.contains("<<<<<<<"),
        "both sides are in the file, not one: {doc}"
    );
}

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
