//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A conflict confined to generated files is not a conflict.
//!
//! Every scenario runs the real binary and real git in a scratch repository,
//! with the generated hooks active and `cargo mock` and `mock` shimmed to this
//! build, so what is asserted is what a consumer's clone does at `git merge`.
//!
//! The control carries the rest. Without the merge driver the same two
//! branches conflict on the generated document, which is what shows the
//! fixture makes a conflict at all: a no-conflict result over a fixture that
//! never conflicted would pass for any reason.
//!
//! What git does with a `pre-merge-commit` hook was measured first, on git
//! 2.43: the hook's staged change does not reach the merge commit. So a merge
//! that makes its own commit is followed up by a second commit (`post-merge`),
//! and one that stopped is regenerated at the commit that concludes it
//! (`pre-commit`, where the staged change does count). Both arms are below.

mod common;

use std::fs;
use std::process::Command;

use common::{Repo, write};

#[test]
fn control_without_the_driver_the_two_documents_conflict() {
    let r = Repo::new();
    r.two_sides();
    // The driver is registered in the clone's config and nowhere else, so
    // removing it is removing the thing under test.
    r.git(&["config", "--local", "--remove-section", "merge.mockspace-generated"]);
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(
        !merged.status.success(),
        "the fixture must conflict without the driver: {}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
    assert!(
        r.unmerged().contains(&"docs/ALPHA.md".to_string()),
        "{:?} {}{}",
        r.unmerged(),
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
}

#[test]
fn documents_differing_on_both_sides_merge_with_no_conflict() {
    let r = Repo::new();
    r.two_sides();
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(
        merged.status.success(),
        "{}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
    assert!(r.unmerged().is_empty());

    // what generation gives on the merged tree, and the merged tree holds it
    let doc = r.doc();
    assert!(doc.contains("one, edited on a"), "{doc}");
    assert!(doc.contains("five, edited on dev"), "{doc}");
    r.generate();
    assert_eq!(r.doc(), doc, "regenerating the merged tree changes nothing");
    assert_eq!(
        r.status(),
        "",
        "the merged tree is committed, nothing is left staged or modified: {}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );

    // the merge commit, then the follow-up that regenerated, named for it
    let subjects = r.subjects();
    assert_eq!(
        subjects[0], "chore: regenerate generated files after merge",
        "{subjects:?}"
    );
    assert_eq!(subjects[1], "chore: merge a", "{subjects:?}");
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert_eq!(
        in_head, doc,
        "the committed document is the regenerated one"
    );
}

#[test]
fn a_source_conflict_beside_them_stops_only_that_file() {
    let r = Repo::new();
    // the same template line on both sides: a real conflict, beside documents
    // that also differ
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success(), "the template conflicts");
    assert_eq!(
        r.unmerged(),
        ["mock/ALPHA.md.tmpl"],
        "only the source file stops the merge"
    );

    // resolved by hand, committed through the real hooks: the document is
    // regenerated at the commit that concludes the merge
    write(
        &r.root.join("mock/ALPHA.md.tmpl"),
        "one\ntwo\nthree, resolved\nfour\nfive\n",
    );
    r.git(&["add", "mock/ALPHA.md.tmpl"]);
    let done = r.git_out(&["commit", "-q", "-m", "chore: merge a"]);
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(in_head.contains("three, resolved"), "{in_head}");
    assert!(
        !in_head.contains("three, on dev") && !in_head.contains("three, on a"),
        "{in_head}"
    );
    r.generate();
    assert_eq!(
        r.git(&["show", "HEAD:docs/ALPHA.md"]),
        r.doc(),
        "HEAD holds the regenerated file"
    );
    assert_eq!(r.status(), "");
}

#[test]
fn a_merge_with_no_generated_change_is_left_alone() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    write(&r.root.join("notes.txt"), "notes, edited on a\n");
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit notes"]);
    r.git(&["switch", "-q", "dev"]);
    write(&r.root.join("other.txt"), "other\n");
    r.git(&["add", "other.txt"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add other"]);
    let before = r.subjects().len();

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success());
    // exactly the two commits the merge brings: a's and the merge itself
    assert_eq!(r.subjects().len(), before + 2, "{:?}", r.subjects());
    assert_eq!(r.subjects()[0], "chore: merge a");
}

#[test]
fn aborting_a_stopped_merge_still_works() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    let head = r.git(&["rev-parse", "HEAD"]);
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success());

    r.git(&["merge", "--abort"]);
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head);
    assert!(r.unmerged().is_empty());
    assert_eq!(r.status(), "");
    assert!(!r.root.join(".git/MERGE_HEAD").exists());
}

/// The measurement the vehicle rests on, kept as a test so a git that changes
/// it says so. A `pre-merge-commit` hook that stages a change does not get it
/// into the merge commit, which is why a merge that makes its own commit is
/// followed up at `post-merge` rather than regenerated before the commit.
///
/// The arm that does carry a staged change is `pre-commit` at the commit that
/// concludes a stopped merge, and the scenario above that runs it is its test.
#[test]
fn git_drops_what_a_pre_merge_commit_hook_stages() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "commit.gpgsign", "false"]);
    write(&root.join("gen.txt"), "base\n");
    write(&root.join("src.txt"), "base\n");
    write(&root.join("other.txt"), "base\n");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["switch", "-q", "-c", "side"]);
    write(&root.join("other.txt"), "side\n");
    git(&["commit", "-qam", "side"]);
    git(&["switch", "-q", "main"]);
    write(&root.join("src.txt"), "main\n");
    git(&["commit", "-qam", "main"]);

    let hook = |name: &str| {
        let path = root.join(".git/hooks").join(name);
        write(
            &path,
            "#!/bin/sh\necho regenerated > gen.txt\ngit add gen.txt\n",
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    };

    // a clean merge: the hook runs and its staged change is not in the commit
    hook("pre-merge-commit");
    git(&["merge", "-q", "-m", "merged", "side"]);
    assert_eq!(
        git(&["show", "HEAD:gen.txt"]),
        "base\n",
        "not carried into the merge commit"
    );
    assert_eq!(
        git(&["status", "--porcelain"]),
        "M  gen.txt\n",
        "left staged beside it"
    );
}

#[test]
fn a_generated_document_added_on_both_sides_merges_with_no_conflict() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.add_document("a", "BETA");
    r.git(&["switch", "-q", "dev"]);
    r.add_document("dev", "GAMMA");

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.unmerged().is_empty(), "{:?}", r.unmerged());
    assert!(r.root.join("docs/BETA.md").exists());
    assert!(r.root.join("docs/GAMMA.md").exists());
    assert_eq!(r.status(), "", "{}", Repo::failure(&merged));
}

#[test]
fn a_squash_merge_commit_carries_the_regenerated_files() {
    // `git merge --squash` leaves no `MERGE_HEAD` and makes no commit, and the
    // driver has kept ours for the document, so the commit that follows is the
    // only place the merged tree can be regenerated.
    let r = Repo::new();
    r.two_sides();
    let squashed = r.git_out(&["-c", "merge.ff=true", "merge", "--squash", "a"]);
    assert!(squashed.status.success(), "{}", Repo::failure(&squashed));
    let done = r.git_out(&["commit", "-q", "-m", "chore: squash a"]);
    assert!(done.status.success(), "{}", Repo::failure(&done));
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(in_head.contains("one, edited on a"), "{in_head}");
    assert!(in_head.contains("five, edited on dev"), "{in_head}");
    r.generate();
    assert_eq!(r.doc(), in_head, "HEAD holds the regenerated file");
    assert_eq!(r.status(), "");
}

/// A generated document deleted on one side and edited on the other is a
/// modify/delete, which git does not hand to a merge driver at all, so nothing
/// here resolves it and the merge stops.
#[test]
#[ignore = "catalogue: modify/delete of a generated document stops the merge, git calls no driver there"]
fn a_generated_document_deleted_on_one_side_merges_with_no_conflict() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "one", "one, edited on a");
    r.git(&["switch", "-q", "dev"]);
    r.git(&["rm", "-q", "docs/ALPHA.md"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: drop alpha"]);
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
}
