//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A rebase over generated documents that both sides changed finishes without
//! stopping, and its tip is the generation of the final tree.
//!
//! The replayed commits in between may carry the upstream's copy of a document,
//! which is why only the tip is asserted: the `post-rewrite` hook regenerates
//! once the rebase is done and commits what differs on top of it. A rebase
//! stopped on a source conflict regenerates when it is continued, and a clone
//! whose hooks do not run gets plain git's conflict, since nothing would put the
//! dropped side back.

mod common;

use common::{Repo, write};

/// What every finished rebase leaves: on the branch, no rebase in progress, both
/// sides' edits in the committed document, and that document exactly what
/// generation gives on the committed templates.
fn assert_tip_regenerated(r: &Repo, output: &str) {
    assert_eq!(
        r.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "a",
        "{output}"
    );
    assert!(!r.root.join(".git/rebase-merge").exists(), "{output}");
    assert!(!r.root.join(".git/rebase-apply").exists(), "{output}");
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(in_head.contains("one, edited on a"), "{in_head}\n{output}");
    assert!(
        in_head.contains("five, edited on dev"),
        "{in_head}\n{output}"
    );
    assert!(!in_head.contains("<<<<<<<"), "{in_head}");
    r.generate();
    assert_eq!(r.doc(), in_head, "HEAD holds the regenerated file");
    assert_eq!(r.status(), "", "{output}");
}

#[test]
fn a_rebase_over_conflicting_generated_documents_finishes_regenerated() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["switch", "-q", "a"]);
    let rebased = r.git_out(&["rebase", "dev"]);
    let out = Repo::failure(&rebased);
    assert!(rebased.status.success(), "{out}");
    assert_tip_regenerated(&r, &out);
    assert_eq!(
        r.subjects()[0],
        "chore: regenerate generated files after rebase",
        "{:?}",
        r.subjects()
    );
}

#[test]
fn a_rebase_with_the_apply_backend_finishes_regenerated() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["switch", "-q", "a"]);
    let rebased = r.git_out(&["rebase", "--apply", "dev"]);
    let out = Repo::failure(&rebased);
    assert!(rebased.status.success(), "{out}");
    assert_tip_regenerated(&r, &out);
}

#[test]
fn a_pull_with_rebase_finishes_regenerated() {
    let r = Repo::new();
    r.two_sides();
    // the repository as its own remote: `dev` is upstream of `a`
    r.git(&["remote", "add", "origin", r.root.to_str().unwrap()]);
    r.git(&["fetch", "-q", "origin"]);
    r.git(&["switch", "-q", "a"]);
    r.git(&["branch", "-q", "--set-upstream-to=origin/dev"]);
    let pulled = r.git_out(&["pull", "--rebase"]);
    let out = Repo::failure(&pulled);
    assert!(pulled.status.success(), "{out}");
    assert_tip_regenerated(&r, &out);
}

#[test]
fn a_rebase_stopped_on_a_source_conflict_regenerates_when_continued() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    r.git(&["switch", "-q", "a"]);

    let stopped = r.git_out(&["rebase", "dev"]);
    assert!(!stopped.status.success(), "{}", Repo::failure(&stopped));
    assert_eq!(
        r.unmerged(),
        ["mock/ALPHA.md.tmpl"],
        "only the source stops the rebase, the document does not"
    );

    write(
        &r.root.join("mock/ALPHA.md.tmpl"),
        "one\ntwo\nthree, resolved\nfour\nfive\n",
    );
    r.git(&["add", "mock/ALPHA.md.tmpl"]);
    let done = r
        .cmd("git")
        .env("GIT_EDITOR", "true")
        .args(["rebase", "--continue"])
        .output()
        .unwrap();
    assert!(done.status.success(), "{}", Repo::failure(&done));
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(in_head.contains("three, resolved"), "{in_head}");
    assert!(
        !in_head.contains("three, on dev") && !in_head.contains("three, on a"),
        "{in_head}"
    );
    r.generate();
    assert_eq!(r.doc(), in_head);
    assert_eq!(r.status(), "");
}

#[test]
fn an_aborted_rebase_leaves_the_branch_where_it_was() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    r.git(&["switch", "-q", "a"]);
    let before = r.git(&["rev-parse", "HEAD"]);
    let stopped = r.git_out(&["rebase", "dev"]);
    assert!(!stopped.status.success());
    r.git(&["rebase", "--abort"]);
    assert_eq!(r.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(r.status(), "");
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
