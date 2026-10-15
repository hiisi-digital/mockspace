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
    common::write(&hooks.join("post-merge"), "#!/bin/sh\nexit 0\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            hooks.join("post-merge"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
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
fn a_rebase_in_a_fresh_worktree_conflicts_as_plain_git_does() {
    let r = Repo::new();
    r.two_sides();
    let wt = r.worktree("a");
    let rebased = r.git_in(&wt, &["rebase", "dev"]);
    assert!(!rebased.status.success(), "{}", Repo::failure(&rebased));
    let doc = fs::read_to_string(wt.join("docs/ALPHA.md")).unwrap();
    assert!(doc.contains("<<<<<<<"), "both sides are in the file: {doc}");
}

#[test]
fn a_merge_in_a_fresh_worktree_conflicts_as_plain_git_does() {
    let r = Repo::new();
    r.two_sides();
    let wt = r.worktree("a");
    let merged = r.git_in(&wt, &["merge", "dev", "-m", "chore: merge dev"]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    let doc = fs::read_to_string(wt.join("docs/ALPHA.md")).unwrap();
    assert!(doc.contains("<<<<<<<"), "both sides are in the file: {doc}");
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
