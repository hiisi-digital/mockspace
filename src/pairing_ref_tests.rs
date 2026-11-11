//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A pairing resolved through a real ref store, on real git and a bare remote.

use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace_store::{Edit, Reapply, RefPair, RefStore, Store};

use super::*;
use crate::pairing::{DEFAULT_WINDOW_SECS, tree_digest};
use mockspace_store::test_support::{Remote, git};

const T1: &str = "id = \"t1\"\n";
const T2: &str = "id = \"t2\"\n";

fn store_at(remote: &Remote, dir: &Path) -> RefStore {
    RefStore::new(
        dir,
        remote.bare.to_str().unwrap(),
        RefPair::ref_form("registry").unwrap(),
    )
}

fn seeded(remote: &Remote, name: &str) -> (RefStore, PathBuf) {
    let dir = remote.clone_dir(name);
    let store = store_at(remote, &dir);
    store
        .apply(
            &Edit::new()
                .put("ticket/t1.toml", T1)
                .put("ticket/t2.toml", T2),
            "seed",
            &Reapply,
        )
        .unwrap();
    (store, dir)
}

fn expected_digest() -> String {
    tree_digest([("ticket/t1.toml", T1.as_bytes()), ("ticket/t2.toml", T2.as_bytes())]).unwrap()
}

/// A commit with the tree of the tip and the two given times, made the way a rebase or a rehouse
/// leaves one: another hash, the same files. The ref in `dir` is moved onto it.
fn same_files_at(dir: &Path, author: i64, committer: i64) -> String {
    let tree = git(dir, &["rev-parse", "refs/mock/registry^{tree}"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit-tree", "-m", "registry: seed", &tree])
        .env_remove("GIT_DIR")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_AUTHOR_DATE", format!("{author} +0000"))
        .env("GIT_COMMITTER_DATE", format!("{committer} +0000"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let commit = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    git(dir, &["update-ref", "refs/mock/registry", &commit]);
    commit
}

#[test]
fn a_pairing_of_the_tip_is_its_hash_its_author_time_and_the_digest_of_its_files() {
    let remote = Remote::new();
    let (store, dir) = seeded(&remote, "a");
    let p = pairing_of(&store, None).unwrap();
    assert_eq!(Some(p.hash().to_owned()), store.local_tip().unwrap());
    assert_eq!(
        p.time().to_string(),
        git(&dir, &["log", "-1", "--format=%at", p.hash()])
    );
    // worked out again here from the files as the test knows them, with no git object in it
    assert_eq!(p.digest(), expected_digest());
    assert_eq!(
        resolve(&store, &p, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::ByHash(p.hash().to_owned())
    );
    assert_eq!(pairing_of(&store, Some(p.hash())).unwrap(), p);
}

#[test]
fn a_pairing_of_what_is_not_there_is_an_error_and_not_a_pairing_of_nothing() {
    let remote = Remote::new();
    let empty = store_at(&remote, &remote.clone_dir("e"));
    assert!(pairing_of(&empty, None).unwrap_err().contains("no commits"));
    let (store, _) = seeded(&remote, "a");
    let err = pairing_of(&store, Some(&"0".repeat(40))).unwrap_err();
    assert!(err.contains("is not a commit"), "{err}");
    assert!(pairing_of(&store, Some("--all")).is_err());
}

#[test]
fn a_rehoused_commit_is_found_by_author_time_and_digest_and_a_rebase_does_not_hide_it() {
    let remote = Remote::new();
    let (store, _) = seeded(&remote, "a");
    let recorded = pairing_of(&store, None).unwrap();
    let near = recorded.time() + 120;
    let three_days = 3 * 24 * 60 * 60;
    // a rebase keeps the author time and resets the committer time to when it ran
    let (_, rebased) = seeded(&remote, "r");
    let moved = same_files_at(&rebased, near, near + three_days);
    assert_ne!(moved, recorded.hash());
    assert_eq!(
        resolve(&store_at(&remote, &rebased), &recorded, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::ByTimeAndDigest(moved)
    );
    // the control for the time: the same files, authored three days off, are not taken
    let (_, far) = seeded(&remote, "f");
    same_files_at(&far, recorded.time() + three_days, recorded.time());
    assert_eq!(
        resolve(&store_at(&remote, &far), &recorded, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::Missing
    );
    // the control for the digest: another file, authored in the window, is not taken
    let (other, changed) = seeded(&remote, "c");
    other
        .apply(
            &Edit::new().put("ticket/t3.toml", "id = \"t3\"\n"),
            "t3",
            &Reapply,
        )
        .unwrap();
    same_files_at(&changed, near, near);
    assert_eq!(
        resolve(&store_at(&remote, &changed), &recorded, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::Missing
    );
}

#[test]
fn a_hash_that_is_there_with_other_files_is_a_mismatch_carrying_the_digest_found() {
    let remote = Remote::new();
    let (store, _) = seeded(&remote, "a");
    let real = pairing_of(&store, None).unwrap();
    let wrong = Pairing::new(real.hash(), real.time(), "0".repeat(64)).unwrap();
    assert_eq!(
        resolve(&store, &wrong, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::Mismatch {
            hash:  real.hash().to_owned(),
            found: Some(expected_digest()),
        }
    );
}

#[test]
fn a_commit_missing_from_a_cut_history_is_truncated_and_not_missing() {
    let remote = Remote::new();
    let (full, _) = seeded(&remote, "a");
    let old = pairing_of(&full, None).unwrap();
    full.apply(
        &Edit::new().put("ticket/t3.toml", "id = \"t3\"\n"),
        "t3",
        &Reapply,
    )
    .unwrap();
    let new = pairing_of(&full, None).unwrap();
    let cut = store_at(&remote, &remote.clone_dir("s")).with_depth(1);
    cut.refresh().unwrap();
    assert_eq!(
        resolve(&cut, &old, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::Truncated
    );
    // the controls: the commit the cut kept, and the older one where the history is whole
    assert_eq!(
        resolve(&cut, &new, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::ByHash(new.hash().to_owned())
    );
    assert_eq!(
        resolve(&full, &old, DEFAULT_WINDOW_SECS).unwrap(),
        Resolution::ByHash(old.hash().to_owned())
    );
}

/// The tip's tree with `ticket/link` added as a symbolic link, in a commit of its own that no ref
/// holds. Its target text is the text of `ticket/t1.toml`, which is the file it would be taken for.
fn tip_with_a_link(dir: &Path) -> String {
    let idx = dir.join(".git/link.idx");
    let _ = std::fs::remove_file(&idx);
    let run = |args: &[&str]| -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env("GIT_INDEX_FILE", &idx)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    run(&["read-tree", "refs/mock/registry"]);
    let blob = git(dir, &["rev-parse", "refs/mock/registry:ticket/t1.toml"]);
    run(&["update-index", "--add", "--cacheinfo", &format!("120000,{blob},ticket/link")]);
    let tree = run(&["write-tree"]);
    run(&["commit-tree", "-m", "a link", &tree])
}

#[test]
fn a_digest_is_refused_for_a_tree_with_a_symbolic_link_and_not_taken_for_a_file() {
    let remote = Remote::new();
    let (store, dir) = seeded(&remote, "a");
    let linked = tip_with_a_link(&dir);
    let err = digest_of(&store, &linked).unwrap_err();
    assert!(err.contains("ticket/link"), "{err}");
    // the control: the tip itself, without the link, has its digest
    let tip = store.local_tip().unwrap().unwrap();
    assert_eq!(digest_of(&store, &tip).unwrap(), expected_digest());
}
