//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What registers the driver and the hooks, and when it does not.

mod common;

use std::fs;

use common::{BIN, Repo};

#[test]
fn running_mockspace_registers_the_driver_and_the_hooks() {
    let r = Repo::untouched();
    r.generate();
    assert!(r.config("merge.mockspace-generated.driver").is_some());
    assert!(r.config("core.hooksPath").is_some());
}

#[test]
fn the_opt_out_registers_no_driver_and_no_hooks() {
    let r = Repo::untouched();
    let out = r
        .cmd(BIN)
        .current_dir(r.root.join("mock"))
        .env("MOCKSPACE_NO_AUTO_ACTIVATE", "1")
        .arg("--generate-only")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert_eq!(r.config("merge.mockspace-generated.driver"), None);
    assert_eq!(r.config("merge.mockspace-generated.name"), None);
    assert_eq!(r.config("core.hooksPath"), None);
}

// --- the driver keeps ours only where a hook of ours will follow it -------------

#[test]
fn with_the_hooks_off_a_merge_conflicts_as_plain_git_does() {
    // `core.hooksPath` replaced, or a client that never runs hooks: nothing
    // regenerates after the merge, so keeping ours would commit a stale document
    // that pushes cleanly. The driver asks git where `post-merge` would be.
    let r = Repo::new();
    r.two_sides();
    let merged =
        r.git_out(&["-c", "core.hooksPath=/dev/null", "merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
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
fn with_the_hooks_off_a_rebase_conflicts_as_plain_git_does() {
    let r = Repo::new();
    r.two_sides();
    r.git(&["switch", "-q", "a"]);
    let rebased = r.git_out(&["-c", "core.hooksPath=/dev/null", "rebase", "dev"]);
    assert!(!rebased.status.success(), "{}", Repo::failure(&rebased));
    assert!(
        r.unmerged().contains(&"docs/ALPHA.md".to_string()),
        "{:?}",
        r.unmerged()
    );
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

#[test]
fn a_hook_that_is_not_ours_does_not_count() {
    // an executable `post-merge` that is somebody else's regenerates nothing
    let r = Repo::new();
    r.two_sides();
    let hooks = r.root.join("other-hooks");
    common::write_executable(&hooks.join("post-merge"), "#!/bin/sh\nexit 0\n");
    let merged = r.git_out(&[
        "-c",
        &format!("core.hooksPath={}", hooks.display()),
        "merge",
        "a",
        "-m",
        "chore: merge a",
    ]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.doc().contains("<<<<<<<"), "{}", r.doc());
}

// --- the hook that regenerates is the one in this working tree ------------------

/// `<mock>/target/hooks/` is per working tree: absent in a fresh linked
/// worktree and after `cargo clean`. The durable hook in the config directory
/// delegates to it and, with it missing, quietly does nothing, so a driver that
/// looked only at the durable hook kept ours and nothing regenerated.
#[test]
fn a_rebase_in_a_fresh_worktree_where_cargo_mock_does_not_resolve_conflicts_as_plain_git_does() {
    let r = Repo::new();
    r.two_sides();
    let wt = r.worktree("a");
    let rebased = r.git_in_env(&wt, &[("MS_NO_MOCK", "1")], &["rebase", "dev"]);
    assert!(!rebased.status.success(), "{}", Repo::failure(&rebased));
    let doc = fs::read_to_string(wt.join("docs/ALPHA.md")).unwrap();
    assert!(doc.contains("<<<<<<<"), "both sides are in the file: {doc}");
}

#[test]
fn a_merge_in_a_fresh_worktree_where_cargo_mock_does_not_resolve_conflicts_as_plain_git_does() {
    let r = Repo::new();
    r.two_sides();
    let wt = r.worktree("a");
    let merged = r.git_in_env(
        &wt,
        &[("MS_NO_MOCK", "1")],
        &["merge", "dev", "-m", "chore: merge dev"],
    );
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    let doc = fs::read_to_string(wt.join("docs/ALPHA.md")).unwrap();
    assert!(doc.contains("<<<<<<<"), "both sides are in the file: {doc}");
}

#[test]
fn a_merge_in_a_fresh_worktree_where_cargo_mock_resolves_builds_the_hook_and_regenerates() {
    // the other arm: the hook the driver asked for is not there, and the merge builds it
    let r = Repo::new();
    r.two_sides();
    let wt = r.worktree("a");
    assert!(!wt.join("mock/target/hooks/post-merge").exists());
    let merged = r.git_in(&wt, &["merge", "dev", "-m", "chore: merge dev"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(wt.join("mock/target/hooks/post-merge").exists(), "the merge built the hook");
    let in_head = r.git_in(&wt, &["show", "HEAD:docs/ALPHA.md"]);
    let in_head = String::from_utf8_lossy(&in_head.stdout);
    assert!(in_head.contains("one, edited on a"), "{in_head}");
    assert!(in_head.contains("five, edited on dev"), "{in_head}");
    assert!(!in_head.contains("<<<<<<<"), "{in_head}");
}

#[test]
fn a_worktree_that_has_generated_its_hooks_regenerates() {
    // the control: the same rebase, once `mock` has run in the worktree
    let r = Repo::new();
    r.two_sides();
    let wt = r.worktree("a");
    r.generate_in(&wt);
    assert!(wt.join("mock/target/hooks/post-rewrite").exists());
    // generation rewrites the tracked agent hooks for this path, hence the autostash
    let rebased = r.git_in(&wt, &["rebase", "--autostash", "dev"]);
    assert!(rebased.status.success(), "{}", Repo::failure(&rebased));
    let in_head = r.git_in(&wt, &["show", "HEAD:docs/ALPHA.md"]);
    let in_head = String::from_utf8_lossy(&in_head.stdout);
    assert!(in_head.contains("one, edited on a"), "{in_head}");
    assert!(in_head.contains("five, edited on dev"), "{in_head}");
}

// --- the mock directory is whichever one this working tree has ------------------

/// The mock directory differs across branches and worktrees (one branch renames
/// `mock/` to `linux/`), so no string kept per clone can say where the generated
/// hooks are. A leftover `mock/target/hooks` made the path kept at registration
/// look installed while the durable hook, looking in `linux/target/hooks`,
/// found nothing and did nothing. The driver asks the hook instead.
#[test]
fn a_merge_on_a_branch_with_another_mock_directory_conflicts_as_plain_git_does() {
    let r = Repo::new();
    // d: the template edit and its regenerated document, in `mock/`
    r.git(&["switch", "-q", "-c", "d"]);
    r.side("d", "one", "one, edited on d");
    // l: `mock/` is `linux/` here, and the document was edited by hand on the
    // same line, which the template edit on d regenerates differently
    r.git(&["switch", "-q", "dev"]);
    r.git(&["switch", "-q", "-c", "l"]);
    r.git(&["mv", "mock", "linux"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: mock is linux"]);
    let doc = r.root.join("docs/ALPHA.md");
    let text = fs::read_to_string(&doc)
        .unwrap()
        .replace("one\n", "one, by hand on l\n");
    fs::write(&doc, text).unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: edit by hand on l"]);
    // what a clone that was on `mock/` before keeps: generated hooks there, none
    // under `linux/`
    assert!(r.root.join("linux").is_dir() && !r.root.join("mock/ALPHA.md.tmpl").exists());
    let leftover = r.root.join("mock/target/hooks");
    fs::create_dir_all(&leftover).unwrap();
    for hook in ["post-merge", "post-rewrite"] {
        common::write_executable(&leftover.join(hook), "#!/bin/sh\nexit 0\n");
    }
    fs::remove_dir_all(r.root.join("linux/target/hooks")).ok();

    let merged = r.git_env(&[("MS_NO_MOCK", "1")], &["merge", "d", "-m", "chore: merge d"]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(&doc).unwrap();
    assert!(
        text.contains("<<<<<<<"),
        "both sides are in the file: {text}"
    );
    assert!(!r.kept_ours().exists(), "nothing was promised");
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
