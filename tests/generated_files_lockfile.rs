//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two branches that each add a workspace member merge without a conflict in `Cargo.lock`, and
//! the lock that comes out holds both, written by the same hooks a developer runs: nothing here
//! settles the lock by hand.

mod common;

use std::process::Command;

use common::{Repo, write};

fn member(r: &Repo, name: &str) {
    let dir = r.root.join("mock/members").join(name);
    write(
        &dir.join("Cargo.toml"),
        &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    );
    write(&dir.join("src/lib.rs"), "");
}

/// What a developer's own build does to the lock: resolve the manifests as they stand.
fn resolve(r: &Repo) {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
        .arg(r.root.join("mock/Cargo.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// `dev` holds a workspace with the member `base` and its lock; `m1` and `m2` each add a member
/// and resolve their own lock, and edit the template so a merge of them stops on it.
fn two_member_branches(r: &Repo, edit_template: bool) {
    write(
        &r.root.join("mock/Cargo.toml"),
        "[workspace]\nmembers = [\"members/*\"]\nresolver = \"2\"\n",
    );
    member(r, "base");
    resolve(r);
    // the lock exists now, so generation attaches the driver to it
    r.generate();
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: base"]);
    for name in ["m1", "m2"] {
        r.git(&["switch", "-q", "-c", name, "dev"]);
        member(r, name);
        resolve(r);
        if edit_template {
            r.side(name, "three", &format!("three, on {name}"));
        } else {
            r.git(&["add", "-A"]);
            r.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: add {name}")]);
        }
    }
    r.git(&["switch", "-q", "m1"]);
}

fn assert_lock_holds_every_member(lock: &str) {
    for name in ["base", "m1", "m2"] {
        assert!(lock.contains(&format!("name = \"{name}\"")), "{name} is in the lock: {lock}");
    }
    assert!(!lock.contains("<<<<<<<"), "{lock}");
}

#[test]
fn git_alone_conflicts_in_the_lock_of_two_branches_that_each_add_a_member() {
    // the control: without the driver the same merge stops on the lock
    let r = Repo::new();
    two_member_branches(&r, false);
    r.git(&["config", "--unset", "merge.mockspace-lockfile.driver"]);
    let merged = r.git_out(&["-c", "merge.mockspace-lockfile.driver=false", "merge", "m2", "-m", "chore: merge m2"]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.unmerged().contains(&"mock/Cargo.lock".to_string()), "{:?}", r.unmerged());
}

#[test]
fn a_merge_of_two_added_members_is_clean_and_the_lock_holds_both() {
    let r = Repo::new();
    two_member_branches(&r, false);
    let merged = r.git_out(&["merge", "m2", "-m", "chore: merge m2"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert_lock_holds_every_member(&r.git(&["show", "HEAD:mock/Cargo.lock"]));
    assert!(!r.kept_ours().exists(), "the engine settled it, so the record is gone");
}

#[test]
fn a_stopped_merge_concluded_by_a_commit_carries_a_lock_with_both_members() {
    let r = Repo::new();
    two_member_branches(&r, true);
    let merged = r.git_out(&["merge", "m2", "-m", "chore: merge m2"]);
    assert!(!merged.status.success(), "the template conflicts: {}", Repo::failure(&merged));
    assert!(r.kept_ours().exists());
    write(&r.root.join("mock/ALPHA.md.tmpl"), "one\ntwo\nthree, resolved\nfour\nfive\n");
    r.git(&["add", "mock/ALPHA.md.tmpl"]);
    let done = r.git_out(&["commit", "-q", "-m", "chore: merge m2"]);
    assert!(done.status.success(), "{}", Repo::failure(&done));
    assert_lock_holds_every_member(&r.git(&["show", "HEAD:mock/Cargo.lock"]));
    assert!(!r.kept_ours().exists());
}
